#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_time;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

static NOW: AtomicU64 = AtomicU64::new(0);
static GPU_LATENCY: AtomicU64 = AtomicU64::new(200);
static GPU_DONE: AtomicBool = AtomicBool::new(false);
static BAD_RELEASE: AtomicBool = AtomicBool::new(false);
static FAIL_GPU: AtomicBool = AtomicBool::new(false);
static SUBMITS: AtomicUsize = AtomicUsize::new(0);
static READS: AtomicUsize = AtomicUsize::new(0);
static DROPS: AtomicUsize = AtomicUsize::new(0);
static STORAGE: Mutex<Option<Arc<Mutex<Vec<u8>>>>> = Mutex::new(None);

pub struct Instant(u64);
impl Instant {
    pub fn now() -> Self {
        Self(NOW.load(Ordering::SeqCst))
    }
    pub fn as_millis(self) -> u64 {
        self.0
    }
}

mod gpu {
    pub mod resident {
        use crate::*;
        pub struct ResidentDmaBuffer {
            data: Arc<Mutex<Vec<u8>>>,
        }
        impl ResidentDmaBuffer {
            pub fn allocate_zeroed(bytes: usize, _: usize) -> Option<Self> {
                let data = Arc::new(Mutex::new(vec![0; bytes]));
                *STORAGE.lock().unwrap() = Some(data.clone());
                Some(Self { data })
            }
            pub fn phys(&self) -> u64 {
                0x2000_0000
            }
            pub fn flush(&self) {
                assert!(GPU_DONE.load(Ordering::SeqCst));
            }
            pub fn read(&self, offset: usize, destination: &mut [u8]) -> bool {
                assert!(
                    GPU_DONE.load(Ordering::SeqCst),
                    "pending GPU memory cannot be read"
                );
                READS.fetch_add(1, Ordering::SeqCst);
                destination.copy_from_slice(
                    &self.data.lock().unwrap()[offset..offset + destination.len()],
                );
                true
            }
        }
        impl Drop for ResidentDmaBuffer {
            fn drop(&mut self) {
                DROPS.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}

mod intel {
    pub mod gpgpu {
        use crate::*;
        #[derive(Clone, Copy)]
        pub struct SpiritVfxControl;
        #[derive(Clone, Copy)]
        pub struct SpiritVfxSubmission(usize);
        #[derive(Clone, Copy)]
        pub struct GpgpuRgba8Surface {
            pub phys: u64,
            pub gpu: u64,
            pub bytes: usize,
            pub width: u32,
            pub height: u32,
            pub pitch_bytes: u32,
        }
        impl GpgpuRgba8Surface {
            pub fn new(
                phys: u64,
                gpu: u64,
                bytes: usize,
                width: u32,
                height: u32,
                pitch_bytes: u32,
            ) -> Option<Self> {
                Some(Self {
                    phys,
                    gpu,
                    bytes,
                    width,
                    height,
                    pitch_bytes,
                })
            }
            pub fn new_bgra(
                phys: u64,
                gpu: u64,
                bytes: usize,
                width: u32,
                height: u32,
                pitch_bytes: u32,
            ) -> Option<Self> {
                assert_eq!(gpu, 0x3ffc_0000);
                Self::new(phys, gpu, bytes, width, height, pitch_bytes)
            }
        }
        pub struct GpgpuRgba8ReleaseFence {
            phys: u64,
            bytes: usize,
        }
        impl GpgpuRgba8ReleaseFence {
            pub fn matches(self, phys: u64, bytes: usize) -> bool {
                self.phys == phys && self.bytes == bytes
            }
        }
        pub enum SpiritVfxCompletion {
            Pending,
            Complete(GpgpuRgba8ReleaseFence),
            Failed,
            InvalidSubmission,
        }
        struct Job {
            id: usize,
            started: u64,
            source: GpgpuRgba8Surface,
            target: GpgpuRgba8Surface,
        }
        static JOB: Mutex<Option<Job>> = Mutex::new(None);
        pub fn submit_spirit_vfx_frame(
            target: GpgpuRgba8Surface,
            source: GpgpuRgba8Surface,
            _: SpiritVfxControl,
            _: u32,
        ) -> Option<SpiritVfxSubmission> {
            let mut pending = JOB.lock().unwrap();
            assert!(
                pending.is_none(),
                "staging allocation cannot be overwritten before retirement"
            );
            GPU_DONE.store(false, Ordering::SeqCst);
            let id = SUBMITS.fetch_add(1, Ordering::SeqCst) + 1;
            *pending = Some(Job {
                id,
                started: NOW.load(Ordering::SeqCst),
                source,
                target,
            });
            Some(SpiritVfxSubmission(id))
        }
        pub fn poll_spirit_vfx_submission(handle: SpiritVfxSubmission) -> SpiritVfxCompletion {
            let mut pending = JOB.lock().unwrap();
            let Some(job) = pending.as_ref() else {
                return SpiritVfxCompletion::InvalidSubmission;
            };
            assert_eq!(job.id, handle.0);
            if FAIL_GPU.load(Ordering::SeqCst) {
                return SpiritVfxCompletion::Failed;
            }
            if NOW.load(Ordering::SeqCst) < job.started + GPU_LATENCY.load(Ordering::SeqCst) {
                return SpiritVfxCompletion::Pending;
            }
            STORAGE
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .fill(job.source.phys as u8);
            GPU_DONE.store(true, Ordering::SeqCst);
            let release = GpgpuRgba8ReleaseFence {
                phys: job.target.phys
                    + if BAD_RELEASE.load(Ordering::SeqCst) {
                        4096
                    } else {
                        0
                    },
                bytes: job.target.bytes,
            };
            *pending = None;
            SpiritVfxCompletion::Complete(release)
        }
    }
}

mod spirit {
    #[derive(Clone, Copy)]
    pub struct SpiritSurfaceLayout {
        pub width: u32,
        pub height: u32,
        pub pitch_bytes: u32,
        pub byte_len: usize,
    }
    #[derive(Debug, PartialEq)]
    pub enum SpiritSubmitError {
        HardwareNotReady,
        InvalidGpuRelease,
        InvalidCommand,
        GpuSubmissionFailed,
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
    }
    #[path = "@TRUEOS_ROOT@/src/spirit/codex_render.rs"]
    pub mod codex_render;
    #[path = "@TRUEOS_ROOT@/src/spirit/codex_handoff/geometry.rs"]
    pub mod geometry;

