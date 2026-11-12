fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let n = floor(param(0u));
    return vec4<f32>(floor(c.rgb * n) / (n - 1.0), c.a);
}
