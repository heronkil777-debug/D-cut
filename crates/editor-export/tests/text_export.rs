// SPDX-License-Identifier: AGPL-3.0-or-later

//! Burned-in titles/subtitles through the GPU composition path, checked with ffprobe and by
//! decoding the exported frames. Skips when FFmpeg or a GPU adapter is unavailable.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, atomic::AtomicBool},
};

use editor_core::{
    MediaAsset, Project, ProjectFormat, TextOverlay, Track, TrackKind, add_media_clips_command,
};
use editor_export::{ExportOptions, ExportPath, export_project};
use editor_media::timeline::ExportEncoder;

fn available() -> bool {
    let ffmpeg = Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|output| output.status.success());
    ffmpeg && editor_render::headless_device().is_ok()
}

fn ffmpeg(args: &[&str]) {
    let status = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .status()
        .expect("ffmpeg");
    assert!(status.success());
}

fn frame_at(path: &Path, index: u64, width: u32, height: u32) -> Vec<u8> {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .arg("-vf")
        .arg(format!(
            "select=eq(n\\,{index}),scale={width}:{height},format=gray"
        ))
        .args(["-frames:v", "1", "-f", "rawvideo", "pipe:1"])
        .output()
        .expect("ffmpeg");
    assert!(output.status.success());
    output.stdout
}

