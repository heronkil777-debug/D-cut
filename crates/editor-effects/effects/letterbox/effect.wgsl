fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let aspect = effect.resolution.x / effect.resolution.y;
    let visible = min(1.0, aspect / param(0u));
    let bar = (1.0 - visible) * 0.5;
    if (uv.y < bar || uv.y > 1.0 - bar) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    return src(uv);
}
