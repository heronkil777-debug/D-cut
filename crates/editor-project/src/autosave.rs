// SPDX-License-Identifier: AGPL-3.0-or-later

//! Autosave and crash recovery. Each running editor writes `autosave-<pid>.dcut` plus a small
//! metadata file; a clean exit deletes both. Files whose process no longer exists are offered
//! for recovery on the next start.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use editor_core::Project;
use serde::{Deserialize, Serialize};

use crate::{ProjectFileError, load, save};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutosaveMeta {
    pub pid: u32,
    pub original: Option<PathBuf>,
    pub saved_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Recoverable {
    pub project_path: PathBuf,
    pub meta: AutosaveMeta,
}

pub fn autosave_dir() -> PathBuf {
    std::env::var_os("D_CUT_AUTOSAVE_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
                })
                .map(|state| state.join("d-cut/autosave"))
        })
        .unwrap_or_else(|| std::env::temp_dir().join("d-cut-autosave"))
}

fn paths(dir: &Path, pid: u32) -> (PathBuf, PathBuf) {
    (
        dir.join(format!("autosave-{pid}.dcut")),
        dir.join(format!("autosave-{pid}.json")),
    )
}

fn io_error(path: &Path, source: std::io::Error) -> ProjectFileError {
    ProjectFileError::Io {
        path: path.to_owned(),
        source,
    }
}

/// Writes this process's autosave (atomic, like a normal save).
pub fn write_autosave(
    project: &Project,
    original: Option<&Path>,
) -> Result<PathBuf, ProjectFileError> {
    write_autosave_in(&autosave_dir(), std::process::id(), project, original)
}

pub fn write_autosave_in(
    dir: &Path,
    pid: u32,
    project: &Project,
    original: Option<&Path>,
) -> Result<PathBuf, ProjectFileError> {
    fs::create_dir_all(dir).map_err(|source| io_error(dir, source))?;
    let (project_path, meta_path) = paths(dir, pid);
    save(&project_path, project)?;
    let meta = AutosaveMeta {
        pid,
        original: original.map(Path::to_owned),
        saved_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |age| age.as_secs()),
    };
    let json = serde_json::to_vec_pretty(&meta)?;
    fs::write(&meta_path, json).map_err(|source| io_error(&meta_path, source))?;
    Ok(project_path)
}

/// Removes this process's autosave (clean exit or after an explicit save).
pub fn clear_autosave() {
    clear_autosave_in(&autosave_dir(), std::process::id());
}

pub fn clear_autosave_in(dir: &Path, pid: u32) {
    let (project_path, meta_path) = paths(dir, pid);
    let _ = fs::remove_file(project_path);
    let _ = fs::remove_file(meta_path);
}

fn process_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// Autosaves left by editors that are no longer running (newest first).
pub fn find_recoverable() -> Vec<Recoverable> {
    find_recoverable_in(&autosave_dir())
}

pub fn find_recoverable_in(dir: &Path) -> Vec<Recoverable> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<Recoverable> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|meta_path| {
            let meta: AutosaveMeta = serde_json::from_slice(&fs::read(&meta_path).ok()?).ok()?;
            let project_path = meta_path.with_extension("dcut");
            (project_path.is_file() && meta.pid != std::process::id() && !process_alive(meta.pid))
                .then_some(Recoverable { project_path, meta })
        })
        .collect();
    found.sort_by_key(|item| std::cmp::Reverse(item.meta.saved_at_unix));
    found
}

/// Loads a recoverable project and removes its files (the caller now owns the project).
pub fn recover(item: &Recoverable) -> Result<Project, ProjectFileError> {
    let project = load(&item.project_path)?;
    discard(item);
    Ok(project)
}

pub fn discard(item: &Recoverable) {
    let _ = fs::remove_file(&item.project_path);
    let _ = fs::remove_file(item.project_path.with_extension("json"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crashed_session_is_recoverable_and_live_or_own_sessions_are_not() {
        let dir = std::env::temp_dir().join(format!("d-cut-autosave-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let project = Project::default();
        // A pid that cannot exist (beyond pid_max) simulates a crashed editor.
        write_autosave_in(
            &dir,
            99_999_999,
            &project,
            Some(Path::new("/videos/a.dcut")),
        )
        .expect("write");
        write_autosave_in(&dir, std::process::id(), &project, None).expect("own");
        write_autosave_in(&dir, 1, &project, None).expect("live pid 1");
        let found = find_recoverable_in(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].meta.pid, 99_999_999);
        assert_eq!(
            found[0].meta.original.as_deref(),
            Some(Path::new("/videos/a.dcut"))
        );
        let recovered = recover(&found[0]).expect("recover");
        assert_eq!(recovered, project);
        assert!(find_recoverable_in(&dir).is_empty());
        clear_autosave_in(&dir, std::process::id());
        assert!(
            !dir.join(format!("autosave-{}.dcut", std::process::id()))
                .exists()
        );
        let _ = fs::remove_dir_all(dir);
    }
}
