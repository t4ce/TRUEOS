struct SceneUniform {
    mvp: mat4x4<f32>,
    model: mat4x4<f32>,
    tuning: vec4<f32>,
    tint: vec4<f32>,
    warp: vec4<f32>,
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

fn perlin_gradient(cell: vec3<f32>) -> vec3<f32> {
    let hashes = vec3<f32>(
        dot(cell, vec3<f32>(127.1, 311.7, 74.7)),
        dot(cell, vec3<f32>(269.5, 183.3, 246.1)),
        dot(cell, vec3<f32>(113.5, 271.9, 124.6)),
    );
    let gradient = fract(sin(hashes) * 43758.5453) * 2.0 - 1.0;
    return normalize(gradient + vec3<f32>(0.0001));
}

fn perlin_noise(position: vec3<f32>) -> f32 {
    let cell = floor(position);
    let local = fract(position);
    let fade = local * local * local * (local * (local * 6.0 - 15.0) + 10.0);

    let n000 = dot(perlin_gradient(cell), local);
    let n100 = dot(
        perlin_gradient(cell + vec3<f32>(1.0, 0.0, 0.0)),
        local - vec3<f32>(1.0, 0.0, 0.0),
    );
    let n010 = dot(
        perlin_gradient(cell + vec3<f32>(0.0, 1.0, 0.0)),
        local - vec3<f32>(0.0, 1.0, 0.0),
    );
    let n110 = dot(
        perlin_gradient(cell + vec3<f32>(1.0, 1.0, 0.0)),
        local - vec3<f32>(1.0, 1.0, 0.0),
    );
    let n001 = dot(
        perlin_gradient(cell + vec3<f32>(0.0, 0.0, 1.0)),
        local - vec3<f32>(0.0, 0.0, 1.0),
    );
    let n101 = dot(
        perlin_gradient(cell + vec3<f32>(1.0, 0.0, 1.0)),
        local - vec3<f32>(1.0, 0.0, 1.0),
    );
    let n011 = dot(
        perlin_gradient(cell + vec3<f32>(0.0, 1.0, 1.0)),
        local - vec3<f32>(0.0, 1.0, 1.0),
    );
    let n111 = dot(
        perlin_gradient(cell + vec3<f32>(1.0, 1.0, 1.0)),
        local - vec3<f32>(1.0, 1.0, 1.0),
    );

    let x00 = mix(n000, n100, fade.x);
    let x10 = mix(n010, n110, fade.x);
    let x01 = mix(n001, n101, fade.x);
    let x11 = mix(n011, n111, fade.x);
    let y0 = mix(x00, x10, fade.y);
    let y1 = mix(x01, x11, fade.y);
    return mix(y0, y1, fade.z) * 1.5;
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    var warped_position = input.position;
    if scene.warp.x > 0.0 {
        let motion = scene.warp.y * vec3<f32>(0.31, 0.23, -0.27);
        let displacement = perlin_noise(input.position * 2.4 + motion) * scene.warp.x;
        warped_position += input.normal * displacement;
    }
    output.clip_position = scene.mvp * vec4<f32>(warped_position, 1.0);
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
