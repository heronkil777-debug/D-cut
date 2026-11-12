fn effect_main(uv: vec2<f32>) -> vec4<f32> {
    let c = src(uv);
    let p = floor(uv * effect.resolution);
    let n = fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453) - 0.5;
    return vec4<f32>(c.rgb + vec3<f32>(n * param(0u)), c.a);
}
