#!/usr/bin/env python3
"""Render the complete Codex SVG to native64 straight-alpha PNG/RGBA.

Requires Pillow, librsvg2, Cairo and GObject. No alpha threshold, background
removal or palette quantization: this is the UI tile, not the packed chest art.
"""
import argparse
import ctypes as C
import ctypes.util
import hashlib
import io
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parent


def rasterize_svg(svg, size):
    rsvg = C.CDLL(ctypes.util.find_library('rsvg-2'))
    cairo = C.CDLL(ctypes.util.find_library('cairo'))
    gobject = C.CDLL(ctypes.util.find_library('gobject-2.0'))
    class Rect(C.Structure):
        _fields_ = [(name, C.c_double) for name in ['x', 'y', 'width', 'height']]
    rsvg.rsvg_handle_new_from_data.argtypes = [C.c_char_p, C.c_size_t, C.POINTER(C.c_void_p)]
    rsvg.rsvg_handle_new_from_data.restype = C.c_void_p
    rsvg.rsvg_handle_render_document.argtypes = [C.c_void_p, C.c_void_p, C.POINTER(Rect), C.POINTER(C.c_void_p)]
    rsvg.rsvg_handle_render_document.restype = C.c_int
    cairo.cairo_image_surface_create.argtypes = [C.c_int, C.c_int, C.c_int]
    cairo.cairo_image_surface_create.restype = C.c_void_p
    cairo.cairo_create.argtypes = [C.c_void_p]
    cairo.cairo_create.restype = C.c_void_p
    write_callback = C.CFUNCTYPE(C.c_int, C.c_void_p, C.POINTER(C.c_ubyte), C.c_uint)
    cairo.cairo_surface_write_to_png_stream.argtypes = [C.c_void_p, write_callback, C.c_void_p]
    cairo.cairo_surface_write_to_png_stream.restype = C.c_int
    for name in ['cairo_destroy', 'cairo_surface_destroy']:
        getattr(cairo, name).argtypes = [C.c_void_p]
    gobject.g_object_unref.argtypes = [C.c_void_p]
    error = C.c_void_p()
    handle = rsvg.rsvg_handle_new_from_data(svg, len(svg), C.byref(error))
    assert handle, 'SVG parse failed'
    surface = cairo.cairo_image_surface_create(0, size, size)
    context = cairo.cairo_create(surface)
    try:
        assert rsvg.rsvg_handle_render_document(handle, context, C.byref(Rect(0, 0, size, size)), C.byref(error))
        chunks = []
        @write_callback
        def write_png(closure, data, count):
            chunks.append(C.string_at(data, count))
            return 0
        assert cairo.cairo_surface_write_to_png_stream(surface, write_png, None) == 0
        # PNG export unpremultiplies Cairo's white fade correctly into straight RGBA.
        return Image.open(io.BytesIO(b''.join(chunks))).convert('RGBA')
    finally:
        cairo.cairo_destroy(context)
        cairo.cairo_surface_destroy(surface)
        gobject.g_object_unref(handle)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--codex-root', type=Path, help='Also synchronize/verify this Codex checkout')
    args = parser.parse_args()
    svg = (ROOT / 'codex-tile.svg').read_bytes()
    tile = rasterize_svg(svg, 64)
    raw = tile.tobytes()
    assert len(raw) == 64 * 64 * 4
    assert tile.getpixel((0, 0))[3] == 0
    assert tile.getpixel((32, 32)) == (128, 128, 128, 102), 'Gray background must remain'
    png = io.BytesIO()
    tile.save(png, format='PNG')
    outputs = {ROOT / 'codex-tile64.rgba': raw, ROOT / 'codex-tile64.png': png.getvalue()}
    if args.codex_root:
        dest = args.codex_root.resolve() / 'codex-rs/tui/src/tui'
        assert dest.is_dir(), 'Expected a Codex checkout'
        outputs.update({dest / 'trueos-collapse.svg': svg,
                        dest / 'trueos-collapse.png': png.getvalue(),
                        dest / 'trueos-collapse.rgba': raw})
    for path, content in outputs.items():
        if args.check:
            assert path.read_bytes() == content, str(path) + ' is stale'
        else:
            path.write_bytes(content)
    print(('Verified' if args.check else 'Rendered') + ' native64 complete gray-backed tile; RGBA SHA256 ' + hashlib.sha256(raw).hexdigest())


if __name__ == '__main__':
    main()
