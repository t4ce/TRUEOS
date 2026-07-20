struct SceneUniform {
    mvp: mat4x4<f32>,
    model: mat4x4<f32>,
    tuning: vec4<f32>,
    tint: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> scene: SceneUniform;

@group(0) @binding(1)
var video_texture: texture_2d<f32>;

@group(0) @binding(2)
var video_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = scene.mvp * vec4<f32>(input.position, 1.0);
    output.normal = normalize((scene.model * vec4<f32>(input.normal, 0.0)).xyz);
    output.uv = input.uv;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(video_texture, video_sampler, input.uv).rgb;
    let luminance = dot(sampled, vec3<f32>(0.2126, 0.7152, 0.0722));
    let saturated = mix(vec3<f32>(luminance), sampled, scene.tuning.z);

    let light_direction = normalize(vec3<f32>(0.45, 0.7, 0.6));
    let diffuse = 0.28 + 0.72 * max(dot(normalize(input.normal), light_direction), 0.0);
    let light = mix(1.0, diffuse, scene.tuning.y);
    let color = saturated * scene.tint.rgb * scene.tuning.x * light;

    return vec4<f32>(color, 1.0);
}
