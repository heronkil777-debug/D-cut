fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let line = step(0.5, fract(uv.y * effect.resolution.y / param(1u)));
    return vec4<f32>(c.rgb * (1.0 - param(0u) * line), c.a);
}
