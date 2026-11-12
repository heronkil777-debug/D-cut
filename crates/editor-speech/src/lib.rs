// SPDX-License-Identifier: AGPL-3.0-or-later

//! Offline speech recognition with whisper.cpp's `whisper-cli` (a separate process, like
//! FFmpeg). Models are downloaded only on explicit request, verified with SHA-256 and kept
//! outside the AppImage. No other network access happens here.

use std::{
    ffi::OsString,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SpeechError {
    #[error("could not run {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} failed ({status}): {stderr}")]
    Failed {
        program: String,
        status: String,
        stderr: String,
    },
    #[error("file error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("checksum mismatch for {path}: expected {expected}, got {actual}")]
    Checksum {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("could not parse whisper output: {0}")]
    Parse(String),
    #[error("download cancelled")]
    Cancelled,
}

/// A downloadable model. License is recorded in THIRD_PARTY_LICENSES before any download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelInfo {
    pub name: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
    pub license: &'static str,
}

/// Whisper small (multilingual), the approved default for PT-BR.
pub const MODEL_SMALL: ModelInfo = ModelInfo {
    name: "Whisper small (multilíngue)",
    file: "ggml-small.bin",
    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
    sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
    bytes: 487_601_967,
    license: "MIT (pesos OpenAI Whisper, MIT)",
};

pub fn models_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir)
        .join("d-cut/models")
}

pub fn model_path(model: &ModelInfo) -> PathBuf {
    models_dir().join(model.file)
}

/// Present with the expected size (the full SHA-256 is checked when downloading).
pub fn model_ready(model: &ModelInfo) -> bool {
    fs::metadata(model_path(model)).is_ok_and(|meta| meta.len() == model.bytes)
}

