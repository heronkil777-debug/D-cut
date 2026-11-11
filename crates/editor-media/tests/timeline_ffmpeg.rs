// SPDX-License-Identifier: AGPL-3.0-or-later

//! Integration tests that run the real FFmpeg/ffprobe binaries. They are skipped (with a
//! message) when FFmpeg is not installed, so `cargo test` still works on minimal machines.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, atomic::AtomicBool},
};

use editor_core::{
    Clip, MediaAsset, Project, ProjectFormat, Track, TrackKind, add_media_clips_command,
    delete_clips_command, split_at_command,
};
use editor_media::{
    probe,
    timeline::{ExportEncoder, TimelineRender, export_timeline, render_frame},
};

fn have_ffmpeg() -> bool {
    let ok = |program: &str| {
        Command::new(program)
            .arg("-version")
            .output()
            .is_ok_and(|output| output.status.success())
    };
    ok("ffmpeg") && ok("ffprobe")
}

fn work_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("d-cut-it-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn ffmpeg(args: &[&str]) {
    let status = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .status()
        .expect("ffmpeg runs");
    assert!(status.success(), "ffmpeg {args:?} failed");
}

/// 4 s, 1280x720, 30 fps testsrc2 with a 440 Hz tone, keyframe every 2 s (forces real seeking).
fn landscape_fixture(dir: &Path) -> PathBuf {
    let path = dir.join("landscape.mp4");
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=1280x720:rate=30",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000",
        "-t",
        "4",
        "-c:v",
        "libx264",
        "-g",
        "60",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        path.to_str().expect("utf-8 path"),
    ]);
    path
}

fn asset_for(path: &Path, id: u64, format: ProjectFormat) -> MediaAsset {
    let info = probe(path).expect("probe");
    let fps = format.frames_per_second();
    MediaAsset {
        id,
        path: path.display().to_string(),
        content_hash: None,
        duration_frames: (info.duration_seconds * fps).floor() as u64,
        width: info.video.as_ref().map_or(2, |video| video.width),
        height: info.video.as_ref().map_or(2, |video| video.height),
        has_video: info.video.is_some(),
        has_audio: info.audio.is_some(),
        rotation_degrees: info
            .video
            .as_ref()
            .map_or(0, |video| video.rotation_degrees),
    }
}

fn empty_project(format: ProjectFormat) -> Project {
    Project {
        format,
        tracks: vec![
            Track {
                id: 1,
                name: "V1".into(),
                kind: TrackKind::Video,
                clips: Vec::new(),
            },
            Track {
                id: 2,
                name: "A1".into(),
                kind: TrackKind::Audio,
                clips: Vec::new(),
            },
        ],
        ..Project::default()
    }
}

