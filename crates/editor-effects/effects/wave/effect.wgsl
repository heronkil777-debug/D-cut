fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let shift = param(0u) * sin(uv.y * param(1u) * 6.2831853);
    return src(vec2<f32>(uv.x + shift, uv.y));
}
