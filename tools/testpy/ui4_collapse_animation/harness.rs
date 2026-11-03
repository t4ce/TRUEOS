#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_time;
use core::sync::atomic::{AtomicU64, Ordering};

static NOW: AtomicU64 = AtomicU64::new(0);
pub struct Instant(u64);
impl Instant {
    pub fn now() -> Self { Self(NOW.load(Ordering::Relaxed)) }
    pub fn as_millis(&self) -> u64 { self.0 }
}

mod intel {
    #[derive(Clone, Copy)] pub struct GucBcs0RgbaSurface {
        pub phys: u64, pub gpu: u64, pub bytes: usize, pub width: u32, pub height: u32, pub pitch_bytes: u32,
    }
    #[derive(Clone, Copy)] pub struct GucBcs0RgbaCopy {
        pub source: GucBcs0RgbaSurface, pub source_x: u32, pub source_y: u32,
        pub destination_x: u32, pub destination_y: u32, pub width: u32, pub height: u32,
    }
    #[derive(Clone, Copy)] pub struct GucBcs0CopySubmission;
    #[derive(Clone, Copy)] pub enum GucBcs0CopyCompletion { Pending, Complete, Failed, InvalidSubmission }
    #[derive(Clone, Copy)] pub enum GucBcs0CopySubmitError { Busy, Unavailable, SubmitFailed, InvalidRequest }
    pub fn dma_flush(_: *mut u8, _: usize) {}
}

mod r { pub mod services { pub mod vcpy_service {
    use crate::intel::*;
    use core::sync::atomic::{AtomicUsize, Ordering};
    // Queue mode: busy, success, ambiguous failure, unavailable.
    pub static QUEUE: AtomicUsize = AtomicUsize::new(0);
    // Completion mode: pending, complete, failure.
    pub static POLL: AtomicUsize = AtomicUsize::new(0);
    pub static COPIES: AtomicUsize = AtomicUsize::new(0);
    pub fn queue_rgba_copies_with_scanout_cache(destination: GucBcs0RgbaSurface,
        copies: &[GucBcs0RgbaCopy], cache: &[bool]) -> Result<GucBcs0CopySubmission, GucBcs0CopySubmitError>
    {
        match QUEUE.load(Ordering::Relaxed) {
            0 => return Err(GucBcs0CopySubmitError::Busy),
            2 => return Err(GucBcs0CopySubmitError::SubmitFailed),
            3 => return Err(GucBcs0CopySubmitError::Unavailable),
            _ => {}
        }
        assert_eq!(cache.len(), copies.len());
        COPIES.fetch_add(copies.len(), Ordering::Relaxed);
        for copy in copies {
            for row in 0..copy.height as usize {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        (copy.source.phys as *const u8).add((copy.source_y as usize + row) * copy.source.pitch_bytes as usize + copy.source_x as usize * 4),
                        (destination.phys as *mut u8).add((copy.destination_y as usize + row) * destination.pitch_bytes as usize + copy.destination_x as usize * 4),
                        copy.width as usize * 4,
                    );
                }
            }
        }
        Ok(GucBcs0CopySubmission)
    }
    pub fn poll_rgba_copies(_: GucBcs0CopySubmission) -> GucBcs0CopyCompletion {
        match POLL.load(Ordering::Relaxed) {
            0 => GucBcs0CopyCompletion::Pending,
            1 => GucBcs0CopyCompletion::Complete,
            _ => GucBcs0CopyCompletion::Failed,
        }
    }
} } }

