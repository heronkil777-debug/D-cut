// SPDX-License-Identifier: AGPL-3.0-or-later

//! Desktop editor shell. Media loading and playback are delegated to worker-backed services.

use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use anyhow::{Context, Result};
use editor_core::{
    Clip, CommandHistory, EditCommand, MediaAsset, Project, Track, TrackKind, PRODUCT_NAME,
    split_clip_command,
};
use editor_media::{MediaInfo, probe};
use editor_render::QuadRenderer;
use eframe::egui;

const BG: egui::Color32 = egui::Color32::from_rgb(16, 18, 24);
const PANEL: egui::Color32 = egui::Color32::from_rgb(25, 28, 37);
const PANEL_ALT: egui::Color32 = egui::Color32::from_rgb(32, 36, 47);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(119, 94, 245);
const TEAL: egui::Color32 = egui::Color32::from_rgb(43, 195, 185);

#[derive(Debug, Clone)]
struct ImportedMedia {
    id: u64,
    path: PathBuf,
    duration_seconds: f64,
    has_video: bool,
    has_audio: bool,
    summary: String,
}

struct DcutApp {
    quad_renderer: Option<QuadRenderer>,
    is_playing: bool,
    playhead_seconds: f32,
    zoom: f32,
    selected_tool: &'static str,
    status: String,
    media: Vec<ImportedMedia>,
    selected_media: Option<u64>,
    selected_clip: Option<(u64, u64)>,
    project: Project,
    project_path: String,
    history: CommandHistory,
    next_id: u64,
    pending_media: HashSet<PathBuf>,
    import_sender: Sender<(PathBuf, Result<MediaInfo, String>)>,
    import_receiver: Receiver<(PathBuf, Result<MediaInfo, String>)>,
}

