// Retained glTF metallic/roughness material, with MikkTSpace tangent vertices.
// Color textures use sRGB surface formats; all lighting arithmetic is linear.
// Material64B: baseFactor; emissive.xyz/normalScale; metallic/roughness/AO/cutoff;
// flags.x = doubleSided(bit2), flags.y = base1/MR2/emissive4/AO8/normal16.
// flags.z = output: 0 PBR, 1 base color, 2 world normal, 3 UV, 4 magenta.
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


struct Material {
    base_color_factor: vec4<f32>,
    emissive_normal_scale: vec4<f32>,
    metallic_roughness_occlusion_cutoff: vec4<f32>,
    flags: vec4<u32>,
}
@group(0) @binding(0) var<storage, read> cameras: array<Camera>;
@group(0) @binding(1) var<storage, read> instances: array<GpuInstanceData>;
@group(0) @binding(2) var<storage, read> compacted_indices: array<u32>;
@group(0) @binding(3) var base_color_texture: texture_2d<f32>;
@group(0) @binding(4) var material_sampler: sampler;
@group(0) @binding(5) var metallic_roughness_texture: texture_2d<f32>;
@group(0) @binding(6) var normal_texture: texture_2d<f32>;
@group(0) @binding(7) var occlusion_texture: texture_2d<f32>;
@group(0) @binding(8) var emissive_texture: texture_2d<f32>;
@group(0) @binding(9) var<storage, read> material: Material;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
}
struct VertexOutput {
    @invariant @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) world_tangent: vec4<f32>,
}
fn safe_normalize(v: vec3<f32>) -> vec3<f32> {
    return v * inverseSqrt(max(dot(v, v), 1e-12));
}
@vertex
fn vs_main(input: VertexInput, @builtin(instance_index) slot: u32) -> VertexOutput {
    let inst = instances[compacted_indices[slot]];
    let world = inst.transform * vec4<f32>(input.position, 1.0);
    let normal_matrix = mat3x3<f32>(inst.normal_mat_0.xyz, inst.normal_mat_1.xyz, inst.normal_mat_2.xyz);
    let model_linear = mat3x3<f32>(inst.transform[0].xyz, inst.transform[1].xyz, inst.transform[2].xyz);
    let handedness = select(-1.0, 1.0, determinant(model_linear) >= 0.0);
    var output: VertexOutput;
    output.clip_position = cameras[0].view_proj * world;
    output.world_position = world.xyz;
    output.world_normal = normal_matrix * input.normal;
    output.uv = input.uv;
    output.world_tangent = vec4<f32>(model_linear * input.tangent.xyz, input.tangent.w * handedness);
    return output;
}
fn pow5(x: f32) -> f32 {
    let x2 = x * x;
    return x2 * x2 * x;
}
fn fresnel(f0: vec3<f32>, cosine: f32) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow5(1.0 - clamp(cosine, 0.0, 1.0));
}
// GGX distribution, height-correlated Smith visibility, and Schlick Fresnel.
fn direct_light(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, radiance: vec3<f32>,
                albedo: vec3<f32>, metallic: f32, roughness: f32) -> vec3<f32> {
    let h = safe_normalize(v + l);
    let nv = max(dot(n, v), 0.0001);
    let nl = max(dot(n, l), 0.0);
    let nh = max(dot(n, h), 0.0);
    let vh = max(dot(v, h), 0.0);
    let alpha = roughness * roughness;
    let alpha2 = alpha * alpha;
    let d = nh * nh * (alpha2 - 1.0) + 1.0;
    let distribution = alpha2 / max(3.14159265359 * d * d, 1e-7);
    let ggx_v = nl * sqrt(nv * nv * (1.0 - alpha2) + alpha2);
    let ggx_l = nv * sqrt(nl * nl * (1.0 - alpha2) + alpha2);
    let visibility = 0.5 / max(ggx_v + ggx_l, 1e-6);
    let f = fresnel(mix(vec3<f32>(0.04), albedo, metallic), vh);
    let diffuse = (vec3<f32>(1.0) - f) * (1.0 - metallic) * albedo / 3.14159265359;
    return (diffuse + f * distribution * visibility) * radiance * nl;
}
// An analytic studio sky supplies indirect reflection without an environment asset.
// Roughness broadens the softbox highlight; AO affects only this indirect term.
fn environment(r: vec3<f32>, roughness: f32) -> vec3<f32> {
    let sky = mix(vec3<f32>(0.025, 0.03, 0.045), vec3<f32>(0.30, 0.34, 0.42), clamp(r.y * 0.5 + 0.5, 0.0, 1.0));
    let box = pow(max(dot(r, normalize(vec3<f32>(-0.35, 0.75, 0.56))), 0.0), mix(96.0, 3.0, roughness));
    return sky + vec3<f32>(1.6, 1.5, 1.35) * box;
}
fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, 12.92 * x, x <= vec3<f32>(0.0031308));
}
fn tone_map(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}
@fragment
fn fs_main(input: VertexOutput, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    // Keep the full pipeline ABI, but isolate coverage from every sampler
    // transaction and PBR calculation when the draw selects solid output.
    if material.flags.z == 4u {
        return vec4<f32>(1.0, 0.0, 1.0, 1.0);
    }
    // Bind every slot to a valid surface, even when its presence bit is absent.
    let sampled_base = textureSample(base_color_texture, material_sampler, input.uv);
    let sampled_mr = textureSample(metallic_roughness_texture, material_sampler, input.uv);
    let sampled_normal = textureSample(normal_texture, material_sampler, input.uv);
    let sampled_ao = textureSample(occlusion_texture, material_sampler, input.uv);
    let sampled_emissive = textureSample(emissive_texture, material_sampler, input.uv);
    let maps = material.flags.y;
    let base = select(vec4<f32>(1.0), sampled_base, (maps & 1u) != 0u) * material.base_color_factor;
    let mr = select(vec2<f32>(1.0), sampled_mr.gb, (maps & 2u) != 0u);
    let metallic = clamp(mr.y * material.metallic_roughness_occlusion_cutoff.x, 0.0, 1.0);
    let roughness = clamp(mr.x * material.metallic_roughness_occlusion_cutoff.y, 0.045, 1.0);
    let ao_sample = select(1.0, sampled_ao.r, (maps & 8u) != 0u);
    let ao = mix(1.0, ao_sample, material.metallic_roughness_occlusion_cutoff.z);
    let emission = select(vec3<f32>(1.0), sampled_emissive.rgb, (maps & 4u) != 0u) * material.emissive_normal_scale.xyz;
    let normal_texel = select(vec3<f32>(0.5, 0.5, 1.0), sampled_normal.xyz, (maps & 16u) != 0u);
    let normal_ts = safe_normalize((normal_texel * 2.0 - 1.0) * vec3<f32>(material.emissive_normal_scale.w, material.emissive_normal_scale.w, 1.0));
    let geometric_n = safe_normalize(input.world_normal);
    let tangent = safe_normalize(input.world_tangent.xyz - geometric_n * dot(geometric_n, input.world_tangent.xyz));
    let bitangent = cross(geometric_n, tangent) * select(-1.0, 1.0, input.world_tangent.w >= 0.0);
    var n = safe_normalize(tangent * normal_ts.x + bitangent * normal_ts.y + geometric_n * normal_ts.z);
    if (material.flags.x & 4u) != 0u && !front_facing {
        n = -n;
    }
    let v = safe_normalize(cameras[0].position_near.xyz - input.world_position);
    let f0 = mix(vec3<f32>(0.04), base.rgb, metallic);
    let nv = max(dot(n, v), 0.0);
    let f = f0 + (max(vec3<f32>(1.0 - roughness), f0) - f0) * pow5(1.0 - nv);
    let diffuse_ambient = base.rgb * (1.0 - metallic) * (vec3<f32>(1.0) - f) * vec3<f32>(0.20, 0.22, 0.26);
    let specular_ambient = environment(reflect(-v, n), roughness) * f;
    var color = (diffuse_ambient + specular_ambient) * ao;
    color += direct_light(n, v, normalize(vec3<f32>(0.55, 0.75, 0.65)), vec3<f32>(3.4, 3.15, 2.85), base.rgb, metallic, roughness);
    color += direct_light(n, v, normalize(vec3<f32>(-0.75, 0.3, 0.55)), vec3<f32>(0.8, 1.05, 1.45), base.rgb, metallic, roughness);
    color += direct_light(n, v, normalize(vec3<f32>(0.15, 0.55, -0.85)), vec3<f32>(1.2, 1.55, 2.0), base.rgb, metallic, roughness);
    color += emission;
    // A uniform final selector preserves the full shader and its four-varying
    // ABI, so diagnostics do not change the VF/VUE/SBE payload being tested.
    var display = linear_to_srgb(tone_map(color));
    switch material.flags.z {
        case 1u: { display = linear_to_srgb(base.rgb); }
        case 2u: { display = geometric_n * 0.5 + 0.5; }
        case 3u: { display = vec3<f32>(fract(input.uv), 0.0); }
        default: {}
    }
    // Every output is opaque; diagnostic color cannot hide a missing write.
    return vec4<f32>(display, 1.0);
}
