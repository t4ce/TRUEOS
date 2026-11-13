//! Exercise the production cold loader and catalog; only DMA/cache operations
//! are mocked. No tar/PNG decoder or legacy archive exists in this harness.
#![allow(dead_code)]
extern crate alloc;
use std::sync::atomic::AtomicUsize;
#[cfg(test)]
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
static BUFFERS: Mutex<Vec<Arc<Mutex<Vec<u8>>>>> = Mutex::new(Vec::new());
static WRITES: AtomicUsize = AtomicUsize::new(0);
static FLUSHES: AtomicUsize = AtomicUsize::new(0);
#[macro_export]
macro_rules! log_info {
    (target: $target:expr; $($arg:tt)*) => { let _ = format_args!($($arg)*); };
}
pub use log_info as log_error;
mod intel {
    pub const fn align_up(n: usize, a: usize) -> Option<usize> {
        match n.checked_add(a - 1) {
            Some(v) => Some(v & !(a - 1)),
            None => None,
        }
    }
    pub mod gpgpu {
        pub const DIRECT_RCS_PPGTT_LIMIT_BYTES: u64 = 0x1_0000_0000;
    }
}
mod gpu {
    pub mod resident {
        pub struct ResidentDmaBuffer {
            data: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
        }
        impl ResidentDmaBuffer {
            pub fn allocate_zeroed(n: usize, a: usize) -> Option<Self> {
                assert_eq!(a, 4096);
                assert_eq!(n % 4096, 0);
                let data = std::sync::Arc::new(std::sync::Mutex::new(vec![0; n]));
                crate::BUFFERS.lock().unwrap().push(data.clone());
                Some(Self { data })
            }
            pub fn phys(&self) -> u64 {
                0x1000_0000
            }
            pub fn bytes(&self) -> usize {
                self.data.lock().unwrap().len()
            }
            pub fn write(&self, offset: usize, src: &[u8]) -> bool {
                assert_eq!(offset % 4096, 0);
                assert_eq!(src.len(), 256 * 256 * 4);
                let mut data = self.data.lock().unwrap();
                let Some(dst) = data.get_mut(offset..offset + src.len()) else {
                    return false;
                };
                dst.copy_from_slice(src);
                crate::WRITES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                true
            }
            pub fn flush(&self) {
                crate::FLUSHES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }
}
#[path = "@TRUEOS_ROOT@/src/spirit/lilly.rs"]
mod lilly;

#[test]
fn full_pack_upload_alignment_and_legacy_playback_contract() {
    assert!(lilly::prepare_resident_once());
    assert!(lilly::prepare_resident_once());
    let bytes = include_bytes!("@TRUEOS_ROOT@/tools/Lilly.lpak");
    let decoded = trueos_lily_pack::decode(bytes).unwrap();
    assert_eq!(decoded.frames.len(), 607);
    assert_eq!(WRITES.load(Ordering::SeqCst), 607);
    assert_eq!(FLUSHES.load(Ordering::SeqCst), 1);
    assert_eq!(BUFFERS.lock().unwrap().len(), 1);
    assert_eq!(lilly::resident_frame_count(), 490);
    let buffers = BUFFERS.lock().unwrap();
    let data = buffers[0].lock().unwrap();
    assert_eq!(data.len(), 607 * 256 * 256 * 4);
    for (i, info) in decoded.frames.iter().enumerate() {
        let surface = lilly::resident_frame(&info.path).unwrap();
        assert_eq!(
            surface,
            lilly::resident_frame(&format!("Lilly/{}", info.path)).unwrap()
        );
        assert_eq!(
            (
                surface.width,
                surface.height,
                surface.pitch_bytes,
                surface.bytes
            ),
            (256, 256, 1024, 262144)
        );
        let offset = i * 262144;
        assert_eq!(surface.phys, 0x1000_0000 + offset as u64);
        assert_eq!(surface.gpu, 0x3000_0000 + offset as u64);
        assert!(surface.gpu + surface.bytes as u64 <= 0x4000_0000);
        let upload = &data[offset..offset + 262144];
        let crop = decoded.cropped_rgba8(i).unwrap();
        let [_, _, w, h] = info.rect.map(usize::from);
        let left = (256 - w) / 2;
        let top = 256 - h;
        for y in 0..256 {
            let row = &upload[y * 1024..(y + 1) * 1024];
            if y < top {
                assert!(row.iter().all(|&v| v == 0));
                continue;
            }
            assert!(row[..left * 4].iter().all(|&v| v == 0));
            assert_eq!(
                &row[left * 4..(left + w) * 4],
                &crop[(y - top) * w * 4..(y - top + 1) * w * 4]
            );
            assert!(row[(left + w) * 4..].iter().all(|&v| v == 0));
        }
    }
    let sources = include_str!("@TRUEOS_ROOT@/tools/Lilly.sources");
    let mut count = 0;
    for line in include_str!("@TRUEOS_ROOT@/tools/Lilly.catalog")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let fields: Vec<_> = line.split('|').collect();
        let (key, legacy, kind, period) = (
            fields[0],
            fields[1],
            fields[2],
            fields[3].parse::<u16>().unwrap(),
        );
        let semantics: Vec<_> = fields[4].split(',').collect();
        let source_map: Vec<usize> = if fields.len() == 6 {
            fields[5].split(',').map(|n| n.parse().unwrap()).collect()
        } else {
            (1..=7).collect()
        };
        let replacement = sources
            .lines()
            .filter_map(|l| l.split_once('|'))
            .find(|(d, _)| *d == legacy)
            .unwrap()
            .1;
        let asset = lilly::resident_asset(key).unwrap();
        let animation = lilly::resident_animation(key).unwrap();
        assert_eq!(asset.key, key);
        assert_eq!(asset.frame_period_ms, period);
        assert_eq!(animation.cycle_duration_ms(), u64::from(period) * 7);
        assert_eq!(
            asset.playback,
            match kind {
                "loop" => lilly::LillyPlayback::Loop,
                "once" => lilly::LillyPlayback::Once,
                "once_hold" => lilly::LillyPlayback::OnceHold,
                _ => panic!(),
            }
        );
        for logical in 0..7 {
            let source = source_map[logical];
            let expected =
                lilly::resident_frame(&format!("{replacement}/frame_{:02}.png", source + 1))
                    .unwrap();
            assert_eq!(asset.parts[logical].semantic, semantics[logical]);
            assert_eq!(asset.parts[logical].surface, expected);
            assert_eq!(
                lilly::resident_part(key, semantics[logical]),
                Some(expected)
            );
            assert_eq!(
                animation
                    .frame_at_elapsed(logical as u64 * u64::from(period))
                    .unwrap()
                    .surface,
                expected
            );
            assert_eq!(
                lilly::resident_frame(&format!("Lilly/{legacy}/frame_{source:02}.png")),
                Some(expected)
            );
            assert_eq!(
                lilly::resident_frame(&format!("{legacy}/frame_{source:02}.png")),
                Some(expected)
            );
        }
        match kind {
            "loop" => assert_eq!(
                animation
                    .frame_at_elapsed(animation.cycle_duration_ms())
                    .unwrap(),
                asset.parts[0]
            ),
            "once" => assert!(
                animation
                    .frame_at_elapsed(animation.cycle_duration_ms())
                    .is_none()
            ),
            "once_hold" => assert_eq!(
                animation
                    .frame_at_elapsed(animation.cycle_duration_ms())
                    .unwrap(),
                asset.parts[6]
            ),
            _ => unreachable!(),
        }
        if key == "wave.two_hands" {
            assert_eq!(replacement, "Waving_up/smile");
        }
        if key == "idle.uncrossed.scheming" {
            assert_eq!(replacement, "Idle_up/Uncrossed-Arms/idle-quiet-laugh");
        }
        count += 1;
    }
    assert_eq!(count, 70);
    assert!(lilly::resident_frame("Static_up/codex.png").is_some());
    for n in 2..=8 {
        assert!(lilly::resident_frame(&format!("Static_up/wish-c/frame_{n:02}.png")).is_some());
    }
    assert!(lilly::resident_frame("Headset_up/static.png").is_some());
    assert!(lilly::resident_frame("Freestyle_up/clippy/frame_05.png").is_some());
    assert!(lilly::resident_asset("freestyle.clippy").is_none()); // extras do not change scheduling
    let loader = include_str!("@TRUEOS_ROOT@/src/spirit/lilly.rs");
    assert!(!loader.contains("Lilly.tar.lz4"));
    println!(
        "607 exact bottom-centered DMA uploads; all 70 legacy keys and 490 logical frame slots retain metadata/timing/repeats; no legacy archive dependency"
    );
}
