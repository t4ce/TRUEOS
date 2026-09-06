#!/usr/bin/env python3
"""Translate a useful ShaderToy GLSL subset to TRUEOS C++ for OpenCL.

The generated kernel deliberately has the same two-buffer shape as the Spirit
preview kernels: one read-only control buffer and one linear BGRA8 output.
This is a source adapter, not a second rendering backend.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import re


class AdapterError(ValueError):
    pass


@dataclass
class Token:
    kind: str
    text: str


_TOKEN_RE = re.compile(
    r"(?P<space>\s+)"
    r"|(?P<line_comment>//[^\n]*(?:\n|$))"
    r"|(?P<block_comment>/\*.*?\*/)"
    r'|(?P<string>"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\')'
    r"|(?P<number>(?:0[xX][0-9A-Fa-f]+(?:[uU])?|(?:\d+\.\d*|\.\d+|\d+[eE][+-]?\d+|\d+\.\d*[eE][+-]?\d+)(?:[fF])?|\d+(?:[uU])?))"
    r"|(?P<ident>[A-Za-z_][A-Za-z0-9_]*)"
    r"|(?P<symbol>.)",
    re.DOTALL,
)

_TRIVIA = {"space", "line_comment", "block_comment"}

_TYPE_NAMES = {
    "vec2": "float2",
    "vec3": "float3",
    "vec4": "float4",
    "ivec2": "int2",
    "ivec3": "int3",
    "ivec4": "int4",
    "uvec2": "uint2",
    "uvec3": "uint3",
    "uvec4": "uint4",
    "bvec2": "int2",
    "bvec3": "int3",
    "bvec4": "int4",
}

_CONSTRUCTORS = {
    "vec2": "st_vec2",
    "vec3": "st_vec3",
    "vec4": "st_vec4",
    "ivec2": "st_ivec2",
    "ivec3": "st_ivec3",
    "ivec4": "st_ivec4",
    "uvec2": "st_uvec2",
    "uvec3": "st_uvec3",
    "uvec4": "st_uvec4",
    "bvec2": "st_ivec2",
    "bvec3": "st_ivec3",
    "bvec4": "st_ivec4",
}

_FUNCTION_MAP = {
    "abs": "st_abs",
    "fract": "st_fract",
    "mod": "st_mod",
    "atan": "st_atan",
    "pow": "st_pow",
    "min": "st_min",
    "max": "st_max",
    "clamp": "st_clamp",
    "step": "st_step",
    "smoothstep": "st_smoothstep",
    "mix": "st_mix",
    "reflect": "st_reflect",
    "refract": "st_refract",
    "faceforward": "st_faceforward",
    "floatBitsToInt": "as_int",
    "floatBitsToUint": "as_uint",
    "intBitsToFloat": "as_float",
    "uintBitsToFloat": "as_float",
}

_UNIFORMS = {
    "iResolution": "(st->resolution_time.xyz)",
    "iTime": "(st->resolution_time.w)",
    "iTimeDelta": "(st->timing.x)",
    "iFrameRate": "(st->timing.y)",
    "iSampleRate": "(st->timing.z)",
    "iFrame": "((int)st->timing.w)",
    "iMouse": "(st->mouse)",
    "iDate": "(st->date)",
}

_UNSUPPORTED_PATTERNS = (
    (re.compile(r"\biChannel(?:Time|Resolution|[0-3])\b"),
     "iChannel textures and per-channel state are not supported by the current TRUEOS two-buffer shader ABI"),
    (re.compile(r"\b(?:texture|textureLod|textureGrad|texelFetch|textureSize)\s*\("),
     "texture sampling requires an iChannel/resource ABI and is not supported yet"),
    (re.compile(r"\b(?:dFdx|dFdy|fwidth)\s*\("),
     "screen-space derivatives are not available in the current compute-kernel adapter"),
    (re.compile(r"\bmainSound\s*\("),
     "sound shaders are not image shaders and are not supported by this preview"),
    (re.compile(r"\b(?:sampler2D|sampler3D|samplerCube)\b"),
     "samplers require an iChannel/resource ABI and are not supported yet"),
)


def _strip_glsl_declarations(source: str) -> str:
    source = re.sub(r"(?m)^\s*#\s*version[^\n]*(?:\n|$)", "", source)
    source = re.sub(r"(?m)^\s*precision\s+\w+\s+\w+\s*;\s*(?:\n|$)", "", source)
    standard_uniforms = "|".join(map(re.escape, _UNIFORMS))
    source = re.sub(
        rf"(?m)^\s*uniform\s+[^;]*\b(?:{standard_uniforms})\b[^;]*;\s*(?:\n|$)",
        "",
        source,
    )
    return source


def _tokenize(source: str) -> list[Token]:
    tokens: list[Token] = []
    position = 0
    for match in _TOKEN_RE.finditer(source):
        if match.start() != position:
            raise AdapterError(f"cannot tokenize shader near byte {position}")
        kind = match.lastgroup
        assert kind is not None
        tokens.append(Token(kind, match.group()))
        position = match.end()
    if position != len(source):
        raise AdapterError(f"cannot tokenize shader near byte {position}")
    return tokens


def _significant(tokens: list[Token]) -> list[int]:
    return [index for index, token in enumerate(tokens) if token.kind not in _TRIVIA]


def _matching_parens(tokens: list[Token], significant: list[int]) -> dict[int, int]:
    stack: list[int] = []
    pairs: dict[int, int] = {}
    for pos, token_index in enumerate(significant):
        text = tokens[token_index].text
        if text == "(":
            stack.append(pos)
        elif text == ")":
            if not stack:
                raise AdapterError("unmatched ')' in shader")
            opening = stack.pop()
            pairs[opening] = pos
            pairs[pos] = opening
    if stack:
        raise AdapterError("unmatched '(' in shader")
    return pairs


def _function_sites(
    tokens: list[Token], significant: list[int], parens: dict[int, int]
) -> tuple[set[str], set[int]]:
    names: set[str] = set()
    definition_openings: set[int] = set()
    brace_depth = 0
    control = {"if", "for", "while", "switch"}
    for pos, token_index in enumerate(significant):
        text = tokens[token_index].text
        if text == "{":
            brace_depth += 1
            continue
        if text == "}":
            brace_depth -= 1
            if brace_depth < 0:
                raise AdapterError("unmatched '}' in shader")
            continue
        if brace_depth != 0 or text != "(" or pos == 0 or pos not in parens:
            continue
        name_token = tokens[significant[pos - 1]]
        close_pos = parens[pos]
        next_pos = close_pos + 1
        next_text = tokens[significant[next_pos]].text if next_pos < len(significant) else ""
        previous_is_return_type = (
            pos >= 2 and tokens[significant[pos - 2]].kind == "ident"
        )
        if (
            name_token.kind == "ident"
            and name_token.text not in control
            and (next_text == "{" or (next_text == ";" and previous_is_return_type))
        ):
            names.add(name_token.text)
            definition_openings.add(pos)
    if brace_depth != 0:
        raise AdapterError("unmatched '{' in shader")
    return names, definition_openings


def _float_suffix(text: str) -> str:
    if text[-1:] in "fF" or text[-1:] in "uU" or text.lower().startswith("0x"):
        return text
    if "." in text or "e" in text.lower():
        return text + "f"
    return text


def _needs_invocation_state(source: str) -> bool:
    """Select invocation storage for writable or aggregate globals.

    Function parameters are not globals. Keep the established constant-only
    translation unchanged for sources that need no writable invocation state.
    """
    tokens = _tokenize(_strip_glsl_declarations(source))
    significant = _significant(tokens)
    # A conservative write scan is enough to preserve initialized scalar
    # globals that the Image pass updates. A shadowed local may also select
    # the aggregate path; C++ member/local scope then preserves its meaning.
    written_names: set[str] = set()
    depth = 0
    for pos, index in enumerate(significant):
        value = tokens[index].text
        if value == "{":
            depth += 1
        elif value == "}":
            depth -= 1
        elif depth > 0 and tokens[index].kind == "ident":
            following = [tokens[i].text for i in significant[pos + 1:pos + 3]]
            previous = [tokens[i].text for i in significant[max(0, pos - 2):pos]]
            if ((following[:1] == ["="] and following != ["=", "="])
                    or following in [[op, "="] for op in "+-*/%&|^"]
                    or following in [["+", "+"], ["-", "-"]]
                    or previous in [["+", "+"], ["-", "-"]]):
                written_names.add(value)
    depth = 0
    paren_depth = 0
    types = {"bool", "int", "uint", "float", "mat2", "mat3", *_TYPE_NAMES}
    for pos, index in enumerate(significant):
        value = tokens[index].text
        if value == "{":
            depth += 1
        elif value == "}":
            depth -= 1
        elif value == "(":
            paren_depth += 1
        elif value == ")":
            paren_depth -= 1
        elif depth == 0 and paren_depth == 0 and value in types:
            if pos + 2 < len(significant):
                name = tokens[significant[pos + 1]]
                following = tokens[significant[pos + 2]].text
                if name.kind == "ident" and following in {";", ",", "["}:
                    return True
                if (name.kind == "ident" and following == "="
                        and (value not in {"bool", "int", "uint", "float"}
                             or name.text in written_names)):
                    return True
    return False


def _uses_scaled_matrix_constructor(source: str) -> bool:
    tokens = _tokenize(source)
    significant = _significant(tokens)
    parens = _matching_parens(tokens, significant)
    for pos, index in enumerate(significant[:-1]):
        if tokens[index].text not in {"mat2", "mat3"}:
            continue
        if tokens[significant[pos + 1]].text != "(":
            continue
        closing = parens[pos + 1]
        if ((closing + 1 < len(significant)
             and tokens[significant[closing + 1]].text == "*")
                or (pos > 0 and tokens[significant[pos - 1]].text == "*")):
            return True
    return False


def _lower_swizzle_multiply(source: str) -> str:
    """C++ cannot bind a swizzle to our matrix operator*= reference.

    Rewrite simple variable swizzles as ordinary assignments, preserving RHS
    precedence. Do not duplicate an indexed or otherwise evaluated lvalue.
    """
    tokens = _tokenize(source)
    significant = _significant(tokens)
    before: dict[int, str] = {}
    for pos in range(len(significant) - 4):
        indices = significant[pos:pos + 5]
        base, dot, swizzle, multiply, equals = (tokens[i] for i in indices)
        if (base.kind != "ident" or dot.text != "."
                or multiply.text != "*" or equals.text != "="
                or not any(re.fullmatch(f"[{alphabet}]{{2,4}}", swizzle.text)
                           for alphabet in ("xyzw", "rgba", "stpq"))
                or (pos > 0 and tokens[significant[pos - 1]].text == ".")):
            continue
        nested = 0
        end = pos + 5
        while end < len(significant):
            value = tokens[significant[end]].text
            if nested == 0 and value in {";", ",", ")", "]", "}"}:
                break
            if value in {"(", "["}:
                nested += 1
            elif value in {")", "]"}:
                nested -= 1
            end += 1
        if end == len(significant):
            raise AdapterError("unterminated swizzle compound assignment")
        multiply.text = f"= {base.text}.{swizzle.text} * ("
        equals.text = ""
        index = significant[end]
        before[index] = before.get(index, "") + ")"
    return "".join(before.get(index, "") + token.text
                   for index, token in enumerate(tokens))


def translate_body(source: str, *, private_globals: bool = False) -> str:
    source = _lower_swizzle_multiply(source)
    source = _strip_glsl_declarations(source)
    if not re.search(r"\bvoid\s+mainImage\s*\(", source):
        raise AdapterError("paste an Image pass containing 'void mainImage(...)'")
    for pattern, message in _UNSUPPORTED_PATTERNS:
        if pattern.search(source):
            raise AdapterError(message)

    tokens = _tokenize(source)
    significant = _significant(tokens)
    parens = _matching_parens(tokens, significant)
    function_names, definition_openings = _function_sites(tokens, significant, parens)
    main_image_definitions = sum(
        1
        for opening in definition_openings
        if tokens[significant[opening - 1]].text == "mainImage"
    )
    if main_image_definitions == 0:
        raise AdapterError("mainImage must be a top-level function definition")
    if main_image_definitions != 1:
        raise AdapterError(
            f"found {main_image_definitions} top-level mainImage definitions; "
            "replace the existing editor source before pasting a new shader"
        )

    before: dict[int, list[str]] = {}
    after: dict[int, list[str]] = {}

    def add_before(index: int, text: str) -> None:
        before.setdefault(index, []).append(text)

    def add_after(index: int, text: str) -> None:
        after.setdefault(index, []).append(text)

    # GLSL globals are private to each fragment invocation. An initialized
    # scalar is normally used as a casually spelled constant, while the same
    # spelling in OpenCL becomes device-global storage and causes IGC to emit a
    # symbol-table pseudo-kernel. Spell simple scalar initializers as macros so
    # they leave no device-global symbol at all. A later write remains a clear
    # compiler error because the macro expands to a parenthesized value.
    brace_depth = 0
    for pos, token_index in enumerate(significant):
        text = tokens[token_index].text
        if text == "{":
            brace_depth += 1
            continue
        if text == "}":
            brace_depth -= 1
            continue
        if private_globals or brace_depth != 0 or text not in {"bool", "int", "uint", "float"}:
            continue
        if pos + 1 >= len(significant):
            continue
        name = tokens[significant[pos + 1]]
        if name.kind != "ident":
            continue
        equals_index: int | None = None
        scan = pos + 2
        while scan < len(significant):
            scanned = tokens[significant[scan]].text
            if scanned in {"{", "}"}:
                break
            if scanned == "(" and equals_index is None:
                break
            if scanned == "=":
                equals_index = significant[scan]
            if scanned == ";":
                if equals_index is not None:
                    if (pos > 0
                            and tokens[significant[pos - 1]].text == "const"):
                        tokens[significant[pos - 1]].text = "#define"
                        tokens[token_index].text = ""
                    else:
                        tokens[token_index].text = "#define"
                    tokens[equals_index].text = " ("
                    tokens[significant[scan]].text = ")\n"
                break
            scan += 1

    # Compact ShaderToy sources commonly omit the initializer of an induction
    # scalar (for example `float a=.5, t=iTime, i; ++i`). Leaving that as LLVM
    # `undef` is both non-deterministic and unlike the zero value those shaders
    # are authored around. Initialize only scalar declarators inside a body;
    # function parameters and return types are deliberately excluded.
    parameter_positions: set[int] = set()
    for opening in definition_openings:
        parameter_positions.update(range(opening + 1, parens[opening]))
    brace_depth = 0
    for pos, token_index in enumerate(significant):
        text = tokens[token_index].text
        if text == "{":
            brace_depth += 1
            continue
        if text == "}":
            brace_depth -= 1
            continue
        if (brace_depth == 0 or pos in parameter_positions
                or text not in {"bool", "int", "uint", "float"}):
            continue
        if pos + 1 >= len(significant):
            continue
        name = tokens[significant[pos + 1]]
        if name.kind != "ident":
            continue
        if (pos + 2 < len(significant)
                and tokens[significant[pos + 2]].text == "("):
            continue
        nested = 0
        has_initializer = False
        scan = pos + 1
        while scan < len(significant):
            scanned = tokens[significant[scan]].text
            if scanned in {"(", "["}:
                nested += 1
            elif scanned in {")", "]"}:
                if nested == 0:
                    break
                nested -= 1
            elif nested == 0 and scanned == "=":
                has_initializer = True
            elif nested == 0 and scanned in {",", ";"}:
                if not has_initializer:
                    add_before(significant[scan], " = 0")
                if scanned == ";":
                    break
                has_initializer = False
            elif nested == 0 and scanned in {"{", "}"}:
                break
            scan += 1

    # Every user function receives the same uniform pointer. This makes
    # ShaderToy's implicit globals explicit all the way through the call tree.
    for pos, token_index in enumerate(significant):
        token = tokens[token_index]
        if token.text != "(" or pos == 0 or pos not in parens:
            continue
        name = tokens[significant[pos - 1]].text
        if name not in function_names:
            continue
        close_pos = parens[pos]
        close_index = significant[close_pos]
        empty = close_pos == pos + 1
        argument = "__global const ShaderToyUniforms *st" if pos in definition_openings else "st"
        add_before(close_index, argument if empty else ", " + argument)
        if pos in definition_openings:
            next_pos = close_pos + 1
            if (next_pos < len(significant)
                    and tokens[significant[next_pos]].text == "{"):
                add_after(significant[next_pos], "\n    (void)st;")

    # GLSL parameter directions become actual C++ references.
    parameter_depth = 0
    for pos, token_index in enumerate(significant):
        text = tokens[token_index].text
        if text == "(":
            parameter_depth += 1
        elif text == ")":
            parameter_depth -= 1
        elif parameter_depth > 0 and text in {"in", "out", "inout"}:
            tokens[token_index].text = ""
            if text in {"out", "inout"}:
                type_pos = pos + 1
                while type_pos < len(significant):
                    candidate_index = significant[type_pos]
                    candidate = tokens[candidate_index]
                    if candidate.kind == "ident":
                        add_after(candidate_index, " &")
                        break
                    type_pos += 1

    for pos, token_index in enumerate(significant):
        token = tokens[token_index]
        if token.kind == "number":
            token.text = _float_suffix(token.text)
            continue
        if token.kind != "ident":
            continue

        next_text = ""
        if pos + 1 < len(significant):
            next_text = tokens[significant[pos + 1]].text

        if token.text in _UNIFORMS:
            token.text = _UNIFORMS[token.text]
        elif token.text in _CONSTRUCTORS and next_text == "(":
            token.text = _CONSTRUCTORS[token.text]
        elif token.text in _TYPE_NAMES:
            token.text = _TYPE_NAMES[token.text]
        elif token.text in _FUNCTION_MAP and next_text == "(":
            token.text = _FUNCTION_MAP[token.text]

    rendered: list[str] = []
    for index, token in enumerate(tokens):
        rendered.extend(before.get(index, ()))
        rendered.append(token.text)
        rendered.extend(after.get(index, ()))
    return "".join(rendered)


_PRELUDE = r'''// Generated by tools/shadertoy-cpp-offline/adapter.py.
// Do not publish this session artifact; paste the licensed ShaderToy source again.

#if !defined(__OPENCL_CPP_VERSION__)
#error "ShaderToy sessions require C++ for OpenCL"
#endif

#define TRUEOS_REQD_SUB_GROUP_SIZE_16 \
    __attribute__((intel_reqd_sub_group_size(16)))

// ShaderToy accepts exploratory locals that are temporarily unused. Keep the
// production -Werror policy for real diagnostics without rejecting that style.
#pragma clang diagnostic ignored "-Wunused-variable"

struct ShaderToyUniforms {
    float4 resolution_time;
    float4 mouse;
    float4 date;
    float4 timing;
};

inline constexpr float2 st_vec2(float v) { return (float2)(v); }
inline constexpr float2 st_vec2(float x, float y) { return (float2)(x, y); }
inline constexpr float2 st_vec2(float2 v) { return v; }
inline constexpr float2 st_vec2(float3 v) { return v.xy; }
inline constexpr float2 st_vec2(float4 v) { return v.xy; }

inline constexpr float3 st_vec3(float v) { return (float3)(v); }
inline constexpr float3 st_vec3(float x, float y, float z) { return (float3)(x, y, z); }
inline float3 st_vec3(float2 xy, float z) { return (float3)(xy, z); }
inline float3 st_vec3(float x, float2 yz) { return (float3)(x, yz); }
inline constexpr float3 st_vec3(float3 v) { return v; }
inline constexpr float3 st_vec3(float4 v) { return v.xyz; }

inline constexpr float4 st_vec4(float v) { return (float4)(v); }
inline constexpr float4 st_vec4(float x, float y, float z, float w) { return (float4)(x, y, z, w); }
inline float4 st_vec4(float2 xy, float z, float w) { return (float4)(xy, z, w); }
inline float4 st_vec4(float x, float2 yz, float w) { return (float4)(x, yz, w); }
inline float4 st_vec4(float x, float y, float2 zw) { return (float4)(x, y, zw); }
inline float4 st_vec4(float2 xy, float2 zw) { return (float4)(xy, zw); }
inline float4 st_vec4(float3 xyz, float w) { return (float4)(xyz, w); }
inline float4 st_vec4(float x, float3 yzw) { return (float4)(x, yzw); }
inline constexpr float4 st_vec4(float4 v) { return v; }

inline int2 st_ivec2(int v) { return (int2)(v); }
inline int2 st_ivec2(int x, int y) { return (int2)(x, y); }
inline int2 st_ivec2(float2 v) { return convert_int2(v); }
inline int3 st_ivec3(int v) { return (int3)(v); }
inline int3 st_ivec3(int x, int y, int z) { return (int3)(x, y, z); }
inline int3 st_ivec3(float3 v) { return convert_int3(v); }
inline int4 st_ivec4(int v) { return (int4)(v); }
inline int4 st_ivec4(int x, int y, int z, int w) { return (int4)(x, y, z, w); }
inline int4 st_ivec4(float4 v) { return convert_int4(v); }
inline uint2 st_uvec2(uint v) { return (uint2)(v); }
inline uint2 st_uvec2(uint x, uint y) { return (uint2)(x, y); }
inline uint2 st_uvec2(float2 v) { return convert_uint2(v); }
inline uint3 st_uvec3(uint v) { return (uint3)(v); }
inline uint3 st_uvec3(uint x, uint y, uint z) { return (uint3)(x, y, z); }
inline uint3 st_uvec3(float3 v) { return convert_uint3(v); }
inline uint4 st_uvec4(uint v) { return (uint4)(v); }
inline uint4 st_uvec4(uint x, uint y, uint z, uint w) { return (uint4)(x, y, z, w); }
inline uint4 st_uvec4(float4 v) { return convert_uint4(v); }

template <typename T> inline T st_fract(T x) { return x - floor(x); }
inline float st_abs(float x) { return fabs(x); }
inline float2 st_abs(float2 x) { return fabs(x); }
inline float3 st_abs(float3 x) { return fabs(x); }
inline float4 st_abs(float4 x) { return fabs(x); }
template <typename T> inline T st_mod(T x, T y) { return x - y * floor(x / y); }
inline float2 st_mod(float2 x, float y) { return x - y * floor(x / y); }
inline float3 st_mod(float3 x, float y) { return x - y * floor(x / y); }
inline float4 st_mod(float4 x, float y) { return x - y * floor(x / y); }
template <typename T> inline T st_atan(T x) { return atan(x); }
template <typename T> inline T st_atan(T y, T x) { return atan2(y, x); }
// GLSL leaves pow undefined for negative bases. These forms preserve the
// useful non-negative ShaderToy case while avoiding IGC's external pow helper
// and its implicit constant surface (which the bare-metal ABI cannot bind).
inline float st_pow(float x, float y) { return native_exp2(y * native_log2(x)); }
inline float2 st_pow(float2 x, float2 y) { return native_exp2(y * native_log2(x)); }
inline float3 st_pow(float3 x, float3 y) { return native_exp2(y * native_log2(x)); }
inline float4 st_pow(float4 x, float4 y) { return native_exp2(y * native_log2(x)); }
inline float2 st_pow(float2 x, float y) { return st_pow(x, (float2)(y)); }
inline float3 st_pow(float3 x, float y) { return st_pow(x, (float3)(y)); }
inline float4 st_pow(float4 x, float y) { return st_pow(x, (float4)(y)); }

template <typename T> inline T st_min(T a, T b) { return min(a, b); }
template <typename T> inline T st_max(T a, T b) { return max(a, b); }
template <typename T> inline T st_clamp(T x, T lo, T hi) { return clamp(x, lo, hi); }
template <typename T> inline T st_step(T edge, T x) { return step(edge, x); }
template <typename T> inline T st_smoothstep(T a, T b, T x) { return smoothstep(a, b, x); }
template <typename T> inline T st_mix(T a, T b, T x) { return mix(a, b, x); }

#define ST_VECTOR_SCALAR_OVERLOADS(N) \
inline float##N st_min(float##N a, float b) { return min(a, (float##N)(b)); } \
inline float##N st_min(float a, float##N b) { return min((float##N)(a), b); } \
inline float##N st_max(float##N a, float b) { return max(a, (float##N)(b)); } \
inline float##N st_max(float a, float##N b) { return max((float##N)(a), b); } \
inline float##N st_clamp(float##N x, float lo, float hi) { return clamp(x, (float##N)(lo), (float##N)(hi)); } \
inline float##N st_step(float edge, float##N x) { return step((float##N)(edge), x); } \
inline float##N st_smoothstep(float a, float b, float##N x) { return smoothstep((float##N)(a), (float##N)(b), x); } \
inline float##N st_mix(float##N a, float##N b, float x) { return mix(a, b, x); }
ST_VECTOR_SCALAR_OVERLOADS(2)
ST_VECTOR_SCALAR_OVERLOADS(3)
ST_VECTOR_SCALAR_OVERLOADS(4)

struct mat2 {
    float2 c0;
    float2 c1;
    inline mat2() : c0((float2)(1.0f, 0.0f)), c1((float2)(0.0f, 1.0f)) {}
    inline mat2(float d) : c0((float2)(d, 0.0f)), c1((float2)(0.0f, d)) {}
    inline mat2(float a, float b, float c, float d)
        : c0((float2)(a, b)), c1((float2)(c, d)) {}
    inline mat2(float4 v) : c0(v.xy), c1(v.zw) {}
    inline mat2(float2 a, float2 b) : c0(a), c1(b) {}
};
inline float2 operator*(mat2 m, float2 v) { return m.c0 * v.x + m.c1 * v.y; }
inline float2 operator*(float2 v, mat2 m) { return (float2)(dot(v, m.c0), dot(v, m.c1)); }
inline float2 &operator*=(float2 &v, mat2 m) { v = v * m; return v; }
inline mat2 operator*(mat2 a, mat2 b) { return mat2(a * b.c0, a * b.c1); }

struct mat3 {
    float3 c0;
    float3 c1;
    float3 c2;
    inline mat3()
        : c0((float3)(1.0f, 0.0f, 0.0f)),
          c1((float3)(0.0f, 1.0f, 0.0f)),
          c2((float3)(0.0f, 0.0f, 1.0f)) {}
    inline mat3(float d)
        : c0((float3)(d, 0.0f, 0.0f)),
          c1((float3)(0.0f, d, 0.0f)),
          c2((float3)(0.0f, 0.0f, d)) {}
    inline mat3(float a, float b, float c,
                float d, float e, float f,
                float g, float h, float i)
        : c0((float3)(a, b, c)),
          c1((float3)(d, e, f)),
          c2((float3)(g, h, i)) {}
    inline mat3(float3 a, float3 b, float3 c) : c0(a), c1(b), c2(c) {}
};
inline float3 operator*(mat3 m, float3 v) {
    return m.c0 * v.x + m.c1 * v.y + m.c2 * v.z;
}
inline float3 operator*(float3 v, mat3 m) {
    return (float3)(dot(v, m.c0), dot(v, m.c1), dot(v, m.c2));
}
inline float3 &operator*=(float3 &v, mat3 m) { v = v * m; return v; }
inline mat3 operator*(mat3 a, mat3 b) {
    return mat3(a * b.c0, a * b.c1, a * b.c2);
}

inline float2 st_reflect(float2 incident, float2 normal) {
    return incident - 2.0f * dot(normal, incident) * normal;
}
inline float3 st_reflect(float3 incident, float3 normal) {
    return incident - 2.0f * dot(normal, incident) * normal;
}
inline float4 st_reflect(float4 incident, float4 normal) {
    return incident - 2.0f * dot(normal, incident) * normal;
}

inline float2 st_refract(float2 incident, float2 normal, float eta) {
    float ni = dot(normal, incident);
    float k = 1.0f - eta * eta * (1.0f - ni * ni);
    return k < 0.0f ? (float2)(0.0f)
                    : eta * incident - (eta * ni + sqrt(k)) * normal;
}
inline float3 st_refract(float3 incident, float3 normal, float eta) {
    float ni = dot(normal, incident);
    float k = 1.0f - eta * eta * (1.0f - ni * ni);
    return k < 0.0f ? (float3)(0.0f)
                    : eta * incident - (eta * ni + sqrt(k)) * normal;
}
inline float4 st_refract(float4 incident, float4 normal, float eta) {
    float ni = dot(normal, incident);
    float k = 1.0f - eta * eta * (1.0f - ni * ni);
    return k < 0.0f ? (float4)(0.0f)
                    : eta * incident - (eta * ni + sqrt(k)) * normal;
}

template <typename T>
inline T st_faceforward(T normal, T incident, T reference) {
    return dot(reference, incident) < 0.0f ? normal : -normal;
}

'''

_EPILOGUE = r'''

static inline uint st_pack_rgba8(float4 color)
{
    float4 c = clamp(color, (float4)(0.0f), (float4)(1.0f));
    uint b = (uint)(c.z * 255.0f + 0.5f);
    uint g = (uint)(c.y * 255.0f + 0.5f);
    uint r = (uint)(c.x * 255.0f + 0.5f);
    // ShaderToy's Image canvas is opaque; its alpha channel is not used as a
    // desktop-compositor blend factor. Preserve that visual contract for UI4.
    return 0xFF000000u | (b << 16) | (g << 8) | r;
}

kernel TRUEOS_REQD_SUB_GROUP_SIZE_16 void shadertoy_image(
    __global uint *output,
    __global const ShaderToyUniforms *uniforms,
    uint width,
    uint height,
    uint pitch_bytes)
{
    uint x = get_global_id(0);
    uint y = get_global_id(1);
    if (x >= width || y >= height) {
        return;
    }
    float2 frag_coord = (float2)((float)x + 0.5f, (float)(height - y) - 0.5f);
    float4 frag_color = (float4)(0.0f, 0.0f, 0.0f, 1.0f);
    mainImage(frag_color, frag_coord, uniforms);
    output[(size_t)y * (pitch_bytes / sizeof(uint)) + x] = st_pack_rgba8(frag_color);
}
'''


_MATRIX_SCALAR_HELPERS = r'''
// GLSL matrix constructors may be scaled without splatting the scalar into
// the vector overload. Keep each column in the matrix result.
inline mat2 operator*(mat2 m, float s) { return mat2(m.c0 * s, m.c1 * s); }
inline mat2 operator*(float s, mat2 m) { return m * s; }
inline mat3 operator*(mat3 m, float s) { return mat3(m.c0 * s, m.c1 * s, m.c2 * s); }
inline mat3 operator*(float s, mat3 m) { return m * s; }

'''


_FOVEATED_COORDINATES = r'''
    float2 output_pixel = convert_float2((uint2)(x,y)) + (float2)(0.5f);
    if (uniforms->render_control.x == 2u) {
        float2 sample_pixel = st_focus_to_sample(output_pixel, uniforms->focus_control);
        sample_pixel *= convert_float2(uniforms->render_control.yz) / uniforms->resolution_time.xy;
        float4 color = st_focus_bilinear(source, sample_pixel - (float2)(0.5f), uniforms->render_control);
        output[(size_t)y * (pitch_bytes/4u) + x] = st_pack_rgba8(color);
        return;
    }
    if (uniforms->render_control.x == 1u) {
        output_pixel *= uniforms->resolution_time.xy / convert_float2((uint2)(width,height));
        output_pixel = st_focus_to_output(output_pixel, uniforms->focus_control);
    }
    float2 frag_coord = (float2)(output_pixel.x, uniforms->resolution_time.y-output_pixel.y);
'''


def adapt(source: str, kernel_name: str = "shadertoy_image", *, foveated: bool = False) -> str:
    if "\x00" in source:
        raise AdapterError("shader source contains a NUL byte")
    if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", kernel_name) is None:
        raise AdapterError(f"invalid generated kernel name: {kernel_name!r}")
    epilogue = _EPILOGUE.replace("shadertoy_image", kernel_name)
    prelude = _PRELUDE
    if foveated:
        # Opt-in ABI: first 64 uniform bytes are unchanged; a third buffer is
        # read only by the resolve branch. Inline helpers into the authenticated
        # generated source so there is no untracked runtime include dependency.
        prelude = prelude.replace("    float4 timing;", "    float4 timing;\n"
                                  "    uint4 render_control;\n    float4 focus_control;")
        helpers = "".join(Path(__file__).with_name(name).read_text(encoding="utf-8")
                          for name in ("foveated_coordinates.clcpp", "foveated.clcpp"))
        epilogue = epilogue.replace("kernel TRUEOS_REQD", helpers + "\nkernel TRUEOS_REQD")
        epilogue = epilogue.replace("    uint pitch_bytes)",
                                   "    uint pitch_bytes,\n    __global const uint *source)")
        epilogue = epilogue.replace(
            "    float2 frag_coord = (float2)((float)x + 0.5f, (float)(height - y) - 0.5f);",
            _FOVEATED_COORDINATES)
    private_globals = _needs_invocation_state(source)
    body = translate_body(source, private_globals=private_globals)
    helpers = _MATRIX_SCALAR_HELPERS if _uses_scaled_matrix_constructor(source) else ""
    if re.search(r"\.\s*[rgba]{1,4}\b", body):
        # The pinned Clang supports these GLSL/OpenCL vector aliases but warns
        # for the CLC++ language version. Their lowering is identical to xyzw;
        # retaining the spelling also avoids renaming user struct members.
        helpers += '#pragma clang diagnostic ignored "-Wopencl-unsupported-rgba"\n'
    if private_globals:
        # GLSL globals belong to one fragment invocation. A private aggregate
        # keeps member/helper access and local shadowing in C++ scope, without
        # introducing shared device globals or another kernel argument.
        body = "struct ShaderToyInvocation {\n" + body + "\n};\n"
        epilogue = epilogue.replace(
            "    mainImage(frag_color, frag_coord, uniforms);",
            "    ShaderToyInvocation invocation = {};\n"
            "    invocation.mainImage(frag_color, frag_coord, uniforms);",
        )
    return prelude + helpers + body + epilogue
