// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{path::PathBuf, str::FromStr};

use anyhow::{Context, Result, bail};
use editor_core::PRODUCT_NAME;
use editor_media::{ExportProfile, ExportRequest, Rational};

fn main() {
    if let Err(error) = run() {
        eprintln!("d-cut-cli: {error:#}");
        std::process::exit(2);
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version" | "-V") => {
            println!("{PRODUCT_NAME} {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("validate-project") => {
            let path = args
                .next()
                .context("usage: d-cut-cli validate-project <project.dcut>")?;
            if args.next().is_some() {
                bail!("validate-project accepts exactly one project path");
            }
            let project = editor_project::load(&path)
                .with_context(|| format!("could not validate project {}", path))?;
            let clip_count: usize = project.tracks.iter().map(|track| track.clips.len()).sum();
            println!(
                "Valid D-cut project: {}x{} at {}/{} fps, {} media assets, {} tracks, {} clips",
                project.format.width,
                project.format.height,
                project.format.frame_rate_numerator,
                project.format.frame_rate_denominator,
                project.media.len(),
                project.tracks.len(),
                clip_count,
            );
            Ok(())
        }
        Some("import-srt") => run_import_srt(args),
        Some("probe") => {
            let path = args.next().context("usage: d-cut-cli probe <media-file>")?;
            if args.next().is_some() {
                bail!("probe accepts exactly one media path");
            }
            let info =
                editor_media::probe(&path).with_context(|| format!("could not probe {path}"))?;
            println!("Duration: {:.3} s", info.duration_seconds);
            if let Some(video) = info.video {
                println!(
                    "Video: {} {}x{} rotation={} fps={}",
                    video.codec,
                    video.width,
                    video.height,
                    video.rotation_degrees,
                    video.frame_rate.map_or_else(
                        || "unknown".to_owned(),
                        |rate| format!("{}/{}", rate.numerator, rate.denominator)
                    )
                );
            }
            if let Some(audio) = info.audio {
                println!(
                    "Audio: {} {} Hz channels={}",
                    audio.codec,
                    audio
                        .sample_rate
                        .map_or_else(|| "unknown".to_owned(), |rate| rate.to_string()),
                    audio
                        .channels
                        .map_or_else(|| "unknown".to_owned(), |channels| channels.to_string())
                );
            }
            Ok(())
        }
        Some("render-project") => run_render_project(args),
        Some("download-model") => run_download_model(),
        Some("bench-preview") => run_bench_preview(args),
        Some("transcribe") => run_transcribe(args),
        Some("export-quality") => run_export(args, false),
        Some("export-vaapi") => run_export(args, true),
        Some("--help" | "-h") | None => {
            println!(
                "D-cut local project and media tools\n\nUsage:\n  d-cut-cli --version\n  d-cut-cli validate-project <project.dcut>\n  d-cut-cli import-srt <input.srt> <output.dcut> <fps-num> <fps-den>\n  d-cut-cli probe <media-file>\n  d-cut-cli download-model            (opt-in: Whisper small, 487 MB, MIT, SHA-256 verified)\n  d-cut-cli transcribe <project.dcut> [--output out.dcut] [--language pt] [--threads N]\n  d-cut-cli render-project <project.dcut> <output.mp4> [--fast] [--crf N] [--preset NAME] [--qp N] [--vaapi-device PATH] [--gpu-composite]\n  d-cut-cli export-quality <input> <output> <start-sec> <duration-sec> <width> <height> <fps-num> <fps-den> <crf>\n  d-cut-cli export-vaapi <input> <output> <start-sec> <duration-sec> <width> <height> <fps-num> <fps-den> <device> <qp>\n\nrender-project uses the same FFmpeg timeline graph as the editor export: H.264 + AAC 48 kHz,\nyuv420p, BT.709, +faststart. --fast uses VAAPI (h264_vaapi); the default uses libx264 CRF 20.\nFFmpeg and ffprobe must be installed (or set D_CUT_FFMPEG / D_CUT_FFPROBE)."
            );
            Ok(())
        }
        Some(command) => bail!("unsupported command: {command}"),
    }
}

