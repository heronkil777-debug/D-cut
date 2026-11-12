// SPDX-License-Identifier: AGPL-3.0-or-later

//! Runs a chain of catalog effects on the GPU (ping-pong render targets, one pipeline per
//! package). Used by both the preview and the export compositor.

use std::{collections::HashMap, sync::Arc};

use editor_effects::{EffectPackage, MAX_PARAMS};

/// One effect application: the validated package and its uniform values.
#[derive(Clone)]
pub struct EffectStep {
    pub package: Arc<EffectPackage>,
    pub params: [f32; MAX_PARAMS],
}

const UNIFORM_BYTES: u64 = (4 + MAX_PARAMS as u64) * 4;

struct Targets {
    views: [wgpu::TextureView; 2],
    _textures: [wgpu::Texture; 2],
    width: u32,
    height: u32,
}

pub(crate) struct EffectRunner {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    format: wgpu::TextureFormat,
    pipelines: HashMap<String, wgpu::RenderPipeline>,
    uniforms: Vec<wgpu::Buffer>,
    targets: Option<Targets>,
}

impl EffectRunner {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("d-cut-effect-layout"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("d-cut-effect-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("d-cut-effect-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            layout,
            pipeline_layout,
            sampler,
            format,
            pipelines: HashMap::new(),
            uniforms: Vec::new(),
            targets: None,
        }
    }

    fn pipeline(
        &mut self,
        device: &wgpu::Device,
        package: &EffectPackage,
    ) -> &wgpu::RenderPipeline {
        let id = package.manifest.id.clone();
        let (layout, format) = (&self.pipeline_layout, self.format);
        self.pipelines.entry(id).or_insert_with(|| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&package.manifest.id),
                source: wgpu::ShaderSource::Wgsl(package.shader.as_str().into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&package.manifest.id),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
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
            })
        })
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self
            .targets
            .as_ref()
            .is_some_and(|targets| targets.width == width && targets.height == height)
        {
            return;
        }
        let make = || {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("d-cut-effect-target"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let textures = [make(), make()];
        let views = [
            textures[0].create_view(&wgpu::TextureViewDescriptor::default()),
            textures[1].create_view(&wgpu::TextureViewDescriptor::default()),
        ];
        self.targets = Some(Targets {
            views,
            _textures: textures,
            width,
            height,
        });
    }

    /// Records the chain into `encoder`; returns the view holding the result (None if empty).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::TextureView,
        width: u32,
        height: u32,
        steps: &[EffectStep],
    ) -> Option<&wgpu::TextureView> {
        if steps.is_empty() || width == 0 || height == 0 {
            return None;
        }
        self.ensure_targets(device, width, height);
        while self.uniforms.len() < steps.len() {
            self.uniforms
                .push(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("d-cut-effect-uniforms"),
                    size: UNIFORM_BYTES,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
        }
        for (index, step) in steps.iter().enumerate() {
            let mut values = vec![width as f32, height as f32, 0.0, 0.0];
            values.extend_from_slice(&step.params);
            let bytes: Vec<u8> = values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect();
            queue.write_buffer(&self.uniforms[index], 0, &bytes);
            self.pipeline(device, &step.package);
        }
        let targets = self.targets.as_ref()?;
        for (index, step) in steps.iter().enumerate() {
            let source = if index == 0 {
                input
            } else {
                &targets.views[(index - 1) % 2]
            };
            let destination = &targets.views[index % 2];
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("d-cut-effect-bind-group"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.uniforms[index].as_entire_binding(),
                    },
                ],
            });
            let pipeline = self.pipelines.get(&step.package.manifest.id)?;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("d-cut-effect-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: destination,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
        Some(&targets.views[(steps.len() - 1) % 2])
    }
}

/// One transition application between two frames.
#[derive(Clone)]
pub struct TransitionStep {
    pub package: Arc<EffectPackage>,
    pub params: [f32; MAX_PARAMS],
    pub progress: f32,
}

/// Renders a two-input transition package into its own target.
pub(crate) struct TransitionRunner {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    format: wgpu::TextureFormat,
    pipelines: HashMap<String, wgpu::RenderPipeline>,
    uniform: wgpu::Buffer,
    target: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
}

impl TransitionRunner {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let texture = |binding| wgpu::BindGroupLayoutEntry {
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
            label: Some("d-cut-transition-layout"),
            entries: &[
                texture(0),
                texture(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("d-cut-transition-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("d-cut-transition-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("d-cut-transition-uniforms"),
            size: UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            layout,
            pipeline_layout,
            sampler,
            format,
            pipelines: HashMap::new(),
            uniform,
            target: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        from: &wgpu::TextureView,
        to: &wgpu::TextureView,
        width: u32,
        height: u32,
        step: &TransitionStep,
    ) -> Option<&wgpu::TextureView> {
        if self
            .target
            .as_ref()
            .is_none_or(|(_, _, w, h)| *w != width || *h != height)
        {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("d-cut-transition-target"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.target = Some((texture, view, width, height));
        }
        let mut values = vec![
            width as f32,
            height as f32,
            step.progress.clamp(0.0, 1.0),
            0.0,
        ];
        values.extend_from_slice(&step.params);
        let bytes: Vec<u8> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        queue.write_buffer(&self.uniform, 0, &bytes);
        let (layout, format) = (&self.pipeline_layout, self.format);
        let package = &step.package;
        let pipeline = self
            .pipelines
            .entry(package.manifest.id.clone())
            .or_insert_with(|| {
                let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(&package.manifest.id),
                    source: wgpu::ShaderSource::Wgsl(package.shader.as_str().into()),
                });
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(&package.manifest.id),
                    layout: Some(layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vs_main"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some("fs_main"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
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
                })
            });
        let (_, target_view, _, _) = self.target.as_ref()?;
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("d-cut-transition-bind-group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(from),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(to),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.uniform.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("d-cut-transition-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..6, 0..1);
        drop(pass);
        Some(target_view)
    }
}
