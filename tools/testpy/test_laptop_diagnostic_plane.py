#!/usr/bin/env python3
"""Exercise the complete font-free diagnostic writer with guarded CPU surfaces.

No font, atlas or glyph modules are supplied to this harness. Only retained
storage, the clock and DMA flush recording are adapters; production geometry,
pixel writes, callbacks and installation/ownership execute unchanged.
"""

from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]

ADAPTERS = r'''
#![allow(dead_code)]
extern crate self as spin;
extern crate self as trueos_time;
use std::sync::{Mutex as StdMutex, OnceLock};
use std::sync::atomic::{AtomicU64, Ordering};
pub struct Once<T>(OnceLock<T>);
impl<T> Once<T> {
    pub const fn new() -> Self { Self(OnceLock::new()) }
    pub fn get(&self) -> Option<&T> { self.0.get() }
    pub fn call_once(&self, f: impl FnOnce() -> T) -> &T { self.0.get_or_init(f) }
}
pub struct Mutex<T>(StdMutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(StdMutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_,T> { self.0.lock().unwrap() }
}
static CLOCK: AtomicU64 = AtomicU64::new(0);
pub struct Instant(u64);
impl Instant {
    pub fn now() -> Self { Self(CLOCK.load(Ordering::Relaxed)) }
    pub fn as_millis(&self) -> u64 { self.0 }
}
type Flush = (usize,usize,usize,usize);
static FLUSHES: StdMutex<Vec<Flush>> = StdMutex::new(Vec::new());
mod intel {
    pub fn dma_flush_strided_rows(ptr: *mut u8, width: usize, pitch: usize, rows: usize) -> bool {
        crate::FLUSHES.lock().unwrap().push((ptr as usize,width,pitch,rows));
        true
    }
}
'''

