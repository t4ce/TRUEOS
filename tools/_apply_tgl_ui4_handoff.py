#!/usr/bin/env python3
"""Temporary exact-base editor, removed from the final experiment diff."""
from pathlib import Path
import hashlib
import re

ROOT = Path(__file__).resolve().parents[1]
EXPECTED = {
    "src/intel/mod.rs": "a3fd7806449497652b8c6811bb0f5b8d4c4982a9",
    "src/intel/tgl_native_panel.rs": "0243626804fc276a9a2b63155182dd7838440087",
    "src/intel/display.rs": "c7bab142d05163d2da585e86bd33a0749c2772f8",
}
SOURCES = {}
for path, expected in EXPECTED.items():
    data = (ROOT / path).read_bytes()
    digest = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
    if digest != expected:
        raise SystemExit(f"refusing changed source: {path}: {digest} != {expected}")
    SOURCES[path] = data.decode("utf-8")


def replace_once(source: str, old: str, new: str) -> str:
    if source.count(old) != 1:
        raise RuntimeError(f"expected exactly one occurrence: {old!r}")
    return source.replace(old, new, 1)


def function_span(source: str, name: str) -> tuple[int, int]:
    matches = list(re.finditer(r"(?m)^(?:pub(?:\([^\n]*?\))? )?(?:const )?fn " + re.escape(name) + r"\(", source))
    if len(matches) != 1:
        raise RuntimeError(f"ambiguous function: {name}")
    start = matches[0].start()
    end = source.find("\n}", start)
    if end < 0:
        raise RuntimeError(f"missing function end: {name}")
    return start, end + 2


def prepend(source: str, name: str, body: str) -> str:
    start, end = function_span(source, name)
    opening = source.find("{\n", start, end)
    if opening < 0:
        raise RuntimeError(f"missing function body: {name}")
    return source[:opening + 2] + body + source[opening + 2:]


def in_function(source: str, name: str, old: str, new: str, count: int) -> str:
    start, end = function_span(source, name)
    fragment = source[start:end]
    if fragment.count(old) != count:
        raise RuntimeError(f"{name}: expected {count} occurrences of {old!r}, found {fragment.count(old)}")
    return source[:start] + fragment.replace(old, new) + source[end:]


path = "src/intel/mod.rs"
SOURCES[path] = replace_once(SOURCES[path],
    """        // Native-panel-only cycle: no desktop DBUF/plane-stack takeover after
        // this probe, even if firmware did not provide the expected route.
        let _ = self::tgl_native_panel::init_once(dev);""",
    """        // Keep the proven native timing/source/scaler handoff, then enter
        // the ordinary UI4 plane/bootstrap path with native-sized backing.
        // An unsupported firmware route still does not get a desktop fallback.
        if self::tgl_native_panel::init_once(dev) {
            self::display::init_primary_boot_surface(dev);
        }""")

path = "src/intel/tgl_native_panel.rs"
panel = SOURCES[path]
panel = replace_once(panel,
    "//! surface, source geometry, and pipe/primary scaler bindings. This deliberately\n//! does not publish the desktop UI4 five-plane stack as ready.",
    "//! surface, source geometry, and pipe/primary scaler bindings. On success the\n//! caller continues into the ordinary UI4 bootstrap; this probe itself does not\n//! publish plane-stack readiness.")
panel = replace_once(panel,
    "// A separate, temporary startup seal; the PCI alias and GuC bring-up stay intact.\npub(crate) const SPIRIT_RESEALED: bool = true;",
    "// The native-panel-only observation cycle is complete. Let normal Spirit\n// startup run; the PCI alias, GuC and genuine completion checks stay intact.\npub(crate) const SPIRIT_RESEALED: bool = false;")
panel = replace_once(panel, "const SURFACE_GPU: u64", "pub(super) const SURFACE_GPU: u64")
panel = replace_once(panel, "const SURFACE_GPU_CAPACITY: u64", "pub(super) const SURFACE_GPU_CAPACITY: u64")
panel = replace_once(panel,
    "pub(crate) fn spirit_resealed() -> bool {",
    """/// Stable layout selection, published only after this native surface latches.
pub(super) fn native_scanout_ready() -> bool {
    SCANOUT_LATCHED.load(Ordering::Acquire)
}

pub(crate) fn spirit_resealed() -> bool {""")
panel = replace_once(panel,
    "/// Standalone native Pipe A proof. The caller must not subsequently run the\n/// desktop five-plane bootstrap on this target, including when this fails.",
    "/// Establish native Pipe A before ordinary UI4 plane initialization. Only a\n/// successful native handoff may continue; retain the old route on failure.")
SOURCES[path] = panel

path = "src/intel/display.rs"
display = replace_once(SOURCES[path], "mod regs;\npub(super) use self::regs::*;",
                       "mod regs;\npub(super) use self::regs::*;\nmod native_ui4;")
for name, body in {
    "primary_surface_gpu_for_pipe": "    if native_ui4::active(pipe) {\n        return Some(native_ui4::PRIMARY_GPU);\n    }\n",
    "primary_surface_gpu_capacity": "    if native_ui4::active(pipe) {\n        return native_ui4::PRIMARY_BYTES;\n    }\n",
    "overlay_surface_gpu_for_index": "    if native_ui4::active(pipe) {\n        return native_ui4::overlay_gpu(plane_slot, index);\n    }\n",
    "primary_swap_surface_gpu_for_index": "    if native_ui4::active(pipe) {\n        return native_ui4::primary_swap_gpu(index);\n    }\n",
    "primary_compose_rcs_gpu_for_surface": "    if native_ui4::active(surface.pipe) {\n        return native_ui4::compose_gpu(0, surface.buffer_index);\n    }\n",
    "overlay_compose_rcs_gpu_for_surface": "    if native_ui4::active(surface.pipe) {\n        return native_ui4::compose_gpu(surface.plane_slot, surface.buffer_index);\n    }\n",
}.items():
    display = prepend(display, name, body)
for name, old, new, count in [
    ("ensure_overlay_surface_for_pipe", "OVERLAY_SWAP_GPU_STRIDE", "native_ui4::overlay_capacity(pipe)", 2),
    ("ensure_primary_swap_surface_for_pipe", "PRIMARY_SWAP_GPU_STRIDE", "native_ui4::primary_swap_capacity(pipe)", 2),
    ("compose_premultiplied_rgba_tiles_into_primary_gpgpu", "COMPOSE_RCS_GPU_ALIAS_BYTES", "native_ui4::compose_capacity(surface.pipe, 0)", 1),
    ("compose_premultiplied_rgba_tiles_into_overlay_gpgpu", "COMPOSE_RCS_GPU_ALIAS_BYTES", "native_ui4::compose_capacity(surface.pipe, surface.plane_slot)", 1),
    ("compose_premultiplied_rgba_tiles_into_primary_gpgpu", "        primary.gpu,", "        native_ui4::base_gpu(primary.pipe, primary.gpu),", 1),
]:
    display = in_function(display, name, old, new, count)
SOURCES[path] = display

# No source is written unless every exact-base replacement above succeeded.
for path, source in SOURCES.items():
    (ROOT / path).write_text(source)
    print(f"updated {path}", flush=True)