fn run_import_srt(mut args: impl Iterator<Item = String>) -> Result<()> {
    let input = args.next().context(
        "usage: d-cut-cli import-srt <input.srt> <output.dcut> <fps-numerator> <fps-denominator>",
    )?;
    let output = args.next().context("missing output project path")?;
    let numerator: u32 = parse_next(&mut args, "frame-rate numerator")?;
    let denominator: u32 = parse_next(&mut args, "frame-rate denominator")?;
    if args.next().is_some() {
        bail!("import-srt received too many arguments");
    }
    let source = std::fs::read_to_string(&input)
        .with_context(|| format!("could not read subtitle file {input}"))?;
    let project = editor_core::Project {
        text_overlays: editor_project::parse_srt(&source, 1, None, numerator, denominator)?,
        ..editor_core::Project::default()
    };
    editor_project::save(&output, &project)
        .with_context(|| format!("could not save project {output}"))?;
    println!(
        "Imported {} subtitle cues into {}",
        project.text_overlays.len(),
        output
    );
    Ok(())
}

fn run_render_project(mut args: impl Iterator<Item = String>) -> Result<()> {
    let project_path = args
        .next()
        .context("usage: d-cut-cli render-project <project.dcut> <output.mp4> [options]")?;
    let output = PathBuf::from(args.next().context("missing output path")?);
    let mut fast = false;
    let mut crf: u8 = 20;
    let mut preset = "medium".to_owned();
    let mut qp: u8 = 22;
    let mut device = PathBuf::from("/dev/dri/renderD128");
    let mut force_gpu = false;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--fast" => fast = true,
            "--gpu-composite" => force_gpu = true,
            "--crf" => crf = parse_next(&mut args, "CRF")?,
            "--preset" => preset = args.next().context("missing preset")?,
            "--qp" => qp = parse_next(&mut args, "QP")?,
            "--vaapi-device" => {
                device = PathBuf::from(args.next().context("missing VAAPI device")?);
            }
            other => bail!("unknown option {other}"),
        }
    }
    let project = editor_project::load(&project_path)
        .with_context(|| format!("could not load project {project_path}"))?;
    let total = project.duration_frames();
    if total == 0 {
        bail!("the project timeline is empty");
    }
    let encoder = if fast {
        editor_media::timeline::ExportEncoder::Vaapi { device, qp }
    } else {
        editor_media::timeline::ExportEncoder::X264 { crf, preset }
    };
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut last_percent = None;
    let report = editor_export::export_project(
        &project,
        &output,
        editor_export::ExportOptions {
            encoder,
            force_gpu,
            device: None,
        },
        &cancel,
        |progress| {
            let percent = (progress.fraction * 100.0).floor() as u32;
            if last_percent != Some(percent) {
                last_percent = Some(percent);
                eprint!("\rrendering… {percent:3}%");
            }
        },
    )
    .context("export failed")?;
    eprintln!();
    println!(
        "Path: {:?} · {} frames in {:.2} s ({:.1} fps)",
        report.path,
        report.frames,
        report.seconds,
        report.frames as f64 / report.seconds.max(1e-6)
    );
    let info = editor_media::probe(&output).context("could not probe the exported file")?;
    println!(
        "Rendered {} ({:.3} s, {} frames at {}/{} fps)",
        output.display(),
        info.duration_seconds,
        total,
        project.format.frame_rate_numerator,
        project.format.frame_rate_denominator
    );
    Ok(())
}

