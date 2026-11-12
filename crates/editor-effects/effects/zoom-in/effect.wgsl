fn transition_main(uv: vec2<f32>) -> vec4<f32> {
    let p = progress();
    let zoomed = (uv - vec2<f32>(0.5)) / (1.0 + param(0u) * p) + vec2<f32>(0.5);
    let incoming = (uv - vec2<f32>(0.5)) * (1.0 + param(0u) * (1.0 - p)) + vec2<f32>(0.5);
    return mix(from_color(zoomed), to_color(incoming), smoothstep(0.0, 1.0, p));
}
