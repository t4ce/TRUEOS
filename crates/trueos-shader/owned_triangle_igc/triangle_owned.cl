typedef float4 TrueosVec4;

__kernel void trueos_triangle_vertices(__global TrueosVec4 *out_vertices) {
    const uint lane = get_global_id(0);
    if (lane == 0) {
        out_vertices[0] = (TrueosVec4)(-0.55f, -0.45f, 0.0f, 1.0f);
    } else if (lane == 1) {
        out_vertices[1] = (TrueosVec4)(0.55f, -0.45f, 0.0f, 1.0f);
    } else if (lane == 2) {
        out_vertices[2] = (TrueosVec4)(0.0f, 0.55f, 0.0f, 1.0f);
    }
}

__kernel void trueos_triangle_pixel(__global TrueosVec4 *out_color) {
    out_color[0] = (TrueosVec4)(1.0f, 0.2f, 0.1f, 1.0f);
}