impl DcutApp {
    fn new(context: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG;
        visuals.window_fill = PANEL;
        visuals.widgets.active.bg_fill = ACCENT;
        visuals.selection.bg_fill = ACCENT;
        context.egui_ctx.set_visuals(visuals);
        let quad_renderer = context.wgpu_render_state.as_ref().map(QuadRenderer::new);
        let status = if quad_renderer.is_some() {
            "Renderer ready · drop video or audio files to import".to_owned()
        } else {
            tracing::error!("eframe did not provide its wgpu render state");
            "Shared wgpu renderer unavailable".to_owned()
        };
        let (import_sender, import_receiver) = mpsc::channel();
        let mut project = Project::default();
        project.tracks = vec![
            Track { id: 1, name: "V1 · Video".to_owned(), kind: TrackKind::Video, clips: Vec::new() },
            Track { id: 2, name: "A1 · Audio".to_owned(), kind: TrackKind::Audio, clips: Vec::new() },
        ];
        let project_path = std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join("Videos/untitled.dcut").display().to_string())
            .unwrap_or_else(|| "untitled.dcut".to_owned());
        Self {
            quad_renderer,
            is_playing: false,
            playhead_seconds: 0.0,
            zoom: 1.0,
            selected_tool: "Select",
            status,
            media: Vec::new(),
            selected_media: None,
            selected_clip: None,
            project,
            project_path,
            history: CommandHistory::default(),
            next_id: 3,
            pending_media: HashSet::new(),
            import_sender,
            import_receiver,
        }
    }

    fn receive_dropped_files(&mut self, context: &egui::Context) {
        let dropped = context.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect::<Vec<_>>()
        });
        for path in dropped {
            self.queue_media_probe(path, context);
        }

        for (path, result) in self.import_receiver.try_iter() {
            self.pending_media.remove(&path);
            match result {
                Ok(info) => {
                    let has_video = info.video.is_some();
                    let has_audio = info.audio.is_some();
                    let description = match (&info.video, &info.audio) {
                        (Some(video), Some(audio)) => format!(
                            "{}×{} · {} · {} Hz audio",
                            video.width,
                            video.height,
                            video.codec,
                            audio.sample_rate.unwrap_or(0)
                        ),
                        (Some(video), None) => {
                            format!("{}×{} · {}", video.width, video.height, video.codec)
                        }
                        (None, Some(audio)) => format!(
                            "{} Hz audio · {}",
                            audio.sample_rate.unwrap_or(0),
                            audio.codec
                        ),
                        (None, None) => "Unknown media stream".to_owned(),
                    };
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    let id = self.next_id;
                    self.next_id = self.next_id.saturating_add(1);
                    let fps = self.project.format.frames_per_second();
                    let duration_frames = (info.duration_seconds * fps).ceil().max(1.0) as u64;
                    let (width, height, rotation_degrees) = info.video.as_ref().map_or(
                        (self.project.format.width, self.project.format.height, 0),
                        |video| (video.width, video.height, video.rotation_degrees),
                    );
                    self.project.media.push(MediaAsset {
                        id,
                        path: path.to_string_lossy().into_owned(),
                        content_hash: None,
                        duration_frames,
                        width,
                        height,
                        has_video,
                        has_audio,
                        rotation_degrees,
                    });
                    self.status = format!("Imported {name} · use Add to timeline to edit");
                    self.selected_media = Some(id);
                    self.media.push(ImportedMedia {
                        id,
                        path,
                        duration_seconds: info.duration_seconds,
                        has_video,
                        has_audio,
                        summary: description,
                    });
                }
                Err(error) => {
                    self.status = format!("Could not import {}: {error}", path.display());
                    tracing::warn!(path = %path.display(), %error, "media import failed");
                }
            }
        }
    }

    fn queue_media_probe(&mut self, path: PathBuf, context: &egui::Context) {
        if self.media.iter().any(|media| media.path == path)
            || !self.pending_media.insert(path.clone())
        {
            return;
        }
        let sender = self.import_sender.clone();
        let context = context.clone();
        let worker_path = path.clone();
        thread::spawn(move || {
            let result = probe(&worker_path).map_err(|error| error.to_string());
            let _ = sender.send((worker_path, result));
            context.request_repaint();
        });
        self.status = format!("Reading media metadata for {}…", path.display());
    }

    fn add_selected_media_to_timeline(&mut self) {
        let Some(media_id) = self.selected_media else {
            self.status = "Select a media item in the library first".to_owned();
            return;
        };
        let Some(asset) = self.project.media.iter().find(|item| item.id == media_id) else {
            self.status = "Selected media is no longer in this project".to_owned();
            return;
        };
        let duration_frames = asset.duration_frames;
        let has_video = asset.has_video;
        let has_audio = asset.has_audio;
        let track_ids: Vec<u64> = self.project.tracks.iter()
            .filter(|track| (track.kind == TrackKind::Video && has_video)
                || (track.kind == TrackKind::Audio && has_audio))
            .map(|track| track.id)
            .collect();
        if track_ids.is_empty() {
            self.status = "No compatible video/audio track is available".to_owned();
            return;
        }
        for track_id in track_ids {
            let Some(track) = self.project.tracks.iter().find(|track| track.id == track_id) else { continue };
            let start = track.clips.iter().filter_map(Clip::timeline_end_frame).max().unwrap_or(0);
            let mut replacement = track.clips.clone();
            let clip_id = self.next_id;
            self.next_id = self.next_id.saturating_add(1);
            replacement.push(Clip {
                id: clip_id,
                media_id,
                timeline_start_frame: start,
                source_start_frame: 0,
                duration_frames,
            });
            let command = EditCommand::ReplaceTrackClips {
                track_id,
                expected: track.clips.clone(),
                replacement,
            };
            if let Err(error) = self.history.execute(&mut self.project, command) {
                self.status = format!("Could not add clip: {error}");
                return;
            }
            self.selected_clip = Some((track_id, clip_id));
        }
        self.status = "Clip added at the end of its media track(s)".to_owned();
    }

    fn split_selected_clip(&mut self) {
        let Some((track_id, clip_id)) = self.selected_clip else {
            self.status = "Select a timeline clip, then position the playhead to split".to_owned();
            return;
        };
        let frame = (f64::from(self.playhead_seconds) * self.project.format.frames_per_second()).round() as u64;
        let command = split_clip_command(&self.project, track_id, clip_id, frame, self.next_id);
        match command.and_then(|command| self.history.execute(&mut self.project, command)) {
            Ok(()) => {
                self.next_id = self.next_id.saturating_add(1);
                self.status = format!("Split clip at frame {frame}");
            }
            Err(error) => self.status = format!("Split failed: {error}"),
        }
    }

    fn delete_selected_clip(&mut self) {
        let Some((track_id, clip_id)) = self.selected_clip else {
            self.status = "Select a timeline clip to delete".to_owned();
            return;
        };
        let Some(track) = self.project.tracks.iter().find(|track| track.id == track_id) else { return };
        let replacement = track.clips.iter().filter(|clip| clip.id != clip_id).cloned().collect();
        let command = EditCommand::ReplaceTrackClips {
            track_id,
            expected: track.clips.clone(),
            replacement,
        };
        match self.history.execute(&mut self.project, command) {
            Ok(()) => {
                self.selected_clip = None;
                self.status = "Deleted selected clip (gap retained)".to_owned();
            }
            Err(error) => self.status = format!("Delete failed: {error}"),
        }
    }

    fn save_project(&mut self) {
        let path = expand_project_path(&self.project_path);
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            if let Err(error) = fs::create_dir_all(parent) {
                self.status = format!("Could not create project folder: {error}");
                return;
            }
        }
        match editor_project::save(&path, &self.project) {
            Ok(()) => self.status = format!("Saved project to {}", path.display()),
            Err(error) => self.status = format!("Could not save project: {error}"),
        }
    }

    fn open_project(&mut self) {
        let path = expand_project_path(&self.project_path);
        match editor_project::load(&path) {
            Ok(project) => {
                self.next_id = project.media.iter().map(|item| item.id)
                    .chain(project.tracks.iter().flat_map(|track| std::iter::once(track.id).chain(track.clips.iter().map(|clip| clip.id))))
                    .chain(project.text_overlays.iter().map(|item| item.id))
                    .chain(project.transitions.iter().map(|item| item.id))
                    .max().unwrap_or(0).saturating_add(1);
                self.media = project.media.iter().map(|asset| ImportedMedia {
                    id: asset.id,
                    path: PathBuf::from(&asset.path),
                    duration_seconds: asset.duration_frames as f64 / project.format.frames_per_second(),
                    has_video: asset.has_video,
                    has_audio: asset.has_audio,
                    summary: format!("{}×{} · saved project media", asset.width, asset.height),
                }).collect();
                self.project = project;
                self.selected_media = None;
                self.selected_clip = None;
                self.playhead_seconds = 0.0;
                self.history = CommandHistory::default();
                self.status = format!("Opened {}", path.display());
            }
            Err(error) => self.status = format!("Could not open project: {error}"),
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("main-toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(PRODUCT_NAME);
                ui.separator();
                for tool in ["Select", "Split"] {
                    if ui.selectable_label(self.selected_tool == tool, tool).clicked() {
                        self.selected_tool = tool;
                        if tool == "Split" { self.split_selected_clip(); }
                    }
                }
                if ui.button("Delete clip").clicked() { self.delete_selected_clip(); }
                if ui.add_enabled(self.history.can_undo(), egui::Button::new("Undo")).clicked() {
                    match self.history.undo(&mut self.project) {
                        Ok(true) => self.status = "Undo complete".to_owned(),
                        Ok(false) => {},
                        Err(error) => self.status = format!("Undo failed: {error}"),
                    }
                }
                if ui.add_enabled(self.history.can_redo(), egui::Button::new("Redo")).clicked() {
                    match self.history.redo(&mut self.project) {
                        Ok(true) => self.status = "Redo complete".to_owned(),
                        Ok(false) => {},
                        Err(error) => self.status = format!("Redo failed: {error}"),
                    }
                }
                ui.separator();
                if ui.button("Import media").clicked() {
                    self.status =
                        "Drag video or audio files from your file manager into this window".to_owned();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
                    if ui.button("Export").clicked() {
                        self.status = "Timeline export is not implemented yet; this project can be saved and edited".to_owned();
                    }
                    if ui.button("Open").clicked() { self.open_project(); }
                    if ui.button("Save").clicked() { self.save_project(); }
                });
            });
        });
    }

    fn media_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("media-library")
            .default_size(220.0)
            .show(ui, |ui| {
                ui.heading("Media library");
                ui.add_space(8.0);
                if self.media.is_empty() {
                    ui.label("No media imported");
                    ui.add_space(6.0);
                    ui.small("Drag video or audio files from your file manager into this window.");
                } else {
                    ui.label(format!("{} item(s)", self.media.len()));
                    ui.add_space(6.0);
                    let mut clicked_media = None;
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for media in &self.media {
                            let name = media.path.file_name()
                                .map(|name| name.to_string_lossy())
                                .unwrap_or_else(|| media.path.as_os_str().to_string_lossy());
                            let response = ui.selectable_label(self.selected_media == Some(media.id), name.as_ref());
                            if response.clicked() { clicked_media = Some(media.id); }
                            ui.indent(media.id, |ui| {
                                ui.small(&media.summary);
                                ui.small(format!("{:.2} s", media.duration_seconds));
                            });
                        }
                    });
                    if clicked_media.is_some() { self.selected_media = clicked_media; }
                }
                ui.add_space(8.0);
                if ui.add_enabled(self.selected_media.is_some(), egui::Button::new("＋  Add to timeline")).clicked() {
                    self.add_selected_media_to_timeline();
                }
                if ui.button("＋  Import media").clicked() {
                    self.status = "Drag video or audio files from your file manager into this window".to_owned();
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.separator();
                    ui.small("Local project · no telemetry");
                });
            });
    }

    fn inspector_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("inspector")
            .default_size(240.0)
            .show(ui, |ui| {
                ui.heading("Inspector");
                ui.add_space(12.0);
                ui.label("Project file path");
                ui.text_edit_singleline(&mut self.project_path);
                ui.small("Use Open / Save in the toolbar. ~ is supported.");
                ui.separator();
                if let Some((track_id, clip_id)) = self.selected_clip {
                    ui.strong("Selected clip");
                    ui.small(format!("Track {track_id} · Clip {clip_id}"));
                    ui.small("Delete keeps the gap; Undo restores edits.");
                } else if let Some(media_id) = self.selected_media {
                    ui.strong("Selected library item");
                    ui.small(format!("Media {media_id}"));
                } else {
                    ui.label("Nothing selected");
                }
                ui.separator();
                ui.label("Project format");
                ui.horizontal(|ui| {
                    ui.label(format!("{} × {}", self.project.format.width, self.project.format.height));
                    ui.label(format!("{:.3} fps", self.project.format.frames_per_second()));
                });
                ui.add_space(12.0);
                ui.label("Preview quality");
                ui.horizontal(|ui| {
                    let _ = ui.selectable_label(true, "Full");
                    let _ = ui.selectable_label(false, "½");
                    let _ = ui.selectable_label(false, "¼");
                });
            });
    }

    fn timeline_panel(&mut self, ui: &mut egui::Ui) {
        let fps = self.project.format.frames_per_second().max(1.0);
        let tracks = self.project.tracks.clone();
        let media = self.media.clone();
        let total_frames = tracks.iter().flat_map(|track| track.clips.iter())
            .filter_map(Clip::timeline_end_frame).max().unwrap_or(0);
        let max_seconds = (total_frames as f32 / fps as f32).max(1.0);
        let mut clicked_clip = None;
        egui::Panel::bottom("timeline")
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Timeline");
                    ui.separator();
                    if ui.button(if self.is_playing { "Pause" } else { "Play" }).clicked() {
                        self.is_playing = !self.is_playing;
                    }
                    ui.label(format!("{:02}:{:02}.{:03}",
                        (self.playhead_seconds / 60.0) as u32,
                        (self.playhead_seconds as u32) % 60,
                        (self.playhead_seconds.fract() * 1000.0) as u32));
                    ui.add(egui::Slider::new(&mut self.playhead_seconds, 0.0..=max_seconds)
                        .show_value(false).text("Playhead"));
                    if ui.button("−").clicked() { self.zoom = (self.zoom * 0.8).max(0.25); }
                    ui.add(egui::Slider::new(&mut self.zoom, 0.25..=4.0).show_value(false));
                    if ui.button("＋").clicked() { self.zoom = (self.zoom * 1.25).min(4.0); }
                });
                ui.add_space(6.0);
                egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                    for track in &tracks {
                        let color = match track.kind {
                            TrackKind::Video => ACCENT,
                            TrackKind::Audio => egui::Color32::from_rgb(218, 145, 61),
                            TrackKind::Subtitle => TEAL,
                        };
                        let width = (max_seconds * 80.0 * self.zoom).max(500.0);
                        ui.horizontal(|ui| {
                            ui.add_sized([92.0, 38.0], egui::Label::new(&track.name));
                            let (lane, _) = ui.allocate_exact_size(egui::vec2(width, 38.0), egui::Sense::hover());
                            ui.painter().rect_filled(lane, egui::CornerRadius::ZERO, PANEL);
                            ui.painter().line_segment([lane.left_top(), lane.right_top()], egui::Stroke::new(1.0, PANEL_ALT));
                            for clip in &track.clips {
                                let x = lane.left() + clip.timeline_start_frame as f32 / fps as f32 * 80.0 * self.zoom;
                                let clip_width = (clip.duration_frames as f32 / fps as f32 * 80.0 * self.zoom).max(16.0);
                                let rect = egui::Rect::from_min_size(egui::pos2(x, lane.top() + 4.0), egui::vec2(clip_width, 30.0));
                                let selected = self.selected_clip == Some((track.id, clip.id));
                                ui.painter().rect_filled(rect, egui::CornerRadius::same(4), if selected { TEAL } else { color });
                                let title = media.iter().find(|item| item.id == clip.media_id)
                                    .and_then(|item| item.path.file_name())
                                    .map(|name| name.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| format!("Media {}", clip.media_id));
                                ui.painter().text(rect.left_center() + egui::vec2(7.0, 0.0), egui::Align2::LEFT_CENTER,
                                    title, egui::FontId::proportional(11.0), egui::Color32::WHITE);
                                let response = ui.interact(rect, egui::Id::new(("timeline-clip", track.id, clip.id)), egui::Sense::click());
                                if response.clicked() { clicked_clip = Some((track.id, clip.id)); }
                            }
                            let playhead_x = lane.left() + self.playhead_seconds * 80.0 * self.zoom;
                            ui.painter().line_segment([egui::pos2(playhead_x, lane.top()), egui::pos2(playhead_x, lane.bottom())], egui::Stroke::new(2.0, egui::Color32::WHITE));
                            if track.clips.is_empty() {
                                ui.painter().text(lane.left_center() + egui::vec2(8.0, 0.0), egui::Align2::LEFT_CENTER,
                                    "Add a library item to place clips here", egui::FontId::proportional(11.0), egui::Color32::GRAY);
                            }
                        });
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.small(&self.status);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.small("Local edit"); });
                });
            });
        if let Some(selection) = clicked_clip { self.selected_clip = Some(selection); }
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    ui.label("PREVIEW · video decoding is not connected yet");
                    ui.add_space(8.0);
                    let available = ui.available_size();
                    let width = available.x.min(available.y * 16.0 / 9.0).max(1.0);
                    let size = egui::vec2(width, width * 9.0 / 16.0);
                    if let Some(renderer) = &self.quad_renderer {
                        renderer.paint(ui, size);
                    } else {
                        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                        ui.painter()
                            .rect_filled(rect, egui::CornerRadius::ZERO, PANEL);
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "GPU preview unavailable",
                            egui::FontId::proportional(16.0),
                            egui::Color32::LIGHT_GRAY,
                        );
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("⏮").clicked() {
                            self.playhead_seconds = 0.0;
                        }
                        if ui.button(if self.is_playing { "⏸" } else { "▶" }).clicked() {
                            self.is_playing = !self.is_playing;
                        }
                        if ui.button("⏭").clicked() {
                            self.playhead_seconds += 1.0;
                        }
                        ui.label(format!("{}", self.status));
                    });
                });
            });
    }
}

