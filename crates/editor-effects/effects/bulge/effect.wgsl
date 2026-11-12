fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let aspect = effect.resolution.x / effect.resolution.y;
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(aspect, 1.0);
    let d = length(p);
    let r = param(1u);
    var q = p;
    if (d < r) {
        let t = d / r;
        q = p * mix(1.0, t, param(0u) * (1.0 - t));
    }
    return src(q / vec2<f32>(aspect, 1.0) + vec2<f32>(0.5));
}
