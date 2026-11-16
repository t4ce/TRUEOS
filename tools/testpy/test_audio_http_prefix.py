#!/usr/bin/env python3
"""Exercise production completed-clip snapshots across late joins and replacement.

No browser, socket, app build, publication or physical rig is involved. The real
sink implementation owns and releases the PCM; tests observe its public snapshot
contract and Arc lifetime instead of duplicating its state machine.
"""

from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;

#[path = "@SINK@"]
mod sink;

#[cfg(test)]
mod prefix_tests {
    use super::sink::Sink;
    use alloc::{sync::Arc, vec, vec::Vec};

    fn pcm(frames: usize, seed: u8) -> Vec<u8> {
        // Distinctive non-silent frame zero makes a lost prefix observable.
        (0..frames * 4)
            .map(|index| seed.wrapping_add((index % 251) as u8))
            .collect()
    }

    fn finish(sink: &mut Sink, bytes: usize) {
        let mut output = vec![0; bytes];
        sink.render(&mut output);
        assert!(!sink.busy());
    }

    #[test]
    fn joining_one_second_late_and_reconnecting_both_keep_the_complete_prefix() {
        let mut sink = Sink::new();
        let expected = pcm(48_000 * 3, 37);
        sink.submit(&expected).unwrap();
        let first_id = sink.snapshot().unwrap().id;
        // Reproduce the live clock consuming one second before a GET arrives.
        let mut live_tick = [0; 1_920];
        for _ in 0..100 {
            sink.render(&mut live_tick);
        }
        assert!(sink.busy());
        let late_join = sink.snapshot().unwrap();
        assert_eq!(late_join.id, first_id);
        assert_eq!(late_join.pcm.as_ref(), expected.as_slice());
        assert_eq!(&late_join.pcm[..16], &expected[..16]);
        // A retried browser request gets its own full snapshot, not the live
        // playback cursor or an already-consumed prefix.
        sink.render(&mut live_tick);
        let reconnect = sink.snapshot().unwrap();
        assert_eq!(reconnect.id, first_id);
        assert_eq!(reconnect.pcm.as_ref(), expected.as_slice());
        assert!(Arc::ptr_eq(&late_join.pcm, &reconnect.pcm));
        finish(&mut sink, expected.len());
        let after_completion = sink.snapshot().unwrap();
        assert_eq!(after_completion.id, first_id);
        assert_eq!(after_completion.pcm.as_ref(), expected.as_slice());
    }

    #[test]
    fn busy_first_wins_and_only_valid_accepted_clips_advance_the_id() {
        let mut sink = Sink::new();
        assert!(!sink.busy());
        assert!(sink.snapshot().is_none());
        assert_eq!(sink.submit(&[]), Err(-22));
        assert_eq!(sink.submit(&[1, 2]), Err(-22));
        assert!(sink.snapshot().is_none());

        let first = pcm(8, 11);
        let competing = pcm(5, 93);
        sink.submit(&first).unwrap();
        let accepted = sink.snapshot().unwrap();
        assert_ne!(accepted.id, 0);
        assert!(sink.busy());
        // Successful discard is the existing exclusive-source contract.
        assert_eq!(sink.submit(&competing), Ok(()));
        let unchanged = sink.snapshot().unwrap();
        assert_eq!(unchanged.id, accepted.id);
        assert_eq!(unchanged.pcm.as_ref(), first.as_slice());
        assert!(Arc::ptr_eq(&unchanged.pcm, &accepted.pcm));

        finish(&mut sink, first.len());
        assert_eq!(sink.submit(&[7, 8]), Err(-22));
        assert_eq!(sink.snapshot().unwrap().id, accepted.id);
        sink.submit(&competing).unwrap();
        let replacement = sink.snapshot().unwrap();
        assert_eq!(replacement.id, accepted.id + 1);
        assert_eq!(replacement.pcm.as_ref(), competing.as_slice());
        assert!(sink.busy());
    }

    #[test]
    fn an_inflight_download_snapshot_outlives_two_replacements_and_its_caller() {
        let mut sink = Sink::new();
        let mut caller = pcm(1_300, 29);
        let expected = caller.clone();
        sink.submit(&caller).unwrap();
        let download = sink.snapshot().unwrap();
        caller.fill(0);
        drop(caller);
        // A slow bounded download can retain an immutable old Arc while the
        // latest accepted clip changes. No old PCM is overwritten in place.
        finish(&mut sink, expected.len());
        let second = pcm(4, 133);
        sink.submit(&second).unwrap();
        finish(&mut sink, second.len());
        let third = pcm(9, 215);
        sink.submit(&third).unwrap();
        let latest = sink.snapshot().unwrap();
        assert_eq!(latest.id, download.id + 2);
        assert_eq!(latest.pcm.as_ref(), third.as_slice());
        assert!(!Arc::ptr_eq(&download.pcm, &latest.pcm));
        assert_eq!(download.pcm.as_ref(), expected.as_slice());
        drop(sink);
        assert_eq!(download.pcm.as_ref(), expected.as_slice());
    }

    #[test]
    fn sink_retains_only_the_last_clip_when_no_download_keeps_an_old_snapshot() {
        let mut sink = Sink::new();
        let first = pcm(32, 17);
        sink.submit(&first).unwrap();
        let weak_first = {
            let snapshot = sink.snapshot().unwrap();
            Arc::downgrade(&snapshot.pcm)
        };
        finish(&mut sink, first.len());
        assert!(weak_first.upgrade().is_some(), "completed clip remains replayable");
        let second = pcm(3, 51);
        sink.submit(&second).unwrap();
        assert!(weak_first.upgrade().is_none(), "replacement releases old storage");
        let weak_last = {
            let snapshot = sink.snapshot().unwrap();
            assert_eq!(snapshot.pcm.len(), second.len());
            Arc::downgrade(&snapshot.pcm)
        };
        finish(&mut sink, second.len());
        assert!(weak_last.upgrade().is_some());
        drop(sink);
        assert!(weak_last.upgrade().is_none());
    }
}
'''


def main():
    work_root = ROOT / "target/audio-http-prefix-tests"
    work_root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="case-", dir=work_root) as directory:
        work = Path(directory)
        source = work / "tests.rs"
        binary = work / "tests"
        source.write_text(HARNESS.replace("@SINK@", str(ROOT / "src/r/services/audio_http/sink.rs")))
        subprocess.run([
            "rustup", "run", "nightly-2026-07-10", "rustc", "--edition=2024", "--test",
            str(source), "-o", str(binary),
        ], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
