// Deliberately smaller than HelioV's material shader. This authenticated
// package is an opt-in bring-up rung: it proves a fixed mip-0 texel fetch
// without relying on implicit derivatives or filtering. It must never become
// a fallback renderer for the voxel world.
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@group(0) @binding(0)
var base_color_texture: texture_2d<f32>;

@group(0) @binding(1)
var base_color_sampler: sampler;

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(position, 1.0);
    output.uv = uv;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let wrapped_uv = fract(input.uv);
    let texel = vec2<i32>(wrapped_uv * vec2<f32>(16.0, 16.0));
    return textureLoad(base_color_texture, texel, 0);
}
