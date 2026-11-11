// SPDX-License-Identifier: AGPL-3.0-or-later

//! Per-media thumbnails and waveforms, produced on one background worker.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
};

use editor_media::{MediaInfo, Waveform, compute_waveform, decode_thumbnail};
use eframe::egui;

enum Job {
    Thumbnail {
        media_id: u64,
        path: PathBuf,
        info: MediaInfo,
    },
    Waveform {
        media_id: u64,
        path: PathBuf,
    },
}

enum Done {
    Thumbnail(u64, Option<egui::ColorImage>),
    Waveform(u64, Option<Arc<Waveform>>),
}

pub struct AssetCache {
    jobs: Sender<Job>,
    results: Receiver<Done>,
    requested: HashSet<(u64, bool)>,
    pub thumbnails: HashMap<u64, egui::TextureHandle>,
    pub waveforms: HashMap<u64, Arc<Waveform>>,
}

impl AssetCache {
    pub fn new(ctx: egui::Context) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<Job>();
        let (done_tx, done_rx) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("d-cut-assets".into())
            .spawn(move || {
                while let Ok(job) = job_rx.recv() {
                    let done = match job {
                        Job::Thumbnail {
                            media_id,
                            path,
                            info,
                        } => {
                            let image = info.video.as_ref().and_then(|video| {
                                let at = (info.duration_seconds * 0.1).min(1.0);
                                match decode_thumbnail(&path, video, at, 160, 160) {
                                    Ok((width, height, rgba)) => {
                                        Some(egui::ColorImage::from_rgba_unmultiplied(
                                            [width as usize, height as usize],
                                            &rgba,
                                        ))
                                    }
                                    Err(error) => {
                                        tracing::warn!(%error, "thumbnail failed");
                                        None
                                    }
                                }
                            });
                            Done::Thumbnail(media_id, image)
                        }
                        Job::Waveform { media_id, path } => {
                            let waveform = compute_waveform(&path, 100, 60_000)
                                .map_err(|error| tracing::warn!(%error, "waveform failed"))
                                .ok()
                                .map(Arc::new);
                            Done::Waveform(media_id, waveform)
                        }
                    };
                    if done_tx.send(done).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        if let Err(error) = spawned {
            tracing::error!(%error, "could not start asset thread");
        }
        Self {
            jobs: job_tx,
            results: done_rx,
            requested: HashSet::new(),
            thumbnails: HashMap::new(),
            waveforms: HashMap::new(),
        }
    }

    pub fn request(&mut self, media_id: u64, path: PathBuf, info: &MediaInfo) {
        if info.video.is_some() && self.requested.insert((media_id, true)) {
            let _ = self.jobs.send(Job::Thumbnail {
                media_id,
                path: path.clone(),
                info: info.clone(),
            });
        }
        if info.audio.is_some() && self.requested.insert((media_id, false)) {
            let _ = self.jobs.send(Job::Waveform { media_id, path });
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        for done in self.results.try_iter() {
            match done {
                Done::Thumbnail(media_id, Some(image)) => {
                    let texture = ctx.load_texture(
                        format!("thumb-{media_id}"),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.thumbnails.insert(media_id, texture);
                }
                Done::Waveform(media_id, Some(waveform)) => {
                    self.waveforms.insert(media_id, waveform);
                }
                Done::Thumbnail(_, None) | Done::Waveform(_, None) => {}
            }
        }
    }

    pub fn clear(&mut self) {
        self.requested.clear();
        self.thumbnails.clear();
        self.waveforms.clear();
    }
}
