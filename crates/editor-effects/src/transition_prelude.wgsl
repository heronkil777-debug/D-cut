// D-cut transition prelude (prepended to every transition package).
// A package defines transition_main(uv: vec2<f32>) -> vec4<f32>.
struct TransitionUniforms {
    resolution: vec2<f32>,
    progress_value: f32,
    padding: f32,
    p: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var from_texture: texture_2d<f32>;
@group(0) @binding(1) var to_texture: texture_2d<f32>;
@group(0) @binding(2) var src_sampler: sampler;
@group(0) @binding(3) var<uniform> transition: TransitionUniforms;

fn param(i: u32) -> f32 {
    return transition.p[i / 4u][i % 4u];
}

// 0 at the start of the transition, 1 at the end.
fn progress() -> f32 {
    return transition.progress_value;
}

fn from_color(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(from_texture, src_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
}

fn to_color(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(to_texture, src_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
}

fn texel() -> vec2<f32> {
    return 1.0 / transition.resolution;
}

struct TransitionVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> TransitionVertex {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let uv = corners[index];
    var out: TransitionVertex;
    out.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(input: TransitionVertex) -> @location(0) vec4<f32> {
    return clamp(transition_main(input.uv), vec4<f32>(0.0), vec4<f32>(1.0));
}

// ---- package shader ----
