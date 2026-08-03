// Validated with tools/shadertoy-cpp-offline.

#define mul(a, b) vec2(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x)

void mainImage(out vec4 fragColor, in vec2 fragCoord)
{
    float z = 1.0 - (float(iTime) / 10.0);
    float t = -0.05 * float(iTime);
    vec2 coord = vec2(
        fragCoord.x / iResolution.x * (3.0 * z) - (2.0 * z) + t,
        fragCoord.y / iResolution.y * (2.0 * z) - (1.0 * z) + t
    );
    vec2 cm = vec2(0, 0);
    int j = 0;
    for (int i = 0; i < 25; i++) {
        j++;
        cm = mul(cm, cm);
        cm = cm + coord;
        if (dot(cm, cm) > 4.0)
            break;
    }
    fragColor = vec4(j) / 24.0;
}
