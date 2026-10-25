#!/usr/bin/env python3
"""Compile the actual ring/service code with microfont; no kernel/hardware claim."""
from pathlib import Path
import os
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
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
    #[test] fn boot_and_usb_progress_replay_when_r8_tiles_become_ready() {
        let pixels = Box::leak(vec![BG; 127 * 91].into_boxed_slice());
        let surface = Surface { phys: 0x1000, gpu: 0xE0000000,
            virt: pixels.as_mut_ptr() as usize, byte_len: pixels.len()*4,
            width: 127, height: 91, pitch_bytes: 127*4 };
        crate::microfont_mask_service::READY.store(false,std::sync::atomic::Ordering::SeqCst);
        assert!(unsafe { install(surface) });
        assert!(pixels.iter().all(|&pixel|pixel==BG));
        crate::microfont_mask_service::READY.store(true,std::sync::atomic::Ordering::SeqCst);
        font_ready();
        assert!(pixels.iter().any(|&pixel| pixel != BG));
        let initial = pixels.to_vec();
        assignment_changed(true);
        assert_ne!(&pixels[..], &initial[..]);
        let assigned = pixels.to_vec();
        assignment_changed(false);
        assert_ne!(&pixels[..], &assigned[..]);
        let unassigned = pixels.to_vec();
        usb_stage("ECM: read MAC string");
        assert_ne!(&pixels[..], &unassigned[..]);
        let nic_progress = pixels.to_vec();
        usb_controller_stage("xHCI: another controller");
        assert_eq!(&pixels[..], &nic_progress[..]);
        assignment_changed(true);
        assert_ne!(&pixels[..], &unassigned[..]);
        crate::intel::take_flushes();
    }
    #[test] fn verdict_is_centered_fourfold_and_only_one_line() {
        for (assigned, text) in [(true, &b"Yes"[..]), (false, &b"No"[..])] {
            let width = 127; let height = 91; let pitch = width + 9;
            let guard = 0x13572468;
            let mut pixels = vec![guard; 16 + pitch * height + 16];
            let surface = Surface { phys: 0x1000, gpu: 0xE0000000,
                virt: unsafe { pixels.as_mut_ptr().add(16) } as usize,
                byte_len: pitch * height * 4, width: width as u32,
                height: height as u32, pitch_bytes: (pitch * 4) as u32 };
            assert!(surface.valid());
            unsafe { clear_surface(surface); }
            paint_verdict(surface, assigned);
            let left = (width - text.len() * microfont::FWIDTH * SCALE) / 2;
            let top = (height - microfont::FHEIGHT * SCALE) / 2;
            let mut expected = vec![BG; width * height];
            for (column, &byte) in text.iter().enumerate() {
                for y in 0..microfont::FHEIGHT * SCALE {
                    for x in 0..microfont::FWIDTH * SCALE {
                        expected[(top+y)*width + left+column*microfont::FWIDTH*SCALE+x] =
                            {
                                let a=u32::from(crate::microfont_mask_service::ready_atlas().unwrap().coverage(
                                    byte as char,x as u32,y as u32,(microfont::FWIDTH*SCALE) as u32,
                                    (microfont::FHEIGHT*SCALE) as u32));
                                0xff00_0000 | ((255-a)*0x010101)
                            };
                    }
                }
            }
            for y in 0..height {
                assert_eq!(&pixels[16+y*pitch..16+y*pitch+width], &expected[y*width..(y+1)*width]);
                assert!(pixels[16+y*pitch+width..16+(y+1)*pitch].iter().all(|&p| p == guard));
            }
            assert!(pixels[..16].iter().chain(pixels[16+pitch*height..].iter()).all(|&p| p == guard));
            assert_eq!(crate::intel::take_flushes(), vec![(surface.virt+top*surface.pitch_bytes as usize+left*4,
                text.len()*microfont::FWIDTH*SCALE*4, pitch*4, microfont::FHEIGHT*SCALE)]);
        }
    }
    #[test] fn too_small_surface_cannot_underflow_centering() {
        let base = Surface { phys: 0x1000, gpu: 0, virt: 0x1000,
            byte_len: 72*44*4, width: 72, height: 44, pitch_bytes: 72*4 };
        assert!(base.valid());
        assert!(!Surface { width: 71, ..base }.valid());
        assert!(!Surface { height: 43, ..base }.valid());
    }
}
'''
POLICY_HARNESS = r'''
mod allcaps {
    pub mod hv { pub const CODEX_EXECUTOR_EXPERIMENT_ENABLED: bool = false; }
    pub mod wls { pub const CPU_TRACK_COUNT: usize = 256; }
}
mod policy {
    const WORKER_SLOT_LIMIT: usize = 256;
    fn topology_core_slot_count() -> usize { 8 }
    fn is_background_worker_slot(slot: u32) -> bool { slot >= 2 }
    #[path = "../application_policy.rs"] mod application_policy;
    __FUNCTIONS__
    #[test] fn microfont_does_not_reserve_last_ap() {
        assert_eq!(last_ap_service_slot(), None);
        assert!(is_general_background_worker_slot(7));
        assert!(is_general_background_worker_slot(6));
        assert!(!is_general_background_worker_slot(0));
        assert!(!is_general_background_worker_slot(1));
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
    microfont_spawn = spawn[spawn.index('fn spawn_microfont_log('):spawn.index('fn ui4_slot4_service_gate(')]
    assert 'spawn_on_eff_worker' in microfont_spawn and 'worker.cpu_slot()' in microfont_spawn
    assert 'last_ap_service_worker' not in microfont_spawn
    assert 'microfont_log_service' not in workers[workers.index('pub fn last_ap_service_slot('):workers.index('pub fn is_last_ap_service_slot(')]
    assert 'microfont-log' in spawn and 'ui4_slot4_service_gate' in spawn
    assert 'if readback_ok {' in panel and 'microfont_log_service::install' in panel
    assert 'microfont_log_boot_surface' in display and 'screenlog::owns_plane' in display
    assert 'Timer::after' in service and 'from_millis(PERIOD_MS)' in service
    assert 'spin_loop' not in service and 'PAGE:' not in service
    assert 'copy_for_screen' not in service and 'fn paint_verdict' in service
    assert 'ASSIGNMENT_WAIT_MS' not in service and 'fn usb_stage' in service
    print('PASS: regular E-core-preferred placement, existing log ring, live USB status, native/UI4 handoff intact', flush=True)
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
// Mock only the already-produced R8 tiles; this suite checks the status sink.
mod microfont_mask_service {
    pub static READY:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(true);
    pub struct Atlas;
    pub fn ready_atlas()->Option<&'static Atlas> {
        READY.load(std::sync::atomic::Ordering::SeqCst).then_some(&Atlas)
    }
    impl Atlas {
        pub fn coverage(&self,ch:char,x:u32,y:u32,_:u32,_:u32)->u8 {
            if ch==' ' {0} else {((ch as u32+x+3*y)%4*85) as u8}
        }
    }
}

