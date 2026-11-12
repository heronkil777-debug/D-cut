fn transition_main(uv: vec2<f32>) -> vec4<f32> {
    return mix(from_color(uv), to_color(uv), progress());
}
