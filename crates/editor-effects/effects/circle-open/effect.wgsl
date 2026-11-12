fn transition_main(uv: vec2<f32>) -> vec4<f32> {
    let aspect = transition.resolution.x / transition.resolution.y;
    let d = length((uv - vec2<f32>(0.5)) * vec2<f32>(aspect, 1.0)) / length(vec2<f32>(aspect, 1.0) * 0.5);
    let s = param(0u);
    let r = progress() * (1.0 + 2.0 * s) - s;
    let inside = 1.0 - smoothstep(r - s, r + s, d);
    return mix(from_color(uv), to_color(uv), inside);
}
