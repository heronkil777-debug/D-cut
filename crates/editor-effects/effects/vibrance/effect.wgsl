fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let maxc = max(c.r, max(c.g, c.b));
    let minc = min(c.r, min(c.g, c.b));
    let boost = param(0u) * (1.0 - (maxc - minc));
    let l = dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    return vec4<f32>(mix(vec3<f32>(l), c.rgb, 1.0 + boost), c.a);
}
