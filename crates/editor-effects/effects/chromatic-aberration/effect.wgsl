fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let d = (uv - vec2<f32>(0.5)) * texel() * param(0u) * 2.0;
    let r = src(uv + d).r;
    let g = src(uv).g;
    let b = src(uv - d).b;
    return vec4<f32>(r, g, b, 1.0);
}
