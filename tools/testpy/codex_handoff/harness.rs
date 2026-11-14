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
    #[derive(Clone, Copy)]
    pub struct SpiritSurfaceLayout {
        pub width: u32,
        pub height: u32,
        pub pitch_bytes: u32,
        pub byte_len: usize,
    }
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
    pub fn source(seq: u64) -> Option<codex_handoff::Source> {
        codex_handoff::source(lilly::resident_frame("Static_up/static.png")?, seq)
    }
}
mod ui4 {
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
    pub struct WindowInteraction {
        pub movable: bool,
        pub maximizable: bool,
        pub receives_input: bool,
        pub primary_activation: bool,
        pub hit_testable: bool,
        pub resize_on_maximize: bool,
    }
    #[derive(Clone, Copy)]
    pub struct WindowSnapshot {
        pub owner: WindowOwner,
        pub id: WindowId,
        pub frame: FrameHandle,
        pub placement: Placement,
        pub presentation_placement: Placement,
        pub interaction: WindowInteraction,
    }
    // Deliberately no frame-pool APIs: detached flight cannot allocate/publish
    // a window image. The only broker work is the lifecycle boundary.
    pub static QUERIES: crate::AtomicU64 = crate::AtomicU64::new(0);
    pub static PARKS: crate::AtomicU64 = crate::AtomicU64::new(0);
    pub static COMPOSITIONS: crate::AtomicU64 = crate::AtomicU64::new(0);
    pub mod cursor_frame_inout {
        pub fn frame_parked(_: super::WindowOwner, _: super::WindowId) {
            super::PARKS.fetch_add(1, crate::Ordering::SeqCst);
        }
    }
    pub mod input_broker {
        pub fn notify_slot4_visual_change() {}
    }
    pub fn codex_handoff_changed(_: WindowOwner, _: WindowId) {
        input_broker::notify_slot4_visual_change();
    }
    pub mod collapse_animation {
        pub static CANCEL_AT_TRANSFER: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        pub fn active_for(_: super::WindowOwner, window: super::WindowId) -> bool {
            if CANCEL_AT_TRANSFER.swap(false, crate::Ordering::SeqCst) {
                super::cancel(window.0);
            }
            crate::Instant::now().as_millis() < 240
        }
    }
    pub mod window_broker {
        use super::*;
        pub fn collapse_state(_: WindowOwner, _: WindowId) -> Option<bool> {
            QUERIES.fetch_add(1, crate::Ordering::SeqCst);
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
                placement: p,
                presentation_placement: p,
                interaction: WindowInteraction {
                    movable: true,
                    maximizable: true,
                    receives_input: true,
                    primary_activation: true,
                    hit_testable: true,
                    resize_on_maximize: true,
                },
            }))
        }
        pub fn collapse_animation_changed(_: WindowOwner, _: WindowId) {
            COMPOSITIONS.fetch_add(1, crate::Ordering::SeqCst);
        }
    }
    #[path = "@TRUEOS_ROOT@/src/ui4/codex_handoff.rs"]
    pub mod codex_handoff;
    pub fn queue(id: u32) {
        codex_handoff::queue(
            WindowOwner::Shell,
            WindowId(id),
            FrameHandle(9),
            crate::spirit::codex_handoff::geometry::Point { x: 100, y: 250 },
            233,
        );
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
fn detached_sprite_settles_and_flies_without_window_or_lilly_frame_updates() {
    use spirit::codex_handoff as a;
    time(0);
    ui4::queue(2);
    time(1000);
    ui4::advance();
    assert!(!a::active(), "other apps retain ordinarycollapse");
    time(0);
    ui4::queue(1);
    assert!(
        !ui4::snapshot(1).interaction.hit_testable,
        "selection disabled from request"
    );
    assert!(
        ui4::snapshot(1).placement.visible,
        "initial shrink may finish"
    );
    assert_eq!(ui4::PARKS.load(Ordering::SeqCst), 1);
    assert!(
        ui4::folded().is_some(),
        "circle restores even during shrink"
    );
    time(239);
    ui4::advance();
    assert!(!a::active());
    time(240);
    ui4::advance();
    assert!(a::needs_crossed());
    assert!(
        !ui4::snapshot(1).placement.visible,
        "no compact window during settlement"
    );
    assert_eq!(
        ui4::snapshot(1).frame,
        ui4::FrameHandle(9),
        "sprite never replaces producer frame"
    );
    assert_eq!(ui4::COMPOSITIONS.load(Ordering::SeqCst), 1);
    let queries = ui4::QUERIES.load(Ordering::SeqCst);
    for t in [240, 500, 739] {
        time(t);
        ui4::advance();
        assert_eq!(a::software_visual().unwrap().pose.icon.x, 100);
        assert!(spirit::source(1).is_none());
    }
    let mut positions = Vec::new();
    for t in [740, 756, 773, 790, 806, 823, 840, 856, 873, 890, 900] {
        time(t);
        ui4::advance();
        positions.push(a::software_visual().unwrap().pose.icon.x);
        assert!(
            spirit::source(1).is_none(),
            "outside plane needs no Lilly composition"
        );
    }
    assert!(
        positions.windows(2).all(|p| p[0] < p[1]),
        "60Hz software motion advances without cursor acknowledgements"
    );
    assert!(a::visual().is_none());
    assert_eq!(
        ui4::QUERIES.load(Ordering::SeqCst),
        queries,
        "flight does not query window state"
    );
    assert_eq!(
        ui4::COMPOSITIONS.load(Ordering::SeqCst),
        1,
        "flight does not dirty application compositor"
    );
    time(940);
    assert!(a::fast_updates());
    let last = a::software_visual().unwrap();
    let frame = spirit::source(10).unwrap();
    assert_eq!(
        frame.surface.phys, 1,
        "GPU receives the unmodified Lilly sprite"
    );
    let layout = spirit::SpiritSurfaceLayout {
        width: 256,
        height: 256,
        pitch_bytes: 1024,
        byte_len: 256 * 256 * 4,
    };
    let mut rendered = vec![0; layout.byte_len];
    for p in rendered.chunks_exact_mut(4) {
        p.copy_from_slice(&[20, 30, 40, 255]);
    }
    let before = rendered.clone();
    assert!(frame.stamp.unwrap().apply(&mut rendered, layout));
    assert_ne!(before, rendered, "logo appears after GPU shading");
    assert_eq!(
        &rendered[..4],
        &[20, 30, 40, 255],
        "Lilly VFX pixels away from logo survive"
    );
    assert_eq!(
        a::software_visual().unwrap(),
        last,
        "await first cursor latch at overlap"
    );
    time(956);
    spirit::source(11).unwrap();
    a::presented(10);
    assert!(a::visual().is_none(), "stale ack cannot move software half");
    a::presented(11);
    let overlap = a::software_visual().unwrap();
    assert!(overlap.pose.near_cursor(0));
    assert_eq!(overlap.opacity, 233);
    crate::CX.store(600, Ordering::SeqCst);
    time(2000);
    spirit::source(12).unwrap();
    a::presented(12);
    assert_eq!(
        a::visual().unwrap().pose.icon.x,
        713,
        "live plane destination"
    );
    assert!(
        !a::visual().unwrap().hidden,
        "wait for crossed arms before WishC"
    );
    a::crossed_ready();
    spirit::source(13).unwrap();
    a::presented(13);
    assert!(a::software_visual().is_none());
    time(2100);
    assert!(
        spirit::source(16).unwrap().stamp.is_none(),
        "authored ring frames use normal VFX without a native stamp"
    );
    time(4240);
    spirit::source(14).unwrap();
    a::presented(14);
    assert!(!a::active());
    assert!(
        !ui4::snapshot(1).placement.visible,
        "parked independently of ability completion"
    );
    ui4::queue(3);
    ui4::advance();
    assert!(a::active());
    assert!(
        !ui4::snapshot(1).placement.visible,
        "older fold stays parked"
    );
    ui4::cancel(3);
    assert!(!a::active());
    assert!(ui4::snapshot(3).placement.visible);
    ui4::cancel(1);
    assert!(ui4::snapshot(1).placement.visible);
    assert!(ui4::snapshot(1).interaction.hit_testable);
    assert!(ui4::folded().is_none());
    // Cancel an in-flight submission: its late acknowledgement cannot respawn.
    ui4::queue(4);
    ui4::advance();
    time(6000);
    let canceled_frame = spirit::source(15).unwrap();
    ui4::cancel(4);
    let mut canceled_pixels = before.clone();
    assert!(
        canceled_frame
            .stamp
            .unwrap()
            .apply(&mut canceled_pixels, layout)
    );
    assert_eq!(
        canceled_pixels, before,
        "late CPU pass skips a canceled logo"
    );
    a::presented(15);
    assert!(a::software_visual().is_none());
    assert!(a::visual().is_none());
    assert!(ui4::snapshot(4).placement.visible);
    // Restore can race the shrink-complete observation. The transfer must
    // recheck that its ticket is still owned before starting the sprite.
    ui4::queue(5);
    ui4::collapse_animation::CANCEL_AT_TRANSFER.store(true, Ordering::SeqCst);
    ui4::advance();
    assert!(!a::active());
    assert!(ui4::folded().is_none());
    assert!(ui4::snapshot(5).placement.visible);
}
