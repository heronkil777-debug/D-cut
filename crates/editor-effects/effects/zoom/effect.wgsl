fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let center = vec2<f32>(param(1u), param(2u));
    return src(center + (uv - center) / param(0u));
}
