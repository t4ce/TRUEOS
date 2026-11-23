#!/usr/bin/env python3
"""Run the bare-metal RTP microphone state tests on the host."""
import json
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
extern crate self as v;

#[path = @VNET@] pub mod vnet;

#[macro_export]
macro_rules! log_info { (target: $target:expr; $($args:tt)*) => {{ let _ = format_args!($($args)*); }}; }

pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}

mod audio_http {
    const SAMPLE_RATE: usize = 48_000;
    const FRAME_BYTES: usize = 4;
    fn monotonic_millis() -> u64 { 0 }
    #[path = @MIC@] mod rtp_mic;
}
'''


def main():
    harness = HARNESS.replace('@VNET@', json.dumps(str(ROOT / 'crates/trueos-v/src/vnet.rs')))
    harness = harness.replace('@MIC@', json.dumps(str(ROOT / 'src/r/services/audio_http/rtp_mic.rs')))
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / 'rtp_mic.rs'
        binary = Path(directory) / 'test_rtp_mic'
        source.write_text(harness)
        subprocess.run(['rustc', '--edition=2024', '--test', str(source), '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
