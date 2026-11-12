// SPDX-License-Identifier: AGPL-3.0-or-later

//! UI translation. Source strings are pt-BR; `locales/en.tsv` maps them to English
//! (`pt<TAB>en` per line). Missing entries fall back to the pt-BR text.

use std::{
    collections::HashMap,
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};

static ENGLISH: AtomicBool = AtomicBool::new(false);

static EN: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    include_str!("../locales/en.tsv")
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect()
});

pub fn set_language(code: &str) {
    ENGLISH.store(code == "en", Ordering::Relaxed);
}

pub fn tr(text: &'static str) -> &'static str {
    if ENGLISH.load(Ordering::Relaxed) {
        EN.get(text).copied().unwrap_or(text)
    } else {
        text
    }
}

/// Every translatable string used by the UI (kept in sync by a test).
#[cfg(test)]
pub fn catalog_keys() -> Vec<&'static str> {
    EN.keys().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `tr("…")` literal in the UI sources must have an English entry (or be a key name).
    #[test]
    fn all_ui_strings_are_translated() {
        let keys = catalog_keys();
        let identity = [
            "Ctrl+O",
            "Ctrl+S",
            "Ctrl+I",
            "?",
            "Ctrl+Z",
            "Ctrl+Shift+Z / Ctrl+Y",
            "Ctrl+E",
            "Idioma / Language",
            "S",
            "Delete",
            "Shift+Delete",
            "FPS",
            "Home",
            "< Frame",
            "Frame >",
            "End",
            " - ",
            " + ",
            "Preset",
            "PT-BR",
            "EN",
        ];
        let mut missing = Vec::new();
        for source in [include_str!("lib.rs"), include_str!("files.rs")] {
            for part in source.split("tr(\"").skip(1) {
                let Some(text) = part.split("\")").next() else {
                    continue;
                };
                if !keys.contains(&text) && !identity.contains(&text) {
                    missing.push(text.to_owned());
                }
            }
        }
        assert!(missing.is_empty(), "untranslated: {missing:#?}");
        set_language("en");
        assert_eq!(tr("Dividir"), "Split");
        set_language("pt");
        assert_eq!(tr("Dividir"), "Dividir");
    }
}
