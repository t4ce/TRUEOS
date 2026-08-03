// Minimal adapter/runtime smoke shader.
const vec3 BASE_COLOR = vec3(0.15, 0.55, 1.0);

void mainImage(out vec4 fragColor, in vec2 fragCoord)
{
    vec2 uv = (2.0 * fragCoord - iResolution.xy) / iResolution.y;
    float angle = 0.2 * iTime;
    uv *= mat2(cos(angle), -sin(angle), sin(angle), cos(angle));
    float glow = 0.03 / abs(length(uv) - 0.35 - 0.04 * sin(iTime * 2.0));
    vec3 color = BASE_COLOR * glow;
    fragColor = vec4(color, 1.0);
}
