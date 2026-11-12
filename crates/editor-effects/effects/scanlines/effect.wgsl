fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    // Integer row/modulo: the pattern cannot shift with driver rounding.
    let spacing = max(1u, u32(round(param(1u))));
    let row = u32(max(0.0, round(uv.y * effect.resolution.y - 0.5)));
    let line = select(0.0, 1.0, row % spacing == spacing - 1u);
    return vec4<f32>(c.rgb * (1.0 - param(0u) * line), c.a);
}
