// Slim forward path derived from Helio's ForwardLit vertex/lighting contract.
// The Camera and GpuInstanceData declarations intentionally remain byte-for-byte
// layout compatible with libhelio. The output target is cleared transparent.

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

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
}

struct VertexOutput {
    @invariant @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) @interpolate(flat) material_id: u32,
}

@vertex
fn vs_main(input: VertexInput, @builtin(instance_index) slot: u32) -> VertexOutput {
    // This is Helio's second compacted-index indirection. The one indirect draw
    // addresses a dense visible list while transforms stay in GpuInstanceData.
    let instance_id = compacted_indices[slot];
    let inst = instance_data[instance_id];
    let world_position = inst.transform * vec4<f32>(input.position, 1.0);
    let normal_matrix = mat3x3<f32>(
        inst.normal_mat_0.xyz,
        inst.normal_mat_1.xyz,
        inst.normal_mat_2.xyz,
    );

    var output: VertexOutput;
    output.clip_position = cameras[0].view_proj * world_position;
    output.world_normal = normalize(normal_matrix * input.normal);
    output.material_id = inst.material_id;
    return output;
}

fn material_color(material_id: u32) -> vec3<f32> {
    switch material_id & 3u {
        case 0u: { return vec3<f32>(0.25, 0.70, 1.00); }
        case 1u: { return vec3<f32>(1.00, 0.32, 0.62); }
        case 2u: { return vec3<f32>(0.32, 1.00, 0.58); }
        default: { return vec3<f32>(1.00, 0.78, 0.25); }
    }
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(input.world_normal);
    let light_direction = normalize(vec3<f32>(0.35, 0.80, 0.45));
    let diffuse = max(dot(normal, light_direction), 0.0);
    let sky = 0.18 + 0.12 * max(normal.y, 0.0);
    let color = material_color(input.material_id) * (sky + diffuse * 0.82);
    return vec4<f32>(color, 1.0);
}
