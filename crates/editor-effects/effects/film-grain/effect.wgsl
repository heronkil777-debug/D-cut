// Integer hash: identical on every conforming GPU (no sin/fract precision drift).
fn hash_noise(p: vec2<u32>) -> f32 {
    var h = p.x * 374761393u + p.y * 668265263u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h & 0xFFFFFFu) / f32(0xFFFFFFu) - 0.5;
}

fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let p = vec2<u32>(max(vec2<f32>(0.0), round(uv * effect.resolution - vec2<f32>(0.5))));
    return vec4<f32>(c.rgb + vec3<f32>(hash_noise(p) * param(0u)), c.a);
}
