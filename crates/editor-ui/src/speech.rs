// SPDX-License-Identifier: AGPL-3.0-or-later

//! Automatic subtitles window: opt-in model download and background transcription.

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use editor_core::Project;
use editor_media::timeline::{TimelineRender, export_audio_wav};
use editor_speech::{Cue, MODEL_SMALL, download_model, group_words, model_path, transcribe};
use eframe::egui;

#[derive(Debug, Clone)]
pub enum SpeechState {
    Idle,
    Downloading(f64),
    Transcribing,
    Done(Vec<Cue>),
    Failed(String),
}

pub struct SpeechJob {
    pub state: Arc<Mutex<SpeechState>>,
    cancel: Arc<AtomicBool>,
}

impl SpeechJob {
    fn spawn(
        ctx: egui::Context,
        work: impl FnOnce(&Arc<Mutex<SpeechState>>, &Arc<AtomicBool>) + Send + 'static,
    ) -> Self {
        let state = Arc::new(Mutex::new(SpeechState::Idle));
        let cancel = Arc::new(AtomicBool::new(false));
        let (worker_state, worker_cancel) = (Arc::clone(&state), Arc::clone(&cancel));
        let _ = thread::Builder::new()
            .name("d-cut-speech".into())
            .spawn(move || {
                work(&worker_state, &worker_cancel);
                ctx.request_repaint();
            });
        Self { state, cancel }
    }

    pub fn download(ctx: egui::Context) -> Self {
        let repaint = ctx.clone();
        Self::spawn(ctx, move |state, cancel| {
            let result = download_model(&MODEL_SMALL, cancel, |fraction| {
                set(state, SpeechState::Downloading(fraction));
                repaint.request_repaint();
            });
            set(
                state,
                match result {
                    Ok(_) => SpeechState::Idle,
                    Err(error) => SpeechState::Failed(error.to_string()),
                },
            );
        })
    }

    pub fn transcribe(ctx: egui::Context, project: Project) -> Self {
        Self::spawn(ctx, move |state, _| {
            set(state, SpeechState::Transcribing);
            let result = run_transcription(&project);
            set(
                state,
                match result {
                    Ok(cues) => SpeechState::Done(cues),
                    Err(error) => SpeechState::Failed(error),
                },
            );
        })
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> SpeechState {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or(SpeechState::Failed("estado indisponível".into()))
    }
}

fn set(state: &Arc<Mutex<SpeechState>>, value: SpeechState) {
    if let Ok(mut guard) = state.lock() {
        *guard = value;
    }
}

fn run_transcription(project: &Project) -> Result<Vec<Cue>, String> {
    let total = project.duration_frames();
    if total == 0 {
        return Err("a timeline está vazia".into());
    }
    let job = TimelineRender::from_project(project, 0, total, 2, 2).map_err(|e| e.to_string())?;
    let wav: PathBuf =
        std::env::temp_dir().join(format!("d-cut-speech-{}.wav", std::process::id()));
    export_audio_wav(&job, &wav, 16_000).map_err(|e| e.to_string())?;
    let threads = thread::available_parallelism().map_or(4, |count| count.get());
    let result = transcribe(&wav, &model_path(&MODEL_SMALL), "pt", threads);
    let _ = std::fs::remove_file(&wav);
    let words = result.map_err(|e| e.to_string())?;
    tracing::info!(words = words.len(), "transcription finished");
    Ok(group_words(&words, 42, 3500, 700))
}
