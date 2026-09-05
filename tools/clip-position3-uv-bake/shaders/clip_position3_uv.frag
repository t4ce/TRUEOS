#version 450

// Link-only fragment stage. The runtime reuses the decoded Picasso SIMD16 PS.
layout(location = 0) in vec2 texture_uv;
layout(location = 0) out vec4 color;
layout(set = 0, binding = 3) uniform texture2D base_color_texture;
layout(set = 0, binding = 4) uniform sampler base_color_sampler;

void main() {
    color = texture(sampler2D(base_color_texture, base_color_sampler), texture_uv);
}
