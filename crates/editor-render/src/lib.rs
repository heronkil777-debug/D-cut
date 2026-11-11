// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared-device GPU rendering primitives used by the preview and export paths.

use egui::{Sense, Ui, Vec2};
use egui_wgpu::{Callback, CallbackResources, CallbackTrait, RenderState, ScreenDescriptor, wgpu};

/// A simple GPU-rendered quad used to prove the UI and renderer share one wgpu device.
pub struct QuadRenderer;

impl QuadRenderer {
    /// Creates the pipeline on the wgpu device owned by eframe and stores it in its callback resources.
    pub fn new(render_state: &RenderState) -> Self {
        let device = &render_state.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("d-cut-quad-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("d-cut-quad-pipeline-layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("d-cut-quad-pipeline"),
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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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

        let _ = render_state
            .renderer
            .write()
            .callback_resources
            .insert(QuadResources { pipeline });

        Self
    }

    /// Adds a native wgpu paint callback to the egui painter.
    pub fn paint(&self, ui: &mut Ui, size: Vec2) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter()
            .add(Callback::new_paint_callback(rect, QuadCallback));
    }
}

struct QuadResources {
    pipeline: wgpu::RenderPipeline,
}

struct QuadCallback;

impl CallbackTrait for QuadCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen_descriptor: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        _resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        if let Some(resources) = resources.get::<QuadResources>() {
            render_pass.set_pipeline(&resources.pipeline);
            render_pass.draw(0..6, 0..1);
        }
    }
}
