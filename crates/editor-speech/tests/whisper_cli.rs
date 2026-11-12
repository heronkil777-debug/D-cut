// SPDX-License-Identifier: AGPL-3.0-or-later

//! Runs the real whisper-cli when it and the model are available (skips otherwise).
//! Optional quality check: D_CUT_SPEECH_SAMPLE=<16 kHz wav> D_CUT_SPEECH_EXPECT="words,…".

use std::process::Command;

use editor_speech::{
    MODEL_SMALL, group_words, model_path, model_ready, transcribe, whisper_program,
};

#[test]
fn whisper_cli_runs_offline_and_returns_word_timestamps() {
    let available = Command::new(whisper_program())
        .arg("--help")
        .output()
        .is_ok();
    if !available || !model_ready(&MODEL_SMALL) {
        eprintln!("skipping: whisper-cli or model not available");
        return;
    }
    let dir = std::env::temp_dir().join(format!("d-cut-whisper-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let wav = match std::env::var_os("D_CUT_SPEECH_SAMPLE") {
        Some(path) => std::path::PathBuf::from(path),
        None => {
            let wav = dir.join("tone.wav");
            let status = Command::new("ffmpeg")
                .args(["-loglevel", "error", "-y", "-f", "lavfi", "-i"])
                .arg("sine=frequency=300:duration=2:sample_rate=16000")
                .arg(&wav)
                .status()
                .expect("ffmpeg");
            assert!(status.success());
            wav
        }
    };
    let words = transcribe(&wav, &model_path(&MODEL_SMALL), "pt", 4).expect("whisper runs");
    for pair in words.windows(2) {
        assert!(
            pair[0].start_ms <= pair[1].start_ms,
            "timestamps are ordered"
        );
    }
    if let Ok(expected) = std::env::var("D_CUT_SPEECH_EXPECT") {
        let text = group_words(&words, 42, 3500, 700)
            .iter()
            .map(|cue| cue.text.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        for word in expected.split(',') {
            assert!(
                text.contains(&word.to_lowercase()),
                "missing '{word}' in: {text}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}
