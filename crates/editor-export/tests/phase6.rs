// SPDX-License-Identifier: AGPL-3.0-or-later

//! Phase 6 through the real export: transitions, speed, keyframes and clip audio.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, atomic::AtomicBool},
};

use editor_core::{
    AnimationCurve, ClipAudio, EffectInstance, Interpolation, Keyframe, MediaAsset, ParamKeyframes,
    Project, ProjectFormat, Track, TrackKind, TransitionIn, add_media_clips_command,
    set_speed_command, update_clip_command,
};
use editor_export::{ExportOptions, export_project};
use editor_media::timeline::ExportEncoder;

fn available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
        && editor_render::headless_device().is_ok()
}

fn ffmpeg(args: &[&str]) {
    let status = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .status()
        .expect("ffmpeg");
    assert!(status.success(), "{args:?}");
}

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("d-cut-p6-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

fn source(dir: &Path, name: &str, video: &str, seconds: u32) -> PathBuf {
    let path = dir.join(name);
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        &format!("{video}:d={seconds}"),
        "-f",
        "lavfi",
        "-i",
        &format!("sine=frequency=440:sample_rate=48000:duration={seconds}"),
        "-c:v",
        "libx264",
        "-g",
        "15",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        path.to_str().expect("utf-8"),
    ]);
    path
}

fn asset(id: u64, path: &Path, frames: u64) -> MediaAsset {
    MediaAsset {
        id,
        path: path.display().to_string(),
        content_hash: None,
        duration_frames: frames,
        width: 320,
        height: 180,
        has_video: true,
        has_audio: true,
        rotation_degrees: 0,
    }
}

fn project(media: Vec<MediaAsset>) -> Project {
    let track = |id, name: &str, kind| Track {
        id,
        name: name.into(),
        kind,
        clips: Vec::new(),
    };
    Project {
        format: ProjectFormat {
            width: 320,
            height: 180,
            ..ProjectFormat::default()
        },
        media,
        tracks: vec![
            track(10, "V1", TrackKind::Video),
            track(11, "A1", TrackKind::Audio),
        ],
        ..Project::default()
    }
}

fn add(project: &mut Project, media: u64) -> Vec<u64> {
    let (command, created) = add_media_clips_command(project, media).expect("add");
    command.apply(project).expect("applies");
    created
}

