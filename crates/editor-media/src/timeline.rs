// SPDX-License-Identifier: AGPL-3.0-or-later

//! Timeline rendering through one FFmpeg filter graph. Preview frames, preview audio and the
//! final export are produced by the *same* graph builder; only the output size, pixel format
//! and encoder differ. Arguments are passed directly to the process, never through a shell.

use std::{
    ffi::OsString,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use editor_core::{PlanSegment, Project, audio_plans, transition_windows, video_plan};

use crate::{MediaError, Rational, ffmpeg_program};

/// Sample rate used for exported audio (AAC 48 kHz, see BRIEFING section 7.2).
pub const EXPORT_SAMPLE_RATE: u32 = 48_000;

/// Source material referenced by one segment.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderSource {
    pub path: PathBuf,
    pub start_seconds: f64,
    /// Playback speed (source seconds per timeline second).
    pub speed: f64,
    pub audio: AudioMix,
}

/// Per-segment audio processing derived from the clip settings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AudioMix {
    pub gain_db: f64,
    pub fade_in_seconds: f64,
    pub fade_out_seconds: f64,
    pub denoise: bool,
    /// Timeline seconds from the clip start to the segment start.
    pub offset_seconds: f64,
    pub clip_seconds: f64,
}

/// One contiguous timeline range; `None` is black video or silence.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderSegment {
    pub duration_frames: u64,
    pub source: Option<RenderSource>,
}

/// A resolved, self-contained render job for a timeline frame range.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineRender {
    pub width: u32,
    pub height: u32,
    pub frame_rate: Rational,
    pub start_frame: u64,
    pub total_frames: u64,
    pub video: Vec<RenderSegment>,
    pub audio_tracks: Vec<Vec<RenderSegment>>,
}

