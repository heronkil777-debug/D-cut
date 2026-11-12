fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let l = dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let tone = mix(vec3<f32>(0.10, 0.05, 0.35), vec3<f32>(1.0, 0.75, 0.30), l);
    return vec4<f32>(mix(c.rgb, tone, param(0u)), c.a);
}
