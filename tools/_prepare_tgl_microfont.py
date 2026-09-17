#!/usr/bin/env python3
"""One-shot exact-source editor; removed when the tested blobs are committed."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
PATHS = ['src/log_os.rs', 'src/intel/mod.rs', 'src/intel/tgl_native_panel.rs', 'src/intel/display.rs', 'src/ui4/slot4_service.rs']
S = {path: (ROOT / path).read_text() for path in PATHS}


def once(source, old, new):
    assert source.count(old) == 1, (old, source.count(old))
    return source.replace(old, new, 1)


def span(source, name):
    found = list(re.finditer(r'\bfn ' + re.escape(name) + r'\(', source))
    assert len(found) == 1, name
    start = found[0].start()
    end = source.find('\n}', start)
    assert end > start, name
    return start, end + 2


def within(source, name, old, new):
    start, end = span(source, name)
    return source[:start] + once(source[start:end], old, new) + source[end:]


def prepend(source, name, body):
    start, end = span(source, name)
    opening = source.index('{\n', start, end) + 2
    return source[:opening] + body + source[opening:]


p = 'src/log_os.rs'
S[p] = once(S[p], 'use core::fmt;', '''// The physical TGL diagnostic console is an independent, append-only sink.
// Its Trace acceptance does not widen either TCP or UART's area policies.
pub(crate) mod microfont_console;

use core::fmt;''')
S[p] = once(S[p], "static TRUEOS_LOG_SINKS: [&'static dyn log_os_core::GlobalLogSink; 2] =\n    [&TCP_LOG_SINK, &EMULATOR_UART_LOG_SINK];",
    "static TRUEOS_LOG_SINKS: [&'static dyn log_os_core::GlobalLogSink; 3] =\n    [&microfont_console::SINK, &TCP_LOG_SINK, &EMULATOR_UART_LOG_SINK];")

p = 'src/intel/mod.rs'
S[p] = within(S[p], 'init_once',
    '    let guc_boot = guc_boot_enabled_for_device(dev.device_id);',
    '''    // Start the bounded RAM transcript before forcewake/GuC can fail.
    // Never read PCI configuration from inside the logging callback.
    crate::log_os::microfont_console::enable_for_pci_identity(
        crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 0),
        crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 8) as u8,
    );
    let guc_boot = guc_boot_enabled_for_device(dev.device_id);''')

p = 'src/intel/tgl_native_panel.rs'
S[p] = within(S[p], 'init_once', '    RETAINED_FRAME.call_once(|| frame);',
    '''    RETAINED_FRAME.call_once(|| frame);
    // These mapped pages now live until reboot. The CPU logger becomes their
    // only writer; first the primary, later Slot4, may scan the same pixels.
    let log_attached = unsafe {
        crate::log_os::microfont_console::attach(crate::log_os::microfont_console::Surface {
            phys: frame.phys,
            gpu: SURFACE_GPU,
            virt: frame.virt,
            byte_len: FRAME_BYTES,
            width: WIDTH,
            height: HEIGHT,
            pitch_bytes: PITCH_BYTES,
        })
    };
    if log_attached {
        crate::log!("intel/tgl-native-panel: CPU microfont log attached; earlier GT/GuC records replayed; continuing native/UI4 bring-up\\n");
    }''')
S[p] = once(S[p], 'proof=cpu-native-border-and-quadrants backing=retained',
    'proof=cpu-authored-native-surface backing=retained')

p = 'src/intel/display.rs'
S[p] = once(S[p], 'mod regs;', 'mod regs;\nmod microfont_overlay;')
S[p] = within(S[p], 'bootstrap_ui4_rgba8_plane_stack_once',
    'ensure_overlay_surface_for_pipe(dev, pipe, slot, primary.width, primary.height)',
    '''microfont_overlay::bootstrap_surface(pipe, slot, primary.width, primary.height)
                .or_else(|| ensure_overlay_surface_for_pipe(dev, pipe, slot, primary.width, primary.height))''')
S[p] = within(S[p], 'bootstrap_ui4_rgba8_plane_stack_once',
    '        mark_overlay_surface_front(surface);',
    '''        if !microfont_overlay::owns_plane(surface.pipe, surface.plane_slot) {
            mark_overlay_surface_front(surface);
        }''')
S[p] = prepend(S[p], 'ensure_overlay_surface_for_pipe',
    '''    // The diagnostic front is never allocated, cleared, resized or freed
    // through an ordinary UI4 pool, including after the log page becomes full.
    if microfont_overlay::owns_plane(pipe, plane_slot) {
        return None;
    }
''')
S[p] = prepend(S[p], 'queue_ui4_plane_surface_flip',
    '''    if microfont_overlay::owns_plane(PIPES[0], crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT)
        && plane_base == PIPES[0].plane(crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT).base()
    {
        return PlaneSurfaceFlipQueueResult::Rejected;
    }
''')

p = 'src/ui4/slot4_service.rs'
S[p] = prepend(S[p], 'ui4_slot4_service_task',
    '''    // This boot lends only the interaction plane to the CPU log. Do not
    // reseal Spirit, stop the compositor, or touch application planes 0..3.
    if crate::log_os::microfont_console::owns_plane(0, super::INTERACTION_OVERLAY_PLANE_SLOT) {
        crate::log_info!(target: "ui4/slot4";
            "ui4/slot4: CPU microfont log owns interaction plane; application compositor and Spirit unchanged\\n");
        return;
    }
''')

for path, text in S.items():
    (ROOT / path).write_text(text)
    print('updated', path, flush=True)