impl TimelineRender {
    /// Builds a job for `[start, end)` of the project at the given output size.
    pub fn from_project(
        project: &Project,
        start: u64,
        end: u64,
        width: u32,
        height: u32,
    ) -> Result<Self, MediaError> {
        if start >= end {
            return Err(MediaError::InvalidExport("render range is empty"));
        }
        if width < 2 || height < 2 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(MediaError::InvalidExport(
                "render dimensions must be positive even values",
            ));
        }
        let format = project.format;
        let frame_rate = Rational {
            numerator: format.frame_rate_numerator,
            denominator: format.frame_rate_denominator,
        };
        if frame_rate.numerator == 0 || frame_rate.denominator == 0 {
            return Err(MediaError::InvalidExport("frame rate must be positive"));
        }
        let convert = |segments: Vec<PlanSegment>| -> Result<Vec<RenderSegment>, MediaError> {
            segments
                .into_iter()
                .map(|segment| {
                    let source = match segment.source {
                        None => None,
                        Some(source) => {
                            let media = project.media_asset(source.media_id).ok_or(
                                MediaError::InvalidExport("clip references missing media"),
                            )?;
                            let clip = editor_core::find_clip(project, source.clip_id)
                                .map(|(t, c)| &project.tracks[t].clips[c]);
                            let audio = clip.map_or_else(AudioMix::default, |clip| AudioMix {
                                gain_db: f64::from(clip.audio.gain_db),
                                fade_in_seconds: format
                                    .frames_to_seconds(clip.audio.fade_in_frames),
                                fade_out_seconds: format
                                    .frames_to_seconds(clip.audio.fade_out_frames),
                                denoise: clip.audio.denoise,
                                offset_seconds: format.frames_to_seconds(source.clip_offset_frames),
                                clip_seconds: format.frames_to_seconds(clip.duration_frames),
                            });
                            Some(RenderSource {
                                path: PathBuf::from(&media.path),
                                start_seconds: format.frames_to_seconds(source.source_start_frame),
                                speed: f64::from(source.speed),
                                audio,
                            })
                        }
                    };
                    Ok(RenderSegment {
                        duration_frames: segment.duration_frames,
                        source,
                    })
                })
                .collect()
        };
        Ok(Self {
            width,
            height,
            frame_rate,
            start_frame: start,
            total_frames: end - start,
            video: convert(video_plan(project, start, end))?,
            audio_tracks: audio_plans(project, start, end)
                .into_iter()
                .map(convert)
                .collect::<Result<_, _>>()?,
        })
    }

    /// The "other" clip inside each transition window, aligned frame by frame with the main
    /// stream (gaps elsewhere). `None` when no transition intersects `[start, end)`.
    pub fn transition_side(
        project: &Project,
        start: u64,
        end: u64,
        width: u32,
        height: u32,
    ) -> Result<Option<Self>, MediaError> {
        let windows: Vec<_> = transition_windows(project)
            .into_iter()
            .filter(|window| {
                window.start_frame < end && window.start_frame + window.duration_frames > start
            })
            .collect();
        if windows.is_empty() {
            return Ok(None);
        }
        let mut job = Self::from_project(project, start, end, width, height)?;
        job.audio_tracks.clear();
        let format = project.format;
        let clip =
            |id| editor_core::find_clip(project, id).map(|(t, c)| &project.tracks[t].clips[c]);
        let mut segments = Vec::new();
        let mut cursor = start;
        let push_gap = |segments: &mut Vec<RenderSegment>, frames: u64| {
            if frames > 0 {
                segments.push(RenderSegment {
                    duration_frames: frames,
                    source: None,
                });
            }
        };
        for window in &windows {
            let (Some(from), Some(to)) = (clip(window.from_clip), clip(window.to_clip)) else {
                continue;
            };
            let window_end = window.start_frame + window.duration_frames;
            let a = window.start_frame.max(cursor);
            let b = window_end.min(end);
            if a >= b {
                continue;
            }
            push_gap(&mut segments, a - cursor);
            let media = |id| {
                project
                    .media_asset(id)
                    .ok_or(MediaError::InvalidExport("clip references missing media"))
            };
            // Before the cut: the incoming clip's pre-roll.
            if a < window.cut_frame {
                let len = window.cut_frame.min(b) - a;
                let back = to.source_frames_for(window.cut_frame - a);
                segments.push(RenderSegment {
                    duration_frames: len,
                    source: Some(RenderSource {
                        path: PathBuf::from(&media(to.media_id)?.path),
                        start_seconds: format
                            .frames_to_seconds(to.source_start_frame.saturating_sub(back)),
                        speed: f64::from(to.speed),
                        audio: AudioMix::default(),
                    }),
                });
            }
            // After the cut: the outgoing clip's post-roll (held on its last frame if needed).
            if b > window.cut_frame {
                let a2 = a.max(window.cut_frame);
                let asset = media(from.media_id)?;
                let wanted = from.source_end_frame().unwrap_or(0)
                    + from.source_frames_for(a2 - window.cut_frame);
                let last = asset.duration_frames.saturating_sub(2);
                segments.push(RenderSegment {
                    duration_frames: b - a2,
                    source: Some(RenderSource {
                        path: PathBuf::from(&asset.path),
                        start_seconds: format.frames_to_seconds(wanted.min(last)),
                        speed: f64::from(from.speed),
                        audio: AudioMix::default(),
                    }),
                });
            }
            cursor = b;
        }
        push_gap(&mut segments, end - cursor);
        job.video = segments;
        Ok(Some(job))
    }

    /// Replaces sources with their preview proxies where one is ready (preview only).
    pub fn with_proxies(mut self) -> Self {
        let swap = |segment: &mut RenderSegment| {
            if let Some(source) = &mut segment.source
                && let Some(proxy) = crate::proxy::proxy_path(&source.path).filter(|p| p.is_file())
            {
                source.path = proxy;
            }
        };
        self.video.iter_mut().for_each(swap);
        self.audio_tracks.iter_mut().flatten().for_each(swap);
        self
    }

    pub fn duration_seconds(&self) -> f64 {
        self.frames_to_seconds(self.total_frames)
    }

    fn frames_to_seconds(&self, frames: u64) -> f64 {
        frames as f64 * f64::from(self.frame_rate.denominator)
            / f64::from(self.frame_rate.numerator)
    }

    fn frames_to_samples(&self, frames: u64, sample_rate: u32) -> u64 {
        let samples =
            u128::from(frames) * u128::from(sample_rate) * u128::from(self.frame_rate.denominator)
                / u128::from(self.frame_rate.numerator);
        u64::try_from(samples).unwrap_or(u64::MAX)
    }
}

/// Output wiring of a filter graph: input arguments plus the `-filter_complex` script.
#[derive(Debug, Default)]
struct Graph {
    input_args: Vec<OsString>,
    filters: Vec<String>,
    inputs: usize,
}

impl Graph {
    fn add_input(&mut self, source: &RenderSource, seconds: f64) -> usize {
        self.input_args.extend([
            OsString::from("-ss"),
            OsString::from(format!("{:.6}", source.start_seconds.max(0.0))),
            OsString::from("-t"),
            OsString::from(format!("{seconds:.6}")),
            OsString::from("-i"),
            source.path.as_os_str().to_owned(),
        ]);
        self.inputs += 1;
        self.inputs - 1
    }

    fn script(&self) -> String {
        self.filters.join(";\n")
    }
}

