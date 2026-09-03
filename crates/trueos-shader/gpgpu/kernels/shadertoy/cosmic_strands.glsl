// Cosmic Strands
// By Noztol
// Validated with tools/shadertoy-cpp-offline.

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = (fragCoord * 2.0 - iResolution.xy) / iResolution.y;
    uv *= 1.5;
    vec3 finalColor = vec3(0.0);
    float t = iTime * 0.2;
    float angle = 0.5;
    mat2 rot = mat2(cos(angle), -sin(angle), sin(angle), cos(angle));

    for (float i = 0.0; i < 7.0; i++) {
        uv *= rot;
        uv.y += sin(uv.x * (1.5 + i * 0.2) + t * (1.2 + i * 0.5)) * 0.35;
        uv.x += cos(uv.y * (1.2 + i * 0.3) - t * (0.8 + i * 0.4)) * 0.35;

        float glow = 0.015 / (abs(uv.y) + 0.01);
        float pulse = sin(t * 6.0 + i * 2.14) * 0.5 + 0.5;
        pulse = smoothstep(0.1, 0.9, pulse);
        glow *= pulse;

        vec3 color = 0.5 + 0.5 * cos(
            iTime * 0.3 + uv.xyx * 2.0 + vec3(0.0, 2.0, 4.0) + i * 0.8
        );
        finalColor += glow * color;
    }

    vec3 bg = vec3(0.02, 0.0, 0.05) * length(uv);
    finalColor += bg;
    finalColor = finalColor / (1.0 + finalColor);
    finalColor = pow(finalColor, vec3(0.7));
    fragColor = vec4(finalColor, 1.0);
}
