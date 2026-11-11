// SPDX-License-Identifier: AGPL-3.0-or-later

//! D-cut desktop editor. Decoding, waveform/thumbnail extraction, playback audio and export
//! run on worker threads; the UI thread only edits the model and polls channels.

mod assets;
mod export;
mod files;
mod playback;
mod strings;

use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Instant,
};

use anyhow::{Context, Result};
use editor_core::{
    CommandHistory, CoreError, EditCommand, MediaAsset, PRODUCT_NAME, Project, ProjectFormat,
    Track, TrackKind, TrimEdge, add_media_clips_command, clips_at_frame, delete_clips_command,
    find_clip, linked_clip_ids, move_command, move_limits, next_free_id, split_at_command,
    trim_command, trim_limits, unlink_command,
};
use editor_media::{MediaInfo, probe, timeline::TimelineRender};
use editor_render::{FrameRenderer, PreviewFrame};
use eframe::egui;

use crate::{
    assets::AssetCache,
    export::{ExportJob, ExportSettings, JobState, Profile},
    files::{FileDialog, Outcome, Purpose},
    playback::{FrameKey, Playback},
};

const BG: egui::Color32 = egui::Color32::from_rgb(16, 18, 24);
const PANEL: egui::Color32 = egui::Color32::from_rgb(25, 28, 37);
const PANEL_ALT: egui::Color32 = egui::Color32::from_rgb(34, 38, 50);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(119, 94, 245);
const TEAL: egui::Color32 = egui::Color32::from_rgb(43, 195, 185);
const AUDIO: egui::Color32 = egui::Color32::from_rgb(46, 140, 110);
const VIDEO: egui::Color32 = egui::Color32::from_rgb(70, 72, 140);

const HEADER_WIDTH: f32 = 70.0;
const RULER_HEIGHT: f32 = 22.0;
const SNAP_PIXELS: f32 = 8.0;
const EDGE_PIXELS: f32 = 7.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewQuality {
    High,
    Medium,
    Low,
}