/// Final pixel format/upload step for the video chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoOutput {
    Rgba,
    Yuv420p,
    VaapiNv12,
}

fn add_video_chain(graph: &mut Graph, job: &TimelineRender, output: VideoOutput) {
    let (w, h) = (job.width, job.height);
    let rate = format!(
        "{}/{}",
        job.frame_rate.numerator, job.frame_rate.denominator
    );
    let one_frame = job.frames_to_seconds(1);
    let mut labels = Vec::new();
    for (index, segment) in job.video.iter().enumerate() {
        let frames = segment.duration_frames;
        let seconds = job.frames_to_seconds(frames);
        let label = format!("v{index}");
        let filter = match &segment.source {
            Some(source) => {
                let speed = source.speed.max(0.01);
                let input = graph.add_input(source, (seconds + one_frame * 2.0) * speed);
                format!(
                    "[{input}:v:0]setpts=(PTS-STARTPTS)/{speed:.6},\
                     scale={w}:{h}:force_original_aspect_ratio=decrease,\
                     pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1,\
                     fps={rate},format=yuv420p,\
                     tpad=stop_mode=clone:stop_duration={pad:.6},\
                     trim=end_frame={frames},setpts=PTS-STARTPTS[{label}]",
                    pad = seconds + one_frame,
                )
            }
            None => format!(
                "color=c=black:s={w}x{h}:r={rate}:d={dur:.6},format=yuv420p,setsar=1,\
                 trim=end_frame={frames},setpts=PTS-STARTPTS[{label}]",
                dur = seconds + one_frame,
            ),
        };
        graph.filters.push(filter);
        labels.push(label);
    }
    let tail = match output {
        VideoOutput::Rgba => "scale=in_color_matrix=bt601:in_range=tv,format=rgba",
        VideoOutput::Yuv420p => "format=yuv420p",
        VideoOutput::VaapiNv12 => "format=nv12,hwupload",
    };
    let inputs: String = labels.iter().map(|label| format!("[{label}]")).collect();
    graph.filters.push(format!(
        "{inputs}concat=n={}:v=1:a=0,{tail}[vout]",
        labels.len()
    ));
}

fn add_audio_chain(graph: &mut Graph, job: &TimelineRender, sample_rate: u32, sample_fmt: &str) {
    let layout = "aformat=sample_fmts=fltp:channel_layouts=stereo";
    let mut track_labels = Vec::new();
    for (track_index, track) in job.audio_tracks.iter().enumerate() {
        let mut labels = Vec::new();
        for (index, segment) in track.iter().enumerate() {
            let samples = job.frames_to_samples(segment.duration_frames, sample_rate);
            let seconds = job.frames_to_seconds(segment.duration_frames);
            let label = format!("a{track_index}_{index}");
            let filter = match &segment.source {
                Some(source) => {
                    let speed = source.speed.max(0.01);
                    let input = graph.add_input(source, (seconds + 0.1) * speed);
                    let processing = audio_processing(source);
                    format!(
                        "[{input}:a:0]asetpts=PTS-STARTPTS,aresample={sample_rate},{layout}{processing},\
                         apad=whole_len={samples},atrim=end_sample={samples},\
                         asetpts=PTS-STARTPTS[{label}]"
                    )
                }
                None => format!(
                    "anullsrc=r={sample_rate}:cl=stereo,{layout},atrim=end_sample={samples},\
                     asetpts=PTS-STARTPTS[{label}]"
                ),
            };
            graph.filters.push(filter);
            labels.push(label);
        }
        let inputs: String = labels.iter().map(|label| format!("[{label}]")).collect();
        let track_label = format!("at{track_index}");
        graph.filters.push(format!(
            "{inputs}concat=n={}:v=0:a=1[{track_label}]",
            labels.len()
        ));
        track_labels.push(track_label);
    }
    let total_samples = job.frames_to_samples(job.total_frames, sample_rate);
    let output_format = format!(
        "aformat=sample_fmts={sample_fmt}:sample_rates={sample_rate}:channel_layouts=stereo"
    );
    match track_labels.len() {
        0 => graph.filters.push(format!(
            "anullsrc=r={sample_rate}:cl=stereo,atrim=end_sample={total_samples},{output_format}[aout]"
        )),
        1 => graph
            .filters
            .push(format!("[{}]{output_format}[aout]", track_labels[0])),
        count => {
            let inputs: String = track_labels
                .iter()
                .map(|label| format!("[{label}]"))
                .collect();
            graph.filters.push(format!(
                "{inputs}amix=inputs={count}:normalize=0:duration=longest,{output_format}[aout]"
            ));
        }
    }
}

