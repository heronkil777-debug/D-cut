fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let s = vec3<f32>(dot(c.rgb, vec3<f32>(0.393, 0.769, 0.189)), dot(c.rgb, vec3<f32>(0.349, 0.686, 0.168)), dot(c.rgb, vec3<f32>(0.272, 0.534, 0.131)));
    return vec4<f32>(mix(c.rgb, s, param(0u)), c.a);
}
