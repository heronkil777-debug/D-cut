struct VertexOutput {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let positions = array<vec2<f32>, 6>(
        vec2<f32>(-0.8, -0.7),
        vec2<f32>(0.8, -0.7),
        vec2<f32>(0.8, 0.7),
        vec2<f32>(-0.8, -0.7),
        vec2<f32>(0.8, 0.7),
        vec2<f32>(-0.8, 0.7),
    );
    var output: VertexOutput;
    output.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    return output;
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.48, 0.30, 0.92, 1.0);
}
