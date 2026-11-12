fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let rgb = (c.rgb - vec3<f32>(0.5)) * param(1u) + vec3<f32>(0.5) + vec3<f32>(param(0u));
    return vec4<f32>(rgb, c.a);
}
