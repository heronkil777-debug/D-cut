// SPDX-License-Identifier: AGPL-3.0-or-later

//! Text layout and rasterization for titles and subtitles (cosmic-text + embedded Noto Sans
//! Bold, SIL OFL 1.1). Produces a straight-alpha RGBA layer that `editor-render` composites on
//! the GPU, identically for preview and export.

use cosmic_text::{
    Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, fontdb,
};
use editor_core::{Project, TextOverlay};

/// Embedded default font (SIL Open Font License 1.1, see assets/fonts/NotoSans-OFL.txt).
pub const DEFAULT_FONT: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Bold.ttf");
const FONT_FAMILY: &str = "Noto Sans";

/// Overlays visible at a timeline frame, in drawing order.
pub fn active_overlays(project: &Project, frame: u64) -> Vec<&TextOverlay> {
    project
        .text_overlays
        .iter()
        .filter(|overlay| {
            overlay.start_frame <= frame
                && frame < overlay.start_frame.saturating_add(overlay.duration_frames)
        })
        .collect()
}

pub struct TextRenderer {
    fonts: FontSystem,
    cache: SwashCache,
}

impl Default for TextRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl TextRenderer {
    /// Uses only the embedded font, so output is identical on every machine.
    pub fn new() -> Self {
        let mut db = fontdb::Database::new();
        db.load_font_data(DEFAULT_FONT.to_vec());
        db.set_sans_serif_family(FONT_FAMILY);
        Self {
            fonts: FontSystem::new_with_locale_and_db("pt-BR".to_owned(), db),
            cache: SwashCache::new(),
        }
    }

    /// Renders the overlays active at `frame` into an `out_width`×`out_height` layer.
    /// Returns `None` when no text is visible (nothing to composite).
    pub fn render_frame(
        &mut self,
        project: &Project,
        frame: u64,
        out_width: u32,
        out_height: u32,
    ) -> Option<Vec<u8>> {
        let overlays = active_overlays(project, frame);
        if overlays.is_empty() {
            return None;
        }
        Some(self.render_overlays(&overlays, project.format.height, out_width, out_height))
    }

    /// Rasterizes overlays authored for a canvas `canvas_height` pixels tall, scaled to the
    /// output size. Positions are normalized (0..1) centers of the text block.
    pub fn render_overlays(
        &mut self,
        overlays: &[&TextOverlay],
        canvas_height: u32,
        out_width: u32,
        out_height: u32,
    ) -> Vec<u8> {
        let mut layer = vec![0_u8; out_width as usize * out_height as usize * 4];
        let scale = out_height as f32 / canvas_height.max(1) as f32;
        for overlay in overlays {
            self.draw_overlay(overlay, scale, out_width, out_height, &mut layer);
        }
        layer
    }

    fn draw_overlay(
        &mut self,
        overlay: &TextOverlay,
        scale: f32,
        width: u32,
        height: u32,
        layer: &mut [u8],
    ) {
        let font_size = (overlay.font_size * scale).max(1.0);
        let box_width = width as f32 * 0.9;
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(font_size, font_size * 1.25));
        buffer.set_size(Some(box_width), None);
        let attrs = Attrs::new().family(Family::Name(FONT_FAMILY));
        buffer.set_text(
            &overlay.text,
            &attrs,
            Shaping::Advanced,
            Some(Align::Center),
        );
        buffer.shape_until_scroll(&mut self.fonts, false);
        let block_height: f32 = buffer.layout_runs().map(|run| run.line_height).sum();
        let left = (overlay.x * width as f32 - box_width / 2.0).round() as i32;
        let top = (overlay.y * height as f32 - block_height / 2.0).round() as i32;
        let [r, g, b, a] = overlay
            .color
            .map(|channel| (channel.clamp(0.0, 1.0) * 255.0) as u8);
        // Outline for legibility over any footage, then the fill.
        let outline = (font_size * 0.07).round().max(1.0) as i32;
        let outline_alpha = a;
        let mut passes: Vec<(i32, i32, Color)> = Vec::new();
        for dy in -outline..=outline {
            for dx in -outline..=outline {
                if (dx != 0 || dy != 0) && dx * dx + dy * dy <= outline * outline + 1 {
                    passes.push((dx, dy, Color::rgba(0, 0, 0, outline_alpha)));
                }
            }
        }
        passes.push((0, 0, Color::rgba(r, g, b, a)));
        for (dx, dy, color) in passes {
            buffer.draw(
                &mut self.fonts,
                &mut self.cache,
                color,
                |x, y, w, h, pixel| {
                    blend_rect(
                        layer,
                        width,
                        height,
                        left + dx + x,
                        top + dy + y,
                        w,
                        h,
                        pixel,
                    );
                },
            );
        }
    }
}

