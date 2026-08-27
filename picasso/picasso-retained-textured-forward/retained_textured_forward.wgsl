// Picasso retained forward material path.
//
// Geometry and UVs are immutable vertex data. Camera, instance matrices and
// compacted instance IDs are GPU-resident outputs shared with Helio Churn.
// The fragment stage consumes one engine-resolved retained RGBA8 texture.

struct Camera {
    view:           mat4x4<f32>,
    proj:           mat4x4<f32>,
    view_proj:      mat4x4<f32>,
    inv_view_proj:  mat4x4<f32>,
    position_near:  vec4<f32>,
    forward_far:    vec4<f32>,
    jitter_frame:   vec4<f32>,
    prev_view_proj: mat4x4<f32>,
}

struct GpuInstanceData {
    transform:      mat4x4<f32>,
    normal_mat_0:   vec4<f32>,
    normal_mat_1:   vec4<f32>,
    normal_mat_2:   vec4<f32>,
    bounds:         vec4<f32>,
    prev_model:     mat4x4<f32>,
    mesh_id:        u32,
    material_id:    u32,
    flags:          u32,
    lightmap_index: u32,
}

@group(0) @binding(0) var<storage, read> cameras:           array<Camera>;
@group(0) @binding(1) var<storage, read> instance_data:     array<GpuInstanceData>;
@group(0) @binding(2) var<storage, read> compacted_indices: array<u32>;
@group(0) @binding(3) var base_color_texture: texture_2d<f32>;
@group(0) @binding(4) var base_color_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv:       vec2<f32>,
}

struct VertexOutput {
    @invariant @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(input: VertexInput, @builtin(instance_index) slot: u32) -> VertexOutput {
    let instance_id = compacted_indices[slot];
    let inst = instance_data[instance_id];
    let world_position = inst.transform * vec4<f32>(input.position, 1.0);

    var output: VertexOutput;
    output.clip_position = cameras[0].view_proj * world_position;
    output.uv = input.uv;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(base_color_texture, base_color_sampler, input.uv);
}