    // The production module deliberately exposes these only to Spirit.
    use codex_render::{busy, capture, copy_into, invalidate, poll, ready, update};

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::*;

        fn source(id: u64) -> spirit::lilly::LillyResidentFrame {
            spirit::lilly::LillyResidentFrame {
                phys: id,
                gpu: 0x3000_0000,
                bytes: 256 * 256 * 4,
                width: 256,
                height: 256,
                pitch_bytes: 1024,
            }
        }
        fn layout(pitch: u32) -> spirit::SpiritSurfaceLayout {
            spirit::SpiritSurfaceLayout {
                width: 256,
                height: 256,
                pitch_bytes: pitch,
                byte_len: pitch as usize * 256,
            }
        }
        fn update(id: u64) {
            spirit::update(source(id), intel::gpgpu::SpiritVfxControl, 60).unwrap();
        }

        #[test]
        fn pending_character_render_does_not_block_tile_motion_or_leave_ghosts() {
            let layout = layout(1024);
            assert!(spirit::capture(
                source(10),
                &vec![42; layout.byte_len],
                layout
            ));
            update(10);
            assert!(spirit::busy());
            let mut cursor = vec![0; layout.byte_len];
            let icon = include_bytes!("@TRUEOS_ROOT@/tools/lilly/codex-tile64.rgba");
            let mut frames = 0;
            for (frame, t) in (0..200).step_by(17).enumerate() {
                NOW.store(t, Ordering::SeqCst);
                update(10);
                assert_eq!(spirit::copy_into(&mut cursor, layout), Some(source(10)));
                assert!(
                    cursor.iter().all(|pixel| *pixel == 42),
                    "each CPU frame erases the preceding tile"
                );
                let pose = spirit::geometry::Pose {
                    icon: spirit::geometry::Point {
                        x: 16 + frame as i32 * 10,
                        y: 80,
                    },
                    cursor: spirit::geometry::Point::default(),
                };
                spirit::geometry::cursor_bgra_part(pose, icon, 255, &mut cursor, 1024);
                assert!(
                    cursor.iter().any(|pixel| *pixel != 42),
                    "native tile advances over the cached character"
                );
                frames += 1;
            }
            assert_eq!(frames, 12);
            assert_eq!(SUBMITS.load(Ordering::SeqCst), 1);
            assert_eq!(
                READS.load(Ordering::SeqCst),
                0,
                "pending staging pixels are never borrowed"
            );
            NOW.store(200, Ordering::SeqCst);
            spirit::poll().unwrap();
            assert!(!spirit::busy());
            assert_eq!(spirit::copy_into(&mut cursor, layout), Some(source(10)));
            assert!(
                cursor.iter().all(|pixel| *pixel == 10),
                "retired GPU snapshot replaces the clean cache"
            );
        }