mod net { pub mod laptop_profile {
    pub fn assignment_result() -> Option<bool> { None }
} }
mod intel {
    static FLUSHES: std::sync::Mutex<Vec<(usize, usize, usize, usize)>> = std::sync::Mutex::new(Vec::new());
    pub fn dma_cache_flush_range(_: *const u8, _: usize) {}
    pub fn dma_flush_strided_rows(ptr: *mut u8, bytes: usize, stride: usize, rows: usize) -> bool {
        FLUSHES.lock().unwrap().push((ptr as usize, bytes, stride, rows));
        true
    }
    pub fn take_flushes() -> Vec<(usize, usize, usize, usize)> {
        std::mem::take(&mut *FLUSHES.lock().unwrap())
    }
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
microfont = { path = "__MICROFONT__", default-features = false, features = ["raster"] }
'''.replace('__MICROFONT__', str(ROOT/'vendor/microfont')))
        (work / 'policy').mkdir()
        (work / 'application_policy.rs').write_text((ROOT / 'src/workers/application_policy.rs').read_text())
        (work / 'service.rs').write_text(service + PEN_TESTS)
        (work / 'lib.rs').write_text(harness)
        env = dict(os.environ, RUSTUP_TOOLCHAIN='stable', RUST_MIN_STACK=str(16*1024*1024))
        subprocess.run(['cargo', 'test', '--offline', '--lib', '--manifest-path', str(work / 'Cargo.toml'), '--', '--test-threads=1'], cwd=work, env=env, check=True)
    print('PASS: actual ring, unreserved background selector and live USB stage and centered 4x Yes/No renderer tested (not a full kernel build)', flush=True)

if __name__ == '__main__':
    main()
