fn transition_main(uv: vec2<f32>) -> vec4<f32> {
    let p = progress();
    let radius = param(0u) * (1.0 - abs(2.0 * p - 1.0));
    let step_uv = texel() * radius / 2.0;
    var a = vec4<f32>(0.0);
    var b = vec4<f32>(0.0);
    for (var y = -2; y <= 2; y = y + 1) {
        for (var x = -2; x <= 2; x = x + 1) {
            let o = vec2<f32>(f32(x), f32(y)) * step_uv;
            a = a + from_color(uv + o);
            b = b + to_color(uv + o);
        }
    }
    return mix(a / 25.0, b / 25.0, p);
}
