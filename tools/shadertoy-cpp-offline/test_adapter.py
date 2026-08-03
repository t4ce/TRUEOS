#!/usr/bin/env python3

import unittest

from adapter import AdapterError, adapt, translate_body


class AdapterTests(unittest.TestCase):
    def test_threads_uniforms_through_helpers_and_main(self) -> None:
        body = translate_body(
            """
            float shade(vec2 p) { return length(p) + iTime; }
            void mainImage(out vec4 color, in vec2 coord) {
                vec2 uv = (coord - .5*iResolution.xy) / iResolution.y;
                color = vec4(vec3(shade(uv)), 1.0);
            }
            """
        )
        self.assertIn("float shade(float2 p, __global const ShaderToyUniforms *st)", body)
        self.assertIn("void mainImage( float4 & color,  float2 coord, __global const ShaderToyUniforms *st)", body)
        self.assertIn("shade(uv, st)", body)
        self.assertIn("(st->resolution_time.w)", body)
        self.assertIn(".5f", body)
        self.assertIn("1.0f", body)

    def test_vector_constructors_and_glsl_functions(self) -> None:
        body = translate_body(
            """
            void mainImage(out vec4 c, vec2 p) {
                vec3 q = vec3(p, fract(iTime));
                c = vec4(pow(max(q, 0.0), vec3(2.0)), 1.0);
            }
            """
        )
        self.assertIn("float3 q = st_vec3(p, st_fract(", body)
        self.assertIn("st_vec4(st_pow(st_max(q, 0.0f), st_vec3(2.0f)), 1.0f)", body)

    def test_standard_uniform_declaration_is_removed(self) -> None:
        body = translate_body(
            "uniform vec3 iResolution;\nuniform float iTime;\n"
            "void mainImage(out vec4 c, in vec2 p) { c=vec4(iTime); }"
        )
        self.assertNotIn("uniform", body)

    def test_channels_fail_loudly(self) -> None:
        with self.assertRaisesRegex(AdapterError, "iChannel"):
            adapt("void mainImage(out vec4 c, vec2 p){c=texture(iChannel0,p);}")

    def test_derivatives_fail_loudly(self) -> None:
        with self.assertRaisesRegex(AdapterError, "derivatives"):
            adapt("void mainImage(out vec4 c, vec2 p){c=vec4(fwidth(p),0.,1.);}")

    def test_requires_image_entrypoint(self) -> None:
        with self.assertRaisesRegex(AdapterError, "mainImage"):
            adapt("float f(float x) { return x; }")

    def test_duplicate_image_entrypoint_explains_paste_mistake(self) -> None:
        shader = "void mainImage(out vec4 c, vec2 p) { c=vec4(1.); }"
        with self.assertRaisesRegex(AdapterError, "found 2.*replace"):
            adapt(shader + "\n" + shader)

    def test_macro_mandelbrot_shader_translates(self) -> None:
        body = translate_body(
            """
            #define mul(a,b) vec2(a.x*b.x-a.y*b.y,a.x*b.y+a.y*b.x)
            void mainImage(out vec4 fragColor, in vec2 fragCoord) {
                float z = 1.0 - float(iTime) / 10.0;
                vec2 coord = fragCoord / iResolution.xy * z;
                vec2 cm = vec2(0, 0);
                int j = 0;
                for (int i=0; i<25; i++) {
                    j++;
                    cm = mul(cm, cm) + coord;
                    if (dot(cm,cm) > 4.0) break;
                }
                fragColor = vec4(j) / 24.0;
            }
            """
        )
        self.assertIn("#define mul(a,b) st_vec2", body)
        self.assertIn("float((st->resolution_time.w))", body)
        self.assertIn("fragColor = st_vec4(j) / 24.0f", body)


if __name__ == "__main__":
    unittest.main()
