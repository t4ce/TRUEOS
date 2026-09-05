#version 450

// TRUEOS clip-space ABI: five contiguous floats, with no coordinate conversion.
layout(location = 0) in vec3 position;
layout(location = 1) in vec2 uv;
layout(location = 0) out vec2 texture_uv;

void main() {
    gl_Position = vec4(position, 1.0);
    texture_uv = uv;
}
