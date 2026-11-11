// SPDX-License-Identifier: AGPL-3.0-or-later

//! GPU composition on a wgpu device received from outside: the UI passes eframe's device
//! (preview inside egui), the export/CLI passes a headless device. Both use `compose.wgsl`,
//! so the pixels shown in the preview are composited exactly like the exported ones.

use std::sync::Arc;

use egui::{Painter, Rect};
use egui_wgpu::{Callback, CallbackResources, CallbackTrait, RenderState, ScreenDescriptor};
use thiserror::Error;

pub use wgpu;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("no compatible GPU adapter: {0}")]
    Adapter(String),
    #[error("could not create the GPU device: {0}")]
    Device(String),
    #[error("layer size mismatch: expected {expected} bytes, got {actual}")]
    LayerSize { expected: usize, actual: usize },
    #[error("GPU readback failed: {0}")]
    Readback(String),
}

/// A straight-alpha RGBA8 layer. `generation` changes whenever the pixels change.
#[derive(Clone)]
pub struct PreviewFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
    pub generation: u64,
}

/// Shared pipeline: base + overlay textures composited into one target.
struct ComposePipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl ComposePipeline {
    fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("d-cut-compose-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("compose.wgsl").into()),
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                multisampled: false,
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("d-cut-compose-layout"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("d-cut-compose-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("d-cut-compose-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("d-cut-compose-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
        }
    }

    fn bind(
        &self,
        device: &wgpu::Device,
        base: &wgpu::TextureView,
        overlay: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("d-cut-compose-bind-group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(base),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(overlay),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }
}

/// A sampled RGBA texture that is re-created only when its size changes.
struct Layer {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    generation: Option<u64>,
}

impl Layer {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("d-cut-layer"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            width,
            height,
            generation: None,
        }
    }

    fn write(&self, queue: &wgpu::Queue, rgba: &[u8]) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * self.width),
                rows_per_image: Some(self.height),
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }
}

fn check_size(width: u32, height: u32, rgba: &[u8]) -> Result<(), RenderError> {
    let expected = width as usize * height as usize * 4;
    if rgba.len() == expected {
        Ok(())
    } else {
        Err(RenderError::LayerSize {
            expected,
            actual: rgba.len(),
        })
    }
}

/// Creates a headless device (for the CLI/export and tests).
pub fn headless_device() -> Result<(wgpu::Device, wgpu::Queue), RenderError> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .map_err(|error| RenderError::Adapter(error.to_string()))?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("d-cut-headless"),
        ..Default::default()
    }))
    .map_err(|error| RenderError::Device(error.to_string()))
}

/// Offscreen compositor used by the export: uploads base + overlay, renders, reads back RGBA.
pub struct OffscreenCompositor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: ComposePipeline,
    base: Layer,
    overlay: Layer,
    empty_overlay: Layer,
    target: wgpu::Texture,
    target_view: wgpu::TextureView,
    readback: wgpu::Buffer,
    padded_row: u32,
    width: u32,
    height: u32,
}