/// Straight-alpha "over" blend of a solid rectangle into the layer.
#[allow(clippy::too_many_arguments)]
fn blend_rect(
    layer: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: Color,
) {
    let (sr, sg, sb, sa) = color.as_rgba_tuple();
    if sa == 0 {
        return;
    }
    let source_alpha = f32::from(sa) / 255.0;
    for row in y.max(0)..(y + h as i32).min(height as i32) {
        for column in x.max(0)..(x + w as i32).min(width as i32) {
            let index = (row as usize * width as usize + column as usize) * 4;
            let destination_alpha = f32::from(layer[index + 3]) / 255.0;
            let out_alpha = source_alpha + destination_alpha * (1.0 - source_alpha);
            if out_alpha <= 0.0 {
                continue;
            }
            for (channel, source) in [sr, sg, sb].into_iter().enumerate() {
                let destination = f32::from(layer[index + channel]);
                let value = (f32::from(source) * source_alpha
                    + destination * destination_alpha * (1.0 - source_alpha))
                    / out_alpha;
                layer[index + channel] = value.round().clamp(0.0, 255.0) as u8;
            }
            layer[index + 3] = (out_alpha * 255.0).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlay(text: &str, y: f32) -> TextOverlay {
        TextOverlay {
            id: 1,
            track_id: None,
            start_frame: 10,
            duration_frames: 20,
            text: text.to_owned(),
            x: 0.5,
            y,
            font_size: 72.0,
            color: [1.0, 1.0, 1.0, 1.0],
        }
    }

    fn opaque_bounds(layer: &[u8], width: u32) -> Option<(u32, u32, u32, u32)> {
        let mut bounds: Option<(u32, u32, u32, u32)> = None;
        for (index, pixel) in layer.chunks_exact(4).enumerate() {
            if pixel[3] > 128 {
                let (x, y) = (index as u32 % width, index as u32 / width);
                bounds = Some(bounds.map_or((x, y, x, y), |(a, b, c, d)| {
                    (a.min(x), b.min(y), c.max(x), d.max(y))
                }));
            }
        }
        bounds
    }

    #[test]
    fn text_is_rendered_centered_with_white_fill_and_dark_outline() {
        let mut renderer = TextRenderer::new();
        let item = overlay("Olá, mundo", 0.5);
        let layer = renderer.render_overlays(&[&item], 1080, 960, 540);
        let (x0, y0, x1, y1) = opaque_bounds(&layer, 960).expect("text pixels");
        let center_x = (x0 + x1) / 2;
        let center_y = (y0 + y1) / 2;
        assert!((center_x as i32 - 480).abs() < 25, "x center {center_x}");
        assert!((center_y as i32 - 270).abs() < 25, "y center {center_y}");
        let white = layer
            .chunks_exact(4)
            .filter(|p| p[3] == 255 && p[0] > 240 && p[1] > 240 && p[2] > 240)
            .count();
        let dark = layer
            .chunks_exact(4)
            .filter(|p| p[3] == 255 && p[0] < 20)
            .count();
        assert!(white > 200 && dark > 200, "white {white}, dark {dark}");
    }

    #[test]
    fn layer_scales_with_output_size_and_respects_timing() {
        let mut renderer = TextRenderer::new();
        let mut project = Project::default();
        project.text_overlays.push(overlay("Legenda", 0.85));
        assert!(renderer.render_frame(&project, 9, 640, 360).is_none());
        assert!(renderer.render_frame(&project, 30, 640, 360).is_none());
        let small = renderer
            .render_frame(&project, 10, 640, 360)
            .expect("visible");
        let large = renderer
            .render_frame(&project, 29, 1920, 1080)
            .expect("visible");
        let (_, sy0, _, sy1) = opaque_bounds(&small, 640).expect("small");
        let (_, ly0, _, ly1) = opaque_bounds(&large, 1920).expect("large");
        let ratio = (ly1 - ly0) as f32 / (sy1 - sy0) as f32;
        assert!((ratio - 3.0).abs() < 0.35, "height ratio {ratio}");
        assert!(sy0 > 360 / 2, "subtitle sits in the lower half");
    }
}
