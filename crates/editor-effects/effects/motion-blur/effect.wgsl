fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let a = radians(param(1u));
    let dir = vec2<f32>(cos(a), sin(a)) * texel() * param(0u) / 15.0;
    var sum = vec4<f32>(0.0);
    for (var i = -7; i <= 7; i = i + 1) {
        sum = sum + src(uv + dir * f32(i));
    }
    return sum / 15.0;
}
