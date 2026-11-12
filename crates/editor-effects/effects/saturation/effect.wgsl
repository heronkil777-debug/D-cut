fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let luma = dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    return vec4<f32>(mix(vec3<f32>(luma), c.rgb, param(0u)), c.a);
}
