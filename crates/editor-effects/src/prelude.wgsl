// D-cut effect prelude (prepended to every package's effect.wgsl).
// A package defines effect_main(uv: vec2<f32>) -> vec4<f32>.
struct EffectUniforms {
    resolution: vec2<f32>,
    time: f32,
    padding: f32,
    p: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> effect: EffectUniforms;

// Parameter `i` in manifest order.
fn param(i: u32) -> f32 {
    return effect.p[i / 4u][i % 4u];
}

// Source color at `uv` (clamped to the edge).
fn src(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(src_texture, src_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
}

// Size of one source pixel in uv units.
fn texel() -> vec2<f32> {
    return 1.0 / effect.resolution;
}

struct EffectVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> EffectVertex {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let uv = corners[index];
    var out: EffectVertex;
    out.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(input: EffectVertex) -> @location(0) vec4<f32> {
    return clamp(effect_main(input.uv), vec4<f32>(0.0), vec4<f32>(1.0));
}

// ---- package shader ----
