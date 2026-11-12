// 9x9 box filter whose taps are spread over the radius (81 samples, constant cost).
fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let step = texel() * (param(0u) / 4.0);
    var sum = vec4<f32>(0.0);
    for (var y = -4; y <= 4; y = y + 1) {
        for (var x = -4; x <= 4; x = x + 1) {
            sum = sum + src(uv + vec2<f32>(f32(x), f32(y)) * step);
        }
    }
    return sum / 81.0;
}