/// atempo chain (each stage limited to 0.5..=2), noise reduction and gain/fade envelope.
fn audio_processing(source: &RenderSource) -> String {
    let mut filters = String::new();
    let mut speed = source.speed;
    if (speed - 1.0).abs() > 1e-6 {
        while speed > 2.0 {
            filters.push_str(",atempo=2.0");
            speed /= 2.0;
        }
        while speed < 0.5 {
            filters.push_str(",atempo=0.5");
            speed /= 0.5;
        }
        filters.push_str(&format!(",atempo={speed:.6}"));
    }
    let mix = &source.audio;
    if mix.denoise {
        filters.push_str(",afftdn=nf=-25");
    }
    let gain = 10_f64.powf(mix.gain_db / 20.0);
    let mut factors = vec![format!("{gain:.6}")];
    let position = format!("(t+{:.6})", mix.offset_seconds);
    if mix.fade_in_seconds > 0.0 {
        factors.push(format!(
            "min(1,max(0,{position}/{:.6}))",
            mix.fade_in_seconds
        ));
    }
    if mix.fade_out_seconds > 0.0 {
        factors.push(format!(
            "min(1,max(0,({:.6}-{position})/{:.6}))",
            mix.clip_seconds, mix.fade_out_seconds
        ));
    }
    if factors.len() > 1 || (gain - 1.0).abs() > 1e-6 {
        filters.push_str(&format!(",volume='{}':eval=frame", factors.join("*")));
    }
    filters
}

fn base_command() -> Command {
    let mut command = Command::new(ffmpeg_program());
    command.args(["-hide_banner", "-nostdin", "-loglevel", "error"]);
    command
}

fn spawn_error(source: std::io::Error) -> MediaError {
    MediaError::Spawn {
        program: ffmpeg_program().to_string_lossy().into_owned(),
        source,
    }
}

/// Collects the last part of a child's stderr on a helper thread so the pipe never blocks.
fn drain_stderr(child: &mut Child) -> Option<thread::JoinHandle<String>> {
    let mut stderr = child.stderr.take()?;
    Some(thread::spawn(move || {
        let mut collected = Vec::new();
        let mut buffer = [0_u8; 4096];
        while let Ok(read) = stderr.read(&mut buffer) {
            if read == 0 {
                break;
            }
            collected.extend_from_slice(&buffer[..read]);
            if collected.len() > 64 * 1024 {
                collected.drain(..collected.len() - 32 * 1024);
            }
        }
        String::from_utf8_lossy(&collected).trim().to_owned()
    }))
}

/// A running FFmpeg process that streams raw RGBA frames of the timeline.
pub struct VideoStream {
    child: Child,
    stdout: std::process::ChildStdout,
    stderr: Option<thread::JoinHandle<String>>,
    pub width: u32,
    pub height: u32,
    frame_bytes: usize,
}

impl VideoStream {
    /// Waits for FFmpeg to exit and returns the tail of its stderr (for error reports).
    pub fn stderr_tail(&mut self) -> String {
        let _ = self.child.wait();
        self.stderr
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default()
    }

    /// Reads the next frame; `Ok(None)` at the end of the stream.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, MediaError> {
        let mut frame = vec![0_u8; self.frame_bytes];
        let mut filled = 0;
        while filled < frame.len() {
            match self.stdout.read(&mut frame[filled..]) {
                Ok(0) => {
                    return if filled == 0 {
                        Ok(None)
                    } else {
                        Err(MediaError::FrameSize {
                            actual: filled,
                            expected: self.frame_bytes,
                        })
                    };
                }
                Ok(read) => filled += read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(MediaError::WaveformIo(error)),
            }
        }
        Ok(Some(frame))
    }
}

