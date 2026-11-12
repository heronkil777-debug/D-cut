// SPDX-License-Identifier: AGPL-3.0-or-later

//! Preview proxies: small, seek-friendly copies of the source media kept in the user cache.
//! Only the preview uses them; exports always read the original files.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::{MediaError, ffmpeg_program};

pub fn proxy_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("d-cut/proxies")
}

/// Cache location keyed by path, size and modification time (a changed source gets a new proxy).
pub fn proxy_path(source: &Path) -> Option<PathBuf> {
    let metadata = std::fs::metadata(source).ok()?;
    let mut hasher = DefaultHasher::new();
    source.canonicalize().ok()?.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|age| age.as_secs())
        .hash(&mut hasher);
    Some(proxy_dir().join(format!("{:016x}.mp4", hasher.finish())))
}

pub fn proxy_ready(source: &Path) -> bool {
    proxy_path(source).is_some_and(|path| path.is_file())
}

/// Transcodes `source` to an upright ≤`max_height`p H.264/AAC proxy with short GOPs.
/// Timestamps are preserved, so timeline seeks map 1:1. Call from a worker thread.
pub fn generate_proxy(source: &Path, max_height: u32) -> Result<PathBuf, MediaError> {
    let target = proxy_path(source).ok_or(MediaError::InvalidMetadata("source is not readable"))?;
    if target.is_file() {
        return Ok(target);
    }
    let directory = proxy_dir();
    std::fs::create_dir_all(&directory).map_err(MediaError::WaveformIo)?;
    let partial = target.with_extension("part.mp4");
    let output = Command::new(ffmpeg_program())
        .args(["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-i"])
        .arg(source)
        .arg("-vf")
        .arg(format!("scale=-2:'min({max_height},ih)'"))
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "26",
            "-g",
            "15",
            "-bf",
            "0",
            "-pix_fmt",
            "yuv420p",
            "-fps_mode",
            "passthrough",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-ac",
            "2",
            "-movflags",
            "+faststart",
        ])
        .arg(&partial)
        .stdin(Stdio::null())
        .output()
        .map_err(|source| MediaError::Spawn {
            program: ffmpeg_program().to_string_lossy().into_owned(),
            source,
        })?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&partial);
        return Err(MediaError::Failed {
            program: ffmpeg_program().to_string_lossy().into_owned(),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    std::fs::rename(&partial, &target).map_err(MediaError::WaveformIo)?;
    Ok(target)
}
