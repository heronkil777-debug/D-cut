// SPDX-License-Identifier: AGPL-3.0-or-later

//! Preview workers. Decoding and audio I/O run on their own threads; the UI thread only
//! polls channels. Audio is the master clock: the playhead is derived from samples that the
//! sound card actually consumed, and video frames are dropped to keep up with it.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use editor_media::timeline::{
    TimelineRender, render_frame, spawn_audio_stream, spawn_video_stream,
};
use eframe::egui;

/// Identifies what a scrub frame shows, so stale results can be recognized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameKey {
    pub frame: u64,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
}

pub struct ScrubResult {
    pub key: FrameKey,
    pub frame: Result<Vec<u8>, String>,
    /// Frame of the other clip when the playhead is inside a transition window.
    pub side: Option<Vec<u8>>,
}

/// Single-frame renderer for a paused playhead. Only the newest request is rendered.
pub struct ScrubDecoder {
    requests: Sender<(FrameKey, TimelineRender, Option<TimelineRender>)>,
    results: Receiver<ScrubResult>,
    pub last_requested: Option<FrameKey>,
}

impl ScrubDecoder {
    pub fn new(ctx: egui::Context) -> Self {
        let (request_tx, request_rx) =
            mpsc::channel::<(FrameKey, TimelineRender, Option<TimelineRender>)>();
        let (result_tx, result_rx) = mpsc::channel();
        thread::Builder::new()
            .name("d-cut-scrub".into())
            .spawn(move || {
                while let Ok(mut request) = request_rx.recv() {
                    // Skip to the newest request; intermediate scrub positions are irrelevant.
                    while let Ok(newer) = request_rx.try_recv() {
                        request = newer;
                    }
                    let (key, job, side_job) = request;
                    let frame = render_frame(&job).map_err(|error| error.to_string());
                    let side = side_job.and_then(|side| render_frame(&side).ok());
                    if result_tx.send(ScrubResult { key, frame, side }).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .map_err(|error| tracing::error!(%error, "could not start scrub thread"))
            .ok();
        Self {
            requests: request_tx,
            results: result_rx,
            last_requested: None,
        }
    }

    pub fn request(&mut self, key: FrameKey, job: TimelineRender, side: Option<TimelineRender>) {
        if self.last_requested == Some(key) {
            return;
        }
        self.last_requested = Some(key);
        let _ = self.requests.send((key, job, side));
    }

    pub fn poll(&self) -> Option<ScrubResult> {
        self.results.try_iter().last()
    }
}

/// Shared state between the audio decoder thread, the cpal callback and the UI.
struct AudioShared {
    queue: Mutex<VecDeque<f32>>,
    played_frames: AtomicU64,
    underruns: AtomicU64,
    finished: AtomicBool,
    started: AtomicBool,
    /// Nanoseconds since `epoch` at the end of the last callback that consumed samples.
    last_callback_nanos: AtomicU64,
    epoch: Instant,
}

/// An open sound-card stream fed by an FFmpeg timeline-mix process.
pub struct AudioOutput {
    stream: cpal::Stream,
    shared: Arc<AudioShared>,
    stop: Arc<AtomicBool>,
    pub sample_rate: u32,
    playing: bool,
}

impl AudioOutput {
    /// Opens the default output device and starts decoding (the stream starts paused).
    pub fn open(job: TimelineRender) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "nenhum dispositivo de áudio de saída".to_owned())?;
        let supported = device
            .default_output_config()
            .map_err(|error| format!("configuração de áudio indisponível: {error}"))?;
        let sample_rate = supported.sample_rate();
        let channels = usize::from(supported.channels().max(1));
        let config = supported.config();
        let shared = Arc::new(AudioShared {
            queue: Mutex::new(VecDeque::with_capacity(sample_rate as usize * 2)),
            played_frames: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            finished: AtomicBool::new(false),
            started: AtomicBool::new(false),
            last_callback_nanos: AtomicU64::new(0),
            epoch: Instant::now(),
        });
        let error_callback = |error| tracing::warn!(%error, "audio stream error");
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => {
                let shared = Arc::clone(&shared);
                device.build_output_stream(
                    config,
                    move |data: &mut [f32], _| fill(&shared, data, channels, |value| value),
                    error_callback,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let shared = Arc::clone(&shared);
                device.build_output_stream(
                    config,
                    move |data: &mut [i16], _| {
                        fill(&shared, data, channels, |value| {
                            (value.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
                        })
                    },
                    error_callback,
                    None,
                )
            }
            other => return Err(format!("formato de áudio não suportado: {other}")),
        }
        .map_err(|error| format!("não foi possível abrir o áudio: {error}"))?;

        let stop = Arc::new(AtomicBool::new(false));
        let decoder_shared = Arc::clone(&shared);
        let decoder_stop = Arc::clone(&stop);
        thread::Builder::new()
            .name("d-cut-audio-decode".into())
            .spawn(move || decode_audio(job, sample_rate, &decoder_shared, &decoder_stop))
            .map_err(|error| {
                format!("não foi possível iniciar o decodificador de áudio: {error}")
            })?;
        Ok(Self {
            stream,
            shared,
            stop,
            sample_rate,
            playing: false,
        })
    }

    pub fn buffered_seconds(&self) -> f64 {
        let queued = self.shared.queue.lock().map_or(0, |queue| queue.len());
        queued as f64 / 2.0 / f64::from(self.sample_rate)
    }

    pub fn is_ready(&self) -> bool {
        self.buffered_seconds() >= 0.15 || self.shared.finished.load(Ordering::Relaxed)
    }

    pub fn start(&mut self) -> Result<(), String> {
        if !self.playing {
            self.stream
                .play()
                .map_err(|error| format!("não foi possível tocar o áudio: {error}"))?;
            self.playing = true;
            self.shared.started.store(true, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Seconds of audio consumed by the device since start (the master clock). Between
    /// callbacks the clock is interpolated with wall time (capped at 50 ms) so it advances
    /// smoothly instead of in callback-sized steps.
    pub fn clock_seconds(&self) -> f64 {
        let consumed =
            self.shared.played_frames.load(Ordering::Relaxed) as f64 / f64::from(self.sample_rate);
        let last = self.shared.last_callback_nanos.load(Ordering::Relaxed);
        if consumed <= 0.0 || last == 0 {
            return consumed;
        }
        let now = self.shared.epoch.elapsed().as_nanos() as f64;
        let since = ((now - last as f64) / 1e9).clamp(0.0, 0.05);
        let starved =
            self.buffered_seconds() <= 0.0 && !self.shared.finished.load(Ordering::Relaxed);
        if starved { consumed } else { consumed + since }
    }

    pub fn underruns(&self) -> u64 {
        self.shared.underruns.load(Ordering::Relaxed)
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.stream.pause();
    }
}

fn fill<T: Copy + Default>(
    shared: &AudioShared,
    data: &mut [T],
    channels: usize,
    convert: impl Fn(f32) -> T,
) {
    let mut played = 0_u64;
    let mut starved = false;
    if let Ok(mut queue) = shared.queue.lock() {
        for frame in data.chunks_mut(channels) {
            if queue.len() >= 2 {
                let left = queue.pop_front().unwrap_or(0.0);
                let right = queue.pop_front().unwrap_or(0.0);
                if channels == 1 {
                    frame[0] = convert((left + right) * 0.5);
                } else {
                    frame[0] = convert(left);
                    frame[1] = convert(right);
                    for sample in &mut frame[2..] {
                        *sample = T::default();
                    }
                }
                played += 1;
            } else {
                frame.fill(T::default());
                starved = true;
            }
        }
    } else {
        data.fill(T::default());
    }
    shared.played_frames.fetch_add(played, Ordering::Relaxed);
    if played > 0 {
        let nanos = u64::try_from(shared.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
        shared
            .last_callback_nanos
            .store(nanos.max(1), Ordering::Relaxed);
    }
    if starved && shared.started.load(Ordering::Relaxed) && !shared.finished.load(Ordering::Relaxed)
    {
        shared.underruns.fetch_add(1, Ordering::Relaxed);
    }
}

fn decode_audio(job: TimelineRender, sample_rate: u32, shared: &AudioShared, stop: &AtomicBool) {
    let mut stream = match spawn_audio_stream(&job, sample_rate) {
        Ok(stream) => stream,
        Err(error) => {
            tracing::error!(%error, "audio preview failed to start");
            shared.finished.store(true, Ordering::Relaxed);
            return;
        }
    };
    let high_water = sample_rate as usize * 2; // one second of stereo samples
    let mut buffer = vec![0.0_f32; 4096];
    while !stop.load(Ordering::Relaxed) {
        let queued = shared.queue.lock().map_or(0, |queue| queue.len());
        if queued > high_water {
            thread::sleep(Duration::from_millis(5));
            continue;
        }
        match stream.read_samples(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                if let Ok(mut queue) = shared.queue.lock() {
                    queue.extend(&buffer[..count]);
                }
            }
            Err(error) => {
                tracing::error!(%error, "audio preview decode error");
                break;
            }
        }
    }
    shared.finished.store(true, Ordering::Relaxed);
}

fn resident_mb() -> f64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|statm| statm.split_whitespace().nth(1)?.parse::<f64>().ok())
        .map_or(0.0, |pages| pages * 4096.0 / 1_048_576.0)
}

/// Decodes a timeline stream on its own thread; frames are tagged with timeline indices.
fn spawn_decoder(
    ctx: egui::Context,
    job: TimelineRender,
    stop: Arc<AtomicBool>,
    name: &str,
) -> Receiver<(u64, Vec<u8>)> {
    let (frame_tx, frame_rx): (SyncSender<(u64, Vec<u8>)>, _) = mpsc::sync_channel(4);
    let spawned = thread::Builder::new().name(name.into()).spawn(move || {
        let mut stream = match spawn_video_stream(&job) {
            Ok(stream) => stream,
            Err(error) => {
                tracing::error!(%error, "preview decoder failed to start");
                return;
            }
        };
        let mut frame_index = job.start_frame;
        while !stop.load(Ordering::Relaxed) {
            match stream.next_frame() {
                Ok(Some(data)) => {
                    if frame_tx.send((frame_index, data)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                    frame_index += 1;
                }
                Ok(None) => break,
                Err(error) => {
                    tracing::error!(%error, "preview decode error");
                    break;
                }
            }
        }
    });
    if let Err(error) = spawned {
        tracing::error!(%error, "could not start decoder thread");
    }
    frame_rx
}

/// Statistics shown by the debug HUD and logged at the end of playback.
#[derive(Debug, Clone, Copy)]
pub struct PlaybackStats {
    pub shown: u64,
    pub dropped: u64,
    pub underruns: u64,
    /// Video frame timestamp minus master clock when the frame was shown, in ms.
    pub last_offset_ms: f64,
    pub max_abs_offset_ms: f64,
    pub sum_offset_ms: f64,
    pub audio_buffer_ms: f64,
    /// Mean offset over the first 10 s of playback, and over the most recent 300 frames.
    first_window: (f64, u64),
    recent: [f64; 300],
    recent_len: usize,
    recent_next: usize,
}

impl PlaybackStats {
    fn record(&mut self, offset_ms: f64, played_seconds: f64) {
        if played_seconds < 10.0 {
            self.first_window.0 += offset_ms;
            self.first_window.1 += 1;
        }
        self.recent[self.recent_next] = offset_ms;
        self.recent_next = (self.recent_next + 1) % self.recent.len();
        self.recent_len = (self.recent_len + 1).min(self.recent.len());
    }

    /// A/V drift: change of the mean offset between the first 10 s and the last ~10 s.
    /// The per-frame offset itself is bounded by one frame period (frame quantization).
    pub fn drift_ms(&self) -> f64 {
        if self.first_window.1 == 0 || self.recent_len == 0 {
            return 0.0;
        }
        let first = self.first_window.0 / self.first_window.1 as f64;
        let recent = self.recent[..self.recent_len].iter().sum::<f64>() / self.recent_len as f64;
        (recent - first).abs()
    }
}

impl Default for PlaybackStats {
    fn default() -> Self {
        Self {
            shown: 0,
            dropped: 0,
            underruns: 0,
            last_offset_ms: 0.0,
            max_abs_offset_ms: 0.0,
            sum_offset_ms: 0.0,
            audio_buffer_ms: 0.0,
            first_window: (0.0, 0),
            recent: [0.0; 300],
            recent_len: 0,
            recent_next: 0,
        }
    }
}

/// Continuous playback from `start_frame`.
pub struct Playback {
    pub start_frame: u64,
    pub end_frame: u64,
    fps: f64,
    frames: Receiver<(u64, Vec<u8>)>,
    side_frames: Option<Receiver<(u64, Vec<u8>)>>,
    side_pending: Option<(u64, Vec<u8>)>,
    stop: Arc<AtomicBool>,
    pending: Option<(u64, Vec<u8>)>,
    pub audio: Option<AudioOutput>,
    pub audio_error: Option<String>,
    wall_start: Option<Instant>,
    pub stats: PlaybackStats,
    video_finished: bool,
    started_at: Instant,
}

pub struct PlaybackTick {
    /// Master-clock position (timeline frame).
    pub clock_frame: u64,
    /// New frame to display, if any.
    pub frame: Option<(u64, Vec<u8>)>,
    /// Matching frame of the transition side stream, if any.
    pub side: Option<Vec<u8>>,
    pub finished: bool,
}

impl Playback {
    pub fn start(
        ctx: egui::Context,
        video_job: TimelineRender,
        side_job: Option<TimelineRender>,
        audio_job: Option<TimelineRender>,
        fps: f64,
    ) -> Self {
        let start_frame = video_job.start_frame;
        let end_frame = video_job.start_frame + video_job.total_frames;
        let stop = Arc::new(AtomicBool::new(false));
        let frame_rx = spawn_decoder(
            ctx.clone(),
            video_job,
            Arc::clone(&stop),
            "d-cut-video-decode",
        );
        let side_frames = side_job
            .map(|job| spawn_decoder(ctx.clone(), job, Arc::clone(&stop), "d-cut-side-decode"));
        let (audio, audio_error) = match audio_job.map(AudioOutput::open) {
            Some(Ok(audio)) => (Some(audio), None),
            Some(Err(error)) => {
                tracing::warn!(%error, "audio playback unavailable; using wall clock");
                (None, Some(error))
            }
            None => (None, None),
        };
        Self {
            start_frame,
            end_frame,
            fps,
            frames: frame_rx,
            side_frames,
            side_pending: None,
            stop,
            pending: None,
            audio,
            audio_error,
            wall_start: None,
            stats: PlaybackStats::default(),
            video_finished: false,
            started_at: Instant::now(),
        }
    }

    fn clock_seconds(&self) -> f64 {
        match (&self.audio, self.wall_start) {
            (Some(audio), _) => audio.clock_seconds(),
            (None, Some(start)) => start.elapsed().as_secs_f64(),
            (None, None) => 0.0,
        }
    }

    fn fetch(&mut self) -> Option<(u64, Vec<u8>)> {
        if let Some(frame) = self.pending.take() {
            return Some(frame);
        }
        match self.frames.try_recv() {
            Ok(frame) => Some(frame),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.video_finished = true;
                None
            }
        }
    }

    /// Advances playback; call once per UI frame.
    pub fn tick(&mut self) -> PlaybackTick {
        // Pre-roll: wait for the first video frame and some buffered audio before the clock runs.
        let waiting_for_start =
            self.wall_start.is_none() && self.audio.as_ref().is_none_or(|audio| !audio.playing);
        if waiting_for_start {
            if self.pending.is_none() {
                self.pending = self.fetch();
            }
            let video_ready = self.pending.is_some() || self.video_finished;
            let audio_ready = self.audio.as_ref().is_none_or(AudioOutput::is_ready);
            let timed_out = self.started_at.elapsed() > Duration::from_secs(5);
            if (video_ready && audio_ready) || timed_out {
                match self.audio.as_mut().map(AudioOutput::start) {
                    Some(Err(error)) => {
                        tracing::warn!(%error, "audio start failed; using wall clock");
                        self.audio_error = Some(error);
                        self.audio = None;
                        self.wall_start = Some(Instant::now());
                    }
                    Some(Ok(())) => {}
                    None => self.wall_start = Some(Instant::now()),
                }
            }
        }
        let clock_seconds = self.clock_seconds();
        let clock_frame = self.start_frame + (clock_seconds * self.fps).floor() as u64;
        let mut newest = None;
        while let Some(frame) = self.fetch() {
            if frame.0 <= clock_frame {
                if newest.is_some() {
                    self.stats.dropped += 1;
                }
                newest = Some(frame);
            } else {
                self.pending = Some(frame);
                break;
            }
        }
        if let Some((frame_index, _)) = &newest {
            let frame_seconds = (*frame_index - self.start_frame) as f64 / self.fps;
            let offset_ms = (frame_seconds - clock_seconds) * 1000.0;
            self.stats.shown += 1;
            self.stats.last_offset_ms = offset_ms;
            self.stats.sum_offset_ms += offset_ms;
            self.stats.max_abs_offset_ms = self.stats.max_abs_offset_ms.max(offset_ms.abs());
            self.stats.record(offset_ms, clock_seconds);
        }
        if let Some(audio) = &self.audio {
            self.stats.underruns = audio.underruns();
            self.stats.audio_buffer_ms = audio.buffered_seconds() * 1000.0;
        }
        let side = newest.as_ref().and_then(|(index, _)| self.side_for(*index));
        let finished = clock_frame >= self.end_frame
            || (self.video_finished && self.pending.is_none() && self.audio.is_none());
        PlaybackTick {
            clock_frame: clock_frame.min(self.end_frame),
            frame: newest,
            side,
            finished,
        }
    }

    /// Side-stream frame with the same timeline index (older ones are discarded).
    fn side_for(&mut self, index: u64) -> Option<Vec<u8>> {
        let receiver = self.side_frames.as_ref()?;
        loop {
            let candidate = match self.side_pending.take() {
                Some(frame) => frame,
                None => receiver.try_recv().ok()?,
            };
            match candidate.0.cmp(&index) {
                std::cmp::Ordering::Less => {}
                std::cmp::Ordering::Equal => return Some(candidate.1),
                std::cmp::Ordering::Greater => {
                    self.side_pending = Some(candidate);
                    return None;
                }
            }
        }
    }

    pub fn using_audio_clock(&self) -> bool {
        self.audio.is_some()
    }

    pub fn log_summary(&self) {
        let played = self.clock_seconds();
        let mean = if self.stats.shown > 0 {
            self.stats.sum_offset_ms / self.stats.shown as f64
        } else {
            0.0
        };
        tracing::info!(
            played_seconds = played,
            audio_clock = self.using_audio_clock(),
            frames_shown = self.stats.shown,
            frames_dropped = self.stats.dropped,
            audio_underruns = self.stats.underruns,
            mean_offset_ms = mean,
            max_abs_offset_ms = self.stats.max_abs_offset_ms,
            drift_ms = self.stats.drift_ms(),
            rss_mb = resident_mb(),
            "playback stopped"
        );
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
