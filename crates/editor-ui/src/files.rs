// SPDX-License-Identifier: AGPL-3.0-or-later

//! Built-in file chooser. It works on any compositor without xdg-desktop-portal, GTK or KDE.

use std::{
    fs,
    path::{Path, PathBuf},
};

use eframe::egui;

use crate::strings;

pub const MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "webm", "avi", "m4v", "mts", "m2ts", "3gp", "mpg", "mpeg", "ts", "mp3",
    "wav", "flac", "ogg", "opus", "m4a", "aac", "wma",
];
pub const PROJECT_EXTENSIONS: &[&str] = &["dcut", "json"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    ImportMedia,
    OpenProject,
    SaveProject,
    ExportVideo,
}

impl Purpose {
    fn title(self) -> &'static str {
        match self {
            Self::ImportMedia => "Importar mídia",
            Self::OpenProject => "Abrir projeto",
            Self::SaveProject => "Salvar projeto",
            Self::ExportVideo => "Arquivo de saída",
        }
    }

    fn is_save(self) -> bool {
        matches!(self, Self::SaveProject | Self::ExportVideo)
    }

    fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::ImportMedia => MEDIA_EXTENSIONS,
            Self::OpenProject | Self::SaveProject => PROJECT_EXTENSIONS,
            Self::ExportVideo => &["mp4"],
        }
    }

    fn default_extension(self) -> &'static str {
        match self {
            Self::ExportVideo => "mp4",
            _ => "dcut",
        }
    }
}

pub enum Outcome {
    Pending,
    Cancelled,
    Picked(Vec<PathBuf>),
}

struct Entry {
    path: PathBuf,
    name: String,
    is_dir: bool,
}

pub struct FileDialog {
    pub purpose: Purpose,
    directory: PathBuf,
    location: String,
    entries: Vec<Entry>,
    selected: Vec<PathBuf>,
    file_name: String,
    show_all: bool,
    show_hidden: bool,
    error: Option<String>,
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn matches_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extensions
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(extension))
        })
}

impl FileDialog {
    pub fn new(purpose: Purpose, directory: PathBuf, file_name: String) -> Self {
        let directory = if directory.is_dir() {
            directory
        } else {
            home_dir()
        };
        let mut dialog = Self {
            purpose,
            location: directory.display().to_string(),
            directory,
            entries: Vec::new(),
            selected: Vec::new(),
            file_name,
            show_all: false,
            show_hidden: false,
            error: None,
        };
        dialog.refresh();
        dialog
    }

    fn change_directory(&mut self, directory: PathBuf) {
        self.directory = directory;
        self.location = self.directory.display().to_string();
        self.selected.clear();
        self.refresh();
    }

