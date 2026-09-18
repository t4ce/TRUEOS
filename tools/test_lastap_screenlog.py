#!/usr/bin/env python3
"""Compile the actual ring/service code with microfont; no kernel/hardware claim."""
from pathlib import Path
import os
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SERVICE = ROOT / 'src/r/services/microfont_log_service.rs'

RING_TESTS = r'''
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn independent_reader_survives_network_drain() {
        let mut ring = TcpLogRing::new();
        let mut cursor = 0;
        let mut out = [0; 16];
        ring.write_bytes(b"alpha\nbeta\n");
        assert_eq!(ring.drain_bytes(6), b"alpha\n");
        assert_eq!(ring.copy_since(&mut cursor, &mut out), (11, 0));
        assert_eq!(&out[..11], b"alpha\nbeta\n");
        assert_eq!(ring.drain_bytes(16), b"beta\n");
        ring.write_bytes(b"gamma\n");
        assert_eq!(ring.copy_since(&mut cursor, &mut out), (6, 0));
        assert_eq!(&out[..6], b"gamma\n");
        assert_eq!(ring.drain_bytes(16), b"gamma\n");
        assert_eq!(ring.copy_since(&mut cursor, &mut out), (0, 0));
    }
    #[test] fn physical_wrap_and_oversized_write_report_loss() {
        let mut ring = TcpLogRing::new();
        let initial = vec![b'a'; MAX_BYTES - 2];
        ring.write_bytes(&initial);
        let mut cursor = (MAX_BYTES - 4) as u64;
        ring.write_bytes(b"BCDEFG");
        let mut out = [0; 8];
        assert_eq!(ring.copy_since(&mut cursor, &mut out), (8, 0));
        assert_eq!(&out, b"aaBCDEFG");
        let mut stale = 0;
        assert_eq!(ring.copy_since(&mut stale, &mut out), (8, 4));
        let huge: Vec<u8> = (0..MAX_BYTES + 17).map(|i| (i % 251) as u8).collect();
        ring.write_bytes(&huge);
        let mut new_reader = 0;
        let (count, lost) = ring.copy_since(&mut new_reader, &mut out);
        assert_eq!((count, lost), (8, (MAX_BYTES + 21) as u64));
        assert_eq!(&out, &huge[17..25]);
    }
    #[test] fn screen_copy_does_not_change_network_output() {
        let mut with_screen = TcpLogRing::new();
        let mut without_screen = TcpLogRing::new();
        let mut cursor = 0;
        for i in 0..1000 {
            let bytes = format!("record {}\n", i);
            with_screen.write_bytes(bytes.as_bytes());
            without_screen.write_bytes(bytes.as_bytes());
            let mut scratch = [0; 7];
            with_screen.copy_since(&mut cursor, &mut scratch);
            if i % 11 == 0 {
                assert_eq!(with_screen.drain_bytes(53), without_screen.drain_bytes(53));
            }
        }
        assert_eq!(with_screen.drain_bytes(MAX_BYTES), without_screen.drain_bytes(MAX_BYTES));
    }
    #[test] fn busy_ring_does_not_spin_the_screen_consumer() {
        let _guard = RING.lock();
        assert!(copy_for_screen(&mut 0, &mut [0; 8]).is_none());
    }
}
'''
PEN_TESTS = r'''
#[cfg(test)] mod tests {
    use super::*;
    fn fixture(columns: usize, rows: usize) -> (Vec<u32>, Surface) {
        let width = columns * microfont::FWIDTH;
        let height = (rows + 1) * microfont::FHEIGHT;
        let pitch = width + 8;
        let mut pixels = vec![0x1357_2468; pitch * height + 32];
        let surface = Surface { phys: 0x1000, gpu: 0xE000_0000,
            virt: unsafe { pixels.as_mut_ptr().add(16) } as usize,
            byte_len: pitch * height * 4, width: width as u32, height: height as u32,
            pitch_bytes: (pitch * 4) as u32 };
        assert!(surface.valid());
        (pixels, surface)
    }
    #[test] fn glyph_matches_microfont_and_padding_is_untouched() {
        let (pixels, surface) = fixture(4, 2);
        let mut pen = Pen::new(surface);
        let _ = pen.write_str("q\nA");
        pen.flush();
        let mut glyph = [BG; microfont::FWIDTH * microfont::FHEIGHT];
        microfont::stamp_bytes(&mut glyph, microfont::FWIDTH, microfont::FHEIGHT, 0, 0, b"q", FG).unwrap();
        let pitch = surface.pitch_bytes as usize / 4;
        for y in 0..microfont::FHEIGHT {
            for x in 0..microfont::FWIDTH {
                assert_eq!(pixels[16 + y*pitch + x], glyph[y*microfont::FWIDTH + x]);
            }
        }
        for y in 0..surface.height as usize {
            assert!(pixels[16+y*pitch+surface.width as usize..16+(y+1)*pitch].iter().all(|p| *p == 0x1357_2468));
        }
        assert!(pixels[..16].iter().all(|p| *p == 0x1357_2468));
        assert!(pixels[pixels.len()-16..].iter().all(|p| *p == 0x1357_2468));
    }
    #[test] fn exact_width_newline_and_full_freeze() {
        let (pixels, surface) = fixture(4, 2);
        let mut pen = Pen::new(surface);
        pen.write_str("abcd\nEFGH").unwrap();
        assert!(pen.full());
        assert_eq!((pen.row, pen.column), (1, 4));
        pen.finish();
        let saved = pixels.clone();
        assert!(pen.write_str("overwrite!").is_err());
        pen.byte(b'X');
        pen.flush();
        assert_eq!(pixels, saved);
    }
    #[test] fn wraps_and_tabs_without_overwrite() {
        let (_pixels, surface) = fixture(8, 3);
        let mut pen = Pen::new(surface);
        pen.write_str("A\rB\tC").unwrap();
        assert_eq!((pen.row, pen.column), (0, 5));
        pen.write_str("DEFZ").unwrap();
        assert_eq!((pen.row, pen.column), (1, 1));
    }
    #[test] fn surface_bounds_and_native_capacity() {
        let (_pixels, surface) = fixture(640, 195);
        let pen = Pen::new(surface);
        assert_eq!((pen.columns, pen.rows), (640, 195));
        assert!(!Surface { byte_len: 4, ..surface }.valid());
        assert!(!Surface { virt: usize::MAX - 3, ..surface }.valid());
        assert!(!Surface { pitch_bytes: 1, ..surface }.valid());
        assert!(!Surface { height: 1, ..surface }.valid());
    }
}
'''
POLICY_HARNESS = r'''
mod r { pub mod services { pub mod microfont_log_service {
    pub fn enabled() -> bool { crate::policy::ACTIVE.load(core::sync::atomic::Ordering::Relaxed) }
} } }
mod policy {
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    pub static ACTIVE: AtomicBool = AtomicBool::new(false);
    static COUNT: AtomicUsize = AtomicUsize::new(8);
    const FIRST_BACKGROUND_SLOT: u32 = 2;
    const WORKER_SLOT_LIMIT: usize = 256;
    fn topology_core_slot_count() -> usize { COUNT.load(Ordering::Relaxed) }
    fn is_background_worker_slot(slot: u32) -> bool { slot >= FIRST_BACKGROUND_SLOT }
    // Actual production functions follow. Only the input topology/profile are mocked.
    __FUNCTIONS__
    #[test] fn reservation_is_final_topology_slot_and_profile_only() {
        assert_eq!(last_ap_service_slot(), None);
        assert!(is_general_background_worker_slot(7));
        ACTIVE.store(true, Ordering::Relaxed);
        assert_eq!(last_ap_service_slot(), Some(7));
        assert!(!is_general_background_worker_slot(7));
        assert!(is_general_background_worker_slot(6));
        assert!(!is_general_background_worker_slot(0));
        assert!(!is_general_background_worker_slot(1));
        COUNT.store(2, Ordering::Relaxed);
        assert_eq!(last_ap_service_slot(), None);
        COUNT.store(256, Ordering::Relaxed);
        assert_eq!(last_ap_service_slot(), Some(255));
        COUNT.store(257, Ordering::Relaxed);
        assert_eq!(last_ap_service_slot(), None);
        ACTIVE.store(false, Ordering::Relaxed);
    }
}
'''

