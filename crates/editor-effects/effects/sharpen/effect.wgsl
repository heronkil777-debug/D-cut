fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let t = texel();
    let c = src(uv);
    let n = src(uv + vec2<f32>(0.0, -t.y)) + src(uv + vec2<f32>(0.0, t.y)) + src(uv + vec2<f32>(-t.x, 0.0)) + src(uv + vec2<f32>(t.x, 0.0));
    return vec4<f32>(c.rgb + (c.rgb * 4.0 - n.rgb) * param(0u) * 0.5, c.a);
}
