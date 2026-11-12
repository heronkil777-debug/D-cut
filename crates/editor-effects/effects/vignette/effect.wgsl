fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let distance = length(uv - vec2<f32>(0.5)) * 1.41421356;
    let edge = 1.0 - param(1u);
    let shade = 1.0 - param(0u) * smoothstep(edge, 1.0, distance);
    return vec4<f32>(c.rgb * shade, c.a);
}