impl PreviewQuality {
    fn long_side(self) -> u32 {
        match self {
            Self::High => 960,
            Self::Medium => 640,
            Self::Low => 426,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::High => "Alta",
            Self::Medium => "Média",
            Self::Low => "Baixa",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragKind {
    Move,
    TrimHead,
    TrimTail,
}

#[derive(Debug, Clone)]
struct DragState {
    kind: DragKind,
    clip_id: u64,
    linked: HashSet<u64>,
    origin_x: f32,
    limits: (i64, i64),
    delta: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingAction {
    Close,
    New,
    Open,
}

enum ProbeKind {
    Import,
    Refresh(u64),
}

struct ProbeDone {
    path: PathBuf,
    kind: ProbeKind,
    result: Result<MediaInfo, String>,
}

/// Debug automation (`D_CUT_AUTOTEST=play:SECONDS,quit:SECONDS`): starts playback and/or closes
/// the window after the given number of seconds, so playback statistics and the clean-exit path
/// can be measured without synthesizing input events. Never enabled unless the variable is set.
struct Autotest {
    started: Instant,
    play_at: Option<f64>,
    quit_at: Option<f64>,
    played: bool,
}

impl Autotest {
    fn from_env() -> Option<Self> {
        let value = std::env::var("D_CUT_AUTOTEST").ok()?;
        let mut test = Self {
            started: Instant::now(),
            play_at: None,
            quit_at: None,
            played: false,
        };
        for part in value.split(',') {
            let (key, seconds) = part.split_once(':')?;
            let seconds: f64 = seconds.trim().parse().ok()?;
            match key.trim() {
                "play" => test.play_at = Some(seconds),
                "quit" => test.quit_at = Some(seconds),
                _ => return None,
            }
        }
        tracing::info!(play = ?test.play_at, quit = ?test.quit_at, "autotest enabled");
        Some(test)
    }
}

#[derive(Default)]
struct Hud {
    visible: bool,
    last_frame: Option<Instant>,
    frame_times: Vec<f32>,
    ui_times: Vec<f32>,
}

impl Hud {
    fn push(values: &mut Vec<f32>, value: f32) {
        values.push(value);
        if values.len() > 120 {
            values.remove(0);
        }
    }

    fn stats(values: &[f32]) -> (f32, f32) {
        if values.is_empty() {
            return (0.0, 0.0);
        }
        let average = values.iter().sum::<f32>() / values.len() as f32;
        let max = values.iter().copied().fold(0.0, f32::max);
        (average, max)
    }
}

fn resident_memory_mb() -> Option<f64> {
    let statm = fs::read_to_string("/proc/self/statm").ok()?;
    let pages: f64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096.0 / 1_048_576.0)
}

fn default_project() -> Project {
    let track = |id, name: &str, kind| Track {
        id,
        name: name.to_owned(),
        kind,
        clips: Vec::new(),
    };
    Project {
        tracks: vec![
            track(1, "V1", TrackKind::Video),
            track(2, "A1", TrackKind::Audio),
            track(3, "A2", TrackKind::Audio),
        ],
        ..Project::default()
    }
}

fn even(value: f64) -> u32 {
    ((value.round().max(2.0) as u32) / 2 * 2).max(2)
}

fn fit_long_side(width: u32, height: u32, long_side: u32) -> (u32, u32) {
    let scale = f64::from(long_side) / f64::from(width.max(height).max(1));
    let scale = scale.min(1.0);
    (
        even(f64::from(width) * scale),
        even(f64::from(height) * scale),
    )
}

fn format_time(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    let rest = seconds - minutes as f64 * 60.0;
    format!("{minutes:02}:{rest:05.2}")
}

fn file_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

/// Picks a project format matching the first imported clip (phone videos become 9:16).
fn format_for_media(info: &MediaInfo) -> Option<ProjectFormat> {
    let video = info.video.as_ref()?;
    let (width, height) = fit_long_side(video.width, video.height, 1920);
    let (numerator, denominator) = match video.frame_rate.map(|rate| rate.as_f64()) {
        Some(fps) if (fps - 23.976).abs() < 0.01 => (24000, 1001),
        Some(fps) if (fps - 24.0).abs() < 0.1 => (24, 1),
        Some(fps) if (fps - 25.0).abs() < 0.1 => (25, 1),
        Some(fps) if (fps - 29.97).abs() < 0.01 => (30000, 1001),
        Some(fps) if (fps - 50.0).abs() < 0.1 => (50, 1),
        Some(fps) if (fps - 59.94).abs() < 0.01 => (60000, 1001),
        Some(fps) if (fps - 60.0).abs() < 0.5 => (60, 1),
        _ => (30, 1),
    };
    Some(ProjectFormat {
        width,
        height,
        frame_rate_numerator: numerator,
        frame_rate_denominator: denominator,
    })
}

fn media_asset(id: u64, path: &Path, info: &MediaInfo, format: ProjectFormat) -> MediaAsset {
    let frames = (info.duration_seconds * format.frames_per_second()).floor() as u64;
    let (width, height, rotation_degrees) = info
        .video
        .as_ref()
        .map_or((format.width, format.height, 0), |video| {
            (video.width, video.height, video.rotation_degrees)
        });
    MediaAsset {
        id,
        path: path.to_string_lossy().into_owned(),
        content_hash: None,
        duration_frames: frames.max(1),
        width,
        height,
        has_video: info.video.is_some(),
        has_audio: info.audio.is_some(),
        rotation_degrees,
    }
}

fn describe(info: &MediaInfo) -> String {
    let mut parts = Vec::new();
    if let Some(video) = &info.video {
        let fps = video
            .frame_rate
            .map_or_else(String::new, |rate| format!(" {:.2}fps", rate.as_f64()));
        parts.push(format!(
            "{}×{} {}{fps}",
            video.width, video.height, video.codec
        ));
        if video.rotation_degrees != 0 {
            parts.push(format!("rot {}°", video.rotation_degrees));
        }
    }
    if let Some(audio) = &info.audio {
        parts.push(format!(
            "{} {}Hz",
            audio.codec,
            audio.sample_rate.unwrap_or(0)
        ));
    }
    parts.push(format_time(info.duration_seconds));
    parts.join(" · ")
}

struct DcutApp {
    renderer: Option<FrameRenderer>,
    project: Project,
    history: CommandHistory,
    project_path: Option<PathBuf>,
    dirty: bool,
    generation: u64,
    media_info: HashMap<u64, MediaInfo>,
    missing_media: HashSet<u64>,
    selected_media: Option<u64>,
    selected_clip: Option<u64>,
    playhead: u64,
    pixels_per_second: f32,
    scroll_px: f32,
    snapping: bool,
    status: String,
    probe_tx: Sender<ProbeDone>,
    probe_rx: Receiver<ProbeDone>,
    pending_probes: HashSet<PathBuf>,
    assets: AssetCache,
    scrub: playback::ScrubDecoder,
    playback: Option<Playback>,
    last_stats: Option<playback::PlaybackStats>,
    preview_frame: Option<PreviewFrame>,
    preview_frame_key: Option<(u64, u64)>,
    frame_counter: u64,
    quality: PreviewQuality,
    dialog: Option<FileDialog>,
    after_save: Option<PendingAction>,
    pending_action: Option<PendingAction>,
    allow_close: bool,
    export_settings: Option<ExportSettings>,
    export_job: Option<ExportJob>,
    drag: Option<DragState>,
    hud: Hud,
    title: String,
    last_dir: PathBuf,
    autotest: Option<Autotest>,
}

impl DcutApp {
    fn new(context: &eframe::CreationContext<'_>, startup_files: Vec<PathBuf>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG;
        visuals.window_fill = PANEL;
        visuals.selection.bg_fill = ACCENT;
        visuals.widgets.active.bg_fill = ACCENT;
        visuals.widgets.hovered.bg_fill = PANEL_ALT;
        context.egui_ctx.set_visuals(visuals);
        // Ctrl +/- is used by the timeline, not for UI scaling.
        context
            .egui_ctx
            .options_mut(|options| options.zoom_with_keyboard = false);
        let renderer = context.wgpu_render_state.as_ref().map(FrameRenderer::new);
        if renderer.is_none() {
            tracing::error!("eframe did not provide its wgpu render state");
        }
        let (probe_tx, probe_rx) = mpsc::channel();
        let ctx = context.egui_ctx.clone();
        let mut app = Self {
            renderer,
            project: default_project(),
            history: CommandHistory::default(),
            project_path: None,
            dirty: false,
            generation: 0,
            media_info: HashMap::new(),
            missing_media: HashSet::new(),
            selected_media: None,
            selected_clip: None,
            playhead: 0,
            pixels_per_second: 60.0,
            scroll_px: 0.0,
            snapping: true,
            status: "Pronto · importe um vídeo para começar".to_owned(),
            probe_tx,
            probe_rx,
            pending_probes: HashSet::new(),
            assets: AssetCache::new(ctx.clone()),
            scrub: playback::ScrubDecoder::new(ctx),
            playback: None,
            last_stats: None,
            preview_frame: None,
            preview_frame_key: None,
            frame_counter: 0,
            quality: PreviewQuality::Medium,
            dialog: None,
            after_save: None,
            pending_action: None,
            allow_close: false,
            export_settings: None,
            export_job: None,
            drag: None,
            hud: Hud::default(),
            title: String::new(),
            last_dir: files::home_dir(),
            autotest: Autotest::from_env(),
        };
        app.hud.visible = std::env::var_os("D_CUT_HUD").is_some();
        for path in startup_files {
            if path
                .extension()
                .is_some_and(|extension| extension == "dcut")
            {
                app.open_project(path);
            } else {
                app.queue_probe(path, ProbeKind::Import, &context.egui_ctx);
            }
        }
        app
    }

    fn fps(&self) -> f64 {
        self.project.format.frames_per_second().max(1.0)
    }

    fn preview_size(&self) -> (u32, u32) {
        fit_long_side(
            self.project.format.width,
            self.project.format.height,
            self.quality.long_side(),
        )
    }

    // ----- editing -----------------------------------------------------------------------

    fn changed(&mut self) {
        self.dirty = true;
        self.generation += 1;
        self.stop_playback();
        if self
            .selected_clip
            .is_some_and(|clip| find_clip(&self.project, clip).is_none())
        {
            self.selected_clip = None;
        }
        let duration = self.project.duration_frames();
        if self.playhead > duration {
            self.playhead = duration;
        }
    }

    fn run_edit(&mut self, command: Result<EditCommand, CoreError>, message: &str) -> bool {
        match command.and_then(|command| self.history.execute(&mut self.project, command)) {
            Ok(()) => {
                self.changed();
                self.status = message.to_owned();
                true
            }
            Err(error) => {
                self.status = format!("Não foi possível: {error}");
                false
            }
        }
    }

    fn undo(&mut self) {
        match self.history.undo(&mut self.project) {
            Ok(true) => {
                self.changed();
                self.status = "Desfeito".into();
            }
            Ok(false) => self.status = "Nada para desfazer".into(),
            Err(error) => self.status = format!("Falha ao desfazer: {error}"),
        }
    }

    fn redo(&mut self) {
        match self.history.redo(&mut self.project) {
            Ok(true) => {
                self.changed();
                self.status = "Refeito".into();
            }
            Ok(false) => self.status = "Nada para refazer".into(),
            Err(error) => self.status = format!("Falha ao refazer: {error}"),
        }
    }

    fn split(&mut self) {
        let targets = match self.selected_clip {
            Some(clip) => vec![clip],
            None => clips_at_frame(&self.project, self.playhead),
        };
        if targets.is_empty() {
            self.status = "Posicione o playhead sobre um clipe para dividir".into();
            return;
        }
        let command = split_at_command(&self.project, &targets, self.playhead);
        let time = format_time(self.project.format.frames_to_seconds(self.playhead));
        self.run_edit(command, &format!("Dividido em {time}"));
    }

    fn delete(&mut self, ripple: bool) {
        let Some(clip) = self.selected_clip else {
            self.status = "Selecione um clipe para excluir".into();
            return;
        };
        let command = delete_clips_command(&self.project, &[clip], ripple);
        let message = if ripple {
            "Clipe excluído e espaço fechado"
        } else {
            "Clipe excluído"
        };
        if self.run_edit(command, message) {
            self.selected_clip = None;
        }
    }

    fn unlink(&mut self) {
        if let Some(clip) = self.selected_clip {
            let command = unlink_command(&self.project, clip);
            self.run_edit(command, "Áudio desvinculado do vídeo");
        }
    }

    fn add_to_timeline(&mut self, media_id: u64) {
        match add_media_clips_command(&self.project, media_id) {
            Ok((command, created)) => {
                if self.run_edit(Ok(command), "Adicionado ao fim da timeline") {
                    self.selected_clip = created.first().copied();
                }
            }
            Err(error) => self.status = format!("Não foi possível adicionar: {error}"),
        }
    }

    fn set_format(&mut self, format: ProjectFormat) {
        if format == self.project.format {
            return;
        }
        let command = EditCommand::SetFormat {
            expected: self.project.format,
            replacement: format,
        };
        self.run_edit(
            Ok(command),
            &format!("Formato: {}×{}", format.width, format.height),
        );
    }

    // ----- media import ------------------------------------------------------------------

    fn queue_probe(&mut self, path: PathBuf, kind: ProbeKind, ctx: &egui::Context) {
        if matches!(kind, ProbeKind::Import)
            && (self
                .project
                .media
                .iter()
                .any(|media| Path::new(&media.path) == path)
                || !self.pending_probes.insert(path.clone()))
        {
            return;
        }
        let sender = self.probe_tx.clone();
        let ctx = ctx.clone();
        let spawned = thread::Builder::new()
            .name("d-cut-probe".into())
            .spawn(move || {
                let result = probe(&path).map_err(|error| error.to_string());
                let _ = sender.send(ProbeDone { path, kind, result });
                ctx.request_repaint();
            });
        if let Err(error) = spawned {
            self.status = format!("Falha ao iniciar leitura de mídia: {error}");
        } else {
            self.status = "Lendo mídia…".into();
        }
    }

    fn receive_probes(&mut self) {
        let done: Vec<ProbeDone> = self.probe_rx.try_iter().collect();
        for ProbeDone { path, kind, result } in done {
            self.pending_probes.remove(&path);
            match (kind, result) {
                (ProbeKind::Refresh(media_id), Ok(info)) => {
                    self.assets.request(media_id, path, &info);
                    self.media_info.insert(media_id, info);
                }
                (ProbeKind::Refresh(media_id), Err(error)) => {
                    tracing::warn!(%error, "media of opened project could not be read");
                    self.missing_media.insert(media_id);
                    self.status = format!("Mídia não encontrada: {}", path.display());
                }
                (ProbeKind::Import, Ok(info)) => self.import_media(path, info),
                (ProbeKind::Import, Err(error)) => {
                    tracing::warn!(%error, path = %path.display(), "import failed");
                    self.status = format!("Não foi possível importar {}: {error}", path.display());
                }
            }
        }
    }

    fn import_media(&mut self, path: PathBuf, info: MediaInfo) {
        let first_media = self.project.media.is_empty() && self.project.duration_frames() == 0;
        let mut commands = Vec::new();
        let mut format = self.project.format;
        if first_media && let Some(media_format) = format_for_media(&info) {
            format = media_format;
            commands.push(EditCommand::SetFormat {
                expected: self.project.format,
                replacement: format,
            });
        }
        let id = next_free_id(&self.project);
        let asset = media_asset(id, &path, &info, format);
        let mut media = self.project.media.clone();
        media.push(asset);
        commands.push(EditCommand::ReplaceMedia {
            expected: self.project.media.clone(),
            replacement: media,
        });
        let name = file_label(&path.to_string_lossy());
        let message = if first_media && commands.len() > 1 {
            format!(
                "Importado {name} · projeto ajustado para {}×{} @ {:.2} fps",
                format.width,
                format.height,
                format.frames_per_second()
            )
        } else {
            format!("Importado {name}")
        };
        if self.run_edit(Ok(EditCommand::Batch(commands)), &message) {
            self.assets.request(id, path, &info);
            self.media_info.insert(id, info);
            self.selected_media = Some(id);
            if self.project.duration_frames() == 0 {
                self.add_to_timeline(id);
                self.status = message;
            }
        }
    }

    fn receive_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        for path in dropped {
            if path
                .extension()
                .is_some_and(|extension| extension == "dcut")
            {
                self.request_action(PendingAction::Open);
                if self.pending_action.is_none() {
                    self.open_project(path);
                }
            } else {
                self.queue_probe(path, ProbeKind::Import, ctx);
            }
        }
    }

    // ----- project files -----------------------------------------------------------------

    fn save_to(&mut self, path: PathBuf) -> bool {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            && let Err(error) = fs::create_dir_all(parent)
        {
            self.status = format!("Não foi possível criar a pasta: {error}");
            return false;
        }
        match editor_project::save(&path, &self.project) {
            Ok(()) => {
                self.status = format!("Projeto salvo em {}", path.display());
                tracing::info!(path = %path.display(), "project saved");
                if let Some(parent) = path.parent() {
                    self.last_dir = parent.to_owned();
                }
                self.project_path = Some(path);
                self.dirty = false;
                true
            }
            Err(error) => {
                self.status = format!("Não foi possível salvar: {error}");
                false
            }
        }
    }

    fn save(&mut self) -> bool {
        match self.project_path.clone() {
            Some(path) => self.save_to(path),
            None => {
                self.open_dialog(Purpose::SaveProject);
                false
            }
        }
    }

    fn open_project(&mut self, path: PathBuf) {
        match editor_project::load(&path) {
            Ok(project) => {
                self.stop_playback();
                self.project = project;
                self.history = CommandHistory::default();
                self.media_info.clear();
                self.missing_media.clear();
                self.assets.clear();
                self.selected_clip = None;
                self.selected_media = None;
                self.playhead = 0;
                self.scroll_px = 0.0;
                self.generation += 1;
                self.dirty = false;
                if let Some(parent) = path.parent() {
                    self.last_dir = parent.to_owned();
                }
                self.status = format!("Projeto aberto: {}", path.display());
                tracing::info!(path = %path.display(), "project opened");
                self.project_path = Some(path);
                let media: Vec<(u64, PathBuf)> = self
                    .project
                    .media
                    .iter()
                    .map(|media| (media.id, PathBuf::from(&media.path)))
                    .collect();
                for (id, media_path) in media {
                    if media_path.exists() {
                        let sender = self.probe_tx.clone();
                        let _ =
                            thread::Builder::new()
                                .name("d-cut-probe".into())
                                .spawn(move || {
                                    let result =
                                        probe(&media_path).map_err(|error| error.to_string());
                                    let _ = sender.send(ProbeDone {
                                        path: media_path,
                                        kind: ProbeKind::Refresh(id),
                                        result,
                                    });
                                });
                    } else {
                        self.missing_media.insert(id);
                    }
                }
                if !self.missing_media.is_empty() {
                    self.status = format!(
                        "Projeto aberto, mas {} arquivo(s) de mídia não foram encontrados",
                        self.missing_media.len()
                    );
                }
            }
            Err(error) => self.status = format!("Não foi possível abrir: {error}"),
        }
    }

    fn new_project(&mut self) {
        self.stop_playback();
        self.project = default_project();
        self.history = CommandHistory::default();
        self.project_path = None;
        self.media_info.clear();
        self.missing_media.clear();
        self.assets.clear();
        self.selected_clip = None;
        self.selected_media = None;
        self.playhead = 0;
        self.scroll_px = 0.0;
        self.generation += 1;
        self.dirty = false;
        self.status = "Novo projeto".into();
    }

    fn request_action(&mut self, action: PendingAction) {
        if self.dirty {
            self.pending_action = Some(action);
        } else {
            self.perform(action);
        }
    }

    fn perform(&mut self, action: PendingAction) {
        match action {
            PendingAction::Close => self.allow_close = true,
            PendingAction::New => self.new_project(),
            PendingAction::Open => self.open_dialog(Purpose::OpenProject),
        }
    }

    fn open_dialog(&mut self, purpose: Purpose) {
        let name = match purpose {
            Purpose::SaveProject => self
                .project_path
                .as_ref()
                .and_then(|path| path.file_name())
                .map_or_else(
                    || "meu-projeto.dcut".to_owned(),
                    |name| name.to_string_lossy().into_owned(),
                ),
            Purpose::ExportVideo => self
                .export_settings
                .as_ref()
                .and_then(|settings| {
                    Path::new(&settings.output)
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| "export.mp4".to_owned()),
            _ => String::new(),
        };
        let directory = match purpose {
            Purpose::ExportVideo => self
                .export_settings
                .as_ref()
                .and_then(|settings| Path::new(&settings.output).parent().map(Path::to_owned))
                .unwrap_or_else(|| self.last_dir.clone()),
            _ => self.last_dir.clone(),
        };
        self.dialog = Some(FileDialog::new(purpose, directory, name));
    }

    fn handle_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.dialog else {
            return;
        };
        let purpose = dialog.purpose;
        match dialog.show(ctx) {
            Outcome::Pending => {}
            Outcome::Cancelled => {
                self.dialog = None;
                self.after_save = None;
            }
            Outcome::Picked(paths) => {
                self.dialog = None;
                if let Some(parent) = paths.first().and_then(|path| path.parent()) {
                    self.last_dir = parent.to_owned();
                }
                match purpose {
                    Purpose::ImportMedia => {
                        for path in paths {
                            self.queue_probe(path, ProbeKind::Import, ctx);
                        }
                    }
                    Purpose::OpenProject => {
                        if let Some(path) = paths.into_iter().next() {
                            self.open_project(path);
                        }
                    }
                    Purpose::SaveProject => {
                        if let Some(path) = paths.into_iter().next()
                            && self.save_to(path)
                            && let Some(action) = self.after_save.take()
                        {
                            self.perform(action);
                        }
                    }
                    Purpose::ExportVideo => {
                        if let (Some(path), Some(settings)) =
                            (paths.into_iter().next(), &mut self.export_settings)
                        {
                            settings.output = path.display().to_string();
                        }
                    }
                }
            }
        }
    }

    // ----- preview / playback ------------------------------------------------------------

    fn stop_playback(&mut self) {
        if let Some(playback) = self.playback.take() {
            playback.log_summary();
            self.last_stats = Some(playback.stats);
        }
    }

    fn toggle_playback(&mut self, ctx: &egui::Context) {
        if self.playback.is_some() {
            self.stop_playback();
            self.status = "Pausado".into();
            return;
        }
        let duration = self.project.duration_frames();
        if duration == 0 {
            self.status = "A timeline está vazia".into();
            return;
        }
        if self.playhead + 1 >= duration {
            self.playhead = 0;
        }
        let (width, height) = self.preview_size();
        let video_job = match TimelineRender::from_project(
            &self.project,
            self.playhead,
            duration,
            width,
            height,
        ) {
            Ok(job) => job,
            Err(error) => {
                self.status = format!("Não foi possível reproduzir: {error}");
                return;
            }
        };
        let has_audio = self
            .project
            .tracks
            .iter()
            .any(|track| track.kind == TrackKind::Audio && !track.clips.is_empty());
        let audio_job = has_audio
            .then(|| {
                TimelineRender::from_project(&self.project, self.playhead, duration, 2, 2).ok()
            })
            .flatten();
        let playback = Playback::start(ctx.clone(), video_job, audio_job, self.fps());
        self.status = match (&playback.audio_error, has_audio) {
            (Some(error), _) => format!("Reproduzindo sem som ({error})"),
            (None, true) => "Reproduzindo (áudio é o relógio)".into(),
            (None, false) => "Reproduzindo (timeline sem áudio)".into(),
        };
        self.playback = Some(playback);
    }

    fn show_frame(&mut self, width: u32, height: u32, rgba: Vec<u8>) {
        self.frame_counter += 1;
        self.preview_frame = Some(PreviewFrame {
            width,
            height,
            rgba: Arc::new(rgba),
            generation: self.frame_counter,
        });
    }

    fn update_preview(&mut self, ctx: &egui::Context) {
        let (width, height) = self.preview_size();
        if let Some(playback) = &mut self.playback {
            let tick = playback.tick();
            self.playhead = tick.clock_frame;
            if let Some((frame_index, rgba)) = tick.frame {
                self.preview_frame_key = Some((frame_index, self.generation));
                self.show_frame(width, height, rgba);
            }
            if tick.finished {
                self.stop_playback();
                self.status = "Fim da timeline".into();
            } else {
                ctx.request_repaint();
            }
            return;
        }
        if let Some(result) = self.scrub.poll() {
            match result.frame {
                Ok(rgba)
                    if rgba.len() == result.key.width as usize * result.key.height as usize * 4 =>
                {
                    self.preview_frame_key = Some((result.key.frame, result.key.generation));
                    self.show_frame(result.key.width, result.key.height, rgba);
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(%error, "preview frame failed");
                    self.status = format!("Falha na prévia: {error}");
                }
            }
        }
        let duration = self.project.duration_frames();
        if duration == 0 {
            self.preview_frame = None;
            self.preview_frame_key = None;
            return;
        }
        let frame = self.playhead.min(duration - 1);
        if self.preview_frame_key == Some((frame, self.generation))
            && self
                .preview_frame
                .as_ref()
                .is_some_and(|current| current.width == width && current.height == height)
        {
            return;
        }
        let key = FrameKey {
            frame,
            generation: self.generation,
            width,
            height,
        };
        if self.scrub.last_requested != Some(key) {
            match TimelineRender::from_project(&self.project, frame, frame + 1, width, height) {
                Ok(job) => self.scrub.request(key, job),
                Err(error) => self.status = format!("Falha na prévia: {error}"),
            }
        }
    }

    fn seek(&mut self, frame: u64) {
        self.stop_playback();
        self.playhead = frame.min(self.project.duration_frames());
    }

    // ----- panels ------------------------------------------------------------------------

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("main-toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(egui::RichText::new(PRODUCT_NAME).color(TEAL));
                ui.separator();
                if ui.button(strings::NEW_PROJECT).clicked() {
                    self.request_action(PendingAction::New);
                }
                if ui.button(strings::OPEN).on_hover_text("Ctrl+O").clicked() {
                    self.request_action(PendingAction::Open);
                }
                if ui.button(strings::SAVE).on_hover_text("Ctrl+S").clicked() {
                    self.save();
                }
                if ui.button(strings::SAVE_AS).clicked() {
                    self.open_dialog(Purpose::SaveProject);
                }
                ui.separator();
                if ui.button(strings::IMPORT).on_hover_text("Ctrl+I").clicked() {
                    self.open_dialog(Purpose::ImportMedia);
                }
                ui.separator();
                if ui
                    .add_enabled(self.history.can_undo(), egui::Button::new(strings::UNDO))
                    .on_hover_text("Ctrl+Z")
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(self.history.can_redo(), egui::Button::new(strings::REDO))
                    .on_hover_text("Ctrl+Shift+Z / Ctrl+Y")
                    .clicked()
                {
                    self.redo();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(format!("v{}", env!("CARGO_PKG_VERSION")));
                    let export_button = egui::Button::new(
                        egui::RichText::new(strings::EXPORT).color(egui::Color32::WHITE),
                    )
                    .fill(ACCENT);
                    if ui
                        .add_enabled(self.project.duration_frames() > 0, export_button)
                        .on_hover_text("Ctrl+E")
                        .clicked()
                    {
                        self.open_export();
                    }
                });
            });
        });
    }

    fn open_export(&mut self) {
        if self.project.duration_frames() == 0 {
            self.status = "Nada para exportar: a timeline está vazia".into();
            return;
        }
        if self.export_settings.is_none() {
            let base = self
                .project_path
                .as_ref()
                .and_then(|path| path.file_stem())
                .map_or_else(
                    || "d-cut-export".to_owned(),
                    |stem| stem.to_string_lossy().into_owned(),
                );
            let directory = {
                let videos = files::home_dir().join("Videos");
                if videos.is_dir() {
                    videos
                } else {
                    self.last_dir.clone()
                }
            };
            self.export_settings = Some(ExportSettings::new(directory.join(format!("{base}.mp4"))));
        }
    }

    fn media_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("media-library")
            .resizable(true)
            .default_size(220.0)
            .min_size(170.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(strings::MEDIA_LIBRARY);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(strings::IMPORT).clicked() {
                            self.open_dialog(Purpose::ImportMedia);
                        }
                    });
                });
                ui.separator();
                if self.project.media.is_empty() {
                    ui.label(strings::NO_MEDIA);
                    ui.small(strings::NO_MEDIA_HINT);
                    if !self.pending_probes.is_empty() {
                        ui.spinner();
                    }
                }
                let mut add = None;
                let media: Vec<MediaAsset> = self.project.media.clone();
                egui::ScrollArea::vertical()
                    .max_height((ui.available_height() - 60.0).max(60.0))
                    .show(ui, |ui| {
                        for asset in &media {
                            let selected = self.selected_media == Some(asset.id);
                            let frame = egui::Frame::new()
                                .fill(if selected { PANEL_ALT } else { PANEL })
                                .corner_radius(4)
                                .inner_margin(4);
                            let response = frame
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        if let Some(texture) = self.assets.thumbnails.get(&asset.id)
                                        {
                                            let size = texture.size_vec2();
                                            let scale = 34.0 / size.y.max(1.0);
                                            ui.image((texture.id(), size * scale.min(1.0)));
                                        } else {
                                            ui.small(if asset.has_video {
                                                "[vídeo]"
                                            } else {
                                                "[áudio]"
                                            });
                                        }
                                        ui.vertical(|ui| {
                                            let name = file_label(&asset.path);
                                            if self.missing_media.contains(&asset.id) {
                                                ui.colored_label(
                                                    egui::Color32::LIGHT_RED,
                                                    format!("! {name}"),
                                                );
                                            } else {
                                                ui.label(egui::RichText::new(name).strong());
                                            }
                                            let summary =
                                                self.media_info.get(&asset.id).map_or_else(
                                                    || {
                                                        format!(
                                                            "{}×{} · {}",
                                                            asset.width,
                                                            asset.height,
                                                            format_time(
                                                                self.project
                                                                    .format
                                                                    .frames_to_seconds(
                                                                        asset.duration_frames
                                                                    )
                                                            )
                                                        )
                                                    },
                                                    describe,
                                                );
                                            ui.small(summary);
                                        });
                                    });
                                })
                                .response
                                .interact(egui::Sense::click());
                            if response.clicked() {
                                self.selected_media = Some(asset.id);
                            }
                            if response.double_clicked() {
                                add = Some(asset.id);
                            }
                            response.on_hover_text("Clique duplo: adicionar à timeline");
                            ui.add_space(3.0);
                        }
                    });
                if let Some(media_id) = add {
                    self.add_to_timeline(media_id);
                }
                ui.add_space(6.0);
                if ui
                    .add_enabled(
                        self.selected_media.is_some(),
                        egui::Button::new(strings::ADD_TO_TIMELINE),
                    )
                    .clicked()
                    && let Some(media_id) = self.selected_media
                {
                    self.add_to_timeline(media_id);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.small(strings::LOCAL_ONLY);
                });
            });
    }

    fn inspector_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(230.0)
            .min_size(190.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading(strings::INSPECTOR);
                    ui.separator();
                    self.clip_inspector(ui);
                    ui.separator();
                    self.format_inspector(ui);
                    ui.separator();
                    ui.label(strings::PREVIEW_QUALITY);
                    ui.horizontal(|ui| {
                        for quality in [
                            PreviewQuality::High,
                            PreviewQuality::Medium,
                            PreviewQuality::Low,
                        ] {
                            if ui
                                .selectable_label(self.quality == quality, quality.label())
                                .clicked()
                            {
                                self.quality = quality;
                                self.stop_playback();
                            }
                        }
                    });
                    let (width, height) = self.preview_size();
                    ui.small(format!("Prévia em {width}×{height}"));
                    ui.separator();
                    ui.checkbox(&mut self.snapping, "Ímã (snap) na timeline");
                    ui.add_space(6.0);
                    ui.small(strings::SHORTCUTS);
                });
            });
    }

    fn clip_inspector(&mut self, ui: &mut egui::Ui) {
        let Some(clip_id) = self.selected_clip else {
            ui.weak(strings::NOTHING_SELECTED);
            return;
        };
        let Some((track_index, clip_index)) = find_clip(&self.project, clip_id) else {
            ui.weak(strings::NOTHING_SELECTED);
            return;
        };
        let track = &self.project.tracks[track_index];
        let clip = track.clips[clip_index].clone();
        let format = self.project.format;
        let name = self
            .project
            .media_asset(clip.media_id)
            .map_or_else(|| "?".to_owned(), |media| file_label(&media.path));
        ui.strong(format!("Clipe em {}", track.name));
        ui.label(name);
        egui::Grid::new("clip-grid").num_columns(2).show(ui, |ui| {
            ui.label("Início");
            ui.label(format_time(
                format.frames_to_seconds(clip.timeline_start_frame),
            ));
            ui.end_row();
            ui.label("Duração");
            ui.label(format_time(format.frames_to_seconds(clip.duration_frames)));
            ui.end_row();
            ui.label("Trecho da fonte");
            ui.label(format!(
                "{} – {}",
                format_time(format.frames_to_seconds(clip.source_start_frame)),
                format_time(
                    format.frames_to_seconds(clip.source_start_frame + clip.duration_frames)
                )
            ));
            ui.end_row();
        });
        let linked = linked_clip_ids(&self.project, clip_id).len() > 1;
        if linked {
            ui.small("Vídeo e áudio vinculados");
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(strings::SPLIT).on_hover_text("S").clicked() {
                self.split();
            }
            if ui.button(strings::DELETE).on_hover_text("Delete").clicked() {
                self.delete(false);
            }
            if ui
                .button(strings::RIPPLE_DELETE)
                .on_hover_text("Shift+Delete")
                .clicked()
            {
                self.delete(true);
            }
            if linked && ui.button(strings::UNLINK).clicked() {
                self.unlink();
            }
        });
    }

    fn format_inspector(&mut self, ui: &mut egui::Ui) {
        ui.label(strings::PROJECT_FORMAT);
        let format = self.project.format;
        let ratios: [(&str, u32, u32); 4] = [
            ("16:9", 16, 9),
            ("9:16", 9, 16),
            ("1:1", 1, 1),
            ("4:5", 4, 5),
        ];
        let current_ratio = ratios
            .iter()
            .find(|(_, w, h)| format.width * h == format.height * w)
            .map_or("personalizado", |(label, _, _)| label);
        let long = format.width.max(format.height);
        let mut new_format = None;
        egui::ComboBox::from_label("Proporção")
            .selected_text(current_ratio)
            .show_ui(ui, |ui| {
                for (label, w, h) in ratios {
                    if ui.selectable_label(current_ratio == label, label).clicked() {
                        let (width, height) = fit_long_side(w * 10_000, h * 10_000, long.max(2));
                        new_format = Some(ProjectFormat {
                            width,
                            height,
                            ..format
                        });
                    }
                }
            });
        let resolutions = [("4K", 3840), ("1080p", 1920), ("720p", 1280)];
        let current_res = resolutions
            .iter()
            .find(|(_, side)| *side == long)
            .map_or("personalizada", |(label, _)| label);
        egui::ComboBox::from_label("Resolução")
            .selected_text(current_res)
            .show_ui(ui, |ui| {
                for (label, side) in resolutions {
                    if ui.selectable_label(current_res == label, label).clicked() {
                        let (width, height) =
                            fit_long_side(format.width * 10_000, format.height * 10_000, side);
                        new_format = Some(ProjectFormat {
                            width,
                            height,
                            ..format
                        });
                    }
                }
            });
        let rates: [(&str, u32, u32); 7] = [
            ("23.976", 24000, 1001),
            ("24", 24, 1),
            ("25", 25, 1),
            ("29.97", 30000, 1001),
            ("30", 30, 1),
            ("50", 50, 1),
            ("60", 60, 1),
        ];
        let current_rate = rates
            .iter()
            .find(|(_, n, d)| {
                *n == format.frame_rate_numerator && *d == format.frame_rate_denominator
            })
            .map_or("outro", |(label, _, _)| label);
        // Clip positions are stored in frames, so the frame rate is fixed once media exists.
        let can_change_rate = self.project.media.is_empty();
        ui.add_enabled_ui(can_change_rate, |ui| {
            egui::ComboBox::from_label("FPS")
                .selected_text(current_rate)
                .show_ui(ui, |ui| {
                    for (label, numerator, denominator) in rates {
                        if ui.selectable_label(current_rate == label, label).clicked() {
                            new_format = Some(ProjectFormat {
                                frame_rate_numerator: numerator,
                                frame_rate_denominator: denominator,
                                ..format
                            });
                        }
                    }
                });
        })
        .response
        .on_disabled_hover_text("O FPS só pode mudar antes de importar mídia");
        ui.small(format!(
            "{} × {} · {:.3} fps",
            format.width,
            format.height,
            format.frames_per_second()
        ));
        if let Some(format) = new_format {
            self.set_format(format);
        }
    }

    fn preview_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(6))
            .show(ui, |ui| {
                let transport_height = 30.0;
                let available = ui.available_size() - egui::vec2(0.0, transport_height);
                let aspect =
                    self.project.format.width as f32 / self.project.format.height.max(1) as f32;
                let mut size = egui::vec2(available.x, available.x / aspect);
                if size.y > available.y {
                    size = egui::vec2(available.y * aspect, available.y);
                }
                let size = size.max(egui::vec2(16.0, 16.0));
                ui.vertical_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, egui::CornerRadius::ZERO, egui::Color32::BLACK);
                    if self.project.duration_frames() == 0 {
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            strings::PREVIEW_EMPTY,
                            egui::FontId::proportional(15.0),
                            egui::Color32::GRAY,
                        );
                    } else if let Some(renderer) = &self.renderer {
                        renderer.paint(&painter, rect, self.preview_frame.clone());
                    } else {
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            strings::RENDERER_MISSING,
                            egui::FontId::proportional(15.0),
                            egui::Color32::LIGHT_RED,
                        );
                    }
                    if self.hud.visible {
                        self.draw_hud(&painter, rect);
                    }
                    ui.horizontal(|ui| {
                        let fps = self.fps();
                        let duration = self.project.duration_frames();
                        ui.add_space((ui.available_width() / 2.0 - 150.0).max(0.0));
                        if ui.button("Início").on_hover_text("Home").clicked() {
                            self.seek(0);
                        }
                        if ui
                            .button("< Frame")
                            .on_hover_text("Seta esquerda")
                            .clicked()
                        {
                            self.seek(self.playhead.saturating_sub(1));
                        }
                        let play_label = if self.playback.is_some() {
                            "Pausar"
                        } else {
                            "Play"
                        };
                        if ui
                            .add(egui::Button::new(egui::RichText::new(play_label).strong()))
                            .on_hover_text("Play/Pause (Espaço)")
                            .clicked()
                        {
                            self.toggle_playback(ui.ctx());
                        }
                        if ui.button("Frame >").on_hover_text("Seta direita").clicked() {
                            self.seek(self.playhead + 1);
                        }
                        if ui.button("Fim").on_hover_text("End").clicked() {
                            self.seek(duration);
                        }
                        ui.monospace(format!(
                            "{} / {}",
                            format_time(self.playhead as f64 / fps),
                            format_time(duration as f64 / fps)
                        ));
                    });
                });
            });
    }

    fn draw_hud(&self, painter: &egui::Painter, rect: egui::Rect) {
        let (frame_avg, frame_max) = Hud::stats(&self.hud.frame_times);
        let (ui_avg, ui_max) = Hud::stats(&self.hud.ui_times);
        let stats = self
            .playback
            .as_ref()
            .map(|playback| playback.stats)
            .or(self.last_stats);
        let mut lines = vec![
            format!("frame {frame_avg:.1} ms (máx {frame_max:.1})"),
            format!("UI {ui_avg:.2} ms (máx {ui_max:.2})"),
            format!("RAM {:.0} MB", resident_memory_mb().unwrap_or(f64::NAN)),
        ];
        if let Some(stats) = stats {
            let mean = if stats.shown > 0 {
                stats.sum_offset_ms / stats.shown as f64
            } else {
                0.0
            };
            lines.push(format!(
                "vídeo mostrados {} · perdidos {}",
                stats.shown, stats.dropped
            ));
            lines.push(format!(
                "A/V offset {:.1} ms (média {mean:.1}, |máx| {:.1})",
                stats.last_offset_ms, stats.max_abs_offset_ms
            ));
            lines.push(format!(
                "áudio buffer {:.0} ms · underruns {}",
                stats.audio_buffer_ms, stats.underruns
            ));
        }
        if let Some(playback) = &self.playback {
            lines.push(if playback.using_audio_clock() {
                "relógio: áudio".into()
            } else {
                "relógio: parede (sem áudio)".into()
            });
        }
        let text = lines.join("\n");
        let galley = painter.layout_no_wrap(
            text,
            egui::FontId::monospace(11.0),
            egui::Color32::from_rgb(180, 255, 200),
        );
        let background = egui::Rect::from_min_size(
            rect.left_top() + egui::vec2(6.0, 6.0),
            galley.size() + egui::vec2(8.0, 8.0),
        );
        painter.rect_filled(
            background,
            egui::CornerRadius::same(3),
            egui::Color32::from_black_alpha(190),
        );
        painter.galley(
            background.min + egui::vec2(4.0, 4.0),
            galley,
            egui::Color32::WHITE,
        );
    }

    fn timeline_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("timeline")
            .resizable(true)
            .default_size(250.0)
            .min_size(160.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button(if self.playback.is_some() {
                            "Pausar"
                        } else {
                            "Play"
                        })
                        .clicked()
                    {
                        self.toggle_playback(ui.ctx());
                    }
                    ui.separator();
                    if ui.button(strings::SPLIT).on_hover_text("S").clicked() {
                        self.split();
                    }
                    if ui
                        .add_enabled(
                            self.selected_clip.is_some(),
                            egui::Button::new(strings::DELETE),
                        )
                        .on_hover_text("Delete")
                        .clicked()
                    {
                        self.delete(false);
                    }
                    if ui
                        .add_enabled(
                            self.selected_clip.is_some(),
                            egui::Button::new(strings::RIPPLE_DELETE),
                        )
                        .on_hover_text("Shift+Delete")
                        .clicked()
                    {
                        self.delete(true);
                    }
                    ui.separator();
                    if ui
                        .button(" - ")
                        .on_hover_text("Diminuir zoom (Ctrl+roda / -)")
                        .clicked()
                    {
                        self.pixels_per_second = (self.pixels_per_second / 1.25).max(2.0);
                    }
                    if ui
                        .button(" + ")
                        .on_hover_text("Aumentar zoom (Ctrl+roda / +)")
                        .clicked()
                    {
                        self.pixels_per_second = (self.pixels_per_second * 1.25).min(1200.0);
                    }
                    if ui
                        .button("Ajustar")
                        .on_hover_text("Mostrar a timeline inteira")
                        .clicked()
                    {
                        self.fit_timeline(ui.available_width());
                    }
                });
                let status_height = 18.0;
                let area_size = egui::vec2(
                    ui.available_width(),
                    (ui.available_height() - status_height).max(60.0),
                );
                let (area, response) =
                    ui.allocate_exact_size(area_size, egui::Sense::click_and_drag());
                self.draw_timeline(ui, area, &response);
                ui.horizontal(|ui| {
                    ui.small(&self.status);
                });
            });
    }

    fn fit_timeline(&mut self, width: f32) {
        let seconds = self
            .project
            .format
            .frames_to_seconds(self.project.duration_frames())
            .max(1.0) as f32;
        self.pixels_per_second = ((width - HEADER_WIDTH - 40.0) / seconds).clamp(2.0, 1200.0);
        self.scroll_px = 0.0;
    }

    fn draw_timeline(&mut self, ui: &mut egui::Ui, area: egui::Rect, response: &egui::Response) {
        let fps = self.fps() as f32;
        let pixels_per_frame = self.pixels_per_second / fps;
        let painter = ui.painter_at(area);
        painter.rect_filled(area, egui::CornerRadius::ZERO, BG);
        let content_left = area.left() + HEADER_WIDTH;
        let content = egui::Rect::from_min_max(egui::pos2(content_left, area.top()), area.max);
        let duration = self.project.duration_frames();

        // Zoom (Ctrl+wheel) around the pointer and wheel scrolling.
        if response.hovered() || response.contains_pointer() {
            let (zoom, scroll, pointer) = ui.input(|input| {
                (
                    input.zoom_delta(),
                    input.smooth_scroll_delta,
                    input.pointer.hover_pos(),
                )
            });
            if (zoom - 1.0).abs() > f32::EPSILON {
                let anchor_x = pointer.map_or(content_left, |pos| pos.x.max(content_left));
                let anchor_seconds =
                    (anchor_x - content_left + self.scroll_px) / self.pixels_per_second;
                self.pixels_per_second = (self.pixels_per_second * zoom).clamp(2.0, 1200.0);
                self.scroll_px =
                    anchor_seconds * self.pixels_per_second - (anchor_x - content_left);
            } else {
                self.scroll_px -= scroll.x + scroll.y;
            }
        }
        let pixels_per_frame_now = self.pixels_per_second / fps;
        let content_width = duration as f32 * pixels_per_frame_now;
        let max_scroll = (content_width - content.width() * 0.5).max(0.0);
        // Keep the playhead visible during playback.
        if self.playback.is_some() {
            let x = self.playhead as f32 * pixels_per_frame_now - self.scroll_px;
            if x > content.width() - 30.0 || x < 0.0 {
                self.scroll_px =
                    self.playhead as f32 * pixels_per_frame_now - content.width() * 0.2;
            }
        }
        self.scroll_px = self.scroll_px.clamp(0.0, max_scroll);
        let _ = pixels_per_frame;
        let ppf = pixels_per_frame_now;
        let scroll = self.scroll_px;
        let x_of = |frame: f64| content_left + frame as f32 * ppf - scroll;
        let frame_of =
            |x: f32| -> u64 { (((x - content_left + scroll) / ppf).round().max(0.0)) as u64 };

        // Ruler.
        let ruler = egui::Rect::from_min_size(
            egui::pos2(content_left, area.top()),
            egui::vec2(content.width(), RULER_HEIGHT),
        );
        painter.rect_filled(ruler, egui::CornerRadius::ZERO, PANEL);
        let steps = [
            0.1_f32, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0,
        ];
        let step = steps
            .iter()
            .copied()
            .find(|step| step * self.pixels_per_second >= 70.0)
            .unwrap_or(600.0);
        let first_tick = (scroll / self.pixels_per_second / step).floor() as i64;
        let last_tick = ((scroll + content.width()) / self.pixels_per_second / step).ceil() as i64;
        for tick in first_tick.max(0)..=last_tick {
            let seconds = tick as f32 * step;
            let x = content_left + seconds * self.pixels_per_second - scroll;
            painter.line_segment(
                [
                    egui::pos2(x, ruler.bottom() - 7.0),
                    egui::pos2(x, ruler.bottom()),
                ],
                egui::Stroke::new(1.0, egui::Color32::GRAY),
            );
            painter.text(
                egui::pos2(x + 3.0, ruler.top() + 2.0),
                egui::Align2::LEFT_TOP,
                format_time(f64::from(seconds)),
                egui::FontId::monospace(10.0),
                egui::Color32::GRAY,
            );
        }

        // Tracks.
        let tracks: Vec<Track> = self.project.tracks.clone();
        let lanes_top = ruler.bottom() + 2.0;
        let lane_space = (area.bottom() - lanes_top - 12.0).max(30.0);
        let lane_height = (lane_space / tracks.len().max(1) as f32 - 3.0).clamp(26.0, 56.0);
        let mut clip_hit = false;
        let drag = self.drag.clone();
        let mut new_drag: Option<DragState> = None;
        let mut finished_drag: Option<DragState> = None;
        let mut snap_targets: Vec<u64> = vec![0, self.playhead];
        for track in &tracks {
            for clip in &track.clips {
                if drag
                    .as_ref()
                    .is_some_and(|drag| drag.linked.contains(&clip.id))
                {
                    continue;
                }
                snap_targets.push(clip.timeline_start_frame);
                snap_targets.push(clip.timeline_start_frame + clip.duration_frames);
            }
        }
        for (index, track) in tracks.iter().enumerate() {
            let top = lanes_top + index as f32 * (lane_height + 3.0);
            let lane = egui::Rect::from_min_size(
                egui::pos2(content_left, top),
                egui::vec2(content.width(), lane_height),
            );
            let header = egui::Rect::from_min_size(
                egui::pos2(area.left(), top),
                egui::vec2(HEADER_WIDTH - 4.0, lane_height),
            );
            painter.rect_filled(header, egui::CornerRadius::same(3), PANEL);
            painter.text(
                header.left_center() + egui::vec2(6.0, 0.0),
                egui::Align2::LEFT_CENTER,
                track.name.clone(),
                egui::FontId::proportional(12.0),
                egui::Color32::LIGHT_GRAY,
            );
            painter.rect_filled(lane, egui::CornerRadius::same(2), PANEL);
            if track.clips.is_empty() && index == 0 && duration == 0 {
                painter.text(
                    lane.left_center() + egui::vec2(10.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    strings::TIMELINE_EMPTY_HINT,
                    egui::FontId::proportional(12.0),
                    egui::Color32::GRAY,
                );
            }
            for clip in &track.clips {
                let mut start = clip.timeline_start_frame as i64;
                let mut length = clip.duration_frames as i64;
                let mut source_start = clip.source_start_frame as i64;
                if let Some(drag) = drag.as_ref().filter(|drag| drag.linked.contains(&clip.id)) {
                    match drag.kind {
                        DragKind::Move => start += drag.delta,
                        DragKind::TrimHead => {
                            start += drag.delta;
                            source_start += drag.delta;
                            length -= drag.delta;
                        }
                        DragKind::TrimTail => length += drag.delta,
                    }
                }
                let x0 = x_of(start as f64);
                let x1 = x_of((start + length) as f64);
                let rect = egui::Rect::from_min_max(
                    egui::pos2(x0, lane.top() + 2.0),
                    egui::pos2(x1.max(x0 + 3.0), lane.bottom() - 2.0),
                );
                if rect.right() < content.left() || rect.left() > content.right() {
                    continue;
                }
                let selected = self.selected_clip.is_some_and(|selected| {
                    selected == clip.id
                        || linked_clip_ids(&self.project, selected).contains(&clip.id)
                });
                let base = match track.kind {
                    TrackKind::Video => VIDEO,
                    TrackKind::Audio => AUDIO,
                    TrackKind::Subtitle => TEAL,
                };
                let clip_painter = painter.with_clip_rect(rect.intersect(content));
                clip_painter.rect_filled(rect, egui::CornerRadius::same(4), base);
                if track.kind == TrackKind::Video
                    && let Some(texture) = self.assets.thumbnails.get(&clip.media_id)
                {
                    let size = texture.size_vec2();
                    let tile_height = rect.height() - 4.0;
                    let tile_width = (tile_height * size.x / size.y.max(1.0)).max(8.0);
                    let mut x = rect.left() + 2.0;
                    let visible_left = content.left() - tile_width;
                    if x < visible_left {
                        x += ((visible_left - x) / tile_width).floor() * tile_width;
                    }
                    while x < rect.right().min(content.right()) {
                        clip_painter.image(
                            texture.id(),
                            egui::Rect::from_min_size(
                                egui::pos2(x, rect.top() + 2.0),
                                egui::vec2(tile_width, tile_height),
                            ),
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::from_white_alpha(170),
                        );
                        x += tile_width + 2.0;
                    }
                }
                if track.kind == TrackKind::Audio
                    && let Some(waveform) = self.assets.waveforms.get(&clip.media_id)
                {
                    let max_rms = waveform.rms.iter().copied().fold(0.0001_f32, f32::max);
                    let bucket_seconds =
                        waveform.samples_per_bucket as f64 / f64::from(waveform.sample_rate);
                    let center = rect.center().y;
                    let half = rect.height() / 2.0 - 3.0;
                    let mut x = rect.left().max(content.left());
                    let right = rect.right().min(content.right());
                    while x < right {
                        let source_frame = source_start as f64 + f64::from((x - x0) / ppf);
                        let seconds = source_frame / f64::from(fps);
                        let bucket = (seconds / bucket_seconds).floor() as usize;
                        if let Some(value) = waveform.rms.get(bucket) {
                            let height = (value / max_rms).sqrt() * half;
                            clip_painter.line_segment(
                                [
                                    egui::pos2(x, center - height),
                                    egui::pos2(x, center + height),
                                ],
                                egui::Stroke::new(1.0, egui::Color32::from_rgb(150, 235, 200)),
                            );
                        }
                        x += 2.0;
                    }
                }
                let label = self
                    .project
                    .media_asset(clip.media_id)
                    .map_or_else(String::new, |media| file_label(&media.path));
                let link = if clip.link_group.is_some() {
                    "[V+A] "
                } else {
                    ""
                };
                clip_painter.text(
                    egui::pos2(rect.left().max(content.left()) + 5.0, rect.top() + 3.0),
                    egui::Align2::LEFT_TOP,
                    format!("{link}{label}"),
                    egui::FontId::proportional(11.0),
                    egui::Color32::WHITE,
                );
                if selected {
                    clip_painter.rect_stroke(
                        rect,
                        egui::CornerRadius::same(4),
                        egui::Stroke::new(2.0, egui::Color32::WHITE),
                        egui::StrokeKind::Inside,
                    );
                }

                // Interaction: body (select/move) and edges (trim). Edges are registered last
                // so they take priority where they overlap the body.
                let visible = rect.intersect(content);
                if visible.width() <= 0.0 {
                    continue;
                }
                let body_id = ui.scope_id().with(("clip", clip.id));
                let body = ui.interact(visible, body_id, egui::Sense::click_and_drag());
                let edge = EDGE_PIXELS.min(rect.width() / 3.0);
                let head_rect = egui::Rect::from_min_max(
                    rect.left_top(),
                    egui::pos2(rect.left() + edge, rect.bottom()),
                )
                .intersect(content);
                let tail_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.right() - edge, rect.top()),
                    rect.right_bottom(),
                )
                .intersect(content);
                let head = (head_rect.width() > 0.0).then(|| {
                    ui.interact(
                        head_rect,
                        ui.scope_id().with(("head", clip.id)),
                        egui::Sense::drag(),
                    )
                });
                let tail = (tail_rect.width() > 0.0).then(|| {
                    ui.interact(
                        tail_rect,
                        ui.scope_id().with(("tail", clip.id)),
                        egui::Sense::drag(),
                    )
                });
                let mut responses = vec![(DragKind::Move, body)];
                if let Some(head) = head {
                    responses.push((DragKind::TrimHead, head));
                }
                if let Some(tail) = tail {
                    responses.push((DragKind::TrimTail, tail));
                }
                for (kind, response) in responses {
                    if response.hovered() || response.dragged() {
                        clip_hit = true;
                        let icon = match kind {
                            DragKind::Move => egui::CursorIcon::Grab,
                            DragKind::TrimHead | DragKind::TrimTail => {
                                egui::CursorIcon::ResizeHorizontal
                            }
                        };
                        ui.ctx().set_cursor_icon(icon);
                    }
                    if response.clicked() {
                        self.selected_clip = Some(clip.id);
                        clip_hit = true;
                    }
                    if response.drag_started() {
                        self.selected_clip = Some(clip.id);
                        let limits = match kind {
                            DragKind::Move => move_limits(&self.project, clip.id),
                            DragKind::TrimHead => {
                                trim_limits(&self.project, clip.id, TrimEdge::Head)
                            }
                            DragKind::TrimTail => {
                                trim_limits(&self.project, clip.id, TrimEdge::Tail)
                            }
                        };
                        if let (Ok(limits), Some(pos)) = (limits, response.interact_pointer_pos()) {
                            new_drag = Some(DragState {
                                kind,
                                clip_id: clip.id,
                                linked: linked_clip_ids(&self.project, clip.id)
                                    .into_iter()
                                    .collect(),
                                origin_x: pos.x,
                                limits,
                                delta: 0,
                            });
                        }
                    }
                    if response.drag_stopped()
                        && let Some(drag) = drag.as_ref().filter(|drag| drag.clip_id == clip.id)
                    {
                        finished_drag = Some(drag.clone());
                    }
                }
            }
        }

        // Update an active drag with snapping.
        if let Some(mut state) = new_drag.or(drag.clone())
            && finished_drag.is_none()
        {
            if let Some(pos) = ui.input(|input| input.pointer.interact_pos()) {
                let mut delta = ((pos.x - state.origin_x) / ppf).round() as i64;
                if self.snapping
                    && let Some((track_index, clip_index)) = find_clip(&self.project, state.clip_id)
                {
                    let clip = &self.project.tracks[track_index].clips[clip_index];
                    let start = clip.timeline_start_frame as i64;
                    let end = start + clip.duration_frames as i64;
                    let moving_edges: Vec<i64> = match state.kind {
                        DragKind::Move => vec![start, end],
                        DragKind::TrimHead => vec![start],
                        DragKind::TrimTail => vec![end],
                    };
                    let mut best: Option<(f32, i64)> = None;
                    for edge in moving_edges {
                        for target in &snap_targets {
                            let candidate = *target as i64 - edge;
                            let distance = ((candidate - delta) as f32 * ppf).abs();
                            if distance <= SNAP_PIXELS && best.is_none_or(|(d, _)| distance < d) {
                                best = Some((distance, candidate));
                            }
                        }
                    }
                    if let Some((_, snapped)) = best {
                        delta = snapped;
                    }
                }
                state.delta = delta.clamp(state.limits.0, state.limits.1);
            }
            self.drag = Some(state);
        }
        if let Some(state) = finished_drag {
            self.drag = None;
            if state.delta != 0 {
                let command = match state.kind {
                    DragKind::Move => move_command(&self.project, state.clip_id, state.delta),
                    DragKind::TrimHead => {
                        trim_command(&self.project, state.clip_id, TrimEdge::Head, state.delta)
                    }
                    DragKind::TrimTail => {
                        trim_command(&self.project, state.clip_id, TrimEdge::Tail, state.delta)
                    }
                };
                let message = match state.kind {
                    DragKind::Move => "Clipe movido",
                    DragKind::TrimHead => "Início do clipe ajustado",
                    DragKind::TrimTail => "Fim do clipe ajustado",
                };
                self.run_edit(command, message);
            }
        } else if self.drag.is_some() && !ui.input(|input| input.pointer.any_down()) {
            self.drag = None;
        }

        // Click / drag on empty space or the ruler moves the playhead.
        if !clip_hit
            && self.drag.is_none()
            && (response.clicked() || response.dragged())
            && let Some(pos) = response.interact_pointer_pos()
            && pos.x >= content_left
        {
            self.seek(frame_of(pos.x));
            if response.clicked() && pos.y > ruler.bottom() {
                self.selected_clip = None;
            }
        }

        // Playhead.
        let playhead_x = x_of(self.playhead as f64);
        if playhead_x >= content.left() && playhead_x <= content.right() {
            painter.line_segment(
                [
                    egui::pos2(playhead_x, ruler.top()),
                    egui::pos2(playhead_x, area.bottom()),
                ],
                egui::Stroke::new(1.5, egui::Color32::WHITE),
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(playhead_x - 6.0, ruler.top()),
                    egui::pos2(playhead_x + 6.0, ruler.top()),
                    egui::pos2(playhead_x, ruler.top() + 9.0),
                ],
                TEAL,
                egui::Stroke::NONE,
            ));
        }

        // Horizontal scroll bar.
        if content_width > content.width() * 0.5 {
            let bar_area = egui::Rect::from_min_max(
                egui::pos2(content_left, area.bottom() - 8.0),
                egui::pos2(area.right(), area.bottom() - 2.0),
            );
            let total = content_width + content.width() * 0.5;
            let thumb_width = (bar_area.width() * content.width() / total).max(24.0);
            let thumb_x =
                bar_area.left() + (bar_area.width() - thumb_width) * (scroll / max_scroll.max(1.0));
            let thumb = egui::Rect::from_min_size(
                egui::pos2(thumb_x, bar_area.top()),
                egui::vec2(thumb_width, bar_area.height()),
            );
            painter.rect_filled(bar_area, egui::CornerRadius::same(3), PANEL);
            painter.rect_filled(thumb, egui::CornerRadius::same(3), PANEL_ALT);
            let bar = ui.interact(
                bar_area,
                ui.scope_id().with("timeline-scroll"),
                egui::Sense::drag(),
            );
            if bar.dragged() {
                let movable = (bar_area.width() - thumb_width).max(1.0);
                self.scroll_px += bar.drag_delta().x * max_scroll / movable;
            }
        }
    }

    fn export_window(&mut self, ctx: &egui::Context) {
        let Some(settings) = &mut self.export_settings else {
            return;
        };
        let mut open = true;
        let mut start = false;
        let mut choose = false;
        let mut cancel = false;
        let mut close = false;
        let job_state = self.export_job.as_ref().map(ExportJob::snapshot);
        let running = matches!(job_state, Some(JobState::Running { .. }));
        let format = self.project.format;
        let duration = format.frames_to_seconds(self.project.duration_frames());
        egui::Window::new(strings::EXPORT_TITLE)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.add_enabled_ui(!running, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Arquivo:");
                        ui.add(
                            egui::TextEdit::singleline(&mut settings.output).desired_width(300.0),
                        );
                        if ui.button(strings::CHOOSE).clicked() {
                            choose = true;
                        }
                    });
                    ui.label(format!(
                        "{}×{} · {:.3} fps · {} · AAC 48 kHz · yuv420p · BT.709 · faststart",
                        format.width,
                        format.height,
                        format.frames_per_second(),
                        format_time(duration)
                    ));
                    ui.separator();
                    ui.radio_value(
                        &mut settings.profile,
                        Profile::Quality,
                        strings::EXPORT_QUALITY,
                    );
                    if settings.profile == Profile::Quality {
                        ui.indent("quality", |ui| {
                            ui.add(
                                egui::Slider::new(&mut settings.crf, 14..=32)
                                    .text("CRF (menor = melhor)"),
                            );
                            egui::ComboBox::from_label("Preset")
                                .selected_text(settings.preset)
                                .show_ui(ui, |ui| {
                                    for preset in ["veryfast", "faster", "fast", "medium", "slow"] {
                                        ui.selectable_value(&mut settings.preset, preset, preset);
                                    }
                                });
                        });
                    }
                    ui.add_enabled_ui(settings.vaapi_available, |ui| {
                        ui.radio_value(&mut settings.profile, Profile::Fast, strings::EXPORT_FAST);
                    })
                    .response
                    .on_disabled_hover_text(format!("{} não encontrado", export::VAAPI_DEVICE));
                    if settings.profile == Profile::Fast {
                        ui.indent("fast", |ui| {
                            ui.add(
                                egui::Slider::new(&mut settings.qp, 14..=36)
                                    .text("QP (menor = melhor)"),
                            );
                        });
                    }
                });
                ui.separator();
                match &job_state {
                    Some(JobState::Running { fraction }) => {
                        ui.add(
                            egui::ProgressBar::new(*fraction as f32)
                                .show_percentage()
                                .animate(true),
                        );
                        if ui.button(strings::CANCEL).clicked() {
                            cancel = true;
                        }
                    }
                    Some(JobState::Done { summary }) => {
                        ui.colored_label(TEAL, "Exportado e conferido com ffprobe:");
                        ui.label(summary);
                        if let Some(job) = &self.export_job {
                            ui.small(format!(
                                "{} · {:.1} s",
                                job.output.display(),
                                job.started.elapsed().as_secs_f32()
                            ));
                        }
                    }
                    Some(JobState::Failed { error }) => {
                        ui.colored_label(egui::Color32::LIGHT_RED, "Falha no export:");
                        egui::ScrollArea::vertical()
                            .max_height(120.0)
                            .show(ui, |ui| {
                                ui.monospace(error);
                            });
                    }
                    None => {}
                }
                ui.horizontal(|ui| {
                    if !running
                        && ui
                            .add(egui::Button::new(strings::EXPORT_START).fill(ACCENT))
                            .clicked()
                    {
                        start = true;
                    }
                    if !running && ui.button(strings::CLOSE).clicked() {
                        close = true;
                    }
                });
            });
        if choose {
            self.open_dialog(Purpose::ExportVideo);
        }
        if cancel && let Some(job) = &self.export_job {
            job.cancel();
        }
        if start {
            self.start_export(ctx);
        }
        if (!open || close) && !running {
            self.export_settings = None;
            self.export_job = None;
        }
    }

    fn start_export(&mut self, ctx: &egui::Context) {
        let Some(settings) = &self.export_settings else {
            return;
        };
        let encoder = settings.encoder();
        let mut output = PathBuf::from(settings.output.trim());
        if output.as_os_str().is_empty() {
            self.status = "Escolha o arquivo de saída".into();
            return;
        }
        if output.extension().is_none() {
            output.set_extension("mp4");
        }
        if let Some(parent) = output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            && let Err(error) = fs::create_dir_all(parent)
        {
            self.status = format!("Não foi possível criar a pasta de saída: {error}");
            return;
        }
        let format = self.project.format;
        let job = match TimelineRender::from_project(
            &self.project,
            0,
            self.project.duration_frames(),
            format.width,
            format.height,
        ) {
            Ok(job) => job,
            Err(error) => {
                self.status = format!("Não foi possível exportar: {error}");
                return;
            }
        };
        self.stop_playback();
        self.export_job = Some(ExportJob::start(ctx.clone(), job, encoder, output));
        self.status = "Exportando…".into();
    }

    fn unsaved_modal(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending_action else {
            return;
        };
        let mut choice = None;
        egui::Window::new(strings::UNSAVED_TITLE)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(strings::UNSAVED_BODY);
                ui.horizontal(|ui| {
                    if ui.button(strings::SAVE).clicked() {
                        choice = Some(0);
                    }
                    if ui.button(strings::DISCARD).clicked() {
                        choice = Some(1);
                    }
                    if ui.button(strings::CANCEL).clicked() {
                        choice = Some(2);
                    }
                });
            });
        match choice {
            Some(0) => {
                self.pending_action = None;
                if self.project_path.is_some() {
                    if self.save() {
                        self.perform(action);
                    }
                } else {
                    self.after_save = Some(action);
                    self.open_dialog(Purpose::SaveProject);
                }
            }
            Some(1) => {
                self.pending_action = None;
                self.dirty = false;
                self.perform(action);
            }
            Some(2) => self.pending_action = None,
            _ => {}
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() || self.pending_action.is_some() {
            return;
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let input = ctx.input(|input| {
            let ctrl = input.modifiers.command;
            let shift = input.modifiers.shift;
            let pressed = |key| input.key_pressed(key);
            (
                ctrl,
                shift,
                [
                    pressed(egui::Key::Space),
                    pressed(egui::Key::S),
                    pressed(egui::Key::B),
                    pressed(egui::Key::Delete) || pressed(egui::Key::Backspace),
                    pressed(egui::Key::Z),
                    pressed(egui::Key::Y),
                    pressed(egui::Key::O),
                    pressed(egui::Key::I),
                    pressed(egui::Key::E),
                    pressed(egui::Key::ArrowLeft),
                    pressed(egui::Key::ArrowRight),
                    pressed(egui::Key::Home),
                    pressed(egui::Key::End),
                    pressed(egui::Key::Plus) || pressed(egui::Key::Equals),
                    pressed(egui::Key::Minus),
                    pressed(egui::Key::F3),
                    pressed(egui::Key::N),
                    pressed(egui::Key::Q),
                ],
            )
        });
        let (ctrl, shift, keys) = input;
        let [
            space,
            s,
            b,
            delete,
            z,
            y,
            o,
            i,
            e,
            left,
            right,
            home,
            end,
            plus,
            minus,
            f3,
            n,
            q,
        ] = keys;
        if ctrl && q {
            self.request_action(PendingAction::Close);
        }
        if space {
            self.toggle_playback(ctx);
        }
        if ctrl && s {
            if shift {
                self.open_dialog(Purpose::SaveProject);
            } else {
                self.save();
            }
        } else if (s && !ctrl) || (ctrl && b) {
            self.split();
        }
        if delete {
            self.delete(shift);
        }
        if ctrl && z {
            if shift {
                self.redo();
            } else {
                self.undo();
            }
        }
        if ctrl && y {
            self.redo();
        }
        if ctrl && o {
            self.request_action(PendingAction::Open);
        }
        if ctrl && n {
            self.request_action(PendingAction::New);
        }
        if ctrl && i {
            self.open_dialog(Purpose::ImportMedia);
        }
        if ctrl && e {
            self.open_export();
        }
        let step = if shift { self.fps().round() as u64 } else { 1 };
        if left {
            self.seek(self.playhead.saturating_sub(step));
        }
        if right {
            self.seek(self.playhead + step);
        }
        if home {
            self.seek(0);
        }
        if end {
            self.seek(self.project.duration_frames());
        }
        if plus {
            self.pixels_per_second = (self.pixels_per_second * 1.25).min(1200.0);
        }
        if minus {
            self.pixels_per_second = (self.pixels_per_second / 1.25).max(2.0);
        }
        if f3 {
            self.hud.visible = !self.hud.visible;
        }
    }

    fn run_autotest(&mut self, ctx: &egui::Context) {
        let Some(test) = &mut self.autotest else {
            return;
        };
        let elapsed = test.started.elapsed().as_secs_f64();
        let play = test.play_at.is_some_and(|at| elapsed >= at)
            && !test.played
            && self.pending_probes.is_empty()
            && self.project.duration_frames() > 0;
        let quit = test.quit_at.is_some_and(|at| elapsed >= at);
        if play {
            test.played = true;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        if play && self.playback.is_none() {
            self.toggle_playback(ctx);
        }
        if quit {
            self.autotest = None;
            self.stop_playback();
            self.dirty = false;
            self.pending_action = None;
            self.allow_close = true;
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self
            .project_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map_or_else(
                || "sem título".to_owned(),
                |name| name.to_string_lossy().into_owned(),
            );
        let title = format!(
            "{PRODUCT_NAME} {} — {name}{}",
            env!("CARGO_PKG_VERSION"),
            if self.dirty { " •" } else { "" }
        );
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

impl eframe::App for DcutApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let frame_start = Instant::now();
        if let Some(last) = self.hud.last_frame {
            Hud::push(
                &mut self.hud.frame_times,
                frame_start.duration_since(last).as_secs_f32() * 1000.0,
            );
        }
        self.hud.last_frame = Some(frame_start);
        let ctx = ui.ctx().clone();

        if ctx.input(|input| input.viewport().close_requested()) && !self.allow_close {
            if self.dirty {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending_action = Some(PendingAction::Close);
            } else {
                self.allow_close = true;
            }
        }
        if self.allow_close && self.pending_action.is_none() {
            self.stop_playback();
            if let Some(job) = &self.export_job {
                job.cancel();
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        self.run_autotest(&ctx);
        self.receive_probes();
        self.assets.poll(&ctx);
        self.receive_dropped_files(&ctx);
        self.shortcuts(&ctx);
        self.toolbar(ui);
        self.timeline_panel(ui);
        self.media_panel(ui);
        self.inspector_panel(ui);
        self.update_preview(&ctx);
        self.preview_panel(ui);
        self.handle_dialog(&ctx);
        self.export_window(&ctx);
        self.unsaved_modal(&ctx);
        self.update_title(&ctx);
        if self.export_job.is_some() || !self.pending_probes.is_empty() || self.drag.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if self.hud.visible {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        Hud::push(
            &mut self.hud.ui_times,
            frame_start.elapsed().as_secs_f32() * 1000.0,
        );
    }

    fn on_exit(&mut self) {
        self.stop_playback();
        if let Some(job) = &self.export_job {
            job.cancel();
        }
        tracing::info!("D-cut exiting");
    }
}

/// Starts the native editor window and initializes file-only local logging.
pub fn run() -> Result<()> {
    let log_path = initialize_file_logging()?;
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(%info, "panic");
        default_hook(info);
    }));
    tracing::info!(version = env!("CARGO_PKG_VERSION"), log = %log_path.display(), "D-cut starting");
    let startup_files: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .filter_map(|path| path.canonicalize().ok())
        .collect();
    let title = format!("{PRODUCT_NAME} {}", env!("CARGO_PKG_VERSION"));
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(title)
        .with_app_id("d-cut")
        .with_inner_size([1366.0, 740.0])
        .with_min_inner_size([960.0, 600.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!(
        "../../../assets/branding/d-cut-icon-256.png"
    )) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        PRODUCT_NAME,
        options,
        Box::new(|context| Ok(Box::new(DcutApp::new(context, startup_files)))),
    )
    .map_err(|error| anyhow::anyhow!("could not run the D-cut desktop window: {error}"))?;
    tracing::info!("D-cut window closed cleanly");
    Ok(())
}

fn initialize_file_logging() -> Result<PathBuf> {
    let state_root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
    let log_directory = state_root.join("d-cut/logs");
    fs::create_dir_all(&log_directory)
        .with_context(|| format!("could not create log directory {}", log_directory.display()))?;
    let log_path = log_directory.join("d-cut.log");
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .context("could not open the D-cut log file")?;
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(log_file)
        .try_init()
        .map_err(anyhow::Error::msg)
        .context("could not initialize local file logging")?;
    Ok(log_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_media::{Rational, VideoStreamInfo};

    #[test]
    fn preview_sizes_are_even_and_fit_the_long_side() {
        assert_eq!(fit_long_side(1920, 1080, 640), (640, 360));
        assert_eq!(fit_long_side(1080, 1920, 640), (360, 640));
        assert_eq!(fit_long_side(1080, 1350, 426), (340, 426));
        assert_eq!(fit_long_side(320, 240, 640), (320, 240));
    }

    #[test]
    fn phone_media_selects_vertical_project_format() {
        let info = MediaInfo {
            duration_seconds: 3.0,
            video: Some(VideoStreamInfo {
                codec: "h264".into(),
                width: 1080,
                height: 1920,
                rotation_degrees: 90,
                frame_rate: Some(Rational {
                    numerator: 30000,
                    denominator: 1001,
                }),
                time_base: None,
            }),
            audio: None,
        };
        let format = format_for_media(&info).expect("format");
        assert_eq!((format.width, format.height), (1080, 1920));
        assert_eq!(
            (format.frame_rate_numerator, format.frame_rate_denominator),
            (30000, 1001)
        );
    }

    #[test]
    fn time_formatting() {
        assert_eq!(format_time(0.0), "00:00.00");
        assert_eq!(format_time(65.5), "01:05.50");
    }
}
