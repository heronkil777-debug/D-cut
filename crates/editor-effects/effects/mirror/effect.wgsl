fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    var q = uv;
    if (param(0u) < 0.5) {
        q.x = 0.5 - abs(uv.x - 0.5);
    } else {
        q.y = 0.5 - abs(uv.y - 0.5);
    }
    return src(q);
}
