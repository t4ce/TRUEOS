#!/usr/bin/env python3
"""Temporary supplement; raw marker and retirement are not interchangeable."""
from pathlib import Path
import runpy
ROOT = Path(__file__).resolve().parents[1]
runpy.run_path(str(ROOT / 'tools/_prepare_font_warm_diag.py'), run_name='__main__')
p = ROOT / 'src/intel/gpgpu/rcs/commands.rs'
s = p.read_text()
old = '    let completed = proof.complete();'
assert s.count(old) == 1
s = s.replace(old, old + '''
    // The public poll result is zero unless BOTH marker and saved-head proof
    // pass. Report the existing raw observation before it is normalized.
    if lane == DirectRcsLane::Font {
        if completed {
            crate::log_font_warm_diag!("phase=font-retire-complete slot={} raw=0x{:08X} saved_head={} tail={}\\n",
                slot, observed, proof.saved_head_bytes, proof.published_tail_bytes);
        } else {
            crate::log_font_warm_diag!("phase=font-retire-incomplete slot={} raw=0x{:08X}/0x{:08X} marker={} saved_head={} tail={}\\n",
                slot, observed, expected, proof.marker_observed as u8,
                proof.saved_head_bytes, proof.published_tail_bytes);
        }
    }''', 1)
p.write_text(s)
p = ROOT / 'src/intel/gpgpu/operations/submission_2d.rs'
s = p.read_text()
s = s.replace('phase=coverage-marker post=', 'phase=coverage-marker retired_marker=')
s = s.replace('pre=0x{:08X}/0x{:08X} post=0x{:08X}/0x{:08X} quarantined=',
              'pre=0x{:08X}/0x{:08X} retired_marker=0x{:08X}/0x{:08X} quarantined=')
p.write_text(s)
print('updated compact font retirement diagnostics using raw marker and existing saved-head proof')
