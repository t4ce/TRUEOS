#!/usr/bin/env python3
"""One-shot preparation of the native-panel UI4 patch; not a build dependency.

This script is temporary branch preparation tooling. It edits only the two
listed Rust sources, verifies every expected anchor, and never publishes Git
refs. The final PR must contain the resulting source diff, not just this script.
"""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
DISPLAY = ROOT / 'src/intel/display.rs'
PANEL = ROOT / 'src/intel/tgl_native_panel.rs'
display = DISPLAY.read_text()
panel = PANEL.read_text()


def replace_once(text: str, old: str, new: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f'Expected one anchor, got {count}: {old[:100]!r}')
    return text.replace(old, new, 1)


def insert_at_function(text: str, name: str, statements: str) -> str:
    pattern = rf'(?m)^fn {re.escape(name)}\([^{{]*\{{\n'
    matches = list(re.finditer(pattern, text))
    if len(matches) != 1:
        raise RuntimeError(f'Expected one function header for {name}, got {len(matches)}')
    end = matches[0].end()
    return text[:end] + statements + text[end:]


# Keep the established desktop layout unchanged. The native profile is selected
# only after the exact physical laptop's native panel setup has succeeded.
display = replace_once(display, 'mod display_metrics;\n', '''mod display_metrics;
mod tgl_ui4_layout;

fn native_ui4_for_pipe(pipe: PipeInfo) -> bool {
    pipe.slot == 0 && crate::intel::tgl_native_panel::ui4_handoff_active()
}

fn ui4_surface_slot_capacity(pipe: PipeInfo, legacy_capacity: u64) -> u64 {
    if native_ui4_for_pipe(pipe) {
        tgl_ui4_layout::SURFACE_SLOT_BYTES
    } else {
        legacy_capacity
    }
}

const _: () = {
    assert!(UI4_DIRECT_SCANOUT_GPU_BASE
        + UI4_DIRECT_SCANOUT_PLANE_COUNT as u64 * UI4_DIRECT_SCANOUT_PLANE_STRIDE
        <= tgl_ui4_layout::GGTT_BASE);
};
''')

display = insert_at_function(display, 'primary_surface_gpu_for_pipe', '''    if native_ui4_for_pipe(pipe) {
        return Some(tgl_ui4_layout::primary_gpu());
    }
''')
display = insert_at_function(display, 'primary_surface_gpu_capacity', '''    if native_ui4_for_pipe(pipe) {
        return tgl_ui4_layout::SURFACE_SLOT_BYTES;
    }
''')
display = insert_at_function(display, 'primary_swap_surface_gpu_for_index', '''    if native_ui4_for_pipe(pipe) {
        return tgl_ui4_layout::primary_swap_gpu(index);
    }
''')
display = insert_at_function(display, 'overlay_surface_gpu_for_index', '''    if native_ui4_for_pipe(pipe) {
        return tgl_ui4_layout::overlay_gpu(plane_slot, index);
    }
''')
display = insert_at_function(display, 'primary_compose_rcs_gpu_for_surface', '''    if native_ui4_for_pipe(surface.pipe) {
        return tgl_ui4_layout::compose_gpu(0, surface.buffer_index);
    }
''')
display = insert_at_function(display, 'overlay_compose_rcs_gpu_for_surface', '''    if native_ui4_for_pipe(surface.pipe) {
        return tgl_ui4_layout::compose_gpu(surface.plane_slot, surface.buffer_index);
    }
''')

for constant in ('OVERLAY_SWAP_GPU_STRIDE', 'PRIMARY_SWAP_GPU_STRIDE'):
    old = f'byte_len as u64 > {constant}'
    count = display.count(old)
    if count != 1:
        # Make an unexpected source layout visible in the preparation log;
        # never silently skip a capacity gate or broaden unrelated checks.
        for number, line in enumerate(display.splitlines(), 1):
            if constant in line:
                print(f'{DISPLAY.name}:{number}: {line}')
        raise RuntimeError(f'Expected one allocation capacity check for {constant}, got {count}')
    display = replace_once(display, old,
        f'byte_len as u64 > ui4_surface_slot_capacity(pipe, {constant})')

panel = replace_once(panel,
    '//! does not publish the desktop UI4 five-plane stack as ready.',
    '//! hands the native mode to the ordinary UI4 plane bootstrap after the\n//! primary surface has latched; no synthetic UI4 readiness is published.')
panel = replace_once(panel,
    '// A separate, temporary startup seal; the PCI alias and GuC bring-up stay intact.\npub(crate) const SPIRIT_RESEALED: bool = true;',
    '// The panel-only experiment is over: normal Spirit startup may proceed.\n// The PCI alias, GuC checks, real allocation and plane-latch checks remain.\npub(crate) const SPIRIT_RESEALED: bool = false;')
panel = replace_once(panel,
    'fn wait_frame(dev: Dev) -> bool {',
    '''/// Select the native surface reservations only for the successfully adopted
/// physical panel. This is not a GuC, RCS, or UI4 execution-success flag.
pub(crate) fn ui4_handoff_active() -> bool {
    SCANOUT_LATCHED.load(Ordering::Acquire)
        && super::claimed_device().is_some_and(is_target)
}

fn wait_frame(dev: Dev) -> bool {''')
panel = replace_once(panel,
    '/// Standalone native Pipe A proof. The caller must not subsequently run the\n/// desktop five-plane bootstrap on this target, including when this fails.',
    '/// Native Pipe A setup followed by the ordinary UI4 bootstrap on success.\n/// A failed native setup retains its surface and does not run the desktop path.')
panel = replace_once(panel,
    '    readback_ok\n}',
    '''    if readback_ok {
        // PIPE_SRC now describes the actual native panel. Re-enter the same
        // allocation, alpha/DBUF, plane-latch and capability-publication path
        // as desktop UI4, with distinct 4K-safe scanout/compositor reservations.
        // Do not retrain eDP or restore the old firmware scaler geometry.
        super::display::log_bsp_display_metrics_probe(dev);
        super::display::init_primary_boot_surface(dev);
        crate::log!(
            "intel/tgl-native-panel: ui4-handoff attempted=1 stack_ready={} spirit_resealed=0 native={}x{} link_timing=preserved\\n",
            super::display::ui4_rgba8_plane_stack_is_ready() as u8,
            WIDTH,
            HEIGHT,
        );
    }
    readback_ok
}''')

# The inspected driver uses an independent UI4 compositor PPGTT. Its sources
# end below 20000000 and all native destination slots end at the 1 GiB limit.
ui_surface = (ROOT / 'src/r/ui_surface.rs').read_text()
if not re.search(r'UI_SURFACE_GPU_LIMIT: u64 = 0x2000_0000;', ui_surface):
    raise RuntimeError('UI source arena moved; review native compositor alias layout')

# No partial writes if any anchor or layout precondition above was unexpected.
DISPLAY.write_text(display)
PANEL.write_text(panel)
print('Applied native TGL UI4 handoff to display.rs and tgl_native_panel.rs')
