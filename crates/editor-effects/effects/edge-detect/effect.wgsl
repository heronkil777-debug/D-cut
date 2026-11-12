fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let t = texel();
    let l = vec3<f32>(0.2126, 0.7152, 0.0722);
    let tl = dot(src(uv + vec2<f32>(-t.x, -t.y)).rgb, l);
    let tc = dot(src(uv + vec2<f32>(0.0, -t.y)).rgb, l);
    let tr = dot(src(uv + vec2<f32>(t.x, -t.y)).rgb, l);
    let ml = dot(src(uv + vec2<f32>(-t.x, 0.0)).rgb, l);
    let mr = dot(src(uv + vec2<f32>(t.x, 0.0)).rgb, l);
    let bl = dot(src(uv + vec2<f32>(-t.x, t.y)).rgb, l);
    let bc = dot(src(uv + vec2<f32>(0.0, t.y)).rgb, l);
    let br = dot(src(uv + vec2<f32>(t.x, t.y)).rgb, l);
    let gx = (tr + 2.0 * mr + br) - (tl + 2.0 * ml + bl);
    let gy = (bl + 2.0 * bc + br) - (tl + 2.0 * tc + tr);
    let e = clamp(length(vec2<f32>(gx, gy)) * param(0u), 0.0, 1.0);
    return vec4<f32>(vec3<f32>(e), 1.0);
}
