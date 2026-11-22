//! Exercise the production cold loader, cursor copy, and playback catalog.
#![allow(dead_code)]
extern crate alloc;
#[macro_export]
macro_rules! log_info {
    (target: $target:expr; $($arg:tt)*) => { let _ = format_args!($($arg)*); };
}
pub use log_info as log_error;
#[path = "@TRUEOS_ROOT@/src/spirit/lilly.rs"]
mod lilly;

#[cfg(test)]
mod tyche {
    pub struct SoftRng;
    impl SoftRng {
        pub fn usize_below(&mut self, _: usize) -> usize { 0 }
        pub fn bool(&mut self) -> bool { false }
    }
}
#[cfg(test)]
#[path = "@TRUEOS_ROOT@/src/spirit/lilly_sequencer.rs"]
mod lilly_sequencer;

#[test]
fn full_pack_cursor_copy_and_legacy_playback_contract() {
    assert!(lilly::prepare_resident_once());
    assert!(lilly::prepare_resident_once());
    let bytes = include_bytes!("@TRUEOS_ROOT@/tools/Lilly.lpak");
    let decoded = trueos_lily_pack::decode(bytes).unwrap();
    assert_eq!(decoded.frames.len(), 607);
    let paths: std::collections::HashSet<_> = decoded.frames.iter().map(|entry| entry.path.as_str()).collect();
    for path in lilly_sequencer::frame_paths() {
        assert!(paths.contains(path), "sequencer frame missing from the embedded pack: {path}");
    }
    assert_eq!(lilly::resident_frame_count(), 490);
    let mut cursor = vec![0; 256 * 256 * 4];
    for (i, info) in decoded.frames.iter().enumerate() {
        let surface = lilly::resident_frame(&info.path).unwrap();
        assert_eq!(
            surface,
            lilly::resident_frame(&format!("Lilly/{}", info.path)).unwrap()
        );
        assert!(lilly::copy_frame(surface, &mut cursor));
        let crop = decoded.cropped_rgba8(i).unwrap();
        let [_, _, w, h] = info.rect.map(usize::from);
        let left = (256 - w) / 2;
        let top = 256 - h;
        for y in 0..256 {
            let row = &cursor[y * 1024..(y + 1) * 1024];
            if y < top {
                assert!(row.iter().all(|&v| v == 0));
                continue;
            }
            assert!(row[..left * 4].iter().all(|&v| v == 0));
            let rgba = &crop[(y - top) * w * 4..(y - top + 1) * w * 4];
            let bgra = &row[left * 4..(left + w) * 4];
            for (&[r, g, b, a], pixel) in rgba.as_chunks::<4>().0.iter().zip(bgra.as_chunks::<4>().0) {
                let scale = |color| (f32::from(color) * f32::from(a) / 255.0).round() as u8;
                assert_eq!(*pixel, [scale(b), scale(g), scale(r), a]);
            }
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
    let frame = lilly::resident_frame("Static_up/codex.png").unwrap();
    let mut short = [12; 32];
    assert!(!lilly::copy_frame(frame, &mut short));
    assert_eq!(short, [12; 32]);
    for n in 2..=8 {
        assert!(lilly::resident_frame(&format!("Static_up/wish-c/frame_{n:02}.png")).is_some());
    }
    assert!(lilly::resident_frame("Headset_up/static.png").is_some());
    assert!(lilly::resident_frame("Freestyle_up/clippy/frame_05.png").is_some());
    assert!(lilly::resident_asset("freestyle.clippy").is_none()); // extras do not change scheduling
    let loader = include_str!("@TRUEOS_ROOT@/src/spirit/lilly.rs");
    assert!(!loader.contains("Lilly.tar.lz4"));
    println!(
        "607 exact bottom-centered cursor copies in premultiplied BGRA; all 70 legacy keys and 490 logical frame slots retain metadata/timing/repeats"
    );
}
