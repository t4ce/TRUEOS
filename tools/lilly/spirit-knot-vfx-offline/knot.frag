#version 300 es
precision highp float;

// Experimental Spirit BACKGROUND only. No logo bitmap, SVG, mask, or atlas.
// Six identical arc / tangent / diagonal ribbons construct the knot per pixel.
// Host inputs mirror Spirit's time, speed, intensity, scale, opacity, two colors.
uniform vec2 uResolution;
uniform float uTime;
uniform float uSpeed;
uniform float uIntensity;
uniform float uScale;
uniform float uOpacity;
uniform vec3 uColorA;
uniform vec3 uColorB;
uniform int uVariant;
out vec4 fragColor;

const float PI = 3.14159265359;
const float TAU = 6.28318530718;

mat2 rotate(float a) {
    float c = cos(a), s = sin(a);
    return mat2(c, s, -s, c);
}

// x: distance to centerline, y: continuous distance along the ribbon.
vec2 segment(vec2 p, vec2 a, vec2 b, float offset) {
    vec2 ab = b - a;
    float h = clamp(dot(p - a, ab) / dot(ab, ab), 0.0, 1.0);
    return vec2(length(p - a - ab * h), offset + h * length(ab));
}

vec2 ribbon(vec2 p) {
    const vec2 c = vec2(-0.1246, -0.4125);
    const float r = 0.358;
    vec2 q = p - c;
    // Upper semicircle. Clamp an angle around its midpoint so both endcaps
    // remain correct across atan's -PI / PI discontinuity.
    float a = mod(atan(q.y, q.x) - 1.5 * PI + PI, TAU) - PI;
    float arcAngle = 1.5 * PI + clamp(a, -0.5 * PI, 0.5 * PI);
    vec2 onArc = c + r * vec2(cos(arcAngle), sin(arcAngle));
    vec2 result = vec2(length(p - onArc), (2.0 * PI - arcAngle) * r);
    float arcLength = PI * r;
    vec2 start = c - vec2(r, 0.0);
    // The final diagonal meets the +120-degree strand's arc tangentially.
    vec2 end = rotate(2.0 * PI / 3.0) * (c + vec2(r, 0.0));
    vec2 corner = vec2(start.x, end.y - (end.x - start.x) / sqrt(3.0));
    vec2 line = segment(p, start, corner, arcLength);
    if (line.x < result.x) result = line;
    line = segment(p, corner, end, arcLength + length(corner - start));
    if (line.x < result.x) result = line;
    return result;
}

float hash21(vec2 p) {
    p = fract(p * vec2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}

void main() {
    // Top-left origin, like Spirit's production allocation. Equal dimensions
    // are intentional: all comparison cells render at the native 256x256.
    vec2 uv = vec2(gl_FragCoord.x, uResolution.y - gl_FragCoord.y) / uResolution;
    float t = uTime * uSpeed;
    vec2 p = (uv - 0.5) * 2.5 / uScale;
    float breath = 1.0 + 0.024 * sin(t * 1.7);
    p /= breath;
    if (uVariant == 2) p = rotate(0.07 * sin(t * 0.55)) * p;
    float aa = 2.5 / (uResolution.x * uScale * breath);
    const float halfWidth = 0.047;
    float nearest = 10.0, ribbonFill = 0.0, signal = 0.0, seam = 0.0;
    for (int i = 0; i < 6; ++i) {
        vec2 q = rotate(-float(i) * PI / 3.0) * p;
        vec2 d = ribbon(q);
        nearest = min(nearest, d.x);
        float fill = 1.0 - smoothstep(halfWidth - aa, halfWidth + aa, d.x);
        // Cyclic over/under, rather than six strokes merging into a rosette.
        // Joins use the +120-degree strand, so the +60-degree undercut does
        // not break the continuous path at its endpoints.
        float nextDistance = ribbon(rotate(-PI / 3.0) * q).x;
        float gap = (1.0 - smoothstep(halfWidth + 0.012, halfWidth + 0.012 + aa,
                                    nextDistance));
        fill *= 1.0 - gap;
        ribbonFill = max(ribbonFill, fill);
        float phase = fract(d.y * 0.65 - t * 0.28 + float(i) / 6.0);
        float head = exp(-pow((phase - 0.5) * 14.0, 2.0));
        signal = max(signal, head * fill);
        seam = max(seam, (1.0 - smoothstep(0.006, 0.006 + aa,
                                         abs(d.x - 0.036))) * (1.0 - gap));
    }
    float distance = max(0.0, nearest - halfWidth);
    float glow = exp(-distance * 28.0);
    float angle = atan(p.y, p.x);
    float hue = 0.5 + 0.5 * sin(angle * 2.0 - t * 0.4 + length(p) * 3.0);
    vec3 color = mix(uColorA, uColorB, hue);
    float alpha;

    if (uVariant == 0) {
        // 1 / Quiet current: steady legible silhouette, a slow specular breath.
        float sheen = pow(0.5 + 0.5 * sin(p.x * 3.0 + p.y * 4.0 - t), 6.0);
        alpha = ribbonFill * (0.45 + 0.18 * sheen) + glow * 0.10;
        color = mix(color, vec3(0.92, 0.98, 1.0), 0.40 * sheen + 0.35 * seam);
    } else if (uVariant == 1) {
        // 2 / Signal runner: six offset packets circulating through the weave.
        alpha = ribbonFill * 0.20 + signal * 0.65 + glow * (0.05 + signal * 0.12);
        color = mix(color, vec3(0.93, 1.0, 1.0), signal * 0.7);
    } else if (uVariant == 2) {
        // 3 / Resonance: outward distance contours, not an additional logo draw.
        float wave = fract(distance * 5.0 - t * 0.23);
        float echo = exp(-pow((wave - 0.5) * 18.0, 2.0))
                   * exp(-distance * 12.0) * smoothstep(0.01, 0.04, distance);
        alpha = ribbonFill * 0.34 + seam * 0.16 + glow * 0.08 + echo * 0.48;
        color = mix(color, uColorB, 0.35 * echo);
    } else {
        // 4 / Spark lattice: hash-positioned motes reveal / leave the ribbons.
        // One grid lookup; no noise octaves, sprite lookup, or particle state.
        vec2 grid = (p + vec2(0.0, t * 0.012)) * 52.0;
        vec2 cell = floor(grid);
        float seed = hash21(cell);
        vec2 offset = vec2(seed, hash21(cell + 7.3)) * 0.55 + 0.225;
        float dotRadius = 0.14 + 0.13 * seed;
        float dotMask = 1.0 - smoothstep(dotRadius, dotRadius + 0.11,
                                        length(fract(grid) - offset));
        float twinkle = 0.35 + 0.65 * pow(0.5 + 0.5 * sin(t * 2.0 + seed * TAU), 2.0);
        float sparks = dotMask * twinkle * exp(-distance * 20.0);
        alpha = ribbonFill * 0.14 + seam * 0.12 + sparks * 0.85 + glow * 0.055;
        color = mix(color, vec3(0.95, 0.99, 1.0), sparks * 0.65);
    }

    // Transparent allocation edges; premultiplied output for Spirit's blend.
    float edge = min(min(uv.x, uv.y), min(1.0 - uv.x, 1.0 - uv.y));
    alpha = clamp(alpha * uIntensity * 0.5 * uOpacity, 0.0, 0.96)
          * smoothstep(0.015625, 0.06, edge);
    fragColor = vec4(color * alpha, alpha);
}
