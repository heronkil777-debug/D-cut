// SPDX-License-Identifier: AGPL-3.0-or-later

//! GPU presentation of preview frames on the wgpu device owned by the UI (eframe).
//! Frames are composited by the shared timeline render graph (see `editor-media::timeline`)
//! and uploaded here into a texture that is drawn inside the egui layout.

use std::sync::Arc;

use egui::{Painter, Rect};
use egui_wgpu::{Callback, CallbackResources, CallbackTrait, RenderState, ScreenDescriptor, wgpu};

/// A decoded RGBA8 preview frame. `generation` changes whenever the pixels change, so the
/// texture is only re-uploaded when needed.
#[derive(Clone)]
pub struct PreviewFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
    pub generation: u64,
}

/// Draws preview frames with a textured-quad pipeline created on eframe's wgpu device.
pub struct FrameRenderer;

impl FrameRenderer {
    /// Creates the pipeline on the shared device and stores it in egui-wgpu's callback resources.
    pub fn new(render_state: &RenderState) -> Self {
        let device = &render_state.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("d-cut-frame-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("frame.wgsl").into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("d-cut-frame-bind-group-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("d-cut-frame-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("d-cut-frame-pipeline"),
            layout: Some(&layout),
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
                    format: render_state.target_format,
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
            label: Some("d-cut-frame-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        // Frames are sRGB-encoded bytes. On an sRGB target, sample through an sRGB view so the
        // hardware decodes and re-encodes; on a linear (egui default) target, pass bytes through.
        let texture_format = if render_state.target_format.is_srgb() {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        let _ = render_state
            .renderer
            .write()
            .callback_resources
            .insert(FrameResources {
                pipeline,
                bind_group_layout,
                sampler,
                texture_format,
                texture: None,
            });
        Self
    }

    /// Paints `frame` stretched into `rect` (the caller keeps the aspect ratio).
    pub fn paint(&self, painter: &Painter, rect: Rect, frame: Option<PreviewFrame>) {
        painter.add(Callback::new_paint_callback(rect, FrameCallback { frame }));
    }
}

struct UploadedTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
    generation: u64,
}

struct FrameResources {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    texture_format: wgpu::TextureFormat,
    texture: Option<UploadedTexture>,
}

impl FrameResources {
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &PreviewFrame) {
        let expected = frame.width as usize * frame.height as usize * 4;
        if frame.width == 0 || frame.height == 0 || frame.rgba.len() != expected {
            return;
        }
        if self
            .texture
            .as_ref()
            .is_some_and(|uploaded| uploaded.generation == frame.generation)
        {
            return;
        }
        let size = wgpu::Extent3d {
            width: frame.width,
            height: frame.height,
            depth_or_array_layers: 1,
        };
        let reuse = self.texture.as_ref().is_some_and(|uploaded| {
            uploaded.width == frame.width && uploaded.height == frame.height
        });
        if !reuse {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("d-cut-preview-frame"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.texture_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("d-cut-preview-frame-bind-group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.texture = Some(UploadedTexture {
                texture,
                bind_group,
                width: frame.width,
                height: frame.height,
                generation: frame.generation,
            });
        }
        if let Some(uploaded) = &mut self.texture {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &uploaded.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &frame.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * frame.width),
                    rows_per_image: Some(frame.height),
                },
                size,
            );
            uploaded.generation = frame.generation;
        }
    }
}

struct FrameCallback {
    frame: Option<PreviewFrame>,
}

impl CallbackTrait for FrameCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let (Some(frame), Some(resources)) = (&self.frame, resources.get_mut::<FrameResources>())
        {
            resources.upload(device, queue, frame);
        }
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
        if let Some(resources) = resources.get::<FrameResources>()
            && let Some(uploaded) = &resources.texture
        {
            render_pass.set_pipeline(&resources.pipeline);
            render_pass.set_bind_group(0, &uploaded.bind_group, &[]);
            render_pass.draw(0..6, 0..1);
        }
    }
}
