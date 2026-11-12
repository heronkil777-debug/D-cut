// SPDX-License-Identifier: AGPL-3.0-or-later

//! Golden-image test for every catalog package (BRIEFING 5.2: no golden, not delivered).
//! Tolerance: `D_CUT_GOLDEN_TOLERANCE` (default 2/255 locally; use a larger value on lavapipe).
//! Regenerate references with `D_CUT_UPDATE_GOLDEN=1` and review the contact sheet written to
//! `D_CUT_CONTACT_SHEET=/path/sheet.rgba` (WIDTH x HEIGHT*N RGBA) before committing.

use std::path::PathBuf;

use editor_effects::Catalog;
use editor_render::{
    EffectStep, OffscreenCompositor, TransitionInput, TransitionStep, headless_device,
};

const WIDTH: u32 = 96;
const HEIGHT: u32 = 64;

/// Deterministic test card: color gradients, a checkerboard and a bright disc.
fn input_image() -> Vec<u8> {
    let mut image = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let checker = ((x / 8 + y / 8) % 2) as u8;
            let dx = x as f32 - 60.0;
            let dy = y as f32 - 30.0;
            let disc = dx * dx + dy * dy < 150.0;
            let r = (x * 255 / (WIDTH - 1)) as u8;
            let g = (y * 255 / (HEIGHT - 1)) as u8;
            let b = if checker == 1 { 200 } else { 40 };
            if disc {
                image.extend_from_slice(&[250, 240, 230, 255]);
            } else {
                image.extend_from_slice(&[r, g, b, 255]);
            }
        }
    }
    image
}

fn golden_path(id: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../editor-effects/effects")
        .join(id)
        .join("golden.rgba")
}

#[test]
fn every_effect_matches_its_golden_image() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("skipping: no GPU adapter (install lavapipe for CI)");
        return;
    };
    let tolerance: u8 = std::env::var("D_CUT_GOLDEN_TOLERANCE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2);
    let update = std::env::var_os("D_CUT_UPDATE_GOLDEN").is_some();
    let catalog = Catalog::builtin().expect("catalog");
    let mut compositor = OffscreenCompositor::new(device, queue, WIDTH, HEIGHT);
    let input = input_image();
    let mut sheet = input.clone();
    let mut failures = Vec::new();
    for package in catalog.effects() {
        let id = &package.manifest.id;
        let step = EffectStep {
            package: package.clone(),
            params: package.uniform_params(&package.golden_params()),
        };
        let output = compositor
            .compose(&input, None, std::slice::from_ref(&step))
            .expect("compose");
        sheet.extend_from_slice(&output);
        let changed = output
            .iter()
            .zip(&input)
            .filter(|(a, b)| a.abs_diff(**b) > 8)
            .count();
        if changed < 100 {
            failures.push(format!(
                "{id}: golden params barely change the image ({changed})"
            ));
        }
        let path = golden_path(id);
        if update {
            std::fs::write(&path, &output).expect("write golden");
            continue;
        }
        match std::fs::read(&path) {
            Ok(golden) if golden.len() == output.len() => {
                let max = golden
                    .iter()
                    .zip(&output)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap_or(0);
                if max > tolerance {
                    failures.push(format!("{id}: max diff {max} > tolerance {tolerance}"));
                }
            }
            Ok(_) => failures.push(format!("{id}: golden has the wrong size")),
            Err(_) => failures.push(format!("{id}: missing golden image {}", path.display())),
        }
    }
    if let Some(path) = std::env::var_os("D_CUT_CONTACT_SHEET") {
        std::fs::write(path, &sheet).expect("write contact sheet");
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn two_stacked_effects_apply_in_order() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let catalog = Catalog::builtin().expect("catalog");
    let mut compositor = OffscreenCompositor::new(device, queue, WIDTH, HEIGHT);
    let input = input_image();
    let step = |id: &str, values: &[f32]| {
        let package = catalog.get(id).expect("package").clone();
        EffectStep {
            params: package.uniform_params(values),
            package,
        }
    };
    let gray = step("saturation", &[0.0]);
    let bright = step("brightness-contrast", &[0.2, 1.0]);
    let both = compositor
        .compose(&input, None, &[gray.clone(), bright.clone()])
        .expect("compose");
    let only_gray = compositor.compose(&input, None, &[gray]).expect("compose");
    // Output is gray (r == g == b) and brighter than the grayscale-only result.
    for (pixel, reference) in both.chunks_exact(4).zip(only_gray.chunks_exact(4)) {
        assert!(pixel[0].abs_diff(pixel[1]) <= 1 && pixel[1].abs_diff(pixel[2]) <= 1);
        assert!(pixel[0] >= reference[0].saturating_add(40).min(254));
    }
}

/// Second test card for transitions: diagonal stripes in two colors.
fn second_image() -> Vec<u8> {
    (0..HEIGHT)
        .flat_map(|y| {
            (0..WIDTH).flat_map(move |x| {
                if ((x + y) / 6) % 2 == 0 {
                    [20, 30, 160, 255]
                } else {
                    [240, 200, 40, 255]
                }
            })
        })
        .collect()
}

#[test]
fn every_transition_matches_its_golden_and_hits_both_ends() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("skipping: no GPU adapter (install lavapipe for CI)");
        return;
    };
    let tolerance: u8 = std::env::var("D_CUT_GOLDEN_TOLERANCE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2);
    let update = std::env::var_os("D_CUT_UPDATE_GOLDEN").is_some();
    let catalog = Catalog::builtin().expect("catalog");
    let mut compositor = OffscreenCompositor::new(device, queue, WIDTH, HEIGHT);
    let (from, to) = (input_image(), second_image());
    let mut sheet = Vec::new();
    let mut failures = Vec::new();
    let max_diff = |a: &[u8], b: &[u8]| {
        a.iter()
            .zip(b)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0)
    };
    for package in catalog.transitions() {
        let id = &package.manifest.id;
        let mut run = |progress: f32| {
            compositor
                .compose_with(
                    &from,
                    None,
                    &[],
                    Some(TransitionInput {
                        side: &to,
                        base_is_from: true,
                        step: TransitionStep {
                            package: package.clone(),
                            params: package.uniform_params(&package.golden_params()),
                            progress,
                        },
                    }),
                )
                .expect("compose")
        };
        let start = run(0.0);
        let middle = run(0.35);
        let end = run(1.0);
        sheet.extend_from_slice(&middle);
        // Edges: pixel-accurate except blur/zoom based ones, which settle within tolerance.
        if max_diff(&start, &from) > 12 {
            failures.push(format!("{id}: progress 0 is not the outgoing frame"));
        }
        if max_diff(&end, &to) > 12 {
            failures.push(format!("{id}: progress 1 is not the incoming frame"));
        }
        let path = golden_path(id);
        if update {
            std::fs::write(&path, &middle).expect("write golden");
            continue;
        }
        match std::fs::read(&path) {
            Ok(golden) if golden.len() == middle.len() => {
                let diff = max_diff(&golden, &middle);
                if diff > tolerance {
                    failures.push(format!("{id}: max diff {diff} > tolerance {tolerance}"));
                }
            }
            _ => failures.push(format!("{id}: missing golden image {}", path.display())),
        }
    }
    if let Some(path) = std::env::var_os("D_CUT_TRANSITION_SHEET") {
        std::fs::write(path, &sheet).expect("write sheet");
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
