fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let aspect = effect.resolution.x / effect.resolution.y;
    var p = (uv - vec2<f32>(0.5)) * vec2<f32>(aspect, 1.0);
    let distance = length(p);
    let radius = param(1u);
    if (distance < radius) {
        let falloff = (radius - distance) / radius;
        let angle = param(0u) * falloff * falloff;
        let s = sin(angle);
        let c = cos(angle);
        p = vec2<f32>(p.x * c - p.y * s, p.x * s + p.y * c);
    }
    return src(p / vec2<f32>(aspect, 1.0) + vec2<f32>(0.5));
}
