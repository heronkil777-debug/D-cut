fn transition_main(uv: vec2<f32>) -> vec4<f32> {
    let p = progress();
    if (uv.y < 1.0 - p) {
        return from_color(uv + vec2<f32>(0.0, p));
    }
    return to_color(uv - vec2<f32>(0.0, 1.0 - p));
}
