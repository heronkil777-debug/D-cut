fn transition_main(uv: vec2<f32>) -> vec4<f32> {
    let p = progress();
    let size = mix(1.0, param(0u), 1.0 - abs(2.0 * p - 1.0));
    let block = texel() * size;
    let center = (floor(uv / block) + vec2<f32>(0.5)) * block;
    return mix(from_color(center), to_color(center), step(0.5, p));
}