impl eframe::App for DcutApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let (save, undo, redo, toggle_play) = ui.input(|input| (
            input.modifiers.ctrl && input.key_pressed(egui::Key::S),
            input.modifiers.ctrl && input.key_pressed(egui::Key::Z),
            input.modifiers.ctrl && input.key_pressed(egui::Key::Y),
            input.key_pressed(egui::Key::Space),
        ));
        if save { self.save_project(); }
        if undo {
            match self.history.undo(&mut self.project) {
                Ok(true) => self.status = "Undo complete".to_owned(),
                Ok(false) => {},
                Err(error) => self.status = format!("Undo failed: {error}"),
            }
        }
        if redo {
            match self.history.redo(&mut self.project) {
                Ok(true) => self.status = "Redo complete".to_owned(),
                Ok(false) => {},
                Err(error) => self.status = format!("Redo failed: {error}"),
            }
        }
        if toggle_play { self.is_playing = !self.is_playing; }
        self.receive_dropped_files(ui.ctx());
        self.toolbar(ui);
        self.timeline_panel(ui);
        self.media_panel(ui);
        self.inspector_panel(ui);
        self.preview(ui);
        if self.is_playing {
            let elapsed = ui.input(|input| input.stable_dt).min(0.1);
            self.playhead_seconds += elapsed;
            ui.ctx().request_repaint();
        }
    }
}

fn expand_project_path(value: &str) -> PathBuf {
    if value == "~" {
        return std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(relative) = value.strip_prefix("~/") {
        return std::env::var_os("HOME").map(|home| PathBuf::from(home).join(relative))
            .unwrap_or_else(|| PathBuf::from(value));
    }
    PathBuf::from(value)
}

/// Starts the native editor window and initializes file-only local logging.
pub fn run() -> Result<()> {
    initialize_file_logging()?;
    let title = format!("{PRODUCT_NAME} {}", env!("CARGO_PKG_VERSION"));
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([1366.0, 768.0])
            .with_min_inner_size([960.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        PRODUCT_NAME,
        options,
        Box::new(|context| Ok(Box::new(DcutApp::new(context)))),
    )
    .context("could not run the D-cut desktop window")
}

fn initialize_file_logging() -> Result<()> {
    let state_root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
    let log_directory = state_root.join("d-cut/logs");
    fs::create_dir_all(&log_directory)
        .with_context(|| format!("could not create log directory {}", log_directory.display()))?;
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_directory.join("d-cut.log"))
        .context("could not open the D-cut log file")?;
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(log_file)
        .try_init()
        .map_err(anyhow::Error::msg)
        .context("could not initialize local file logging")
}