        #[test]
        fn cache_preserves_pitch_and_rejects_invalid_layouts_without_changes() {
            let padded = layout(1056);
            let mut pixels = vec![77; padded.byte_len];
            for row in pixels.chunks_exact_mut(1056) {
                row[..1024].fill(42);
            }
            assert!(spirit::capture(source(10), &pixels, padded));
            let mut output = vec![99; padded.byte_len];
            assert_eq!(spirit::copy_into(&mut output, padded), Some(source(10)));
            for row in output.chunks_exact(1056) {
                assert!(row[..1024].iter().all(|p| *p == 42));
                assert!(row[1024..].iter().all(|p| *p == 99));
            }
            let invalid = spirit::SpiritSurfaceLayout {
                pitch_bytes: 1023,
                ..padded
            };
            let before = output.clone();
            assert!(!spirit::capture(source(20), &output, invalid));
            assert_eq!(spirit::copy_into(&mut output, invalid), None);
            assert_eq!(output, before);
            assert_eq!(spirit::copy_into(&mut output, padded), Some(source(10)));
        }

        #[test]
        fn exact_gpu_release_is_required_before_cache_readback() {
            let layout = layout(1024);
            assert!(spirit::capture(
                source(10),
                &vec![42; layout.byte_len],
                layout
            ));
            update(20);
            BAD_RELEASE.store(true, Ordering::SeqCst);
            NOW.store(200, Ordering::SeqCst);
            assert_eq!(
                spirit::poll(),
                Err(spirit::SpiritSubmitError::InvalidGpuRelease)
            );
            assert_eq!(READS.load(Ordering::SeqCst), 0);
            assert!(spirit::busy(), "unproven target remains pinned");
            let mut pixels = vec![0; layout.byte_len];
            assert_eq!(spirit::copy_into(&mut pixels, layout), Some(source(10)));
            assert!(pixels.iter().all(|p| *p == 42));
            assert_eq!(DROPS.load(Ordering::SeqCst), 0);
        }

        #[test]
        fn invalidation_keeps_pending_storage_but_discards_old_flight_completion() {
            update(10);
            assert!(spirit::busy());
            spirit::invalidate();
            assert!(!spirit::ready());
            let layout = layout(1024);
            assert!(spirit::capture(
                source(20),
                &vec![24; layout.byte_len],
                layout
            ));
            NOW.store(200, Ordering::SeqCst);
            spirit::poll().unwrap();
            assert!(!spirit::busy());
            assert_eq!(READS.load(Ordering::SeqCst), 0);
            let mut pixels = vec![0; layout.byte_len];
            assert_eq!(spirit::copy_into(&mut pixels, layout), Some(source(20)));
            assert!(pixels.iter().all(|p| *p == 24));
            assert_eq!(DROPS.load(Ordering::SeqCst), 0);
        }

        #[test]
        fn character_updates_keep_their_own_thirty_hz_cadence_and_source_identity() {
            GPU_LATENCY.store(0, Ordering::SeqCst);
            update(10);
            NOW.store(16, Ordering::SeqCst);
            update(20);
            assert_eq!(SUBMITS.load(Ordering::SeqCst), 1);
            let layout = layout(1024);
            let mut pixels = vec![0; layout.byte_len];
            assert_eq!(spirit::copy_into(&mut pixels, layout), Some(source(10)));
            NOW.store(33, Ordering::SeqCst);
            update(20);
            assert_eq!(SUBMITS.load(Ordering::SeqCst), 2);
            spirit::poll().unwrap();
            assert_eq!(spirit::copy_into(&mut pixels, layout), Some(source(20)));
            assert!(pixels.iter().all(|p| *p == 20));
        }

        #[test]
        fn failed_gpu_work_never_releases_or_frees_its_staging_allocation() {
            update(10);
            FAIL_GPU.store(true, Ordering::SeqCst);
            assert_eq!(
                spirit::poll(),
                Err(spirit::SpiritSubmitError::GpuSubmissionFailed)
            );
            spirit::invalidate();
            assert!(spirit::busy());
            assert!(!spirit::ready());
            assert_eq!(READS.load(Ordering::SeqCst), 0);
            assert_eq!(DROPS.load(Ordering::SeqCst), 0);
        }
    }
}
