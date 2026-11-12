fn transition_main(uv: vec2<f32>) -> vec4<f32> {
    let p = progress();
    let white = vec4<f32>(1.0);
    if (p < 0.5) {
        return mix(from_color(uv), white, p * 2.0);
    }
    return mix(white, to_color(uv), (p - 0.5) * 2.0);
}
