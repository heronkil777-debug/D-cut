// SPDX-License-Identifier: AGPL-3.0-or-later

//! Final export. Projects without text use the FFmpeg-only timeline graph; projects with
//! titles/subtitles (or when forced) decode the same graph to RGBA, composite text on the GPU
//! with `editor-render` (the shader used by the preview) and encode the result.

use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use editor_core::Project;
use editor_media::{
    MediaError,
    timeline::{
        ExportEncoder, ExportProgress, TimelineRender, export_timeline, spawn_rgba_encoder,
        spawn_video_stream,
    },
};
use editor_render::{OffscreenCompositor, RenderError, headless_device, wgpu};
use editor_text::{TextRenderer, active_overlays};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error("the timeline is empty")]
    Empty,
    #[error("decoder ended early at frame {frame}: {stderr}")]
    ShortDecode { frame: u64, stderr: String },
}

/// Which pipeline produced the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportPath {
    FfmpegOnly,
    GpuComposite,
}

#[derive(Debug, Clone, Copy)]
pub struct ExportReport {
    pub path: ExportPath,
    pub frames: u64,
    pub seconds: f64,
}

pub struct ExportOptions {
    pub encoder: ExportEncoder,
    /// Use the GPU composition path even without text (for comparisons).
    pub force_gpu: bool,
    /// Device to reuse (the UI passes eframe's); `None` creates a headless device.
    pub device: Option<(wgpu::Device, wgpu::Queue)>,
}

pub fn export_project(
    project: &Project,
    output: &Path,
    options: ExportOptions,
    cancel: &Arc<AtomicBool>,
    mut progress: impl FnMut(ExportProgress),
) -> Result<ExportReport, ExportError> {
    let total = project.duration_frames();
    if total == 0 {
        return Err(ExportError::Empty);
    }
    let (width, height) = (project.format.width, project.format.height);
    let job = TimelineRender::from_project(project, 0, total, width, height)?;
    let started = Instant::now();
    if project.text_overlays.is_empty() && !options.force_gpu {
        export_timeline(&job, &options.encoder, output, cancel, progress)?;
        return Ok(ExportReport {
            path: ExportPath::FfmpegOnly,
            frames: total,
            seconds: started.elapsed().as_secs_f64(),
        });
    }
    let (device, queue) = match options.device {
        Some(pair) => pair,
        None => headless_device()?,
    };
    let mut compositor = OffscreenCompositor::new(device, queue, width, height);
    let mut text = TextRenderer::new();
    let mut decoder = spawn_video_stream(&job)?;
    let mut encoder = spawn_rgba_encoder(&job, &options.encoder, output)?;
    let mut cached: Option<(Vec<u64>, Option<Vec<u8>>)> = None;
    for frame in 0..total {
        if cancel.load(Ordering::Relaxed) {
            drop(encoder);
            let _ = std::fs::remove_file(output);
            return Err(MediaError::Cancelled.into());
        }
        let Some(base) = decoder.next_frame()? else {
            return Err(ExportError::ShortDecode {
                frame,
                stderr: decoder.stderr_tail(),
            });
        };
        let active: Vec<u64> = active_overlays(project, frame)
            .iter()
            .map(|overlay| overlay.id)
            .collect();
        if cached.as_ref().is_none_or(|(ids, _)| *ids != active) {
            let layer = text.render_frame(project, frame, width, height);
            cached = Some((active, layer));
        }
        let overlay = cached.as_ref().and_then(|(_, layer)| layer.as_deref());
        let composed = compositor.compose(&base, overlay)?;
        encoder.write_frame(&composed)?;
        if frame % 5 == 0 {
            let seconds = project.format.frames_to_seconds(frame);
            progress(ExportProgress {
                fraction: frame as f64 / total as f64,
                encoded_seconds: seconds,
            });
        }
    }
    encoder.finish()?;
    progress(ExportProgress {
        fraction: 1.0,
        encoded_seconds: project.format.frames_to_seconds(total),
    });
    tracing::info!(
        frames = total,
        seconds = started.elapsed().as_secs_f64(),
        "GPU export finished"
    );
    Ok(ExportReport {
        path: ExportPath::GpuComposite,
        frames: total,
        seconds: started.elapsed().as_secs_f64(),
    })
}