fn export(project: &Project, output: &Path) {
    export_project(
        project,
        output,
        ExportOptions {
            encoder: ExportEncoder::X264 {
                crf: 16,
                preset: "veryfast".into(),
            },
            force_gpu: false,
            device: None,
        },
        &Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .expect("export");
}

/// Average RGB of exported frame `index`.
fn mean_rgb(path: &Path, index: u64) -> [f64; 3] {
    let output = Command::new("ffmpeg")
        .args(["-loglevel", "error", "-i"])
        .arg(path)
        .args([
            "-vf",
            &format!("select=eq(n\\,{index}),format=rgb24"),
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .output()
        .expect("ffmpeg");
    let mut sum = [0.0; 3];
    for pixel in output.stdout.chunks_exact(3) {
        for channel in 0..3 {
            sum[channel] += f64::from(pixel[channel]);
        }
    }
    let count = (output.stdout.len() / 3).max(1) as f64;
    sum.map(|value| value / count)
}

fn frame_gray(path: &Path, index: u64) -> Vec<u8> {
    let output = Command::new("ffmpeg")
        .args(["-loglevel", "error", "-i"])
        .arg(path)
        .args([
            "-vf",
            &format!("select=eq(n\\,{index}),scale=160:90,format=gray"),
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .output()
        .expect("ffmpeg");
    output.stdout
}

fn rms(path: &Path, start: f64, duration: f64) -> f64 {
    let output = Command::new("ffmpeg")
        .args([
            "-loglevel",
            "error",
            "-ss",
            &format!("{start}"),
            "-t",
            &format!("{duration}"),
            "-i",
        ])
        .arg(path)
        .args(["-vn", "-ac", "1", "-f", "f32le", "pipe:1"])
        .output()
        .expect("ffmpeg");
    let samples: Vec<f32> = output
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    (samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / samples.len().max(1) as f64)
        .sqrt()
}

fn diff(a: &[u8], b: &[u8]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| f64::from(x.abs_diff(*y)))
        .sum::<f64>()
        / a.len().max(1) as f64
}

#[test]
fn crossfade_mixes_only_inside_the_window() {
    if !available() {
        eprintln!("skipping: ffmpeg or GPU unavailable");
        return;
    }
    let dir = dir("transition");
    let red = source(&dir, "red.mp4", "color=c=red:s=320x180:r=30", 2);
    let blue = source(&dir, "blue.mp4", "color=c=blue:s=320x180:r=30", 2);
    let mut project = project(vec![asset(1, &red, 60), asset(2, &blue, 60)]);
    add(&mut project, 1);
    let second = add(&mut project, 2)[0];
    update_clip_command(&project, second, |clip| {
        clip.transition_in = Some(TransitionIn {
            package: "crossfade".into(),
            duration_frames: 20,
        });
    })
    .and_then(|command| command.apply(&mut project))
    .expect("transition");
    let output = dir.join("crossfade.mp4");
    export(&project, &output);
    let [r0, _, b0] = mean_rgb(&output, 10);
    let [rm, _, bm] = mean_rgb(&output, 60);
    let [r1, _, b1] = mean_rgb(&output, 100);
    assert!(r0 > 200.0 && b0 < 40.0, "before window: {r0} {b0}");
    assert!(
        (rm - bm).abs() < 60.0 && rm > 80.0 && bm > 80.0,
        "at the cut: {rm} {bm}"
    );
    assert!(b1 > 200.0 && r1 < 40.0, "after window: {r1} {b1}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn double_speed_uses_every_second_source_frame_and_halves_duration() {
    if !available() {
        eprintln!("skipping: ffmpeg or GPU unavailable");
        return;
    }
    let dir = dir("speed");
    let src = source(&dir, "card.mp4", "testsrc2=s=320x180:r=30", 4);
    let mut project = project(vec![asset(1, &src, 120)]);
    let video = add(&mut project, 1)[0];
    // Speed applies to the clip; keep audio in sync by applying it to the linked clip too.
    for id in editor_core::linked_clip_ids(&project, video) {
        set_speed_command(&project, id, 2.0)
            .and_then(|command| command.apply(&mut project))
            .expect("speed");
    }
    assert_eq!(project.duration_frames(), 60);
    let output = dir.join("speed.mp4");
    export(&project, &output);
    for index in [5_u64, 20, 41] {
        let exported = frame_gray(&output, index);
        let expected = frame_gray(&src, index * 2);
        let neighbor = frame_gray(&src, index * 2 + 3);
        assert!(
            diff(&exported, &expected) < diff(&exported, &neighbor),
            "frame {index}"
        );
    }
    let duration = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(&output)
        .output()
        .expect("ffprobe");
    let seconds: f64 = String::from_utf8_lossy(&duration.stdout)
        .trim()
        .parse()
        .unwrap_or(0.0);
    assert!((seconds - 2.0).abs() < 0.07, "duration {seconds}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn keyframed_effect_parameter_animates_over_the_clip() {
    if !available() {
        eprintln!("skipping: ffmpeg or GPU unavailable");
        return;
    }
    let dir = dir("keys");
    let gray = source(&dir, "gray.mp4", "color=c=0x808080:s=320x180:r=30", 2);
    let mut project = project(vec![asset(1, &gray, 60)]);
    let video = add(&mut project, 1)[0];
    let key = |frame, value| Keyframe {
        frame,
        value,
        interpolation_to_next: Interpolation::Linear,
    };
    update_clip_command(&project, video, |clip| {
        clip.effects.push(EffectInstance {
            effect_id: "brightness-contrast".into(),
            params: vec![0.0, 1.0],
            keyframes: vec![ParamKeyframes {
                param: 0,
                curve: AnimationCurve {
                    keyframes: vec![key(0, -0.5), key(59, 0.0)],
                },
            }],
        });
    })
    .and_then(|command| command.apply(&mut project))
    .expect("keyframes");
    let output = dir.join("keys.mp4");
    export(&project, &output);
    let start = mean_rgb(&output, 0)[0];
    let middle = mean_rgb(&output, 30)[0];
    let end = mean_rgb(&output, 59)[0];
    assert!(start < 20.0, "start {start}");
    assert!(
        middle > start + 30.0 && middle < end - 30.0,
        "middle {middle}"
    );
    assert!((end - 128.0).abs() < 10.0, "end {end}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn clip_audio_fade_gain_and_denoise_are_rendered() {
    if !available() {
        eprintln!("skipping: ffmpeg or GPU unavailable");
        return;
    }
    let dir = dir("audio");
    let src = source(&dir, "tone.mp4", "color=c=black:s=320x180:r=30", 3);
    let mut base = project(vec![asset(1, &src, 90)]);
    let created = add(&mut base, 1);
    let audio_clip = created[1];
    let plain = dir.join("plain.mp4");
    export(&base, &plain);
    let mut faded = base.clone();
    update_clip_command(&faded, audio_clip, |clip| {
        clip.audio = ClipAudio {
            gain_db: -20.0,
            fade_in_frames: 30,
            fade_out_frames: 0,
            denoise: true,
        };
    })
    .and_then(|command| command.apply(&mut faded))
    .expect("audio");
    let processed = dir.join("faded.mp4");
    export(&faded, &processed);
    let reference = rms(&plain, 1.5, 0.5);
    let start = rms(&processed, 0.0, 0.15);
    let steady = rms(&processed, 1.5, 0.5);
    assert!(reference > 0.05, "reference {reference}");
    let ratio = steady / reference;
    assert!((0.07..=0.14).contains(&ratio), "-20 dB ratio {ratio}");
    assert!(start < steady * 0.4, "fade-in start {start} vs {steady}");
    let _ = std::fs::remove_dir_all(dir);
}