fn ffprobe_value(path: &Path, args: &[&str]) -> String {
    let output = Command::new("ffprobe")
        .args(["-v", "error"])
        .args(args)
        .args(["-of", "default=noprint_wrappers=1:nokey=1"])
        .arg(path)
        .output()
        .expect("ffprobe runs");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn mean_abs_diff(left: &[u8], right: &[u8]) -> f64 {
    assert_eq!(left.len(), right.len());
    let total: u64 = left
        .iter()
        .zip(right)
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    total as f64 / left.len() as f64
}

/// Reference decode of source frame `index` (30 fps source) without any seeking tricks:
/// decode from the start and select the N-th frame.
fn reference_frame(path: &Path, index: u64, width: u32, height: u32) -> Vec<u8> {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .arg("-vf")
        .arg(format!(
            "select=eq(n\\,{index}),scale={width}:{height}:in_color_matrix=bt601:in_range=tv,format=rgba"
        ))
        .args([
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "pipe:1",
        ])
        .output()
        .expect("ffmpeg runs");
    assert!(output.status.success());
    output.stdout
}

#[test]
fn edited_timeline_exports_valid_h264_aac_mp4() {
    if !have_ffmpeg() {
        eprintln!("skipping: ffmpeg/ffprobe not installed");
        return;
    }
    let dir = work_dir("export");
    let source = landscape_fixture(&dir);
    let format = ProjectFormat::default(); // 1920x1080 @ 30
    let mut project = empty_project(format);
    project.media.push(asset_for(&source, 10, format));
    // Add, split at 1 s, delete the first part with ripple, split again at 1 s and
    // delete the tail -> 1 s of material from source 1..2 s.
    let (command, created) = add_media_clips_command(&project, 10).expect("add");
    command.apply(&mut project).expect("add applies");
    split_at_command(&project, &[created[0]], 30)
        .and_then(|command| command.apply(&mut project))
        .expect("split");
    let first = project.tracks[0].clips[0].id;
    delete_clips_command(&project, &[first], true)
        .and_then(|command| command.apply(&mut project))
        .expect("delete");
    let remaining = project.tracks[0].clips[0].id;
    split_at_command(&project, &[remaining], 30)
        .and_then(|command| command.apply(&mut project))
        .expect("split 2");
    let tail = project.tracks[0].clips[1].id;
    delete_clips_command(&project, &[tail], false)
        .and_then(|command| command.apply(&mut project))
        .expect("delete tail");
    // Leave a 15-frame gap before a second copy of the first 15 source frames (video only).
    project.tracks[0].clips.push(Clip {
        id: 99,
        media_id: 10,
        timeline_start_frame: 45,
        source_start_frame: 0,
        duration_frames: 15,
        link_group: None,
    });
    project.validate().expect("valid project");
    let total = project.duration_frames();
    assert_eq!(total, 60);

    let job = TimelineRender::from_project(&project, 0, total, 1920, 1080).expect("job");
    let output = dir.join("export out.mp4");
    let cancel = Arc::new(AtomicBool::new(false));
    let mut last_fraction = 0.0;
    export_timeline(
        &job,
        &ExportEncoder::X264 {
            crf: 23,
            preset: "veryfast".into(),
        },
        &output,
        &cancel,
        |progress| last_fraction = progress.fraction,
    )
    .expect("export succeeds");
    assert!((last_fraction - 1.0).abs() < f64::EPSILON);

    let video = ffprobe_value(
        &output,
        &[
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            "stream=codec_name,width,height,pix_fmt,avg_frame_rate,nb_read_frames",
        ],
    );
    let lines: Vec<&str> = video.lines().collect();
    assert_eq!(
        lines,
        ["h264", "1920", "1080", "yuv420p", "30/1", "60"],
        "{video}"
    );
    let audio = ffprobe_value(
        &output,
        &[
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_name,sample_rate",
        ],
    );
    assert_eq!(audio.lines().collect::<Vec<_>>(), ["aac", "48000"]);
    let duration: f64 = ffprobe_value(&output, &["-show_entries", "format=duration"])
        .parse()
        .expect("duration");
    assert!(
        (duration - 2.0).abs() <= 1.0 / 30.0 + 0.03,
        "duration {duration}"
    );
    // +faststart: the moov atom precedes mdat.
    let bytes = std::fs::read(&output).expect("read output");
    let find = |tag: &[u8]| bytes.windows(4).position(|window| window == tag);
    assert!(find(b"moov").expect("moov") < find(b"mdat").expect("mdat"));

    // The exported frame 10 equals the source frame 40 (clip starts at source 1 s).
    let exported = reference_frame(&output, 10, 320, 180);
    let expected = reference_frame(&source, 40, 320, 180);
    let neighbor = reference_frame(&source, 43, 320, 180);
    let good = mean_abs_diff(&exported, &expected);
    let bad = mean_abs_diff(&exported, &neighbor);
    assert!(
        good < bad,
        "exported frame mismatch: {good} vs neighbor {bad}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn preview_frames_match_source_frames_on_random_seeks() {
    if !have_ffmpeg() {
        eprintln!("skipping: ffmpeg/ffprobe not installed");
        return;
    }
    let dir = work_dir("seek");
    let source = landscape_fixture(&dir);
    let format = ProjectFormat {
        width: 1280,
        height: 720,
        ..ProjectFormat::default()
    };
    let mut project = empty_project(format);
    project.media.push(asset_for(&source, 10, format));
    let (command, _) = add_media_clips_command(&project, 10).expect("add");
    command.apply(&mut project).expect("add applies");
    let (width, height) = (320, 180);
    // Deterministic pseudo-random frames, including frames right before/after keyframes.
    let mut state = 0x2545_f491_u64;
    let mut frames = vec![0_u64, 59, 60, 61, 119];
    while frames.len() < 20 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        frames.push(state % 119);
    }
    for frame in frames {
        let job =
            TimelineRender::from_project(&project, frame, frame + 1, width, height).expect("job");
        let preview = render_frame(&job).expect("preview frame");
        let expected = reference_frame(&source, frame, width, height);
        let neighbor = if frame + 1 < 120 {
            frame + 1
        } else {
            frame - 1
        };
        let off_by_one = reference_frame(&source, neighbor, width, height);
        let good = mean_abs_diff(&preview, &expected);
        let bad = mean_abs_diff(&preview, &off_by_one);
        assert!(
            good < 2.0 && good < bad,
            "frame {frame}: diff {good}, next-frame diff {bad}"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn rotated_phone_clip_and_vfr_clip_render_with_correct_geometry_and_length() {
    if !have_ffmpeg() {
        eprintln!("skipping: ffmpeg/ffprobe not installed");
        return;
    }
    let dir = work_dir("phone");
    let landscape = landscape_fixture(&dir);
    // Phone-style clip: stored 1280x720 with a 90° display matrix -> displayed 720x1280.
    let rotated = dir.join("rotated.mp4");
    ffmpeg(&[
        "-display_rotation:v:0",
        "90",
        "-i",
        landscape.to_str().expect("utf-8"),
        "-c",
        "copy",
        rotated.to_str().expect("utf-8"),
    ]);
    let info = probe(&rotated).expect("probe rotated");
    let video = info.video.expect("video");
    assert_eq!((video.width, video.height), (720, 1280));
    assert!(matches!(video.rotation_degrees, 90 | 270));

    // VFR clip: 2 s at 30 fps followed by 2 s at 10 fps, stored with variable timestamps.
    let vfr = dir.join("vfr.mp4");
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=640x360:rate=30:duration=2",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=640x360:rate=10:duration=2",
        "-filter_complex",
        "[0:v][1:v]concat=n=2:v=1:a=0[v]",
        "-map",
        "[v]",
        "-fps_mode",
        "vfr",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        vfr.to_str().expect("utf-8"),
    ]);

    // 9:16 project at 30 fps: rotated clip then the VFR clip.
    let format = ProjectFormat {
        width: 1080,
        height: 1920,
        ..ProjectFormat::default()
    };
    let mut project = empty_project(format);
    project.media.push(asset_for(&rotated, 10, format));
    project.media.push(asset_for(&vfr, 11, format));
    for media in [10, 11] {
        let (command, _) = add_media_clips_command(&project, media).expect("add");
        command.apply(&mut project).expect("applies");
    }
    let total = project.duration_frames();
    // A portrait frame from the rotated clip fills the 9:16 canvas: no black side bars.
    let job = TimelineRender::from_project(&project, 15, 16, 270, 480).expect("job");
    let frame = render_frame(&job).expect("frame");
    let column_brightness = |x: usize| -> u64 {
        (0..480)
            .map(|y| {
                let offset = (y * 270 + x) * 4;
                u64::from(frame[offset]) + u64::from(frame[offset + 1])
            })
            .sum()
    };
    assert!(
        column_brightness(2) > 480 * 20,
        "left edge looks letterboxed"
    );

    let job = TimelineRender::from_project(&project, 0, total, 540, 960).expect("job");
    let output = dir.join("vertical.mp4");
    export_timeline(
        &job,
        &ExportEncoder::X264 {
            crf: 28,
            preset: "ultrafast".into(),
        },
        &output,
        &Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .expect("export");
    let counted = ffprobe_value(
        &output,
        &[
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            "stream=width,height,nb_read_frames",
        ],
    );
    assert_eq!(
        counted.lines().collect::<Vec<_>>(),
        ["540", "960", &total.to_string()],
        "{counted}"
    );
    let _ = std::fs::remove_dir_all(dir);
}