    fn refresh(&mut self) {
        self.entries.clear();
        self.error = None;
        let read = match fs::read_dir(&self.directory) {
            Ok(read) => read,
            Err(error) => {
                self.error = Some(format!("Não foi possível abrir a pasta: {error}"));
                return;
            }
        };
        let extensions = self.purpose.extensions();
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if !self.show_hidden && name.starts_with('.') {
                continue;
            }
            let is_dir = path.is_dir();
            if !is_dir && !self.show_all && !matches_extension(&path, extensions) {
                continue;
            }
            self.entries.push(Entry { path, name, is_dir });
        }
        self.entries.sort_by(|left, right| {
            right
                .is_dir
                .cmp(&left.is_dir)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
    }

    fn save_target(&self) -> Option<PathBuf> {
        let name = self.file_name.trim();
        if name.is_empty() {
            return None;
        }
        let mut path = if Path::new(name).is_absolute() {
            PathBuf::from(name)
        } else {
            self.directory.join(name)
        };
        if path.extension().is_none() {
            path.set_extension(self.purpose.default_extension());
        }
        Some(path)
    }

    /// Draws the dialog. Call every frame while it is open.
    pub fn show(&mut self, ctx: &egui::Context) -> Outcome {
        let mut outcome = Outcome::Pending;
        let screen = ctx.content_rect();
        let size = egui::vec2(
            (screen.width() - 80.0).clamp(360.0, 720.0),
            (screen.height() - 120.0).clamp(280.0, 520.0),
        );
        egui::Window::new(self.purpose.title())
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let mut navigate = None;
                ui.horizontal(|ui| {
                    if ui.button("Acima").on_hover_text("Pasta acima").clicked()
                        && let Some(parent) = self.directory.parent()
                    {
                        navigate = Some(parent.to_owned());
                    }
                    if ui.button("Início").on_hover_text("Pasta pessoal").clicked() {
                        navigate = Some(home_dir());
                    }
                    for (label, sub) in [("Vídeos", "Videos"), ("Downloads", "Downloads")] {
                        let candidate = home_dir().join(sub);
                        if candidate.is_dir() && ui.button(label).clicked() {
                            navigate = Some(candidate);
                        }
                    }
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.location)
                            .desired_width(ui.available_width()),
                    );
                    if response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    {
                        let typed = PathBuf::from(self.location.trim());
                        if typed.is_dir() {
                            navigate = Some(typed);
                        } else if typed.is_file() && !self.purpose.is_save() {
                            outcome = Outcome::Picked(vec![typed]);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    let mut changed = ui
                        .checkbox(&mut self.show_all, "Mostrar todos os arquivos")
                        .changed();
                    changed |= ui
                        .checkbox(&mut self.show_hidden, "Mostrar ocultos")
                        .changed();
                    if changed {
                        self.refresh();
                    }
                });
                ui.separator();
                let footer = if self.purpose.is_save() { 64.0 } else { 36.0 };
                let list_height = (ui.available_height() - footer).max(80.0);
                egui::ScrollArea::vertical()
                    .max_height(list_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if let Some(error) = &self.error {
                            ui.colored_label(egui::Color32::LIGHT_RED, error);
                        }
                        for entry in &self.entries {
                            let label = if entry.is_dir {
                                format!("[pasta] {}", entry.name)
                            } else {
                                entry.name.clone()
                            };
                            let selected = self.selected.contains(&entry.path);
                            let response = ui.selectable_label(selected, label);
                            if response.double_clicked() {
                                if entry.is_dir {
                                    navigate = Some(entry.path.clone());
                                } else if !self.purpose.is_save() {
                                    outcome = Outcome::Picked(vec![entry.path.clone()]);
                                } else {
                                    self.file_name = entry.name.clone();
                                }
                            } else if response.clicked() {
                                if entry.is_dir {
                                    continue;
                                }
                                if self.purpose.is_save() {
                                    self.file_name = entry.name.clone();
                                } else if self.purpose == Purpose::ImportMedia
                                    && ui.input(|input| input.modifiers.ctrl)
                                {
                                    if selected {
                                        self.selected.retain(|path| path != &entry.path);
                                    } else {
                                        self.selected.push(entry.path.clone());
                                    }
                                } else {
                                    self.selected = vec![entry.path.clone()];
                                }
                            }
                        }
                        if self.entries.is_empty() && self.error.is_none() {
                            ui.weak("(pasta vazia ou sem arquivos compatíveis)");
                        }
                    });
                ui.separator();
                if self.purpose.is_save() {
                    ui.horizontal(|ui| {
                        ui.label("Nome:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.file_name)
                                .desired_width(ui.available_width()),
                        );
                    });
                }
                ui.horizontal(|ui| {
                    if self.purpose == Purpose::ImportMedia {
                        ui.weak("Ctrl+clique seleciona vários");
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let confirm = match self.purpose {
                            Purpose::ImportMedia => "Importar",
                            Purpose::OpenProject => "Abrir",
                            Purpose::SaveProject => "Salvar",
                            Purpose::ExportVideo => "Usar este arquivo",
                        };
                        let enabled = if self.purpose.is_save() {
                            self.save_target().is_some()
                        } else {
                            !self.selected.is_empty()
                        };
                        if ui
                            .add_enabled(enabled, egui::Button::new(confirm))
                            .clicked()
                        {
                            outcome = if self.purpose.is_save() {
                                self.save_target()
                                    .map_or(Outcome::Pending, |path| Outcome::Picked(vec![path]))
                            } else {
                                Outcome::Picked(self.selected.clone())
                            };
                        }
                        if ui.button(strings::CANCEL).clicked() {
                            outcome = Outcome::Cancelled;
                        }
                    });
                });
                if let Some(directory) = navigate {
                    self.change_directory(directory);
                }
            });
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            outcome = Outcome::Cancelled;
        }
        outcome
    }
}