mod ui4 {
    #[cfg(test)] use super::*;
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct FrameHandle(pub u64);
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct WindowId(pub u32);
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct WindowOwner(pub u32);
    #[derive(Clone, Copy)] pub struct OutputId;
    #[derive(Clone, Copy)] pub enum FrameBuffering { Single, Double }
    #[derive(Clone, Copy)] pub enum FrameCadence { Immutable }
    #[derive(Clone, Copy)] pub enum FrameContent { Image }
    #[derive(Clone, Copy)] pub enum ScanoutFormat { Rgba8888Premultiplied }
    pub struct FrameSpec {
        pub output: OutputId, pub content: FrameContent, pub cadence: FrameCadence,
        pub buffering: FrameBuffering, pub format: ScanoutFormat, pub width: u32,
        pub height: u32, pub base_color: Option<()>,
    }
    #[derive(Clone, Copy)] pub struct FrameReadLease { pub frame: FrameHandle }
    #[derive(Clone, Copy)] pub struct FrameWriteLease { pub frame: FrameHandle }
    #[derive(Clone, Copy)] pub struct Release;
    impl Release { pub fn matches(self, _: u64, _: usize) -> bool { true } }
    #[derive(Clone, Copy)] pub struct FrameRgbaView {
        pub phys: u64, pub gpu: u64, pub virt: *mut u8, pub byte_len: usize,
        pub width: u32, pub height: u32, pub pitch: u32,
        pub gpu_authored: bool, pub gpu_release: Option<Release>,
    }
    unsafe impl Send for FrameRgbaView {}
    unsafe impl Sync for FrameRgbaView {}
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct WindowPlacement {
        pub x: i32, pub y: i32, pub width: u32, pub height: u32,
        pub visible: bool, pub opacity: u8, pub z: i32,
    }
    #[derive(Clone, Copy)] pub struct WindowBackground { pub frame: FrameHandle, pub opacity: u8 }
    #[derive(Clone, Copy)] pub struct Interaction { pub resize_on_maximize: bool }
    #[derive(Clone, Copy)] pub struct WindowSnapshot {
        pub owner: WindowOwner, pub id: WindowId, pub frame: FrameHandle, pub background: Option<WindowBackground>,
        pub output: OutputId, pub arc: u16, pub buffering: FrameBuffering,
        pub interaction: Interaction,
        pub placement: WindowPlacement, pub presentation_placement: WindowPlacement,
    }
    #[derive(Debug)] pub enum FramePoolError { Invalid, Busy }
    #[derive(Debug)] pub enum CollapseError { Ui4, Busy }
    impl From<FramePoolError> for CollapseError { fn from(_: FramePoolError) -> Self { Self::Ui4 } }
    pub struct StoredFrame {
        pub width: u32, pub height: u32, pub pixels: Vec<u8>, pub pitch: usize,
        pub readers: usize, pub writer: bool, pub published: bool,
        pub gpu_authored: bool, pub retired: bool,
    }
    static FRAMES: spin::Mutex<Vec<StoredFrame>> = spin::Mutex::new(Vec::new());
    pub fn create_frame(spec: FrameSpec) -> Result<FrameHandle, FramePoolError> {
        let mut frames = FRAMES.lock();
        let pitch = spec.width as usize * 4 + 16;
        frames.push(StoredFrame { width: spec.width, height: spec.height,
            pixels: vec![0; pitch * spec.height as usize], pitch, readers: 0,
            writer: false, published: false, gpu_authored: false, retired: false });
        Ok(FrameHandle(frames.len() as u64))
    }
    pub fn acquire_frame_buffer(frame: FrameHandle) -> Result<FrameWriteLease, FramePoolError> {
        let mut frames = FRAMES.lock();
        let stored = &mut frames[frame.0 as usize - 1];
        assert!(!stored.published && !stored.writer && !stored.retired);
        stored.writer = true;
        Ok(FrameWriteLease { frame })
    }
    pub fn writable_rgba_view(lease: FrameWriteLease) -> Result<FrameRgbaView, FramePoolError> { view(lease.frame) }
    fn view(frame: FrameHandle) -> Result<FrameRgbaView, FramePoolError> {
        let mut frames = FRAMES.lock();
        let stored = &mut frames[frame.0 as usize - 1];
        assert!(!stored.retired);
        let virt = stored.pixels.as_mut_ptr();
        Ok(FrameRgbaView { phys: virt as u64, gpu: virt as u64, virt,
            byte_len: stored.pixels.len(), width: stored.width, height: stored.height,
            pitch: stored.pitch as u32, gpu_authored: stored.gpu_authored,
            gpu_release: stored.gpu_authored.then_some(Release) })
    }
    pub fn publish_frame_buffer(lease: FrameWriteLease) -> Result<(), FramePoolError> {
        let mut frames = FRAMES.lock();
        let stored = &mut frames[lease.frame.0 as usize - 1];
        assert!(stored.writer);
        stored.writer = false;
        stored.published = true;
        Ok(())
    }
    pub fn cancel_frame_buffer(lease: FrameWriteLease) -> Result<(), FramePoolError> {
        let mut frames = FRAMES.lock();
        let stored = &mut frames[lease.frame.0 as usize - 1];
        assert!(stored.writer);
        stored.writer = false;
        Ok(())
    }
    pub fn acquire_published_frame(frame: FrameHandle) -> Result<FrameReadLease, FramePoolError> {
        let mut frames = FRAMES.lock();
        let stored = &mut frames[frame.0 as usize - 1];
        assert!(stored.published && !stored.retired);
        stored.readers += 1;
        Ok(FrameReadLease { frame })
    }
    pub fn published_rgba_view(lease: FrameReadLease) -> Result<FrameRgbaView, FramePoolError> { view(lease.frame) }
    pub fn release_published_frame(lease: FrameReadLease) -> Result<(), FramePoolError> {
        let mut frames = FRAMES.lock();
        let stored = &mut frames[lease.frame.0 as usize - 1];
        assert!(stored.readers > 0);
        stored.readers -= 1;
        Ok(())
    }
    pub fn output_dimensions() -> Option<(u32, u32)> { Some((1920, 1080)) }
    pub mod compositor_service {
        pub fn composition_source_uses_scanout_cache(_: &super::WindowSnapshot, view: super::FrameRgbaView) -> bool { view.gpu_authored }
    }
    pub mod collapse { pub const COLLAPSED_SIZE: u32 = 64; }
    pub mod window_broker {
        use super::*;
        pub fn retire_frame_when_released(frame: FrameHandle) {
            let mut frames = FRAMES.lock();
            let stored = &mut frames[frame.0 as usize - 1];
            assert_eq!(stored.readers, 0);
            assert!(!stored.writer);
            stored.retired = true;
        }
        pub fn collapse_animation_changed(_: WindowOwner, _: WindowId) {
            // Re-entering proves notifications do not retain the animation mutex.
            let _ = super::collapse_animation::active();
        }
    }
    #[path = "@TRUEOS_ROOT@/src/ui4/window_arc.rs"] mod window_arc;
    #[path = "@TRUEOS_ROOT@/src/ui4/collapse_animation.rs"] mod collapse_animation;

    #[cfg(test)] mod tests {
        use super::*;
        use crate::r::services::vcpy_service as bcs;
        fn frame(width: u32, height: u32, color: [u8; 4], gpu: bool) -> FrameHandle {
            let frame = create_frame(FrameSpec { output: OutputId, content: FrameContent::Image,
                cadence: FrameCadence::Immutable, buffering: FrameBuffering::Single,
                format: ScanoutFormat::Rgba8888Premultiplied, width, height, base_color: None }).unwrap();
            let lease = acquire_frame_buffer(frame).unwrap();
            let view = writable_rgba_view(lease).unwrap();
            for y in 0..height as usize { for x in 0..width as usize {
                unsafe { core::ptr::copy_nonoverlapping(color.as_ptr(), view.virt.add(y * view.pitch as usize + x * 4), 4); }
            }}
            publish_frame_buffer(lease).unwrap();
            FRAMES.lock()[frame.0 as usize - 1].gpu_authored = gpu;
            frame
        }
        fn windows(gpu: bool) -> (WindowSnapshot, WindowSnapshot) {
            NOW.store(100, Ordering::Relaxed);
            bcs::QUEUE.store(0, Ordering::Relaxed);
            bcs::POLL.store(0, Ordering::Relaxed);
            let placement = WindowPlacement { x: 100, y: 100, width: 128, height: 80, visible: true, opacity: 255, z: 1 };
            let source = WindowSnapshot { owner: WindowOwner(1), id: WindowId(1),
                frame: frame(128, 80, [100, 0, 0, 128], gpu), background: None,
                output: OutputId, arc: 0, buffering: FrameBuffering::Double,
                interaction: Interaction { resize_on_maximize: true },
                placement, presentation_placement: placement };
            let placement = WindowPlacement { x: 600, y: 700, width: 64, height: 64, ..placement };
            let icon = WindowSnapshot { frame: frame(64, 64, [0, 80, 0, 255], false),
                placement, presentation_placement: placement, ..source };
            (source, icon)
        }
        fn center_pixel(frame: FrameHandle) -> [u8; 4] {
            let frames = FRAMES.lock();
            let stored = &frames[frame.0 as usize - 1];
            let offset = (stored.height as usize / 2) * stored.pitch + (stored.width as usize / 2) * 4;
            stored.pixels[offset..offset + 4].try_into().unwrap()
        }
        fn readers(frame: FrameHandle) -> usize { FRAMES.lock()[frame.0 as usize - 1].readers }

        #[test] fn cpu_snapshot_is_frozen_and_completes_without_a_producer_resize() {
            let (source, icon) = windows(false);
            collapse_animation::begin(source, icon).unwrap();
            collapse_animation::advance(); // BCS busy: exact CPU snapshot fallback.
            assert_eq!(readers(source.frame), 0);
            let first = collapse_animation::project(icon);
            assert_ne!(first.frame, source.frame);
            assert_eq!(first.placement, icon.placement);
            assert_eq!(center_pixel(first.frame), [100, 0, 0, 128]);
            FRAMES.lock()[source.frame.0 as usize - 1].pixels.fill(0);
            NOW.store(220, Ordering::Relaxed);
            collapse_animation::advance();
            let next = collapse_animation::project(icon);
            assert_eq!(center_pixel(next.frame), [100, 0, 0, 128]);
            assert_eq!(next.placement, icon.placement);
            NOW.store(340, Ordering::Relaxed);
            collapse_animation::advance();
            assert!(!collapse_animation::active());
            assert_eq!(collapse_animation::project(icon).frame, icon.frame);
        }

        #[test] fn cancel_waits_for_submitted_gpu_copy_and_hides_the_old_effect() {
            let (source, icon) = windows(true);
            bcs::QUEUE.store(1, Ordering::Relaxed);
            collapse_animation::begin(source, icon).unwrap();
            collapse_animation::advance();
            assert_eq!(readers(source.frame), 1);
            collapse_animation::cancel(source.owner, source.id);
            assert!(collapse_animation::active());
            assert_eq!(collapse_animation::project(icon).frame, icon.frame);
            assert_eq!(readers(source.frame), 1);
            bcs::POLL.store(1, Ordering::Relaxed);
            collapse_animation::advance();
            assert!(!collapse_animation::active());
            assert_eq!(readers(source.frame), 0);
        }

        #[test] fn a_busy_gpu_capture_can_be_cancelled_before_submission() {
            let (source, icon) = windows(true);
            collapse_animation::begin(source, icon).unwrap();
            collapse_animation::advance();
            assert_eq!(readers(source.frame), 1);
            collapse_animation::cancel(source.owner, source.id);
            assert!(!collapse_animation::active());
            assert_eq!(readers(source.frame), 0);
        }

        #[test] fn pending_capture_preserves_a_small_opening_presentation_exactly() {
            let (mut source, mut icon) = windows(true);
            source.presentation_placement.width = 40;
            source.presentation_placement.height = 25;
            icon.interaction.resize_on_maximize = false;
            collapse_animation::begin(source, icon).unwrap();
            let projected = collapse_animation::project(icon);
            assert_eq!(projected.presentation_placement, source.presentation_placement);
            assert!(projected.interaction.resize_on_maximize);
            assert_eq!(projected.placement, icon.placement);
            collapse_animation::cancel(source.owner, source.id);
        }

        #[test] fn submitted_failure_quarantines_both_sides_without_an_active_effect() {
            let (source, icon) = windows(true);
            bcs::QUEUE.store(2, Ordering::Relaxed);
            collapse_animation::begin(source, icon).unwrap();
            collapse_animation::advance();
            assert!(!collapse_animation::active());
            assert_eq!(readers(source.frame), 1);
            assert_eq!(collapse_animation::project(icon).frame, icon.frame);
        }

        #[test] fn failed_completion_retains_source_and_capture_until_reboot() {
            let (source, icon) = windows(true);
            bcs::QUEUE.store(1, Ordering::Relaxed);
            collapse_animation::begin(source, icon).unwrap();
            collapse_animation::advance();
            bcs::POLL.store(2, Ordering::Relaxed);
            collapse_animation::advance();
            assert!(!collapse_animation::active());
            assert_eq!(readers(source.frame), 1);
            assert_eq!(collapse_animation::project(icon).frame, icon.frame);
        }

        #[test] fn unavailable_gpu_capture_keeps_the_compact_outcome_usable() {
            let (source, icon) = windows(true);
            bcs::QUEUE.store(3, Ordering::Relaxed);
            collapse_animation::begin(source, icon).unwrap();
            collapse_animation::advance();
            assert!(!collapse_animation::active());
            assert_eq!(readers(source.frame), 0);
            assert_eq!(collapse_animation::project(icon).frame, icon.frame);
        }

        #[test] fn one_gpu_snapshot_captures_and_flattens_the_background_pair() {
            let (mut source, icon) = windows(true);
            let background = frame(64, 40, [0, 80, 0, 128], true);
            source.background = Some(WindowBackground { frame: background, opacity: 128 });
            bcs::QUEUE.store(1, Ordering::Relaxed);
            bcs::COPIES.store(0, Ordering::Relaxed);
            collapse_animation::begin(source, icon).unwrap();
            collapse_animation::advance();
            assert_eq!(bcs::COPIES.load(Ordering::Relaxed), 2);
            assert_eq!(readers(source.frame), 1);
            assert_eq!(readers(background), 1);
            bcs::POLL.store(1, Ordering::Relaxed);
            collapse_animation::advance();
            assert_eq!(readers(source.frame), 0);
            assert_eq!(readers(background), 0);
            let projected = collapse_animation::project(icon);
            assert!(projected.background.is_none());
            assert_eq!(center_pixel(projected.frame), [100, 20, 0, 160]);
            collapse_animation::cancel(source.owner, source.id);
        }

        #[test] fn a_new_collapse_survives_an_older_cancelled_pending_capture() {
            let (source, icon) = windows(true);
            bcs::QUEUE.store(1, Ordering::Relaxed);
            collapse_animation::begin(source, icon).unwrap();
            collapse_animation::advance();
            collapse_animation::cancel(source.owner, source.id);
            let new_icon = WindowSnapshot { frame: frame(64, 64, [0, 0, 80, 255], false), ..icon };
            collapse_animation::begin(source, new_icon).unwrap();
            assert_eq!(collapse_animation::project(new_icon).frame, source.frame);
            bcs::POLL.store(1, Ordering::Relaxed);
            collapse_animation::advance();
            collapse_animation::advance();
            assert_eq!(readers(source.frame), 0);
            assert_ne!(collapse_animation::project(new_icon).frame, new_icon.frame);
            collapse_animation::cancel(source.owner, source.id);
        }
    }
}
