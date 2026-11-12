fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let t = param(0u);
    let tint = param(1u);
    let gain = vec3<f32>(1.0 + 0.25 * t, 1.0 + 0.2 * tint, 1.0 - 0.25 * t);
    return vec4<f32>(c.rgb * gain, c.a);
}