impl OffscreenCompositor {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, width: u32, height: u32) -> Self {
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let pipeline = ComposePipeline::new(&device, format);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("d-cut-export-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row = (width * 4).div_ceil(align) * align;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("d-cut-export-readback"),
            size: u64::from(padded_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let empty_overlay = Layer::new(&device, format, 1, 1);
        empty_overlay.write(&queue, &[0, 0, 0, 0]);
        Self {
            base: Layer::new(&device, format, width, height),
            overlay: Layer::new(&device, format, width, height),
            empty_overlay,
            pipeline,
            target,
            target_view,
            readback,
            padded_row,
            width,
            height,
            device,
            queue,
        }
    }

    /// Composites `overlay` (straight alpha) over `base` and returns the RGBA result.
    pub fn compose(&mut self, base: &[u8], overlay: Option<&[u8]>) -> Result<Vec<u8>, RenderError> {
        check_size(self.width, self.height, base)?;
        self.base.write(&self.queue, base);
        let overlay_view = match overlay {
            Some(overlay) => {
                check_size(self.width, self.height, overlay)?;
                self.overlay.write(&self.queue, overlay);
                &self.overlay.view
            }
            None => &self.empty_overlay.view,
        };
        let bind_group = self
            .pipeline
            .bind(&self.device, &self.base.view, overlay_view);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("d-cut-export"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("d-cut-export-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        self.readback
            .map_async(wgpu::MapMode::Read, .., move |result| {
                let _ = sender.send(result);
            });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| RenderError::Readback(error.to_string()))?;
        receiver
            .recv()
            .map_err(|error| RenderError::Readback(error.to_string()))?
            .map_err(|error| RenderError::Readback(error.to_string()))?;
        let mut output = Vec::with_capacity(self.width as usize * self.height as usize * 4);
        {
            let view = self
                .readback
                .get_mapped_range(..)
                .map_err(|error| RenderError::Readback(error.to_string()))?;
            for row in view.chunks_exact(self.padded_row as usize) {
                output.extend_from_slice(&row[..self.width as usize * 4]);
            }
        }
        self.readback.unmap();
        Ok(output)
    }
}

/// Draws preview frames (base video + text overlay) inside egui with the shared pipeline.
pub struct FrameRenderer;

impl FrameRenderer {
    pub fn new(render_state: &RenderState) -> Self {
        let device = &render_state.device;
        // Frames are sRGB-encoded bytes: sample through sRGB views on an sRGB target, pass
        // bytes straight through on a linear target (the egui default). Same math as export.
        let layer_format = if render_state.target_format.is_srgb() {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        let pipeline = ComposePipeline::new(device, render_state.target_format);
        let empty_overlay = Layer::new(device, layer_format, 1, 1);
        empty_overlay.write(&render_state.queue, &[0, 0, 0, 0]);
        let _ = render_state
            .renderer
            .write()
            .callback_resources
            .insert(PreviewResources {
                pipeline,
                layer_format,
                base: None,
                overlay: None,
                empty_overlay,
                bind_group: None,
                bound: (None, None),
            });
        Self
    }

    /// Paints `frame` with optional `overlay` (same size) stretched into `rect`.
    pub fn paint(
        &self,
        painter: &Painter,
        rect: Rect,
        frame: Option<PreviewFrame>,
        overlay: Option<PreviewFrame>,
    ) {
        painter.add(Callback::new_paint_callback(
            rect,
            PreviewCallback { frame, overlay },
        ));
    }
}

struct PreviewResources {
    pipeline: ComposePipeline,
    layer_format: wgpu::TextureFormat,
    base: Option<Layer>,
    overlay: Option<Layer>,
    empty_overlay: Layer,
    bind_group: Option<wgpu::BindGroup>,
    bound: (Option<u64>, Option<u64>),
}

fn upload(
    slot: &mut Option<Layer>,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    frame: &PreviewFrame,
) -> bool {
    if check_size(frame.width, frame.height, &frame.rgba).is_err() {
        return false;
    }
    let recreate = slot
        .as_ref()
        .is_none_or(|layer| layer.width != frame.width || layer.height != frame.height);
    if recreate {
        *slot = Some(Layer::new(device, format, frame.width, frame.height));
    }
    if let Some(layer) = slot {
        if layer.generation != Some(frame.generation) {
            layer.write(queue, &frame.rgba);
            layer.generation = Some(frame.generation);
        }
        return recreate;
    }
    false
}

struct PreviewCallback {
    frame: Option<PreviewFrame>,
    overlay: Option<PreviewFrame>,
}

impl CallbackTrait for PreviewCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(resources) = resources.get_mut::<PreviewResources>() else {
            return Vec::new();
        };
        let Some(frame) = &self.frame else {
            return Vec::new();
        };
        let format = resources.layer_format;
        let mut rebind = upload(&mut resources.base, device, queue, format, frame);
        let overlay_generation = match &self.overlay {
            Some(overlay) if overlay.width == frame.width && overlay.height == frame.height => {
                rebind |= upload(&mut resources.overlay, device, queue, format, overlay);
                Some(overlay.generation)
            }
            _ => None,
        };
        let uses_overlay = overlay_generation.is_some();
        let bound_overlay = resources.bound.1.is_some();
        if (rebind || resources.bind_group.is_none() || uses_overlay != bound_overlay)
            && let Some(base) = &resources.base
        {
            {
                let overlay_view = match (&resources.overlay, uses_overlay) {
                    (Some(layer), true) => &layer.view,
                    _ => &resources.empty_overlay.view,
                };
                resources.bind_group =
                    Some(resources.pipeline.bind(device, &base.view, overlay_view));
            }
        }
        resources.bound = (Some(frame.generation), overlay_generation);
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        if self.frame.is_none() {
            return;
        }
        if let Some(resources) = resources.get::<PreviewResources>()
            && let Some(bind_group) = &resources.bind_group
        {
            render_pass.set_pipeline(&resources.pipeline.pipeline);
            render_pass.set_bind_group(0, bind_group, &[]);
            render_pass.draw(0..6, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offscreen_composite_blends_overlay_over_base() {
        let Ok((device, queue)) = headless_device() else {
            eprintln!("skipping: no GPU adapter available");
            return;
        };
        let (width, height) = (64, 32);
        let mut compositor = OffscreenCompositor::new(device, queue, width, height);
        let base: Vec<u8> = (0..width * height)
            .flat_map(|_| [200, 40, 10, 255])
            .collect();
        // Overlay: left half transparent, right half 50% white.
        let overlay: Vec<u8> = (0..width * height)
            .flat_map(|index| {
                if index % width < width / 2 {
                    [0, 0, 0, 0]
                } else {
                    [255, 255, 255, 128]
                }
            })
            .collect();
        let plain = compositor.compose(&base, None).expect("compose");
        assert_eq!(&plain[..4], &[200, 40, 10, 255]);
        let out = compositor.compose(&base, Some(&overlay)).expect("compose");
        let pixel = |x: u32, y: u32| {
            let index = ((y * width + x) * 4) as usize;
            [out[index], out[index + 1], out[index + 2]]
        };
        assert_eq!(pixel(5, 5), [200, 40, 10]);
        let blended = pixel(50, 20);
        for (channel, expected) in blended.iter().zip([228_i32, 148, 133]) {
            assert!((i32::from(*channel) - expected).abs() <= 2, "{blended:?}");
        }
    }
}
