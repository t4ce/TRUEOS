#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_time;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
static CLOCK: AtomicU64 = AtomicU64::new(0);
static CX: AtomicI32 = AtomicI32::new(500);
static CY: AtomicI32 = AtomicI32::new(300);
#[derive(Clone, Copy)]
pub struct Instant(u64);
impl Instant {
    pub fn now() -> Self {
        Self(CLOCK.load(Ordering::SeqCst))
    }
    pub fn as_millis(self) -> u64 {
        self.0
    }
}
fn time(t: u64) {
    CLOCK.store(t, Ordering::SeqCst);
}
mod hv {
    pub fn vm_run_generation(_: u8) -> Option<u64> {
        Some(1)
    }
    pub fn blueprint_current_run_archive(vm: u8, _: u64) -> Option<String> {
        Some(if vm == 7 { "codex.bp" } else { "other.bp" }.into())
    }
}
mod shell3 {
    pub mod tui {
        pub fn vm_for_window(w: crate::ui4::WindowId) -> Option<u8> {
            Some(if w.0 == 2 { 8 } else { 7 })
        }
    }
}
mod gpu {
    pub mod resident {
        pub struct ResidentDmaBuffer {
            data: spin::Mutex<Vec<u8>>,
        }
        impl ResidentDmaBuffer {
            pub fn allocate_zeroed(n: usize, _: usize) -> Option<Self> {
                Some(Self {
                    data: spin::Mutex::new(vec![0; n]),
                })
            }
            pub fn write(&self, offset: usize, s: &[u8]) -> bool {
                let mut d = self.data.lock();
                d[offset..offset + s.len()].copy_from_slice(s);
                true
            }
            pub fn flush(&self) {}
            pub fn phys(&self) -> u64 {
                0x2000_0000
            }
        }
    }
}
mod intel {
    pub fn dma_flush(_: *mut u8, _: usize) {}
}
mod spirit {
    pub fn marker_anchor() -> Option<(u32, u32, i32, i32, u32, u32)> {
        Some((
            1920,
            1080,
            crate::CX.load(crate::Ordering::SeqCst),
            crate::CY.load(crate::Ordering::SeqCst),
            256,
            256,
        ))
    }
    pub mod gpu_logger {
        pub fn active_snapshot() -> Option<()> {
            None
        }
    }
    pub mod lilly {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct LillyResidentFrame {
            pub phys: u64,
            pub gpu: u64,
            pub bytes: usize,
            pub width: u32,
            pub height: u32,
            pub pitch_bytes: u32,
        }
        pub fn resident_frame(name: &str) -> Option<LillyResidentFrame> {
            Some(LillyResidentFrame {
                phys: if name == "Static_up/static.png" { 1 } else { 2 },
                gpu: 0,
                bytes: 256 * 256 * 4,
                width: 256,
                height: 256,
                pitch_bytes: 1024,
            })
        }
        pub fn copy_frame(_: LillyResidentFrame, dst: &mut [u8]) -> bool {
            for p in dst.chunks_exact_mut(4) {
                p.copy_from_slice(&[80, 60, 100, 255]);
            }
            true
        }
    }
    #[path = "@TRUEOS_ROOT@/src/spirit/codex_handoff.rs"]
    pub mod codex_handoff;
    pub fn source(seq: u64) -> Option<lilly::LillyResidentFrame> {
        codex_handoff::source(lilly::resident_frame("Static_up/static.png")?, seq)
    }
}
mod ui4 {
    use std::collections::BTreeMap;
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum WindowOwner {
        Vm(u8),
        Shell,
        Other,
    }
    impl WindowOwner {
        pub const SHELL3_SERVICE: Self = Self::Shell;
    }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct WindowId(pub u32);
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct FrameHandle(pub u64);
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct Placement {
        pub x: i32,
        pub y: i32,
        pub visible: bool,
        pub opacity: u8,
    }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum FrameBuffering {
        Single,
    }
    #[derive(Clone, Copy)]
    pub struct WindowSnapshot {
        pub owner: WindowOwner,
        pub id: WindowId,
        pub frame: FrameHandle,
        pub output: (),
        pub placement: Placement,
        pub presentation_placement: Placement,
        pub buffering: FrameBuffering,
    }
    pub enum FrameContent {
        Image,
    }
    pub enum FrameCadence {
        Immutable,
    }
    pub enum ScanoutFormat {
        Rgba8888Premultiplied,
    }
    pub struct FrameSpec {
        pub output: (),
        pub content: FrameContent,
        pub cadence: FrameCadence,
        pub buffering: FrameBuffering,
        pub format: ScanoutFormat,
        pub width: u32,
        pub height: u32,
        pub base_color: Option<()>,
    }
    pub struct View {
        pub virt: *mut u8,
        pub byte_len: usize,
        pub pitch: u32,
    }
    static NEXT: crate::AtomicU64 = crate::AtomicU64::new(100);
    static FRAMES: spin::Mutex<BTreeMap<u64, Box<[u8]>>> = spin::Mutex::new(BTreeMap::new());
    pub fn create_frame(s: FrameSpec) -> Result<FrameHandle, ()> {
        assert_eq!((s.width, s.height), (64, 64));
        let n = NEXT.fetch_add(1, crate::Ordering::SeqCst);
        FRAMES.lock().insert(n, vec![0; 16384].into_boxed_slice());
        Ok(FrameHandle(n))
    }
    pub fn acquire_frame_buffer(f: FrameHandle) -> Result<FrameHandle, ()> {
        Ok(f)
    }
    pub fn writable_rgba_view(f: FrameHandle) -> Result<View, ()> {
        let mut frames = FRAMES.lock();
        let data = frames.get_mut(&f.0).ok_or(())?;
        Ok(View {
            virt: data.as_mut_ptr(),
            byte_len: data.len(),
            pitch: 256,
        })
    }
    pub fn publish_frame_buffer(_: FrameHandle) -> Result<(), ()> {
        Ok(())
    }
    pub fn cancel_frame_buffer(_: FrameHandle) -> Result<(), ()> {
        Ok(())
    }
    pub fn codex_handoff_changed(_: WindowOwner, _: WindowId) {}
    pub mod collapse_animation {
        pub fn active_for(_: super::WindowOwner, _: super::WindowId) -> bool {
            crate::Instant::now().as_millis() < 240
        }
    }
    pub mod window_broker {
        use super::*;
        pub fn collapse_state(_: WindowOwner, _: WindowId) -> Option<bool> {
            Some(true)
        }
        pub fn window_snapshot(owner: WindowOwner, id: WindowId) -> Option<WindowSnapshot> {
            let p = Placement {
                x: 100,
                y: 250,
                visible: true,
                opacity: 233,
            };
            Some(codex_handoff::project(WindowSnapshot {
                owner,
                id,
                frame: FrameHandle(9),
                output: (),
                placement: p,
                presentation_placement: p,
                buffering: FrameBuffering::Single,
            }))
        }
        pub fn retire_frame_when_released(_: FrameHandle) {}
        pub fn collapse_animation_changed(_: WindowOwner, _: WindowId) {}
    }
    #[path = "@TRUEOS_ROOT@/src/ui4/codex_handoff.rs"]
    pub mod codex_handoff;
    pub fn queue(id: u32) {
        codex_handoff::queue(WindowOwner::Shell, WindowId(id), FrameHandle(9));
    }
    pub fn advance() {
        codex_handoff::advance();
    }
    pub fn cancel(id: u32) {
        codex_handoff::cancel(WindowOwner::Shell, WindowId(id));
    }
    pub fn snapshot(id: u32) -> WindowSnapshot {
        window_broker::window_snapshot(WindowOwner::Shell, WindowId(id)).unwrap()
    }
    pub fn folded() -> Option<(WindowOwner, WindowId)> {
        codex_handoff::folded_target()
    }
}
#[test]
fn production_arrival_delay_ownership_ack_crossed_gate_wish_and_cancellation() {
    use spirit::codex_handoff as a;
    time(0);
    ui4::queue(2);
    time(1000);
    ui4::advance();
    assert!(!a::active(), "other app must never enter");
    time(0);
    ui4::queue(1);
    time(239);
    ui4::advance();
    assert!(!a::active());
    time(240);
    ui4::advance();
    time(739);
    ui4::advance();
    assert!(!a::active());
    time(740);
    ui4::advance();
    assert!(a::needs_crossed());
    assert!(
        ui4::folded().is_none(),
        "circle must not restore an icon still outside Lilly"
    );
    assert!(spirit::source(1).is_some());
    assert!(a::visual().is_none());
    time(900);
    assert!(spirit::source(2).is_some());
    a::presented(1);
    assert!(a::visual().is_none(), "stale cursor ack must not move UI4");
    a::presented(2);
    ui4::advance();
    assert!(a::visual().is_some());
    assert_ne!(ui4::snapshot(1).frame, ui4::FrameHandle(9));
    assert_eq!(a::visual().unwrap().opacity, 233);
    assert_eq!(
        ui4::snapshot(1).placement.opacity,
        255,
        "shared pixel alpha already includes window opacity"
    );
    crate::CX.store(600, Ordering::SeqCst);
    time(2000);
    spirit::source(3).unwrap();
    a::presented(3);
    ui4::advance();
    assert_eq!(
        a::visual().unwrap().pose.icon.x,
        713,
        "destination follows live Lilly"
    );
    assert!(
        !a::visual().unwrap().hidden,
        "arrival must await crossed static"
    );
    a::crossed_ready();
    spirit::source(4).unwrap();
    a::presented(4);
    ui4::advance();
    assert!(!ui4::snapshot(1).placement.visible);
    assert_eq!(
        ui4::folded(),
        Some((ui4::WindowOwner::Shell, ui4::WindowId(1)))
    );
    time(2000 + 2240);
    spirit::source(5).unwrap();
    a::presented(5);
    ui4::advance();
    assert!(!a::needs_crossed());
    assert!(!ui4::snapshot(1).placement.visible);
    ui4::queue(3);
    time(4240);
    ui4::advance();
    time(4740);
    ui4::advance();
    assert!(a::active());
    spirit::source(6).unwrap();
    a::presented(6);
    ui4::advance();
    assert!(
        !ui4::snapshot(1).placement.visible,
        "prior absorbed window stays absorbed"
    );
    ui4::cancel(3);
    assert!(!a::active());
    assert!(ui4::snapshot(3).placement.visible);
    ui4::cancel(1);
    assert!(
        ui4::snapshot(1).placement.visible,
        "restore exposes original icon"
    );
    assert!(ui4::folded().is_none());
}
