fn transition_main(uv: vec2<f32>) -> vec4<f32> {
    let p = progress();
    let black = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    if (p < 0.5) {
        return mix(from_color(uv), black, p * 2.0);
    }
    return mix(black, to_color(uv), (p - 0.5) * 2.0);
}
