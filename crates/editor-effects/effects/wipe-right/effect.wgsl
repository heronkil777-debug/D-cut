fn transition_main(uv: vec2<f32>) -> vec4<f32> {
    let s = param(0u);
    let edge = progress() * (1.0 + 2.0 * s) - s;
    let t = smoothstep(edge - s, edge + s, uv.x);
    return mix(to_color(uv), from_color(uv), t);
}
