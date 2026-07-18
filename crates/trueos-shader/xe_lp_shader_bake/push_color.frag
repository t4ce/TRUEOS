#version 450

layout(push_constant) uniform DrawColor {
    vec4 rgba;
} draw_color;

layout(location = 0) out vec4 out_color;

void main() {
    out_color = draw_color.rgba;
}
