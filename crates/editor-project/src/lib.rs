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
pub fn load(path: impl AsRef<Path>) -> Result<Project, ProjectFileError> {
    let path = path.as_ref();
    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|source| ProjectFileError::Io {
            path: path.to_owned(),
            source,
        })?;
    let project: Project = serde_json::from_reader(BufReader::new(file))?;
    project.validate()?;
    Ok(project)
}

/// Validates and atomically replaces a project file using a temporary sibling file.
pub fn save(path: impl AsRef<Path>, project: &Project) -> Result<(), ProjectFileError> {
    let path = path.as_ref();
    project.validate()?;
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