def main():
    log = (ROOT / 'src/log_os.rs').read_text()
    service = SERVICE.read_text()
    workers = (ROOT / 'src/workers.rs').read_text()
    spawn = (ROOT / 'src/r/services/spawn_service.rs').read_text()
    display = (ROOT / 'src/intel/display.rs').read_text()
    panel = (ROOT / 'src/intel/tgl_native_panel.rs').read_text()
    assert 'fn copy_for_screen' in log and 'written: u64' in log
    assert 'last_ap_service_slot' in workers and '!is_last_ap_service_slot(cpu_slot)' in workers
    assert 'last_ap_service_worker()' in spawn and 'lastap_spawner.spawn(token)' in spawn
    assert 'const TASK_COUNT: usize = 77' in spawn
    assert 'microfont-log' in spawn and 'ui4_slot4_service_gate' in spawn
    assert 'if readback_ok {' in panel and 'microfont_log_service::install' in panel
    assert 'pub(crate) const SPIRIT_RESEALED: bool = false;' in panel
    assert 'microfont_log_boot_surface' in display and 'screenlog::owns_plane' in display
    assert 'Timer::after' in service and 'from_millis(PERIOD_MS)' in service
    assert 'spin_loop' not in service and 'PAGE:' not in service
    print('PASS: real last-AP placement, existing log ring, bounded task, native/UI4 handoff intact', flush=True)
    start = log.index('pub mod logtotcp {') + len('pub mod logtotcp {')
    end = log.index('    #[trueos_executor::task]', start)
    ring = log[start:end]
    functions = []
    for name in ['last_ap_service_slot', 'is_last_ap_service_slot', 'is_general_background_worker_slot']:
        start = workers.index('pub fn ' + name + '(')
        end = workers.index('\n}', start) + 2
        functions.append(workers[start:end])
    policy = POLICY_HARNESS.replace('__FUNCTIONS__', '\n'.join(functions))
    # Exact production ring prefix; omit only the unrelated asynchronous TCP server.
    harness = '''extern crate alloc;
mod intel {
    pub fn dma_cache_flush_range(_: *const u8, _: usize) {}
    pub fn dma_flush_strided_rows(_: *mut u8, _: usize, _: usize, _: usize) -> bool { true }
}
#[path = "service.rs"] mod service;
mod ring {\n''' + ring + RING_TESTS + '\n}\n' + policy
    with tempfile.TemporaryDirectory(prefix='trueos-screenlog-') as temp:
        work = Path(temp)
        (work / 'Cargo.toml').write_text('''[package]
name = "trueos-screenlog-host-test"
version = "0.0.0"
edition = "2024"
[workspace]
[lib]
path = "lib.rs"
[dependencies]
spin = "0.10"
microfont = "=3.7.8"
''')
        (work / 'service.rs').write_text(service + PEN_TESTS)
        (work / 'lib.rs').write_text(harness)
        env = dict(os.environ, RUSTUP_TOOLCHAIN='stable', RUST_MIN_STACK=str(16*1024*1024))
        subprocess.run(['cargo', 'test', '--lib', '--manifest-path', str(work / 'Cargo.toml'), '--', '--test-threads=1'], cwd=work, env=env, check=True)
    print('PASS: actual ring, LastAP selector and CPU renderer tested (not a full kernel build)', flush=True)

if __name__ == '__main__':
    main()
