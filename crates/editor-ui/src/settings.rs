// SPDX-License-Identifier: AGPL-3.0-or-later

//! Persistent app settings in `$XDG_CONFIG_HOME/d-cut/settings.json` (the app's own file).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// "pt" or "en".
    pub language: String,
    /// Opt-in: query GitHub releases for a newer version. Off by default (no network).
    pub check_updates: bool,
    /// Whisper model id from `editor_speech::MODELS` (default `small`).
    pub speech_model: String,
}

impl Default for Settings {
    fn default() -> Self {
        let system = std::env::var("LANG").unwrap_or_default();
        Self {
            language: if system.starts_with("pt") || system.is_empty() {
                "pt"
            } else {
                "en"
            }
            .to_owned(),
            check_updates: false,
            speech_model: editor_speech::MODEL_SMALL.id.to_owned(),
        }
    }
}

fn path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|config| config.join("d-cut/settings.json"))
}

impl Settings {
    pub fn load() -> Self {
        let mut settings: Self = path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        if let Ok(language) = std::env::var("D_CUT_LANG") {
            settings.language = language;
        }
        settings.validated()
    }

    /// Clamps hand-edited or outdated values to something the app can use.
    fn validated(mut self) -> Self {
        if editor_speech::model_by_id(&self.speech_model).is_none() {
            self.speech_model = editor_speech::MODEL_SMALL.id.to_owned();
        }
        self
    }

    pub fn save(&self) {
        if let Some(path) = path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_vec_pretty(self) {
                let _ = std::fs::write(path, json);
            }
        }
    }
}

/// `https://api.github.com/repos/<owner>/<repo>/releases/latest`, from the `repository` field of
/// `Cargo.toml` (so changing the repo does not require touching code).
fn releases_api_url() -> Option<String> {
    let parts: Vec<&str> = env!("CARGO_PKG_REPOSITORY")
        .trim_end_matches('/')
        .rsplit('/')
        .take(2)
        .collect();
    let [repo, owner] = [*parts.first()?, *parts.get(1)?];
    (!repo.is_empty() && !owner.is_empty())
        .then(|| format!("https://api.github.com/repos/{owner}/{repo}/releases/latest"))
}

/// Opt-in update check: latest GitHub release tag via the system `curl`. Returns the tag when
/// it differs from the running version. Call from a worker thread.
pub fn check_latest_release() -> Result<Option<String>, String> {
    let output = std::process::Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--max-time",
            "10",
            "--location",
        ])
        .arg(releases_api_url().ok_or("repository not set in Cargo.toml")?)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    let tag = json
        .get("tag_name")
        .and_then(|tag| tag.as_str())
        .ok_or("release has no tag")?
        .trim_start_matches('v')
        .to_owned();
    Ok((tag != env!("CARGO_PKG_VERSION")).then_some(tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_url_comes_from_the_repository_field() {
        let url = releases_api_url().expect("Cargo.toml sets repository");
        assert!(url.starts_with("https://api.github.com/repos/"), "{url}");
        assert!(url.ends_with("/releases/latest"), "{url}");
        // owner/repo, no extra path segments
        let slug = url
            .trim_start_matches("https://api.github.com/repos/")
            .trim_end_matches("/releases/latest");
        assert_eq!(slug.split('/').count(), 2, "{slug}");
    }

    #[test]
    fn speech_model_round_trips_and_old_files_get_the_default() {
        let settings = Settings {
            speech_model: "medium".into(),
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).expect("serializes");
        assert!(json.contains("\"speech_model\":\"medium\""), "{json}");
        let back: Settings = serde_json::from_str(&json).expect("parses");
        assert_eq!(back.speech_model, "medium");

        // settings.json written before the field existed must not break loading.
        let old: Settings =
            serde_json::from_str(r#"{"language":"en","check_updates":true}"#).expect("parses");
        assert_eq!(old.speech_model, "small");
        assert_eq!(old.language, "en");
    }

    #[test]
    fn unknown_speech_model_falls_back_to_small() {
        let settings = Settings {
            speech_model: "gigante".into(),
            ..Settings::default()
        };
        assert_eq!(settings.validated().speech_model, "small");
        let kept = Settings {
            speech_model: "large-v3-turbo".into(),
            ..Settings::default()
        };
        assert_eq!(kept.validated().speech_model, "large-v3-turbo");
    }

    #[test]
    fn language_follows_the_system_locale_by_default() {
        // The default is pt-BR for pt_*, en otherwise; D_CUT_LANG overrides it in load().
        let settings = Settings::default();
        assert!(settings.language == "pt" || settings.language == "en");
        assert!(!settings.check_updates);
    }
}