/// Headless preview benchmark: the same streams and GPU composition as the editor preview
/// (main + transition side + audio decoders, effects/keyframes/transition/text at preview size,
/// proxies when ready), as fast as possible. Reports fps and resident memory.
fn run_bench_preview(mut args: impl Iterator<Item = String>) -> Result<()> {
    use editor_media::timeline::{TimelineRender, spawn_audio_stream, spawn_video_stream};
    let path = args
        .next()
        .context("usage: d-cut-cli bench-preview <project.dcut> [--long-side N] [--no-proxies]")?;
    let mut long_side = 640_u32;
    let mut proxies = true;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--long-side" => long_side = parse_next(&mut args, "long side")?,
            "--no-proxies" => proxies = false,
            other => bail!("unknown option {other}"),
        }
    }
    let project = editor_project::load(&path).with_context(|| format!("could not load {path}"))?;
    let total = project.duration_frames();
    let (w, h) = (project.format.width, project.format.height);
    let scale = f64::from(long_side) / f64::from(w.max(h));
    let even = |v: u32| ((f64::from(v) * scale).round() as u32 / 2 * 2).max(2);
    let (width, height) = (even(w), even(h));
    let prepare = |job: TimelineRender| if proxies { job.with_proxies() } else { job };
    let main = prepare(TimelineRender::from_project(
        &project, 0, total, width, height,
    )?);
    let side = TimelineRender::transition_side(&project, 0, total, width, height)?.map(prepare);
    let audio = prepare(TimelineRender::from_project(&project, 0, total, 2, 2)?);
    let audio_thread = std::thread::spawn(move || -> Result<u64> {
        let mut stream = spawn_audio_stream(&audio, 48_000)?;
        let mut buffer = vec![0.0_f32; 8192];
        let mut samples = 0_u64;
        loop {
            let read = stream.read_samples(&mut buffer)?;
            if read == 0 {
                return Ok(samples);
            }
            samples += read as u64;
        }
    });
    let (device, queue) = editor_render::headless_device()?;
    let mut compositor = editor_render::OffscreenCompositor::new(device, queue, width, height);
    let catalog = editor_effects::Catalog::builtin()?;
    let windows = editor_core::transition_windows(&project);
    let mut text = editor_text::TextRenderer::new();
    let mut decoder = spawn_video_stream(&main)?;
    let mut side_decoder = side.map(|job| spawn_video_stream(&job)).transpose()?;
    let started = std::time::Instant::now();
    let mut frames = 0_u64;
    let mut cached: Option<(Vec<u64>, Option<Vec<u8>>)> = None;
    while let Some(base) = decoder.next_frame()? {
        let side_frame = match &mut side_decoder {
            Some(stream) => stream.next_frame()?,
            None => None,
        };
        let active: Vec<u64> = editor_text::active_overlays(&project, frames)
            .iter()
            .map(|o| o.id)
            .collect();
        if cached.as_ref().is_none_or(|(ids, _)| *ids != active) {
            cached = Some((active, text.render_frame(&project, frames, width, height)));
        }
        let overlay = cached.as_ref().and_then(|(_, layer)| layer.as_deref());
        let steps = editor_export::effect_steps(&project, &catalog, frames);
        let transition = match (
            &side_frame,
            editor_export::transition_at(&windows, &catalog, frames),
        ) {
            (Some(side), Some((step, base_is_from))) => Some(editor_render::TransitionInput {
                side,
                base_is_from,
                step,
            }),
            _ => None,
        };
        compositor.compose_with(&base, overlay, &steps, transition)?;
        frames += 1;
    }
    let seconds = started.elapsed().as_secs_f64();
    let samples = audio_thread
        .join()
        .map_err(|_| anyhow::anyhow!("audio thread panicked"))??;
    let rss = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|statm| statm.split_whitespace().nth(1)?.parse::<f64>().ok())
        .map_or(0.0, |pages| pages * 4096.0 / 1_048_576.0);
    let fps = project.format.frames_per_second();
    println!(
        "{frames}/{total} frames at {width}x{height} in {seconds:.1} s = {:.1} fps (needs {fps:.0}); audio {:.1} s decoded; proxies {}; transitions {}; RSS {rss:.0} MB",
        frames as f64 / seconds.max(1e-6),
        samples as f64 / 2.0 / 48_000.0,
        if proxies { "on" } else { "off" },
        windows.len()
    );
    Ok(())
}

fn run_download_model() -> Result<()> {
    let model = editor_speech::MODEL_SMALL;
    println!(
        "Downloading {} ({:.0} MB, license: {}) to {}",
        model.name,
        model.bytes as f64 / 1e6,
        model.license,
        editor_speech::model_path(&model).display()
    );
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut last = None;
    let path = editor_speech::download_model(&model, &cancel, |fraction| {
        let percent = (fraction * 100.0) as u32;
        if last != Some(percent) {
            last = Some(percent);
            eprint!("\rdownloading… {percent:3}%");
        }
    })?;
    eprintln!();
    println!(
        "Model verified (SHA-256 {}) at {}",
        model.sha256,
        path.display()
    );
    Ok(())
}

