// SPDX-License-Identifier: AGPL-3.0-or-later

//! Versioned JSON project persistence. Media is referenced, never copied into the project file.

use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{BufReader, BufWriter},
    path::{Path, PathBuf},
};

use editor_core::{CoreError, Project};
use thiserror::Error;

pub mod subtitles;
pub use subtitles::{SubtitleImportError, parse_srt};

/// Errors returned while reading or writing a D-cut project.
#[derive(Debug, Error)]
pub enum ProjectFileError {
    #[error("could not access project file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid project JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("project validation failed: {0}")]
    Invalid(#[from] CoreError),
    #[error("project path has no file name: {0}")]
    InvalidPath(PathBuf),
}

/// Loads, parses, and validates a project document.
/// Relative media paths are resolved against the project file's directory, so the returned
/// project always holds absolute media paths.
pub fn load(path: impl AsRef<Path>) -> Result<Project, ProjectFileError> {
    let path = path.as_ref();
    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|source| ProjectFileError::Io {
            path: path.to_owned(),
            source,
        })?;
    let mut project: Project = serde_json::from_reader(BufReader::new(file))?;
    project.validate()?;
    let base = project_directory(path);
    for media in &mut project.media {
        media.path = resolve_media_path(&base, &media.path);
    }
    for proxy in &mut project.proxies {
        proxy.path = resolve_media_path(&base, &proxy.path);
    }
    Ok(project)
}

/// Validates and atomically replaces a project file using a temporary sibling file.
/// Absolute media paths are stored relative to the project file's directory.
pub fn save(path: impl AsRef<Path>, project: &Project) -> Result<(), ProjectFileError> {
    let path = path.as_ref();
    project.validate()?;
    let base = project_directory(path);
    let mut project = project.clone();
    for media in &mut project.media {
        media.path = portable_media_path(&base, &media.path);
    }
    for proxy in &mut project.proxies {
        proxy.path = portable_media_path(&base, &proxy.path);
    }
    let project = &project;
    let temporary_path =
        temporary_sibling(path).ok_or_else(|| ProjectFileError::InvalidPath(path.to_owned()))?;
    let result = write_temporary(&temporary_path, project).and_then(|()| {
        fs::rename(&temporary_path, path).map_err(|source| ProjectFileError::Io {
            path: path.to_owned(),
            source,
        })
    });
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

fn project_directory(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map(|current| current.join(path))
            .unwrap_or_else(|_| path.to_owned())
    };
    absolute
        .parent()
        .map(Path::to_owned)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn resolve_media_path(base: &Path, stored: &str) -> String {
    let stored_path = Path::new(stored);
    if stored_path.is_absolute() {
        return stored.to_owned();
    }
    normalize(&base.join(stored_path))
        .to_string_lossy()
        .into_owned()
}

fn portable_media_path(base: &Path, media: &str) -> String {
    let media_path = Path::new(media);
    if !media_path.is_absolute() {
        return media.to_owned();
    }
    relative_path(base, &normalize(media_path))
        .map(|relative| relative.to_string_lossy().into_owned())
        .unwrap_or_else(|| media.to_owned())
}

/// Lexically removes `.` and `..` components (does not touch the file system).
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

/// Relative path from directory `base` to `target`; both must be absolute.
fn relative_path(base: &Path, target: &Path) -> Option<PathBuf> {
    let base = normalize(base);
    let base_parts: Vec<_> = base.components().collect();
    let target_parts: Vec<_> = target.components().collect();
    if base_parts.first() != target_parts.first() {
        return None;
    }
    let common = base_parts
        .iter()
        .zip(&target_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let mut relative = PathBuf::new();
    for _ in common..base_parts.len() {
        relative.push("..");
    }
    for part in &target_parts[common..] {
        relative.push(part.as_os_str());
    }
    Some(relative)
}

fn write_temporary(path: &Path, project: &Project) -> Result<(), ProjectFileError> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| ProjectFileError::Io {
            path: path.to_owned(),
            source,
        })?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, project)?;
    use std::io::Write;
    writer.flush().map_err(|source| ProjectFileError::Io {
        path: path.to_owned(),
        source,
    })?;
    writer
        .get_ref()
        .sync_all()
        .map_err(|source| ProjectFileError::Io {
            path: path.to_owned(),
            source,
        })?;
    Ok(())
}

fn temporary_sibling(path: &Path) -> Option<PathBuf> {
    let mut name: OsString = path.file_name()?.to_os_string();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    name.push(format!(".tmp-{}-{stamp}", std::process::id()));
    Some(path.with_file_name(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::Project;

    fn temporary_path() -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("d-cut-project-{}-{stamp}.json", std::process::id()))
    }

    #[test]
    fn project_json_round_trip() {
        let path = temporary_path();
        let project = Project::default();
        save(&path, &project).expect("valid project saves");
        let loaded = load(&path).expect("saved project loads");
        assert_eq!(loaded, project);
        fs::remove_file(path).expect("test project is removed");
    }

    #[test]
    fn media_paths_are_stored_relative_and_resolved_on_load() {
        use editor_core::{Clip, MediaAsset, Track, TrackKind};
        let directory = std::env::temp_dir().join(format!(
            "d-cut-rel-{}-{}",
            std::process::id(),
            temporary_path().display().to_string().len()
        ));
        let project_dir = directory.join("projects");
        fs::create_dir_all(&project_dir).expect("dirs");
        let media_path = directory.join("media/clip one.mp4");
        let project = Project {
            media: vec![MediaAsset {
                id: 1,
                path: media_path.display().to_string(),
                content_hash: None,
                duration_frames: 10,
                width: 2,
                height: 2,
                has_video: true,
                has_audio: false,
                rotation_degrees: 0,
            }],
            tracks: vec![Track {
                id: 2,
                name: "V1".into(),
                kind: TrackKind::Video,
                clips: vec![Clip {
                    id: 3,
                    media_id: 1,
                    timeline_start_frame: 0,
                    source_start_frame: 0,
                    duration_frames: 10,
                    link_group: None,
                }],
            }],
            ..Project::default()
        };
        let project_path = project_dir.join("test.dcut");
        save(&project_path, &project).expect("saves");
        let raw = fs::read_to_string(&project_path).expect("reads");
        assert!(raw.contains("../media/clip one.mp4"), "{raw}");
        let loaded = load(&project_path).expect("loads");
        assert_eq!(loaded, project);
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn unsupported_schema_version_is_rejected() {
        let path = temporary_path();
        fs::write(&path, r#"{"format_version":99,"format":{"width":1,"height":1,"frame_rate_numerator":1,"frame_rate_denominator":1},"media":[],"tracks":[],"text_overlays":[],"animations":[],"transitions":[],"proxies":[]}"#)
            .expect("fixture writes");
        assert!(matches!(
            load(&path),
            Err(ProjectFileError::Invalid(
                CoreError::UnsupportedProjectVersion(99)
            ))
        ));
        fs::remove_file(path).expect("test project is removed");
    }
}
