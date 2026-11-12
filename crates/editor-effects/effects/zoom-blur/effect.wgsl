fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let center = vec2<f32>(0.5);
    var sum = vec4<f32>(0.0);
    for (var i = 0; i < 16; i = i + 1) {
        let scale = 1.0 - param(0u) * f32(i) / 15.0;
        sum = sum + src(center + (uv - center) * scale);
    }
    return sum / 16.0;
}
