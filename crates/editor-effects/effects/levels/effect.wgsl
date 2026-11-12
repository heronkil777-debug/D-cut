fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let range = max(param(1u) - param(0u), 0.001);
    let normalized = clamp((c.rgb - vec3<f32>(param(0u))) / range, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(pow(normalized, vec3<f32>(1.0 / param(2u))), c.a);
}
