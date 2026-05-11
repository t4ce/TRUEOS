#version 450

layout(location = 0) out vec4 out_color;

layout(push_constant) uniform MandelbrotParams {
    layout(offset = 0) vec2 resolution;
    layout(offset = 8) vec2 center;
    layout(offset = 16) float scale;
    layout(offset = 20) uint max_iterations;
} pc;

vec3 palette(float t) {
    vec3 a = vec3(0.050, 0.060, 0.110);
    vec3 b = vec3(0.120, 0.420, 0.780);
    vec3 c = vec3(0.940, 0.420, 0.130);
    vec3 d = vec3(1.000, 0.920, 0.620);

    float band = smoothstep(0.0, 1.0, t);
    vec3 cold = mix(a, b, smoothstep(0.0, 0.52, band));
    vec3 warm = mix(c, d, smoothstep(0.45, 1.0, band));
    return mix(cold, warm, smoothstep(0.35, 0.9, band));
}

void main() {
    vec2 resolution = pc.resolution;
    if (resolution.x <= 0.0 || resolution.y <= 0.0) {
        resolution = vec2(2560.0, 1440.0);
    }

    float scale = pc.scale > 0.0 ? pc.scale : 2.6;
    uint max_iterations = pc.max_iterations == 0u ? 192u : pc.max_iterations;
    vec2 center = pc.center == vec2(0.0) ? vec2(-0.5, 0.0) : pc.center;

    vec2 p = (gl_FragCoord.xy - 0.5 * resolution) / resolution.y;
    vec2 c = center + p * scale;
    vec2 z = vec2(0.0);

    uint iter = 0u;
    for (uint i = 0u; i < max_iterations; i++) {
        float xx = z.x * z.x;
        float yy = z.y * z.y;
        if (xx + yy > 4.0) {
            break;
        }

        z = vec2(xx - yy, 2.0 * z.x * z.y) + c;
        iter = i + 1u;
    }

    if (iter >= max_iterations) {
        out_color = vec4(0.0, 0.0, 0.0, 1.0);
        return;
    }

    float mag2 = max(dot(z, z), 1.0001);
    float smooth_iter = float(iter) + 1.0 - log2(log2(mag2));
    float t = clamp(smooth_iter / float(max_iterations), 0.0, 1.0);
    out_color = vec4(palette(t), 1.0);
}
