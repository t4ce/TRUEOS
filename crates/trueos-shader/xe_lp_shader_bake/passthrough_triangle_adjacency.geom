#version 450

// First adjacency proof: preserve all six input vertices through GS, but emit
// only the ordinary triangle.  The odd vertices remain available for a later
// silhouette/voxel-neighbour diagnostic.
layout(triangles_adjacency) in;
layout(triangle_strip, max_vertices = 3) out;

void main() {
    gl_Position = gl_in[0].gl_Position;
    EmitVertex();
    gl_Position = gl_in[2].gl_Position;
    EmitVertex();
    gl_Position = gl_in[4].gl_Position;
    EmitVertex();
    EndPrimitive();
}
