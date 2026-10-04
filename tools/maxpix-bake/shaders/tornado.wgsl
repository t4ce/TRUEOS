// Seeded once by CPU: phase, height fraction, radial variation, zero offset.
// Native Intel POINTLIST rasterization supplies the point footprint.
struct Camera {
    view: mat4x4<f32>, proj: mat4x4<f32>, view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>, position_near: vec4<f32>, forward_far: vec4<f32>,
    jitter_frame: vec4<f32>, prev_view_proj: mat4x4<f32>,
}
struct Instance {
    transform: mat4x4<f32>, normal0: vec4<f32>, normal1: vec4<f32>, normal2: vec4<f32>,
    bounds: vec4<f32>, prev_model: mat4x4<f32>, mesh_id: u32, material_id: u32,
    flags: u32, lightmap_index: u32,
}
@group(0) @binding(0) var<storage, read> cameras: array<Camera>;
@group(0) @binding(1) var<storage, read> instances: array<Instance>;
@group(0) @binding(2) var<storage, read> compacted: array<u32>;
struct Input { @location(0) seed: vec3<f32>, @location(1) offset: vec3<f32> }
@vertex
fn vs_main(input: Input, @builtin(instance_index) slot: u32) -> @builtin(position) vec4<f32> {
    let time = cameras[0].jitter_frame.z;
    let height = fract(input.seed.y + time * 0.10);
    let radius = (0.25 + 2.5 * height * height) * input.seed.z;
    let angle = input.seed.x + time * (2.3 - 1.2 * height) + height * 9.0;
    let local = vec3<f32>(cos(angle) * radius, height * 8.0 - 4.0, sin(angle) * radius) + input.offset;
    let world = instances[compacted[slot]].transform * vec4<f32>(local, 1.0);
    return cameras[0].view_proj * world;
}
@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.35, 0.78, 1.0, 1.0);
}
