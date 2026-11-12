// 7x7 Gaussian kernel with taps spaced by sigma/2.
fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let sigma = param(0u);
    let spacing = sigma * 0.5;
    var sum = vec4<f32>(0.0);
    var weight_sum = 0.0;
    for (var y = -3; y <= 3; y = y + 1) {
        for (var x = -3; x <= 3; x = x + 1) {
            let offset = vec2<f32>(f32(x), f32(y)) * spacing;
            let weight = exp(-dot(offset, offset) / (2.0 * sigma * sigma));
            sum = sum + src(uv + offset * texel()) * weight;
            weight_sum = weight_sum + weight;
        }
    }
    return sum / weight_sum;
}
