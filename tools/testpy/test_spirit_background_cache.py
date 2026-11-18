#!/usr/bin/env python3
"""Check wall-time sample selection and premultiplied cursor seeding on the host."""

import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
source = (ROOT / "src/spirit/background_cache.rs").read_text()
samples = source[source.index("pub(super) struct Sample"):source.index("pub(super) fn select(")]
playback = source[source.index("struct Playback"):source.index("struct Cache")]
segments = (ROOT / "src/spirit/background_cache/segments.rs").read_text()
segment_types = segments[segments.index("#[derive(Clone)]"):segments.index("static STATE:")]
segment_impl = segments[segments.index("impl State {"):segments.index("pub(super) fn select(")]
support = """
#![allow(dead_code)]
use std::collections::VecDeque;
use std::sync::Arc;
type Frame = Arc<Vec<u8>>;
const SIZE: usize = 256;
const PITCH: usize = SIZE * 4;
const SAMPLE_HZ: f32 = 15.0;
#[derive(Clone, Copy, PartialEq)]
struct Recipe { mode: u32 }
#[derive(Clone, Copy)]
struct SpiritSurfaceLayout {
    width: u32,
    height: u32,
    pitch_bytes: u32,
}
"""
tests = """
#[test]
fn warm_playback_adds_samples_without_changing_the_current_pass() {
    let mut playback = Playback { recipe: Recipe { mode: 2 }, phase: 0.0, samples: 1,
                                  last_ms: 0, speed: 1.0 };
    playback.advance(1000, 1.0, 2);
    assert_eq!(playback.samples, 2);
    assert_eq!(playback.phase, 0.0);
    playback.advance(1050, 1.0, 3);
    assert_eq!(playback.samples, 2);
    assert_eq!(loop_position(playback.phase, playback.samples), (0, 1, 192));
    playback.advance(1150, 1.0, 3);
    assert_eq!(playback.samples, 3);
    assert_eq!(loop_position(playback.phase, playback.samples), (0, 1, 64));
}

#[test]
fn playback_reverses_at_turns_and_wraps_without_a_jump() {
    assert_eq!(loop_position(7.25, 1), (0, 0, 0));
    assert_eq!(loop_position(14.75, 16), (14, 15, 192));
    assert_eq!(loop_position(15.0, 16), (15, 15, 0));
    assert_eq!(loop_position(15.25, 16), (14, 15, 192));
    assert_eq!(loop_position(29.75, 16), (0, 1, 64));
    assert_eq!(loop_position(30.25, 16), (0, 1, 64));
}

#[test]
fn copies_padded_rows_and_preserves_premultiplication_under_gain() {
    let first = Arc::new([5, 10, 80, 100].repeat(SIZE * SIZE));
    let second = Arc::new([25, 40, 240, 250].repeat(SIZE * SIZE));
    let sample = Sample { first, second, first_offset: 0, second_offset: 0,
                          blend: 128, gain: 512 };
    let pitch = PITCH + 64;
    let layout = SpiritSurfaceLayout { width: 256, height: 256, pitch_bytes: pitch as u32 };
    let mut cursor = vec![67; pitch * SIZE];
    assert!(sample.copy_into(&mut cursor, layout));
    for row in cursor.chunks_exact(pitch) {
        assert_eq!(&row[..4], &[22, 36, 233, 255]);
        assert_eq!(&row[PITCH - 4..PITCH], &[22, 36, 233, 255]);
        assert!(row[PITCH..].iter().all(|&byte| byte == 67));
    }
    assert!(!sample.copy_into(&mut cursor[..pitch * SIZE - 1], layout));
}

#[test]
fn rolling_playback_crosses_batch_boundary_and_recovers_without_reversing() {
    let mut state = State { stream: None, dial: None, generation: 0, stopped: false };
    state.activate(Recipe { mode: 2 }, 0, 0.0, 1.0);
    let stream = state.stream.as_mut().unwrap();
    let batch_a = Arc::new(vec![10, 20]);
    stream.frames.extend([0, 1].map(|offset| FrameView { buffer: batch_a.clone(), offset }));
    assert!(stream.sample(100, 1.0, 256).is_some());
    assert_eq!(stream.underruns, 1);
    assert_eq!(stream.sample(200, 1.0, 256).unwrap().first_offset, 1);
    assert_eq!(stream.underruns, 1, "one underrun per held interval");

    let batch_b = Arc::new(vec![30, 40]);
    stream.frames.extend([0, 1].map(|offset| FrameView { buffer: batch_b.clone(), offset }));
    let bridge = stream.sample(230, 1.0, 256).unwrap();
    assert_eq!(bridge.first[bridge.first_offset], 20);
    assert_eq!(bridge.second[bridge.second_offset], 30);
    assert!(bridge.blend > 0);
    assert_eq!(stream.sample(270, 1.0, 256).unwrap().first[0], 30);
    assert_eq!(stream.underruns, 1);
}
"""
with tempfile.TemporaryDirectory(prefix="trueos-spirit-cache-") as tmp:
    path = Path(tmp)
    (path / "cache.rs").write_text(
        support + playback + samples.replace("pub(super) ", "") + segment_types + segment_impl + tests
    )
    subprocess.run(["rustc", "--edition=2024", "--test", str(path / "cache.rs"),
                    "-o", str(path / "test")], check=True)
    subprocess.run([str(path / "test")], check=True)
