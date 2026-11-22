#!/usr/bin/env python3
"""Exercise the production Lilly CPU copy and cursor-buffer ownership."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item


def main():
    submit = item("src/spirit/mod.rs", "submit_lilly_frame")
    source = r'''#![allow(dead_code)]
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SpiritFenceId(u8);
impl SpiritFenceId {
    const FENCE_0: Self = Self(0);
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SpiritFence { sequence: u64 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SpiritFrameLease { fence: SpiritFence }
#[derive(Clone, Copy)]
struct SpiritSurfaceLayout;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpiritBarrierSet { CPU }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpiritFrameProducer { Lilly }
#[derive(Debug, Eq, PartialEq)]
enum SpiritSubmitError { Busy, InvalidCommand, StaleLease }

#[derive(Default)]
struct State {
    events: Vec<&'static str>,
    pixels: [u8; 8],
    handoff: bool,
    busy: bool,
    fail_copy: bool,
    fail_stamp: bool,
    fail_release: bool,
}
thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}
fn record(event: &'static str) {
    STATE.with_borrow_mut(|state| state.events.push(event));
}
fn acquire_frame_for(
    _: SpiritFenceId, required: SpiritBarrierSet, producer: SpiritFrameProducer,
) -> Result<SpiritFrameLease, SpiritSubmitError> {
    assert_eq!(required, SpiritBarrierSet::CPU);
    assert_eq!(producer, SpiritFrameProducer::Lilly);
    record("reserve");
    if STATE.with_borrow(|state| state.busy) {
        return Err(SpiritSubmitError::Busy);
    }
    Ok(SpiritFrameLease { fence: SpiritFence { sequence: 42 } })
}
fn write_cpu<R>(
    _: SpiritFrameLease, write: impl FnOnce(&mut [u8], SpiritSurfaceLayout) -> R,
) -> Result<R, SpiritSubmitError> {
    record("write");
    let mut pixels = [0; 8];
    let result = write(&mut pixels, SpiritSurfaceLayout);
    STATE.with_borrow_mut(|state| state.pixels = pixels);
    Ok(result)
}
fn release_cpu(_: SpiritFrameLease) -> Result<(), SpiritSubmitError> {
    record("flush");
    if STATE.with_borrow(|state| state.fail_release) {
        return Err(SpiritSubmitError::StaleLease);
    }
    record("release");
    Ok(())
}
fn cancel_pending(_: SpiritFrameLease) { record("cancel"); }
mod lilly {
    use super::*;
    #[derive(Clone, Copy)]
    pub struct LillyResidentFrame(pub u8);

    pub fn copy_frame(frame: LillyResidentFrame, pixels: &mut [u8]) -> bool {
        record("copy");
        if STATE.with_borrow(|state| state.fail_copy) {
            return false;
        }
        pixels.fill(frame.0);
        true
    }
}
mod codex_handoff {
    use super::*;
    #[derive(Clone, Copy)]
    pub struct Stamp;
    impl Stamp {
        pub fn apply(self, pixels: &mut [u8], _: SpiritSurfaceLayout) -> bool {
            assert_eq!(pixels, &[27; 8]);
            record("stamp");
            pixels[0] = 93;
            !STATE.with_borrow(|state| state.fail_stamp)
        }
    }
    #[derive(Clone, Copy)]
    pub struct Source {
        pub surface: lilly::LillyResidentFrame,
        pub stamp: Option<Stamp>,
    }
    pub fn source(_: lilly::LillyResidentFrame, sequence: u64) -> Option<Source> {
        assert_eq!(sequence, 42);
        STATE.with_borrow(|state| state.handoff)
            .then_some(Source { surface: lilly::LillyResidentFrame(27), stamp: Some(Stamp) })
    }
}
@SUBMIT@

#[test]
fn normal_frame_copies_decoded_bytes_and_flushes_before_release() {
    assert!(submit_lilly_frame(SpiritFenceId::FENCE_0, lilly::LillyResidentFrame(7)).is_ok());
    STATE.with_borrow(|s| {
        assert_eq!(s.pixels, [7; 8]);
        assert_eq!(s.events, ["reserve", "write", "copy", "flush", "release"]);
    });
}
#[test]
fn codex_handoff_copies_its_sprite_before_stamping_the_tile() {
    STATE.with_borrow_mut(|s| s.handoff = true);
    assert!(submit_lilly_frame(SpiritFenceId::FENCE_0, lilly::LillyResidentFrame(7)).is_ok());
    STATE.with_borrow(|s| {
        assert_eq!(s.pixels, [93, 27, 27, 27, 27, 27, 27, 27]);
        assert_eq!(s.events, ["reserve", "write", "copy", "stamp", "flush", "release"]);
    });
}
#[test]
fn busy_back_buffer_is_never_written() {
    STATE.with_borrow_mut(|s| s.busy = true);
    assert_eq!(submit_lilly_frame(SpiritFenceId::FENCE_0, lilly::LillyResidentFrame(7)),
        Err(SpiritSubmitError::Busy));
    STATE.with_borrow(|s| assert_eq!(s.events, ["reserve"]));
}
#[test]
fn failed_copy_cancels_without_stamping_or_releasing() {
    STATE.with_borrow_mut(|s| { s.handoff = true; s.fail_copy = true; });
    assert_eq!(submit_lilly_frame(SpiritFenceId::FENCE_0, lilly::LillyResidentFrame(7)),
        Err(SpiritSubmitError::InvalidCommand));
    STATE.with_borrow(|s| assert_eq!(s.events, ["reserve", "write", "copy", "cancel"]));
}
#[test]
fn failed_stamp_cancels_without_releasing() {
    STATE.with_borrow_mut(|s| { s.handoff = true; s.fail_stamp = true; });
    assert_eq!(submit_lilly_frame(SpiritFenceId::FENCE_0, lilly::LillyResidentFrame(7)),
        Err(SpiritSubmitError::InvalidCommand));
    STATE.with_borrow(|s| assert_eq!(s.events, ["reserve", "write", "copy", "stamp", "cancel"]));
}
#[test]
fn failed_flush_cancels_without_releasing() {
    STATE.with_borrow_mut(|s| s.fail_release = true);
    assert_eq!(submit_lilly_frame(SpiritFenceId::FENCE_0, lilly::LillyResidentFrame(7)),
        Err(SpiritSubmitError::StaleLease));
    STATE.with_borrow(|s| assert_eq!(s.events, ["reserve", "write", "copy", "flush", "cancel"]));
}
'''.replace("@SUBMIT@", submit)
    with tempfile.TemporaryDirectory(prefix="lilly-cursor-copy-") as directory:
        path = Path(directory)
        test = path / "test.rs"
        binary = path / "test"
        test.write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(test), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
