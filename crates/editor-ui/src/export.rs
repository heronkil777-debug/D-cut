// SPDX-License-Identifier: AGPL-3.0-or-later

//! Export dialog state and the background export job.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Instant,
};

use editor_media::{
    probe,
    timeline::{ExportEncoder, TimelineRender, export_timeline},
};
use eframe::egui;

pub const VAAPI_DEVICE: &str = "/dev/dri/renderD128";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Quality,
    Fast,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JobState {
    Running { fraction: f64 },
    Done { summary: String },
    Failed { error: String },
}

pub struct ExportJob {
    pub state: Arc<Mutex<JobState>>,
    cancel: Arc<AtomicBool>,
    pub output: PathBuf,
    pub started: Instant,
}

impl ExportJob {
    pub fn start(
        ctx: egui::Context,
        job: TimelineRender,
        encoder: ExportEncoder,
        output: PathBuf,
    ) -> Self {
        let state = Arc::new(Mutex::new(JobState::Running { fraction: 0.0 }));
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_state = Arc::clone(&state);
        let worker_cancel = Arc::clone(&cancel);
        let worker_output = output.clone();
        let spawned = thread::Builder::new()
            .name("d-cut-export".into())
            .spawn(move || {
                tracing::info!(output = %worker_output.display(), ?encoder, "export started");
                let progress_state = Arc::clone(&worker_state);
                let progress_ctx = ctx.clone();
                let result =
                    export_timeline(&job, &encoder, &worker_output, &worker_cancel, |progress| {
                        if let Ok(mut state) = progress_state.lock() {
                            *state = JobState::Running {
                                fraction: progress.fraction,
                            };
                        }
                        progress_ctx.request_repaint();
                    });
                let final_state = match result {
                    Ok(()) => {
                        let summary = summarize(&worker_output);
                        tracing::info!(%summary, "export finished");
                        JobState::Done { summary }
                    }
                    Err(error) => {
                        tracing::error!(%error, "export failed");
                        JobState::Failed {
                            error: error.to_string(),
                        }
                    }
                };
                if let Ok(mut state) = worker_state.lock() {
                    *state = final_state;
                }
                ctx.request_repaint();
            });
        if let Err(error) = spawned
            && let Ok(mut state) = state.lock()
        {
            *state = JobState::Failed {
                error: error.to_string(),
            };
        }
        Self {
            state,
            cancel,
            output,
            started: Instant::now(),
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> JobState {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or(JobState::Failed {
                error: "estado do export indisponível".into(),
            })
    }
}

/// Reads the exported file back with ffprobe so the user sees what was actually written.
fn summarize(path: &Path) -> String {
    let size = std::fs::metadata(path).map_or(0, |metadata| metadata.len());
    let size_text = format!("{:.1} MB", size as f64 / 1_000_000.0);
    match probe(path) {
        Ok(info) => {
            let video = info.video.map_or_else(
                || "sem vídeo".to_owned(),
                |video| {
                    let fps = video
                        .frame_rate
                        .map_or_else(|| "?".to_owned(), |rate| format!("{:.3}", rate.as_f64()));
                    format!("{} {}×{} {fps} fps", video.codec, video.width, video.height)
                },
            );
            let audio = info.audio.map_or_else(
                || "sem áudio".to_owned(),
                |audio| format!("{} {} Hz", audio.codec, audio.sample_rate.unwrap_or(0)),
            );
            format!(
                "{video} · {audio} · {:.2} s · {size_text}",
                info.duration_seconds
            )
        }
        Err(error) => format!("arquivo gravado ({size_text}), mas ffprobe falhou: {error}"),
    }
}

/// Settings edited in the export window.
pub struct ExportSettings {
    pub output: String,
    pub profile: Profile,
    pub crf: u8,
    pub preset: &'static str,
    pub qp: u8,
    pub vaapi_available: bool,
}

impl ExportSettings {
    pub fn new(default_output: PathBuf) -> Self {
        Self {
            output: default_output.display().to_string(),
            profile: Profile::Quality,
            crf: 20,
            preset: "fast",
            qp: 22,
            vaapi_available: Path::new(VAAPI_DEVICE).exists(),
        }
    }

    pub fn encoder(&self) -> ExportEncoder {
        match self.profile {
            Profile::Quality => ExportEncoder::X264 {
                crf: self.crf,
                preset: self.preset.to_owned(),
            },
            Profile::Fast => ExportEncoder::Vaapi {
                device: PathBuf::from(VAAPI_DEVICE),
                qp: self.qp,
            },
        }
    }
}