TESTS = r'''
#[cfg(test)] mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    const GUARD: u32 = 0x1234_5678;
    const GREEN: u32 = 0xff00_ff00;
    fn allocation(width: usize, height: usize, padding: usize) -> (Vec<u32>, Surface) {
        let pitch = width + padding;
        let mut pixels = vec![GUARD; pitch * height + 2];
        let surface = Surface { phys: 4096, gpu: 0xE000_0000,
            virt: unsafe { pixels.as_mut_ptr().add(1) } as usize,
            byte_len: pitch * height * 4, width: width as u32, height: height as u32,
            pitch_bytes: (pitch * 4) as u32 };
        (pixels,surface)
    }
    fn flush(surface: Surface, rect: Rect) -> crate::Flush {
        (surface.virt + rect.top * surface.pitch_bytes as usize + rect.left * 4,
         rect.width * 4, surface.pitch_bytes as usize, rect.height)
    }
    fn reset_markers() {
        WIFI_COLOR.store(GREEN,Ordering::Relaxed);
        WIFI_IP_OCTET.store(0b1010_0101,Ordering::Relaxed);
        WIFI_ATTEMPT_MS.store(u64::MAX,Ordering::Relaxed);
        crate::CLOCK.store(1000,Ordering::Relaxed);
        crate::FLUSHES.lock().unwrap().clear();
    }
    fn contains(rect: Rect, x: usize, y: usize) -> bool {
        (rect.left..rect.left + rect.width).contains(&x)
            && (rect.top..rect.top + rect.height).contains(&y)
    }

    #[test]
    fn geometry_centers_the_complete_group_and_rejects_too_small_surfaces() {
        assert_eq!(diagnostic_geometry(1024,128),Some(DiagnosticGeometry {
            square: Rect { left:480, top:20, width:64, height:64 },
            ip: Rect { left:448, top:92, width:128, height:16 },
        }));
        for (width,height) in [(128,88),(513,129),(1024,128),(1025,129),(usize::MAX,usize::MAX)] {
            let g = diagnostic_geometry(width,height).unwrap();
            assert!(g.square.left * 2 + g.square.width <= width);
            assert!(width - (g.square.left * 2 + g.square.width) <= 1);
            assert!(width - (g.ip.left * 2 + g.ip.width) <= 1);
            assert_eq!(g.ip.top - (g.square.top + g.square.height),8);
            let group_bottom = g.ip.top + g.ip.height;
            assert!(height - group_bottom - g.square.top <= 1);
        }
        for (width,height) in [(0,0),(127,128),(1024,87)] {
            assert_eq!(diagnostic_geometry(width,height),None);
        }
    }

    #[test]
    fn repaint_has_only_centered_markers_and_preserves_guards_and_padding() {
        for (width,height,padding) in [(128,88,0),(513,129,7),(1024,128,16)] {
            reset_markers();
            let (pixels,surface) = allocation(width,height,padding);
            assert!(surface.valid());
            repaint(surface);
            let g = diagnostic_geometry(width,height).unwrap();
            let pitch = width + padding;
            let bits = [true,false,true,false,false,true,false,true];
            for y in 0..height { for x in 0..pitch {
                let expected = if x >= width { GUARD }
                    else if contains(g.square,x,y) { GREEN }
                    else if contains(g.ip,x,y) {
                        let local_x = x - g.ip.left;
                        let local_y = y - g.ip.top;
                        let black = (7..10).contains(&(local_x % 16))
                            && if bits[local_x / 16] { (2..14).contains(&local_y) }
                               else { (11..14).contains(&local_y) };
                        if black { 0xff00_0000 } else { 0xffff_ffff }
                    } else { 0 };
                assert_eq!(pixels[1 + y * pitch + x],expected,"pixel {x},{y} on {width}x{height}");
            }}
            assert_eq!(pixels[0],GUARD);
            assert_eq!(*pixels.last().unwrap(),GUARD);
            assert_eq!(*crate::FLUSHES.lock().unwrap(),
                [flush(surface,Rect { left:0,top:0,width,height })]);
        }
    }

    #[test]
    fn partial_updates_write_and_flush_only_their_own_rectangles() {
        reset_markers();
        let (pixels,surface) = allocation(1024,128,16);
        let geometry = diagnostic_geometry(1024,128).unwrap();
        paint_wifi_square(surface);
        assert_eq!(*crate::FLUSHES.lock().unwrap(),[flush(surface,geometry.square)]);
        for y in 0..128 { for x in 0..1040 {
            assert_eq!(pixels[1 + y * 1040 + x],
                if contains(geometry.square,x,y) { GREEN } else { GUARD });
        }}
        crate::FLUSHES.lock().unwrap().clear();
        WIFI_IP_OCTET.store(256,Ordering::Relaxed);
        paint_wifi_ip(surface);
        assert_eq!(*crate::FLUSHES.lock().unwrap(),[flush(surface,geometry.ip)]);
        for y in 0..128 { for x in 0..1040 {
            let expected = if contains(geometry.square,x,y) { GREEN }
                else if contains(geometry.ip,x,y) { 0xffff_ffff } else { GUARD };
            assert_eq!(pixels[1 + y * 1040 + x],expected);
        }}
        assert_eq!(pixels[0],GUARD);
        assert_eq!(*pixels.last().unwrap(),GUARD);
    }

    #[test]
    fn surface_validation_rejects_unsafe_or_inadequate_handoffs() {
        let (_pixels,surface) = allocation(1024,128,16);
        assert!(surface.valid());
        for bad in [Surface { virt:0,..surface },Surface { virt:surface.virt+1,..surface },
            Surface { phys:0,..surface },Surface { phys:surface.phys+1,..surface },
            Surface { gpu:surface.gpu+1,..surface },Surface { pitch_bytes:4095,..surface },
            Surface { byte_len:surface.byte_len-1,..surface },
            Surface { byte_len:usize::MAX,..surface },Surface { virt:usize::MAX-3,..surface },
            Surface { width:127,..surface },Surface { height:87,..surface }] {
            assert!(!bad.valid(),"{bad:?}");
            assert!(!unsafe { install(bad) });
        }
    }

    #[test]
    fn callbacks_retain_marker_state_without_fonts_and_own_only_diagnostics() {
        reset_markers();
        let (pixels,surface) = allocation(1024,128,16);
        let pixels = Box::leak(pixels.into_boxed_slice());
        assert_eq!(surface.virt,unsafe { pixels.as_mut_ptr().add(1) } as usize);
        assert!(unsafe { install(surface) });
        assert!(unsafe { install(surface) },"identical handoff is idempotent");
        assert!(!unsafe { install(Surface { gpu:0xE400_0000,..surface }) });
        assert!(owns_plane(0,6));
        assert!(!owns_plane(0,4));
        assert!(!owns_plane(1,6));
        crate::FLUSHES.lock().unwrap().clear();
        let before = pixels.to_vec();
        assignment_changed(true); usb_stage("USB stalled");
        usb_controller_stage("other controller"); font_ready();
        assert_eq!(&*pixels,before.as_slice());
        assert!(crate::FLUSHES.lock().unwrap().is_empty());
        wifi_color_changed(0xffff_0000);
        let g = diagnostic_geometry(1024,128).unwrap();
        assert_eq!(*crate::FLUSHES.lock().unwrap(),[flush(surface,g.square)]);
        crate::FLUSHES.lock().unwrap().clear();
        wifi_ipv4_changed(Some([192,168,178,66]));
        assert_eq!(*crate::FLUSHES.lock().unwrap(),[flush(surface,g.ip)]);
        assert_eq!(WIFI_IP_OCTET.load(Ordering::Relaxed),66);
        wifi_retry_started();
        wifi_color_changed(0xffff_0000);
        assert_eq!(WIFI_COLOR.load(Ordering::Relaxed),0xff00_00ff);
        crate::CLOCK.store(1500,Ordering::Relaxed);
        wifi_color_changed(0xffff_0000);
        assert_eq!(WIFI_COLOR.load(Ordering::Relaxed),0xffff_0000);
    }
}
'''


def main() -> None:
    implementation = (ROOT / "src/r/services/microfont_log_service.rs").read_text()
    source = ADAPTERS + "\nmod diagnostic {\n" + implementation + TESTS + "\n}\n"
    with tempfile.TemporaryDirectory(prefix="trueos-laptop-diagnostics-") as directory:
        code = Path(directory) / "tests.rs"
        binary = Path(directory) / "tests"
        code.write_text(source)
        subprocess.run([
            "rustc", "--edition=2024", "--target=x86_64-unknown-linux-gnu", "--test",
            "-C", "linker=" + str(ROOT / "tools/rust-host-ld"), str(code), "-o", str(binary),
        ], cwd=ROOT, check=True)
        subprocess.run([str(binary), "--test-threads=1"], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
