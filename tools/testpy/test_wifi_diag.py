#!/usr/bin/env python3
"""Replay outage/recovery evidence through the production bounded diagnostics."""

from pathlib import Path
import json
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

SOURCE = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DriverStatus { Unloaded, Loading, Running }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WifiState { NoHardware, Disabled, Disconnected, Scanning, Connecting, Authenticating, Connected, Failed }
#[path = @DIAGNOSTICS@]
mod diagnostics;
'''


def run():
    with tempfile.TemporaryDirectory(prefix="trueos-wifi-diagnostics-") as temporary:
        directory = Path(temporary)
        source = directory / "tests.rs"
        executable = directory / "tests"
        source.write_text(SOURCE.replace("@DIAGNOSTICS@", json.dumps(str(ROOT / "src/net/wifi_diag.rs"))))
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)], check=True, timeout=15)
        subprocess.run([str(executable)], check=True, timeout=15)


if __name__ == "__main__":
    run()
