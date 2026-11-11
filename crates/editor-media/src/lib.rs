// SPDX-License-Identifier: AGPL-3.0-or-later

//! Local FFmpeg/ffprobe process adapters. Arguments are passed directly, never through a shell.

use std::{
    ffi::OsStr,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use serde::Deserialize;
use thiserror::Error;

pub mod timeline;

/// FFmpeg executable: `D_CUT_FFMPEG` (set by the AppImage when it bundles FFmpeg) or `ffmpeg`.
pub fn ffmpeg_program() -> std::ffi::OsString {
    std::env::var_os("D_CUT_FFMPEG").unwrap_or_else(|| "ffmpeg".into())
}

/// ffprobe executable: `D_CUT_FFPROBE` or `ffprobe`.
pub fn ffprobe_program() -> std::ffi::OsString {
    std::env::var_os("D_CUT_FFPROBE").unwrap_or_else(|| "ffprobe".into())
}

const MAX_FRAME_BYTES: usize = 128 * 1024 * 1024;
const MAX_FRAME_SIDE: u32 = 8192;

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("could not start {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} exited unsuccessfully ({status}): {stderr}")]
    Failed {
        program: String,
        status: String,
        stderr: String,
    },
    #[error("could not parse {tool} output: {source}")]
    Parse {
        tool: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid media metadata: {0}")]
    InvalidMetadata(&'static str),
    #[error("invalid frame request: {0}")]
    InvalidFrame(&'static str),
    #[error("decoded frame size {actual} does not match expected {expected} bytes")]
    FrameSize { actual: usize, expected: usize },
    #[error("waveform request is invalid: {0}")]
    InvalidWaveform(&'static str),
    #[error("could not read FFmpeg waveform output: {0}")]
    WaveformIo(#[source] std::io::Error),
    #[error("invalid export request: {0}")]
    InvalidExport(&'static str),
    #[error("operation cancelled")]
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rational {
    pub numerator: u32,
    pub denominator: u32,
}

impl Rational {
    pub fn parse(value: &str) -> Option<Self> {
        let (numerator, denominator) = value.split_once('/')?;
        let numerator = numerator.parse().ok()?;
        let denominator = denominator.parse().ok()?;
        (numerator > 0 && denominator > 0).then_some(Self {
            numerator,
            denominator,
        })
    }

    pub fn as_f64(self) -> f64 {
        f64::from(self.numerator) / f64::from(self.denominator)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    pub duration_seconds: f64,
    pub video: Option<VideoStreamInfo>,
    pub audio: Option<AudioStreamInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoStreamInfo {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub rotation_degrees: u16,
    pub frame_rate: Option<Rational>,
    pub time_base: Option<Rational>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioStreamInfo {
    pub codec: String,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    time_base: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    duration: Option<String>,
    tags: Option<ProbeTags>,
    side_data_list: Option<Vec<ProbeSideData>>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeTags {
    rotate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeSideData {
    rotation: Option<f64>,
}

/// Runs ffprobe and returns normalized stream information without copying media into the project.
pub fn probe(path: impl AsRef<Path>) -> Result<MediaInfo, MediaError> {
    probe_with(&ffprobe_program(), path)
}

/// Injectable executable variant for installations that use a nonstandard ffprobe location.
pub fn probe_with(program: &OsStr, path: impl AsRef<Path>) -> Result<MediaInfo, MediaError> {
    let path = path.as_ref();
    let output = Command::new(program)
        .args([
            OsStr::new("-v"),
            OsStr::new("error"),
            OsStr::new("-show_streams"),
            OsStr::new("-show_format"),
            OsStr::new("-of"),
            OsStr::new("json"),
        ])
        .arg(path)
        .output()
        .map_err(|source| MediaError::Spawn {
            program: program.to_string_lossy().into_owned(),
            source,
        })?;
    check_output(program, &output)?;
    let parsed: ProbeOutput =
        serde_json::from_slice(&output.stdout).map_err(|source| MediaError::Parse {
            tool: "ffprobe",
            source,
        })?;

    let format_duration = parsed
        .format
        .as_ref()
        .and_then(|format| format.duration.as_deref())
        .and_then(parse_finite_positive);
    let mut video = None;
    let mut audio = None;
    let mut stream_duration = None;
    for stream in parsed.streams {
        if let Some(duration) = stream.duration.as_deref().and_then(parse_finite_positive) {
            stream_duration =
                Some(stream_duration.map_or(duration, |current: f64| current.max(duration)));
        }
        match stream.codec_type.as_deref() {
            Some("video") if video.is_none() => {
                let mut width = stream
                    .width
                    .ok_or(MediaError::InvalidMetadata("video stream has no width"))?;
                let mut height = stream
                    .height
                    .ok_or(MediaError::InvalidMetadata("video stream has no height"))?;
                if width == 0 || height == 0 {
                    return Err(MediaError::InvalidMetadata("video dimensions are zero"));
                }
                let rotation = stream
                    .side_data_list
                    .as_deref()
                    .and_then(|items| items.iter().find_map(|item| item.rotation))
                    .or_else(|| {
                        stream
                            .tags
                            .as_ref()
                            .and_then(|tags| tags.rotate.as_deref())
                            .and_then(|value| value.parse::<f64>().ok())
                    })
                    .unwrap_or(0.0);
                if !rotation.is_finite() {
                    return Err(MediaError::InvalidMetadata("rotation is not finite"));
                }
                let rotation_degrees = normalize_rotation(rotation)?;
                if matches!(rotation_degrees, 90 | 270) {
                    std::mem::swap(&mut width, &mut height);
                }
                video = Some(VideoStreamInfo {
                    codec: stream.codec_name.unwrap_or_else(|| "unknown".to_owned()),
                    width,
                    height,
                    rotation_degrees,
                    frame_rate: stream
                        .avg_frame_rate
                        .as_deref()
                        .and_then(Rational::parse)
                        .or_else(|| stream.r_frame_rate.as_deref().and_then(Rational::parse)),
                    time_base: stream.time_base.as_deref().and_then(Rational::parse),
                });
            }
            Some("audio") if audio.is_none() => {
                audio = Some(AudioStreamInfo {
                    codec: stream.codec_name.unwrap_or_else(|| "unknown".to_owned()),
                    sample_rate: stream
                        .sample_rate
                        .as_deref()
                        .and_then(|value| value.parse().ok()),
                    channels: stream.channels,
                });
            }
            _ => {}
        }
    }
    if video.is_none() && audio.is_none() {
        return Err(MediaError::InvalidMetadata(
            "no audio or video stream found",
        ));
    }
    let duration_seconds =
        format_duration
            .or(stream_duration)
            .ok_or(MediaError::InvalidMetadata(
                "duration is missing or invalid",
            ))?;
    Ok(MediaInfo {
        duration_seconds,
        video,
        audio,
    })
}

impl MediaInfo {
    /// Duration rounded up to project frames so the final partial frame remains addressable.
    pub fn duration_in_frames(&self, frame_rate: Rational) -> Result<u64, MediaError> {
        let frames = self.duration_seconds * frame_rate.as_f64();
        if !frames.is_finite() || frames < 1.0 || frames > u64::MAX as f64 {
            return Err(MediaError::InvalidMetadata(
                "duration cannot be represented in project frames",
            ));
        }
        Ok(frames.ceil() as u64)
    }
}

fn parse_finite_positive(value: &str) -> Option<f64> {
    let parsed = value.parse::<f64>().ok()?;
    (parsed.is_finite() && parsed > 0.0).then_some(parsed)
}

fn normalize_rotation(value: f64) -> Result<u16, MediaError> {
    let rounded = value.round();
    if (value - rounded).abs() > 0.5 {
        return Err(MediaError::InvalidMetadata(
            "rotation is not a quarter turn",
        ));
    }
    let degrees = rounded.rem_euclid(360.0) as u16;
    match degrees {
        0 | 90 | 180 | 270 => Ok(degrees),
        _ => Err(MediaError::InvalidMetadata(
            "rotation is not a quarter turn",
        )),
    }
}

/// Decodes one RGBA frame on demand. Call it from a worker thread, not from the UI update callback.
pub fn decode_rgba_frame(
    path: impl AsRef<Path>,
    timestamp_seconds: f64,
) -> Result<(u32, u32, Vec<u8>), MediaError> {
    decode_rgba_frame_with(&ffmpeg_program(), path, timestamp_seconds)
}

/// Decodes one RGBA frame scaled to fit inside `max_width`×`max_height` (aspect preserved).
/// Used for timeline thumbnails. Call from a worker thread.
pub fn decode_thumbnail(
    path: impl AsRef<Path>,
    video: &VideoStreamInfo,
    timestamp_seconds: f64,
    max_width: u32,
    max_height: u32,
) -> Result<(u32, u32, Vec<u8>), MediaError> {
    let scale = (f64::from(max_width) / f64::from(video.width))
        .min(f64::from(max_height) / f64::from(video.height))
        .min(1.0);
    let even = |value: f64| ((value.round() as u32) / 2 * 2).max(2);
    let scaled = VideoStreamInfo {
        width: even(f64::from(video.width) * scale),
        height: even(f64::from(video.height) * scale),
        ..video.clone()
    };
    decode_rgba_frame_with_video_info(&ffmpeg_program(), path, timestamp_seconds, &scaled)
}

pub fn decode_rgba_frame_with(
    program: &OsStr,
    path: impl AsRef<Path>,
    timestamp_seconds: f64,
) -> Result<(u32, u32, Vec<u8>), MediaError> {
    let path = path.as_ref();
    let media_info = probe(path)?;
    let video = media_info
        .video
        .as_ref()
        .ok_or(MediaError::InvalidMetadata("source has no video stream"))?;
    decode_rgba_frame_with_video_info(program, path, timestamp_seconds, video)
}

/// Uses cached stream metadata so repeated seeks do not spawn an extra ffprobe process per frame.
pub fn decode_rgba_frame_with_video_info(
    program: &OsStr,
    path: impl AsRef<Path>,
    timestamp_seconds: f64,
    video: &VideoStreamInfo,
) -> Result<(u32, u32, Vec<u8>), MediaError> {
    if !timestamp_seconds.is_finite() || timestamp_seconds < 0.0 {
        return Err(MediaError::InvalidFrame(
            "timestamp must be finite and non-negative",
        ));
    }
    let path = path.as_ref();
    if video.width == 0
        || video.height == 0
        || video.width > MAX_FRAME_SIDE
        || video.height > MAX_FRAME_SIDE
    {
        return Err(MediaError::InvalidFrame(
            "decoded frame exceeds the supported dimensions",
        ));
    }
    let expected = (video.width as usize)
        .checked_mul(video.height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|bytes| *bytes <= MAX_FRAME_BYTES)
        .ok_or(MediaError::InvalidFrame(
            "decoded frame exceeds the memory budget",
        ))?;
    let output_size = format!("scale={}:{}", video.width, video.height);
    let seek = format!("{timestamp_seconds:.9}");
    let output = Command::new(program)
        .args([
            OsStr::new("-hide_banner"),
            OsStr::new("-loglevel"),
            OsStr::new("error"),
            OsStr::new("-nostdin"),
            OsStr::new("-ss"),
        ])
        .arg(seek)
        .arg("-i")
        .arg(path)
        .arg("-vf")
        .arg(output_size)
        .args([
            OsStr::new("-frames:v"),
            OsStr::new("1"),
            OsStr::new("-f"),
            OsStr::new("rawvideo"),
            OsStr::new("-pix_fmt"),
            OsStr::new("rgba"),
            OsStr::new("pipe:1"),
        ])
        .output()
        .map_err(|source| MediaError::Spawn {
            program: program.to_string_lossy().into_owned(),
            source,
        })?;
    check_output(program, &output)?;
    if output.stdout.len() != expected {
        return Err(MediaError::FrameSize {
            actual: output.stdout.len(),
            expected,
        });
    }
    Ok((video.width, video.height, output.stdout))
}

fn check_output(program: &OsStr, output: &Output) -> Result<(), MediaError> {
    if output.status.success() {
        return Ok(());
    }
    Err(MediaError::Failed {
        program: program.to_string_lossy().into_owned(),
        status: output.status.to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Waveform {
    pub sample_rate: u32,
    pub samples_per_bucket: u64,
    /// Root-mean-square amplitude in 0.0..=1.0, one value per waveform bucket.
    pub rms: Vec<f32>,
    /// True when the configured point cap reduced temporal resolution.
    pub resolution_limited: bool,
}

/// Extracts a bounded-memory mono waveform. Call from a worker thread, never from the UI callback.
pub fn compute_waveform(
    path: impl AsRef<Path>,
    buckets_per_second: u32,
    max_buckets: usize,
) -> Result<Waveform, MediaError> {
    const SAMPLE_RATE: u32 = 8_000;
    const ABSOLUTE_BUCKET_LIMIT: usize = 100_000;
    if buckets_per_second == 0 || max_buckets == 0 {
        return Err(MediaError::InvalidWaveform(
            "bucket rate and maximum count must be positive",
        ));
    }
    let max_buckets = max_buckets.min(ABSOLUTE_BUCKET_LIMIT);
    let path = path.as_ref();
    let duration = probe(path)?.duration_seconds;
    let expected_samples = (duration * f64::from(SAMPLE_RATE)).ceil();
    if !expected_samples.is_finite() || expected_samples < 1.0 || expected_samples > u64::MAX as f64
    {
        return Err(MediaError::InvalidWaveform(
            "source duration exceeds waveform limits",
        ));
    }
    let desired_buckets = (duration * f64::from(buckets_per_second)).ceil().max(1.0);
    let truncated = desired_buckets > max_buckets as f64;
    let bucket_count = (desired_buckets as usize).min(max_buckets).max(1);
    let sample_count = expected_samples as u64;
    let samples_per_bucket = sample_count.div_ceil(bucket_count as u64).max(1);
    let duration_arg = format!("{duration:.9}");
    let mut child = Command::new(ffmpeg_program())
        .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
        .arg("-i")
        .arg(path)
        .args(["-t", &duration_arg, "-vn", "-ac", "1", "-ar"])
        .arg(SAMPLE_RATE.to_string())
        .args(["-f", "f32le", "pipe:1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| MediaError::Spawn {
            program: "ffmpeg".to_owned(),
            source,
        })?;
    let mut stdout = child.stdout.take().ok_or_else(|| {
        MediaError::WaveformIo(std::io::Error::new(
            ErrorKind::BrokenPipe,
            "FFmpeg stdout was not piped",
        ))
    })?;
    let mut rms = Vec::with_capacity(bucket_count);
    let mut sum_squares = 0.0_f64;
    let mut samples_in_bucket = 0_u64;
    let mut bytes = [0_u8; 4];
    let mut read_error = None;
    while rms.len() < bucket_count {
        match stdout.read_exact(&mut bytes) {
            Ok(()) => {
                let sample = f32::from_le_bytes(bytes);
                if sample.is_finite() {
                    let sample = f64::from(sample.clamp(-1.0, 1.0));
                    sum_squares += sample * sample;
                }
                samples_in_bucket += 1;
                if samples_in_bucket >= samples_per_bucket {
                    rms.push((sum_squares / samples_in_bucket as f64).sqrt() as f32);
                    sum_squares = 0.0;
                    samples_in_bucket = 0;
                }
            }
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => break,
            Err(error) => {
                read_error = Some(error);
                break;
            }
        }
    }
    drop(stdout);
    if let Some(error) = read_error {
        let _ = child.kill();
        let _ = child.wait();
        return Err(MediaError::WaveformIo(error));
    }
    if samples_in_bucket > 0 && rms.len() < bucket_count {
        rms.push((sum_squares / samples_in_bucket as f64).sqrt() as f32);
    }
    let status = if rms.len() >= bucket_count {
        let _ = child.kill();
        child.wait()
    } else {
        child.wait()
    }
    .map_err(MediaError::WaveformIo)?;
    if !status.success() && rms.len() < bucket_count {
        return Err(MediaError::Failed {
            program: "ffmpeg".to_owned(),
            status: status.to_string(),
            stderr: "waveform extraction failed".to_owned(),
        });
    }
    Ok(Waveform {
        sample_rate: SAMPLE_RATE,
        samples_per_bucket,
        rms,
        resolution_limited: truncated,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportProfile {
    /// Software x264 encoding with a constant-rate-factor quality target.
    Quality { crf: u8, preset: String },
    /// Hardware H.264 encoding through VAAPI.
    Vaapi { device: PathBuf, quality: u8 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportRequest {
    pub input: PathBuf,
    pub output: PathBuf,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub width: u32,
    pub height: u32,
    pub frame_rate: Rational,
    pub profile: ExportProfile,
}

/// Builds a safe argument vector for a single-source H.264/AAC export; no shell interpolation is used.
pub fn build_export_command(request: &ExportRequest) -> Result<Command, MediaError> {
    if !request.start_seconds.is_finite()
        || request.start_seconds < 0.0
        || !request.duration_seconds.is_finite()
        || request.duration_seconds <= 0.0
    {
        return Err(MediaError::InvalidExport(
            "time range must be finite, non-negative, and non-empty",
        ));
    }
    if request.width < 2
        || request.height < 2
        || !request.width.is_multiple_of(2)
        || !request.height.is_multiple_of(2)
    {
        return Err(MediaError::InvalidExport(
            "H.264 yuv420p dimensions must be positive even values",
        ));
    }
    if request.frame_rate.numerator == 0 || request.frame_rate.denominator == 0 {
        return Err(MediaError::InvalidExport("frame rate must be positive"));
    }
    if request.input == request.output {
        return Err(MediaError::InvalidExport(
            "input and output paths must differ",
        ));
    }
    let start = format!("{:.9}", request.start_seconds);
    let duration = format!("{:.9}", request.duration_seconds);
    let rate = format!(
        "{}/{}",
        request.frame_rate.numerator, request.frame_rate.denominator
    );
    let filter = format!(
        "scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={}",
        request.width, request.height, request.width, request.height, rate,
    );
    let mut command = Command::new(ffmpeg_program());
    command.args(["-hide_banner", "-nostdin", "-y"]);
    if let ExportProfile::Vaapi { device, .. } = &request.profile {
        command.args(["-vaapi_device"]).arg(device);
    }
    command
        .arg("-ss")
        .arg(&start)
        .arg("-i")
        .arg(&request.input)
        .arg("-t")
        .arg(&duration)
        .args(["-map", "0:v:0", "-map", "0:a:0?", "-vf"])
        .arg(&filter);
    match &request.profile {
        ExportProfile::Quality { crf, preset } => {
            if *crf > 51
                || !matches!(
                    preset.as_str(),
                    "ultrafast"
                        | "superfast"
                        | "veryfast"
                        | "faster"
                        | "fast"
                        | "medium"
                        | "slow"
                        | "slower"
                        | "veryslow"
                )
            {
                return Err(MediaError::InvalidExport("unsupported x264 CRF or preset"));
            }
            let crf = crf.to_string();
            command
                .args(["-c:v", "libx264", "-preset"])
                .arg(preset)
                .arg("-crf")
                .arg(crf);
        }
        ExportProfile::Vaapi { quality, .. } => {
            if *quality > 51 {
                return Err(MediaError::InvalidExport("VAAPI quality must be in 0..=51"));
            }
            let hardware_filter = format!(
                "scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2,format=nv12,hwupload,fps={}",
                request.width, request.height, request.width, request.height, rate,
            );
            let quality = quality.to_string();
            command
                .arg("-vf")
                .arg(hardware_filter)
                .args(["-c:v", "h264_vaapi", "-qp"])
                .arg(quality);
        }
    }
    command.args([
        "-color_primaries",
        "bt709",
        "-color_trc",
        "bt709",
        "-colorspace",
        "bt709",
    ]);
    if matches!(&request.profile, ExportProfile::Quality { .. }) {
        command.args(["-pix_fmt", "yuv420p"]);
    }
    command
        .args([
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-ar",
            "48000",
            "-movflags",
            "+faststart",
        ])
        .arg(&request.output);
    Ok(command)
}

/// Runs a built export request, returning a detailed FFmpeg failure instead of hiding stderr.
pub fn export(request: &ExportRequest) -> Result<(), MediaError> {
    let output = build_export_command(request)?
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| MediaError::Spawn {
            program: "ffmpeg".to_owned(),
            source,
        })?;
    check_output(OsStr::new("ffmpeg"), &output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rational_frame_rates() {
        assert_eq!(
            Rational::parse("30000/1001"),
            Some(Rational {
                numerator: 30000,
                denominator: 1001
            })
        );
        assert_eq!(Rational::parse("0/0"), None);
        assert_eq!(Rational::parse("N/A"), None);
    }

    #[test]
    fn rotation_normalization_accepts_negative_quarter_turns() {
        assert_eq!(normalize_rotation(-90.0).unwrap(), 270);
        assert_eq!(normalize_rotation(180.0).unwrap(), 180);
        assert!(normalize_rotation(45.0).is_err());
    }

    #[test]
    fn export_uses_safe_paths_and_expected_h264_aac_options() {
        let request = ExportRequest {
            input: PathBuf::from("input file.mp4"),
            output: PathBuf::from("output file.mp4"),
            start_seconds: 1.25,
            duration_seconds: 3.0,
            width: 1920,
            height: 1080,
            frame_rate: Rational {
                numerator: 30,
                denominator: 1,
            },
            profile: ExportProfile::Quality {
                crf: 20,
                preset: "medium".to_owned(),
            },
        };
        let args: Vec<String> = build_export_command(&request)
            .expect("request is valid")
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"libx264".to_owned()));
        assert!(args.contains(&"aac".to_owned()));
        assert!(args.contains(&"+faststart".to_owned()));
        assert!(args.contains(&"input file.mp4".to_owned()));
        assert!(args.contains(&"output file.mp4".to_owned()));
    }
}
