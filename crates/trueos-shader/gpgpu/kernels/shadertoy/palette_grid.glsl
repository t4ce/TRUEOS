// Palette Grid Glow
// Validated with tools/shadertoy-cpp-offline.

vec3 palette(float t) {
    vec3 a = vec3(0.5, 0.5, 0.5);
    vec3 b = vec3(0.5, 0.5, 0.5);
    vec3 c = vec3(1.0, 1.0, 1.0);
    vec3 d = vec3(0.2, 0.5, 0.85);
    return a + b * cos(6.28318 * (c * t + d));
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = (fragCoord * 2.0 - iResolution.xy)
        / min(iResolution.x, iResolution.y);
    vec2 uv0 = uv;
    uv = abs(uv);

    float gridScale = 3.0 + sin(iTime * 0.2);
    vec2 gridUV = fract(uv * gridScale) - 0.5;
    float linesX = sin(gridUV.x * 20.0 + iTime * 2.0);
    float linesY = sin(gridUV.y * 20.0 - iTime * 2.0);
    float gridPattern = abs(linesX * linesY);

    float dist = abs(sin(length(gridUV) * 7.0 + iTime * 0.5) / 8.0);
    float glow = pow(0.015 / (dist + 0.001), 1.1);
    vec3 col = palette(length(uv0) + iTime * 0.15 + gridPattern * 0.2);
    vec3 finalColor = col * glow * (gridPattern * 1.5 + 0.2);
    finalColor *= sin(iTime * 0.8) * 0.3 + 0.7;
    fragColor = vec4(pow(finalColor, vec3(0.9)), 1.0);
}