impl Drop for VideoStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts streaming RGBA preview frames for the job.
pub fn spawn_video_stream(job: &TimelineRender) -> Result<VideoStream, MediaError> {
    let mut graph = Graph::default();
    add_video_chain(&mut graph, job, VideoOutput::Rgba);
    let mut command = base_command();
    command
        .args(&graph.input_args)
        .arg("-filter_complex")
        .arg(graph.script())
        .args([
            // The graph already emits exactly one frame per timeline frame; never let the
            // muxer drop/duplicate by timestamp (FFmpeg 7.1 dropped frames here by default).
            "-map",
            "[vout]",
            "-fps_mode",
            "passthrough",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(spawn_error)?;
    let stderr = drain_stderr(&mut child);
    let stdout = child
        .stdout
        .take()
        .ok_or(MediaError::InvalidFrame("FFmpeg stdout was not captured"))?;
    let frame_bytes = job.width as usize * job.height as usize * 4;
    Ok(VideoStream {
        child,
        stdout,
        stderr,
        width: job.width,
        height: job.height,
        frame_bytes,
    })
}

/// Renders exactly one RGBA frame (the first frame of the job), as preview scrubbing does.
pub fn render_frame(job: &TimelineRender) -> Result<Vec<u8>, MediaError> {
    let mut single = job.clone();
    single.total_frames = 1;
    let mut remaining = 1_u64;
    single.video.retain_mut(|segment| {
        if remaining == 0 {
            return false;
        }
        segment.duration_frames = segment.duration_frames.min(remaining);
        remaining -= segment.duration_frames;
        true
    });
    let mut stream = spawn_video_stream(&single)?;
    stream
        .next_frame()?
        .ok_or(MediaError::InvalidFrame("FFmpeg produced no frame"))
}

/// A running FFmpeg process that streams interleaved stereo f32 samples of the timeline mix.
pub struct AudioStream {
    child: Child,
    stdout: std::process::ChildStdout,
    pub sample_rate: u32,
}

impl AudioStream {
    /// Reads up to `buffer.len()` interleaved stereo samples; returns 0 at the end of stream.
    pub fn read_samples(&mut self, buffer: &mut [f32]) -> Result<usize, MediaError> {
        let mut bytes = vec![0_u8; buffer.len() * 4];
        let mut filled = 0;
        while filled < bytes.len() {
            match self.stdout.read(&mut bytes[filled..]) {
                Ok(0) => break,
                Ok(read) => {
                    filled += read;
                    if filled % 4 == 0 {
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(MediaError::WaveformIo(error)),
            }
        }
        let samples = filled / 4;
        for (target, chunk) in buffer.iter_mut().zip(bytes[..samples * 4].chunks_exact(4)) {
            *target = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        Ok(samples)
    }
}

impl Drop for AudioStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts streaming the mixed timeline audio as interleaved stereo f32 at `sample_rate`.
pub fn spawn_audio_stream(
    job: &TimelineRender,
    sample_rate: u32,
) -> Result<AudioStream, MediaError> {
    let mut graph = Graph::default();
    add_audio_chain(&mut graph, job, sample_rate, "flt");
    let mut command = base_command();
    command
        .args(&graph.input_args)
        .arg("-filter_complex")
        .arg(graph.script())
        .args(["-map", "[aout]", "-f", "f32le", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(spawn_error)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(MediaError::InvalidFrame("FFmpeg stdout was not captured"))?;
    Ok(AudioStream {
        child,
        stdout,
        sample_rate,
    })
}

/// Renders the mixed timeline audio to a mono 16-bit WAV (used for speech recognition).
pub fn export_audio_wav(
    job: &TimelineRender,
    output: &Path,
    sample_rate: u32,
) -> Result<(), MediaError> {
    let mut graph = Graph::default();
    add_audio_chain(&mut graph, job, sample_rate, "fltp");
    let result = Command::new(ffmpeg_program())
        .args(["-hide_banner", "-nostdin", "-y", "-loglevel", "error"])
        .args(&graph.input_args)
        .arg("-filter_complex")
        .arg(graph.script())
        .args(["-map", "[aout]", "-ac", "1", "-ar"])
        .arg(sample_rate.to_string())
        .args(["-c:a", "pcm_s16le"])
        .arg(output)
        .stdin(Stdio::null())
        .output()
        .map_err(spawn_error)?;
    if result.status.success() {
        Ok(())
    } else {
        Err(MediaError::Failed {
            program: ffmpeg_program().to_string_lossy().into_owned(),
            status: result.status.to_string(),
            stderr: String::from_utf8_lossy(&result.stderr).trim().to_owned(),
        })
    }
}

/// Video encoder settings for a timeline export (always H.264 + AAC in MP4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportEncoder {
    /// Software libx264 with constant rate factor ("Qualidade").
    X264 { crf: u8, preset: String },
    /// Hardware h264_vaapi with constant QP ("Rápido").
    Vaapi { device: PathBuf, qp: u8 },
}

const X264_PRESETS: [&str; 9] = [
    "ultrafast",
    "superfast",
    "veryfast",
    "faster",
    "fast",
    "medium",
    "slow",
    "slower",
    "veryslow",
];

/// Builds the export command (exposed for tests and for logging).
pub fn build_timeline_export_command(
    job: &TimelineRender,
    encoder: &ExportEncoder,
    output: &Path,
) -> Result<Command, MediaError> {
    let uses_output_as_input = job
        .video
        .iter()
        .chain(job.audio_tracks.iter().flatten())
        .filter_map(|segment| segment.source.as_ref())
        .any(|source| source.path == output);
    if uses_output_as_input {
        return Err(MediaError::InvalidExport(
            "the output file is also used as an input",
        ));
    }
    let mut graph = Graph::default();
    let video_output = match encoder {
        ExportEncoder::X264 { .. } => VideoOutput::Yuv420p,
        ExportEncoder::Vaapi { .. } => VideoOutput::VaapiNv12,
    };
    add_video_chain(&mut graph, job, video_output);
    add_audio_chain(&mut graph, job, EXPORT_SAMPLE_RATE, "fltp");
    let mut command = Command::new(ffmpeg_program());
    command.args(["-hide_banner", "-nostdin", "-y", "-loglevel", "error"]);
    if let ExportEncoder::Vaapi { device, .. } = encoder {
        // Explicit device for the filter graph (`hwupload`); `-vaapi_device` alone failed to
        // reach the graph when frames arrive on a pipe (FFmpeg 7.1/9: ENOSYS on reinit).
        let mut init = std::ffi::OsString::from("vaapi=va:");
        init.push(device.as_os_str());
        command
            .arg("-init_hw_device")
            .arg(init)
            .args(["-filter_hw_device", "va"]);
    }
    command
        .args(&graph.input_args)
        .arg("-filter_complex")
        .arg(graph.script())
        .args(["-map", "[vout]", "-map", "[aout]"]);
    match encoder {
        ExportEncoder::X264 { crf, preset } => {
            if *crf > 51 || !X264_PRESETS.contains(&preset.as_str()) {
                return Err(MediaError::InvalidExport("unsupported x264 CRF or preset"));
            }
            command
                .args(["-c:v", "libx264", "-preset", preset, "-crf"])
                .arg(crf.to_string())
                .args(["-pix_fmt", "yuv420p", "-profile:v", "high"]);
        }
        ExportEncoder::Vaapi { qp, .. } => {
            if *qp > 51 {
                return Err(MediaError::InvalidExport("VAAPI QP must be in 0..=51"));
            }
            command
                .args(["-c:v", "h264_vaapi", "-qp"])
                .arg(qp.to_string());
        }
    }
    let rate = format!(
        "{}/{}",
        job.frame_rate.numerator, job.frame_rate.denominator
    );
    command
        .args(["-r", &rate])
        .args([
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-colorspace",
            "bt709",
            "-color_range",
            "tv",
        ])
        .args(["-c:a", "aac", "-b:a", "192k", "-ar"])
        .arg(EXPORT_SAMPLE_RATE.to_string())
        .args(["-ac", "2", "-movflags", "+faststart"])
        .args(["-progress", "pipe:1", "-nostats"])
        .arg(output);
    Ok(command)
}

fn push_encoder_args(
    command: &mut Command,
    job: &TimelineRender,
    encoder: &ExportEncoder,
    output: &Path,
) -> Result<(), MediaError> {
    match encoder {
        ExportEncoder::X264 { crf, preset } => {
            if *crf > 51 || !X264_PRESETS.contains(&preset.as_str()) {
                return Err(MediaError::InvalidExport("unsupported x264 CRF or preset"));
            }
            command
                .args(["-c:v", "libx264", "-preset", preset, "-crf"])
                .arg(crf.to_string())
                .args(["-pix_fmt", "yuv420p", "-profile:v", "high"]);
        }
        ExportEncoder::Vaapi { qp, .. } => {
            if *qp > 51 {
                return Err(MediaError::InvalidExport("VAAPI QP must be in 0..=51"));
            }
            command
                .args(["-c:v", "h264_vaapi", "-qp"])
                .arg(qp.to_string());
        }
    }
    let rate = format!(
        "{}/{}",
        job.frame_rate.numerator, job.frame_rate.denominator
    );
    command
        .args(["-r", &rate])
        .args([
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-colorspace",
            "bt709",
            "-color_range",
            "tv",
        ])
        .args(["-c:a", "aac", "-b:a", "192k", "-ar"])
        .arg(EXPORT_SAMPLE_RATE.to_string())
        .args(["-ac", "2", "-movflags", "+faststart"])
        .args(["-progress", "pipe:2", "-nostats"])
        .arg(output);
    Ok(())
}

/// A running encoder that receives composited RGBA frames on stdin (GPU composition path).
/// Audio comes from the same timeline graph as the FFmpeg-only export.
pub struct RgbaEncoder {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    stderr: Option<thread::JoinHandle<String>>,
}

impl RgbaEncoder {
    pub fn write_frame(&mut self, rgba: &[u8]) -> Result<(), MediaError> {
        use std::io::Write;
        let stdin = self
            .stdin
            .as_mut()
            .ok_or(MediaError::InvalidExport("encoder input already closed"))?;
        if let Err(error) = stdin.write_all(rgba) {
            // The encoder exited: report its stderr instead of a bare "broken pipe".
            drop(self.stdin.take());
            let _ = self.child.wait();
            let stderr = self
                .stderr
                .take()
                .and_then(|handle| handle.join().ok())
                .unwrap_or_default();
            return Err(MediaError::Failed {
                program: ffmpeg_program().to_string_lossy().into_owned(),
                status: format!("input closed ({error})"),
                stderr,
            });
        }
        Ok(())
    }

    /// Closes the input and waits for FFmpeg to finish writing the file.
    pub fn finish(mut self) -> Result<(), MediaError> {
        drop(self.stdin.take());
        let status = self.child.wait().map_err(spawn_error)?;
        let stderr = self
            .stderr
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        if status.success() {
            Ok(())
        } else {
            Err(MediaError::Failed {
                program: ffmpeg_program().to_string_lossy().into_owned(),
                status: status.to_string(),
                stderr,
            })
        }
    }
}

impl Drop for RgbaEncoder {
    fn drop(&mut self) {
        if self.stdin.is_some() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Starts an encoder for `job.width`×`job.height` RGBA frames on stdin plus the timeline audio.
pub fn spawn_rgba_encoder(
    job: &TimelineRender,
    encoder: &ExportEncoder,
    output: &Path,
) -> Result<RgbaEncoder, MediaError> {
    let rate = format!(
        "{}/{}",
        job.frame_rate.numerator, job.frame_rate.denominator
    );
    let mut graph = Graph {
        input_args: vec![
            "-f".into(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            "rgba".into(),
            "-s".into(),
            format!("{}x{}", job.width, job.height).into(),
            "-framerate".into(),
            rate.clone().into(),
            "-i".into(),
            "pipe:0".into(),
        ],
        filters: Vec::new(),
        inputs: 1,
    };
    let upload = match encoder {
        ExportEncoder::X264 { .. } => "format=yuv420p",
        // Tag the frames with the output color properties *before* uploading: otherwise FFmpeg
        // tries to insert a range/matrix conversion after `hwupload`, which cannot run on
        // VAAPI surfaces ("Impossible to convert ... hwupload" / ENOSYS).
        ExportEncoder::Vaapi { .. } => {
            "format=nv12,setparams=range=tv:colorspace=bt709:color_primaries=bt709:color_trc=bt709,hwupload"
        }
    };
    // RGBA -> limited-range YUV with the same matrix used to decode, so the YUV values of the
    // source survive the RGBA round trip exactly (measured: ~48 dB PSNR vs. 25.8 dB with an
    // explicit BT.709 pair on FFmpeg 9). The file is still tagged BT.709 like the source.
    graph.filters.push(format!(
        "[0:v]scale=out_color_matrix=bt601:out_range=tv,{upload}[vout]"
    ));
    add_audio_chain(&mut graph, job, EXPORT_SAMPLE_RATE, "fltp");
    let mut command = Command::new(ffmpeg_program());
    command.args(["-hide_banner", "-nostdin", "-y", "-loglevel", "error"]);
    if let ExportEncoder::Vaapi { device, .. } = encoder {
        // Explicit device for the filter graph (`hwupload`); `-vaapi_device` alone failed to
        // reach the graph when frames arrive on a pipe (FFmpeg 7.1/9: ENOSYS on reinit).
        let mut init = std::ffi::OsString::from("vaapi=va:");
        init.push(device.as_os_str());
        command
            .arg("-init_hw_device")
            .arg(init)
            .args(["-filter_hw_device", "va"]);
    }
    command
        .args(&graph.input_args)
        .arg("-filter_complex")
        .arg(graph.script())
        .args(["-map", "[vout]", "-map", "[aout]"]);
    push_encoder_args(&mut command, job, encoder, output)?;
    // `-nostdin` only disables interactive keys; frames are read from `pipe:0`.
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(spawn_error)?;
    let stderr = drain_stderr(&mut child);
    let stdin = child.stdin.take();
    Ok(RgbaEncoder {
        child,
        stdin,
        stderr,
    })
}

/// Export progress reported while encoding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportProgress {
    /// Fraction in 0.0..=1.0.
    pub fraction: f64,
    pub encoded_seconds: f64,
}

/// Runs a timeline export, reporting progress. Setting `cancel` kills FFmpeg and returns an error.
/// Call from a worker thread.
pub fn export_timeline(
    job: &TimelineRender,
    encoder: &ExportEncoder,
    output: &Path,
    cancel: &Arc<AtomicBool>,
    mut progress: impl FnMut(ExportProgress),
) -> Result<(), MediaError> {
    let mut command = build_timeline_export_command(job, encoder, output)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(spawn_error)?;
    let stderr = drain_stderr(&mut child);
    let total = job.duration_seconds().max(1e-6);
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if let Some(value) = line.strip_prefix("out_time_us=")
                && let Ok(micros) = value.trim().parse::<i64>()
            {
                let seconds = micros.max(0) as f64 / 1_000_000.0;
                progress(ExportProgress {
                    fraction: (seconds / total).clamp(0.0, 1.0),
                    encoded_seconds: seconds,
                });
            }
        }
    }
    if cancel.load(Ordering::Relaxed) {
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(output);
        return Err(MediaError::Cancelled);
    }
    let status = child.wait().map_err(spawn_error)?;
    let stderr_text = stderr
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    if !status.success() {
        return Err(MediaError::Failed {
            program: ffmpeg_program().to_string_lossy().into_owned(),
            status: status.to_string(),
            stderr: stderr_text,
        });
    }
    progress(ExportProgress {
        fraction: 1.0,
        encoded_seconds: total,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::{Clip, MediaAsset, Track, TrackKind};

    fn project() -> Project {
        Project {
            media: vec![MediaAsset {
                id: 1,
                path: "/tmp/in put.mp4".into(),
                content_hash: None,
                duration_frames: 90,
                width: 1280,
                height: 720,
                has_video: true,
                has_audio: true,
                rotation_degrees: 0,
            }],
            tracks: vec![
                Track {
                    id: 2,
                    name: "V1".into(),
                    kind: TrackKind::Video,
                    clips: vec![Clip {
                        id: 4,
                        media_id: 1,
                        timeline_start_frame: 30,
                        source_start_frame: 15,
                        duration_frames: 30,
                        link_group: Some(6),
                        effects: Vec::new(),
                        speed: 1.0,
                        audio: editor_core::ClipAudio::default(),
                        transition_in: None,
                    }],
                },
                Track {
                    id: 3,
                    name: "A1".into(),
                    kind: TrackKind::Audio,
                    clips: vec![Clip {
                        id: 5,
                        media_id: 1,
                        timeline_start_frame: 30,
                        source_start_frame: 15,
                        duration_frames: 30,
                        link_group: Some(6),
                        effects: Vec::new(),
                        speed: 1.0,
                        audio: editor_core::ClipAudio::default(),
                        transition_in: None,
                    }],
                },
            ],
            ..Project::default()
        }
    }

    #[test]
    fn job_contains_gaps_and_source_offsets() {
        let job = TimelineRender::from_project(&project(), 0, 60, 640, 360).expect("job");
        assert_eq!(job.video.len(), 2);
        assert_eq!(job.video[0].source, None);
        assert_eq!(job.video[0].duration_frames, 30);
        let source = job.video[1].source.as_ref().expect("source");
        assert!((source.start_seconds - 0.5).abs() < 1e-9);
        assert_eq!(job.audio_tracks.len(), 1);
    }

    #[test]
    fn export_command_uses_h264_aac_faststart_and_safe_paths() {
        let job = TimelineRender::from_project(&project(), 0, 60, 1920, 1080).expect("job");
        let command = build_timeline_export_command(
            &job,
            &ExportEncoder::X264 {
                crf: 20,
                preset: "medium".into(),
            },
            Path::new("/tmp/out file.mp4"),
        )
        .expect("command");
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        for expected in [
            "libx264",
            "aac",
            "+faststart",
            "yuv420p",
            "/tmp/in put.mp4",
            "/tmp/out file.mp4",
        ] {
            assert!(args.iter().any(|arg| arg == expected), "missing {expected}");
        }
        let script = args
            .iter()
            .find(|arg| arg.contains("concat"))
            .expect("filter graph");
        assert!(script.contains("trim=end_frame=30"));
        assert!(script.contains("atrim=end_sample=48000"));
    }

    #[test]
    fn exporting_over_an_input_is_rejected() {
        let job = TimelineRender::from_project(&project(), 0, 60, 640, 360).expect("job");
        let result = build_timeline_export_command(
            &job,
            &ExportEncoder::X264 {
                crf: 20,
                preset: "fast".into(),
            },
            Path::new("/tmp/in put.mp4"),
        );
        assert!(result.is_err());
    }
}