fn probe(path: &Path) -> String {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-show_entries",
            "stream=codec_name,width,height,pix_fmt,nb_read_frames",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn project(dir: &Path, format: ProjectFormat) -> Project {
    let source = dir.join("dark.mp4");
    if !source.exists() {
        // Dark gray background so white text with an outline is easy to detect.
        ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "color=c=0x303030:s=640x360:r=30:d=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=2",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            source.to_str().expect("utf-8"),
        ]);
    }
    let mut project = Project {
        format,
        media: vec![MediaAsset {
            id: 1,
            path: source.display().to_string(),
            content_hash: None,
            duration_frames: 60,
            width: 640,
            height: 360,
            has_video: true,
            has_audio: true,
            rotation_degrees: 0,
        }],
        tracks: vec![
            Track {
                id: 2,
                name: "T1".into(),
                kind: TrackKind::Subtitle,
                clips: Vec::new(),
            },
            Track {
                id: 3,
                name: "V1".into(),
                kind: TrackKind::Video,
                clips: Vec::new(),
            },
            Track {
                id: 4,
                name: "A1".into(),
                kind: TrackKind::Audio,
                clips: Vec::new(),
            },
        ],
        ..Project::default()
    };
    let (command, _) = add_media_clips_command(&project, 1).expect("add");
    command.apply(&mut project).expect("applies");
    project
}

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("d-cut-text-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

fn bright_pixels(frame: &[u8]) -> usize {
    frame.iter().filter(|value| **value > 200).count()
}

#[test]
fn title_and_subtitles_are_burned_in_for_vertical_and_square_projects() {
    if !available() {
        eprintln!("skipping: ffmpeg or GPU adapter unavailable");
        return;
    }
    let dir = dir("formats");
    for (width, height) in [(540_u32, 960_u32), (720, 720)] {
        let format = ProjectFormat {
            width,
            height,
            ..ProjectFormat::default()
        };
        let mut project = project(&dir, format);
        let srt = "1\n00:00:01,000 --> 00:00:02,000\nLegenda de teste\n";
        project.text_overlays = editor_project::parse_srt(srt, 100, Some(2), 30, 1).expect("srt");
        project.text_overlays.push(TextOverlay {
            id: 200,
            track_id: Some(2),
            start_frame: 0,
            duration_frames: 15,
            text: "Título".into(),
            x: 0.5,
            y: 0.3,
            font_size: 90.0,
            color: [1.0, 1.0, 1.0, 1.0],
        });
        project.validate().expect("valid");
        let output = dir.join(format!("text-{width}x{height}.mp4"));
        let report = export_project(
            &project,
            &output,
            ExportOptions {
                encoder: ExportEncoder::X264 {
                    crf: 18,
                    preset: "veryfast".into(),
                },
                force_gpu: false,
                device: None,
            },
            &Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .expect("export");
        assert_eq!(report.path, ExportPath::GpuComposite);
        assert_eq!(
            probe(&output),
            format!("h264,{width},{height},yuv420p,60\naac,94")
        );
        let (w, h) = (width / 4, height / 4);
        let title = frame_at(&output, 5, w, h);
        let nothing = frame_at(&output, 20, w, h);
        let subtitle = frame_at(&output, 45, w, h);
        assert!(bright_pixels(&title) > 50, "title missing");
        assert!(bright_pixels(&nothing) == 0, "text outside its time range");
        assert!(bright_pixels(&subtitle) > 50, "subtitle missing");
        // Title sits in the upper part, subtitle (SRT default) in the lower part.
        let row_of_mass = |frame: &[u8]| {
            let (sum, count) = frame
                .iter()
                .enumerate()
                .filter(|(_, value)| **value > 200)
                .fold((0_usize, 0_usize), |(sum, count), (index, _)| {
                    (sum + index / w as usize, count + 1)
                });
            sum as f32 / count.max(1) as f32 / h as f32
        };
        assert!(row_of_mass(&title) < 0.4, "title position");
        assert!(row_of_mass(&subtitle) > 0.6, "subtitle position");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn gpu_path_without_text_matches_ffmpeg_only_path() {
    if !available() {
        eprintln!("skipping: ffmpeg or GPU adapter unavailable");
        return;
    }
    let dir = dir("compare");
    let format = ProjectFormat {
        width: 640,
        height: 360,
        ..ProjectFormat::default()
    };
    let mut project = project(&dir, format);
    // Saturated colors expose color-matrix mistakes that a flat gray source hides.
    let colorful = dir.join("colorful.mp4");
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=640x360:r=30:d=2",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=2",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        colorful.to_str().expect("utf-8"),
    ]);
    project.media[0].path = colorful.display().to_string();
    let mut outputs = Vec::new();
    for force_gpu in [false, true] {
        let output = dir.join(format!("cmp-{force_gpu}.mp4"));
        let report = export_project(
            &project,
            &output,
            ExportOptions {
                encoder: ExportEncoder::X264 {
                    crf: 16,
                    preset: "veryfast".into(),
                },
                force_gpu,
                device: None,
            },
            &Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .expect("export");
        assert_eq!(report.frames, 60);
        assert_eq!(probe(&output), "h264,640,360,yuv420p,60\naac,94");
        outputs.push(output);
    }
    let psnr = Command::new("ffmpeg")
        .args(["-hide_banner", "-i"])
        .arg(&outputs[0])
        .arg("-i")
        .arg(&outputs[1])
        .args(["-lavfi", "psnr", "-f", "null", "-"])
        .output()
        .expect("ffmpeg");
    let log = String::from_utf8_lossy(&psnr.stderr);
    let average: f64 = log
        .split("average:")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .map_or(0.0, |value| {
            if value == "inf" {
                99.0
            } else {
                value.parse().unwrap_or(0.0)
            }
        });
    assert!(average >= 38.0, "GPU vs FFmpeg-only PSNR {average} dB");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn vaapi_export_with_text_works_when_a_render_node_exists() {
    let device = PathBuf::from("/dev/dri/renderD128");
    if !available() || !device.exists() {
        eprintln!("skipping: ffmpeg, GPU adapter or VAAPI render node unavailable");
        return;
    }
    let dir = dir("vaapi");
    let format = ProjectFormat {
        width: 720,
        height: 720,
        ..ProjectFormat::default()
    };
    let mut project = project(&dir, format);
    project.text_overlays.push(TextOverlay {
        id: 300,
        track_id: Some(2),
        start_frame: 0,
        duration_frames: 60,
        text: "VAAPI".into(),
        x: 0.5,
        y: 0.5,
        font_size: 100.0,
        color: [1.0, 1.0, 1.0, 1.0],
    });
    let output = dir.join("vaapi.mp4");
    let report = export_project(
        &project,
        &output,
        ExportOptions {
            encoder: ExportEncoder::Vaapi { device, qp: 22 },
            force_gpu: false,
            device: None,
        },
        &Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .expect("VAAPI export");
    assert_eq!(report.path, ExportPath::GpuComposite);
    assert_eq!(probe(&output), "h264,720,720,yuv420p,60\naac,94");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn clip_effects_are_rendered_into_the_export() {
    if !available() {
        eprintln!("skipping: ffmpeg or GPU adapter unavailable");
        return;
    }
    let dir = dir("effects");
    let format = ProjectFormat {
        width: 320,
        height: 180,
        ..ProjectFormat::default()
    };
    let mut project = project(&dir, format);
    let colorful = dir.join("colorful.mp4");
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=s=320x180:r=30:d=2",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=2",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        colorful.to_str().expect("utf-8"),
    ]);
    project.media[0].path = colorful.display().to_string();
    let video = project
        .tracks
        .iter_mut()
        .find(|track| track.kind == TrackKind::Video)
        .expect("video track");
    video.clips[0].effects.push(editor_core::EffectInstance {
        effect_id: "saturation".into(),
        params: vec![0.0],
        keyframes: Vec::new(),
    });
    let output = dir.join("gray.mp4");
    let report = export_project(
        &project,
        &output,
        ExportOptions {
            encoder: ExportEncoder::X264 {
                crf: 18,
                preset: "veryfast".into(),
            },
            force_gpu: false,
            device: None,
        },
        &Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .expect("export");
    assert_eq!(report.path, ExportPath::GpuComposite);
    // Read the U plane of frame 30: a fully desaturated picture has chroma ~128 everywhere.
    let raw = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(&output)
        .args([
            "-vf",
            "select=eq(n\\,30),format=yuv420p",
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .output()
        .expect("ffmpeg");
    let luma = 320 * 180;
    let chroma = &raw.stdout[luma..luma + luma / 4];
    let max_dev = chroma
        .iter()
        .map(|value| value.abs_diff(128))
        .max()
        .unwrap_or(255);
    assert!(max_dev <= 4, "chroma deviation {max_dev}");
    let _ = std::fs::remove_dir_all(dir);
}
