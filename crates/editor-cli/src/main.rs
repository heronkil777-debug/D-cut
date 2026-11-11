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
        Some("export-quality") => run_export(args, false),
        Some("export-vaapi") => run_export(args, true),
        Some("--help" | "-h") | None => {
            println!(
                "D-cut local project and media tools\n\nUsage:\n  d-cut-cli --version\n  d-cut-cli validate-project <project.dcut>\n  d-cut-cli import-srt <input.srt> <output.dcut> <fps-num> <fps-den>\n  d-cut-cli probe <media-file>\n  d-cut-cli export-quality <input> <output> <start-sec> <duration-sec> <width> <height> <fps-num> <fps-den> <crf>\n  d-cut-cli export-vaapi <input> <output> <start-sec> <duration-sec> <width> <height> <fps-num> <fps-den> <device> <qp>\n\nFFmpeg and ffprobe must be installed separately. Rendering a multi-track project is not implemented yet."
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
    let mut project = editor_core::Project::default();
    project.text_overlays = editor_project::parse_srt(&source, 1, None, numerator, denominator)?;
    editor_project::save(&output, &project)
        .with_context(|| format!("could not save project {output}"))?;
    println!(
        "Imported {} subtitle cues into {}",
        project.text_overlays.len(),
        output
    );
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