fn run_transcribe(mut args: impl Iterator<Item = String>) -> Result<()> {
    let project_path = args
        .next()
        .context("usage: d-cut-cli transcribe <project.dcut> [--output out.dcut]")?;
    let mut output = PathBuf::from(&project_path);
    let mut language = "pt".to_owned();
    let mut threads = std::thread::available_parallelism().map_or(4, |count| count.get());
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--output" => output = PathBuf::from(args.next().context("missing output")?),
            "--language" => language = args.next().context("missing language")?,
            "--threads" => threads = parse_next(&mut args, "threads")?,
            other => bail!("unknown option {other}"),
        }
    }
    let model = editor_speech::MODEL_SMALL;
    if !editor_speech::model_ready(&model) {
        bail!("model not downloaded; run `d-cut-cli download-model` first (opt-in)");
    }
    let mut project = editor_project::load(&project_path)
        .with_context(|| format!("could not load {project_path}"))?;
    let total = project.duration_frames();
    if total == 0 {
        bail!("the project timeline is empty");
    }
    let job = editor_media::timeline::TimelineRender::from_project(&project, 0, total, 2, 2)?;
    let wav = std::env::temp_dir().join(format!("d-cut-speech-{}.wav", std::process::id()));
    editor_media::timeline::export_audio_wav(&job, &wav, 16_000)?;
    let started = std::time::Instant::now();
    let result =
        editor_speech::transcribe(&wav, &editor_speech::model_path(&model), &language, threads);
    let _ = std::fs::remove_file(&wav);
    let words = result?;
    let cues = editor_speech::group_words(&words, 42, 3500, 700);
    let seconds = project.format.frames_to_seconds(total);
    println!(
        "{} words, {} cues in {:.1} s ({:.2}x realtime)",
        words.len(),
        cues.len(),
        started.elapsed().as_secs_f64(),
        seconds / started.elapsed().as_secs_f64().max(1e-6)
    );
    for cue in &cues {
        println!(
            "  {:>7.2}–{:<7.2} {}",
            cue.start_ms as f64 / 1000.0,
            cue.end_ms as f64 / 1000.0,
            cue.text
        );
    }
    let command = editor_speech::subtitles_command(&project, &cues);
    command
        .apply(&mut project)
        .map_err(|error| anyhow::anyhow!("could not add subtitles: {error}"))?;
    editor_project::save(&output, &project)
        .with_context(|| format!("could not save {}", output.display()))?;
    println!("Subtitles saved to {}", output.display());
    Ok(())
}

fn run_export(mut args: impl Iterator<Item = String>, vaapi: bool) -> Result<()> {
    let input = PathBuf::from(args.next().context("missing input media path")?);
    let output = PathBuf::from(args.next().context("missing output media path")?);
    let start_seconds = parse_next(&mut args, "start time in seconds")?;
    let duration_seconds = parse_next(&mut args, "duration in seconds")?;
    let width = parse_next(&mut args, "output width")?;
    let height = parse_next(&mut args, "output height")?;
    let numerator = parse_next(&mut args, "frame-rate numerator")?;
    let denominator = parse_next(&mut args, "frame-rate denominator")?;
    let profile = if vaapi {
        let device = PathBuf::from(args.next().context("missing VAAPI render device path")?);
        let quality = parse_next(&mut args, "VAAPI quality")?;
        ExportProfile::Vaapi { device, quality }
    } else {
        let crf = parse_next(&mut args, "x264 CRF")?;
        ExportProfile::Quality {
            crf,
            preset: "medium".to_owned(),
        }
    };
    if args.next().is_some() {
        bail!("too many export arguments");
    }
    let request = ExportRequest {
        input,
        output,
        start_seconds,
        duration_seconds,
        width,
        height,
        frame_rate: Rational {
            numerator,
            denominator,
        },
        profile,
    };
    editor_media::export(&request).context("FFmpeg export failed")?;
    println!("Export completed: {}", request.output.display());
    Ok(())
}

fn parse_next<T: FromStr>(args: &mut impl Iterator<Item = String>, description: &str) -> Result<T>
where
    T::Err: std::fmt::Display,
{
    let value = args
        .next()
        .with_context(|| format!("missing {description}"))?;
    value
        .parse()
        .map_err(|error| anyhow::anyhow!("invalid {description} {value:?}: {error}"))
}
