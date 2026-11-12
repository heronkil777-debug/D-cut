fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let a = radians(param(0u));
    let k = vec3<f32>(0.57735);
    let rgb = c.rgb * cos(a) + cross(k, c.rgb) * sin(a) + k * dot(k, c.rgb) * (1.0 - cos(a));
    return vec4<f32>(rgb, c.a);
}
