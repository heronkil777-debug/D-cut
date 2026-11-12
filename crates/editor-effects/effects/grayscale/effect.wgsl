fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    return vec4<f32>(mix(c.rgb, vec3<f32>(dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722))), param(0u)), c.a);
}
