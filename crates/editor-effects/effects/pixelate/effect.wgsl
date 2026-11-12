fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let block = texel() * param(0u);
    let center = (floor(uv / block) + vec2<f32>(0.5)) * block;
    return src(center);
}