pub fn sha256_file(path: &Path) -> Result<String, SpeechError> {
    let io = |source| SpeechError::Io {
        path: path.to_owned(),
        source,
    };
    let mut file = File::open(path).map_err(io)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer).map_err(io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Downloads a model with the system `curl` into the models directory, reporting progress
/// (0..=1). The file is only moved into place after its SHA-256 matches.
pub fn download_model(
    model: &ModelInfo,
    cancel: &Arc<AtomicBool>,
    mut progress: impl FnMut(f64),
) -> Result<PathBuf, SpeechError> {
    let target = model_path(model);
    let directory = models_dir();
    fs::create_dir_all(&directory).map_err(|source| SpeechError::Io {
        path: directory.clone(),
        source,
    })?;
    let partial = target.with_extension("part");
    let mut child = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--retry",
            "3",
        ])
        .args(["--continue-at", "-", "--output"])
        .arg(&partial)
        .arg(model.url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| SpeechError::Spawn {
            program: "curl".into(),
            source,
        })?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SpeechError::Cancelled);
        }
        let size = fs::metadata(&partial).map_or(0, |meta| meta.len());
        progress((size as f64 / model.bytes as f64).min(1.0));
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => thread::sleep(Duration::from_millis(250)),
            Err(source) => {
                return Err(SpeechError::Spawn {
                    program: "curl".into(),
                    source,
                });
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|source| SpeechError::Spawn {
            program: "curl".into(),
            source,
        })?;
    if !output.status.success() {
        return Err(SpeechError::Failed {
            program: "curl".into(),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let actual = sha256_file(&partial)?;
    if actual != model.sha256 {
        let _ = fs::remove_file(&partial);
        return Err(SpeechError::Checksum {
            path: partial,
            expected: model.sha256.to_owned(),
            actual,
        });
    }
    fs::rename(&partial, &target).map_err(|source| SpeechError::Io {
        path: target.clone(),
        source,
    })?;
    progress(1.0);
    Ok(target)
}

/// `whisper-cli` executable: `D_CUT_WHISPER` (set by the AppImage) or from PATH.
pub fn whisper_program() -> OsString {
    std::env::var_os("D_CUT_WHISPER").unwrap_or_else(|| "whisper-cli".into())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Deserialize)]
struct WhisperJson {
    transcription: Vec<WhisperSegment>,
}

#[derive(Deserialize)]
struct WhisperSegment {
    offsets: WhisperOffsets,
    text: String,
}

#[derive(Deserialize)]
struct WhisperOffsets {
    from: u64,
    to: u64,
}

/// Parses `whisper-cli -oj` output (with `-ml 1 -sow` each segment is one word).
pub fn parse_whisper_json(json: &str) -> Result<Vec<Word>, SpeechError> {
    let parsed: WhisperJson =
        serde_json::from_str(json).map_err(|error| SpeechError::Parse(error.to_string()))?;
    Ok(parsed
        .transcription
        .into_iter()
        .filter_map(|segment| {
            let text = segment.text.trim().to_owned();
            let is_marker = text.starts_with('[') && text.ends_with(']');
            (!text.is_empty() && !is_marker).then_some(Word {
                start_ms: segment.offsets.from,
                end_ms: segment.offsets.to.max(segment.offsets.from + 1),
                text,
            })
        })
        .collect())
}

/// Groups words into subtitle cues: breaks after sentence punctuation, on pauses longer than
/// `gap_ms`, or before exceeding `max_chars`/`max_ms`.
pub fn group_words(words: &[Word], max_chars: usize, max_ms: u64, gap_ms: u64) -> Vec<Cue> {
    let mut cues: Vec<Cue> = Vec::new();
    let mut current: Option<Cue> = None;
    let mut previous_end = 0;
    for word in words {
        if let Some(cue) = &current {
            let candidate = cue.text.chars().count() + 1 + word.text.chars().count();
            let ends_sentence = cue.text.ends_with(['.', '!', '?']);
            if candidate > max_chars
                || word.end_ms.saturating_sub(cue.start_ms) > max_ms
                || word.start_ms.saturating_sub(previous_end) > gap_ms
                || ends_sentence
            {
                cues.extend(current.take());
            }
        }
        match &mut current {
            Some(cue) => {
                cue.text.push(' ');
                cue.text.push_str(&word.text);
                cue.end_ms = word.end_ms;
            }
            None => {
                current = Some(Cue {
                    start_ms: word.start_ms,
                    end_ms: word.end_ms,
                    text: word.text.clone(),
                });
            }
        }
        previous_end = word.end_ms;
    }
    cues.extend(current);
    cues
}

/// Runs whisper-cli on a 16 kHz mono WAV and returns word-level timestamps.
pub fn transcribe(
    wav: &Path,
    model: &Path,
    language: &str,
    threads: usize,
) -> Result<Vec<Word>, SpeechError> {
    let prefix = wav.with_extension("whisper");
    let output = Command::new(whisper_program())
        .arg("-m")
        .arg(model)
        .arg("-f")
        .arg(wav)
        .args(["-l", language, "-ml", "1", "-sow", "-oj", "-np", "-t"])
        .arg(threads.max(1).to_string())
        .arg("-of")
        .arg(&prefix)
        .stdin(Stdio::null())
        .output()
        .map_err(|source| SpeechError::Spawn {
            program: whisper_program().to_string_lossy().into_owned(),
            source,
        })?;
    if !output.status.success() {
        return Err(SpeechError::Failed {
            program: whisper_program().to_string_lossy().into_owned(),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    // whisper-cli appends ".json" to the `-of` prefix.
    let mut json_name = prefix.clone().into_os_string();
    json_name.push(".json");
    let json_path = PathBuf::from(json_name);
    let json = fs::read_to_string(&json_path).map_err(|source| SpeechError::Io {
        path: json_path.clone(),
        source,
    })?;
    let _ = fs::remove_file(&json_path);
    parse_whisper_json(&json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(start: u64, end: u64, text: &str) -> Word {
        Word {
            start_ms: start,
            end_ms: end,
            text: text.into(),
        }
    }

    #[test]
    fn parses_whisper_json_and_skips_markers() {
        let json = r#"{"transcription":[
            {"timestamps":{"from":"00:00:00,000","to":"00:00:00,400"},"offsets":{"from":0,"to":400},"text":" Olá"},
            {"timestamps":{"from":"00:00:00,400","to":"00:00:00,900"},"offsets":{"from":400,"to":900},"text":" mundo."},
            {"timestamps":{"from":"00:00:01,000","to":"00:00:02,000"},"offsets":{"from":1000,"to":2000},"text":" [Música]"}
        ]}"#;
        let words = parse_whisper_json(json).expect("parse");
        assert_eq!(words, vec![word(0, 400, "Olá"), word(400, 900, "mundo.")]);
        assert!(parse_whisper_json("{").is_err());
    }

    #[test]
    fn groups_words_by_punctuation_pause_and_length() {
        let words = vec![
            word(0, 300, "Bom"),
            word(300, 600, "dia."),
            word(700, 1000, "Hoje"),
            word(1000, 1300, "vamos"),
            word(3000, 3300, "editar"),
            word(3300, 3600, "um"),
            word(3600, 3900, "vídeo"),
        ];
        let cues = group_words(&words, 42, 3500, 800);
        let texts: Vec<&str> = cues.iter().map(|cue| cue.text.as_str()).collect();
        assert_eq!(texts, ["Bom dia.", "Hoje vamos", "editar um vídeo"]);
        assert_eq!((cues[2].start_ms, cues[2].end_ms), (3000, 3900));
        let short = group_words(&words[2..], 10, 3500, 5000);
        assert!(short.iter().all(|cue| cue.text.chars().count() <= 10));
    }

    #[test]
    fn checksum_of_known_bytes() {
        let path = std::env::temp_dir().join(format!("d-cut-sha-{}", std::process::id()));
        fs::write(&path, b"abc").expect("write");
        assert_eq!(
            sha256_file(&path).expect("hash"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = fs::remove_file(path);
    }
}

/// Turns cues into an undoable command: adds a subtitle track if the project has none and
/// appends the cues as text overlays on it.
pub fn subtitles_command(project: &editor_core::Project, cues: &[Cue]) -> editor_core::EditCommand {
    use editor_core::{EditCommand, TextOverlay, TrackKind, add_track_command, next_free_id};
    let mut working = project.clone();
    let mut commands = Vec::new();
    if !working
        .tracks
        .iter()
        .any(|track| track.kind == TrackKind::Subtitle)
    {
        let command = add_track_command(&working, TrackKind::Subtitle);
        if command.clone().apply(&mut working).is_ok() {
            commands.push(command);
        }
    }
    let track_id = working
        .tracks
        .iter()
        .find(|track| track.kind == TrackKind::Subtitle)
        .map(|track| track.id);
    let format = working.format;
    let mut overlays = working.text_overlays.clone();
    for (next_id, cue) in (next_free_id(&working)..).zip(cues) {
        let start = format.seconds_to_frame(cue.start_ms as f64 / 1000.0);
        let end = format
            .seconds_to_frame(cue.end_ms as f64 / 1000.0)
            .max(start + 1);
        overlays.push(TextOverlay {
            id: next_id,
            track_id,
            start_frame: start,
            duration_frames: end - start,
            text: cue.text.clone(),
            x: 0.5,
            y: 0.86,
            font_size: format.height as f32 / 18.0,
            color: [1.0, 1.0, 1.0, 1.0],
        });
    }
    commands.push(EditCommand::ReplaceTextOverlays {
        expected: working.text_overlays.clone(),
        replacement: overlays,
    });
    EditCommand::Batch(commands)
}

#[cfg(test)]
mod command_tests {
    use super::*;
    use editor_core::{CommandHistory, Project};

    #[test]
    fn transcription_is_an_undoable_command() {
        let mut project = Project::default();
        let original = project.clone();
        let cues = vec![
            Cue {
                start_ms: 0,
                end_ms: 1200,
                text: "Olá pessoal".into(),
            },
            Cue {
                start_ms: 1500,
                end_ms: 2600,
                text: "bem-vindos".into(),
            },
        ];
        let mut history = CommandHistory::default();
        let command = subtitles_command(&project, &cues);
        history.execute(&mut project, command).expect("applies");
        assert_eq!(project.text_overlays.len(), 2);
        assert_eq!(project.text_overlays[1].start_frame, 45);
        assert!(
            project
                .tracks
                .iter()
                .any(|t| t.kind == editor_core::TrackKind::Subtitle)
        );
        assert!(history.undo(&mut project).expect("undo"));
        assert_eq!(project, original);
        assert!(history.redo(&mut project).expect("redo"));
        assert_eq!(project.text_overlays.len(), 2);
    }
}
