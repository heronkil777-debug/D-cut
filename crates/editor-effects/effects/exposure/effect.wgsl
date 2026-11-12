fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    return vec4<f32>(c.rgb * exp2(param(0u)), c.a);
}
