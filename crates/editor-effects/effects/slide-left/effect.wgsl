fn transition_main(uv: vec2<f32>) -> vec4<f32> {
    let p = progress();
    if (uv.x < 1.0 - p) {
        return from_color(uv + vec2<f32>(p, 0.0));
    }
    return to_color(uv - vec2<f32>(1.0 - p, 0.0));
}
