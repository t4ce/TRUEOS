#!/usr/bin/env python3
"""Exercise deferred launch failure reporting against the production Matrix echo."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
use std::collections::VecDeque;
use std::sync::Mutex;

struct Lock<T>(Mutex<T>);
impl<T> Lock<T> {
    const fn new(value: T) -> Self { Self(Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
static EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static EXPECT_NOTICE_AT_DISCARD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
mod log_os {
    pub fn blueprint_important_line(message: std::fmt::Arguments<'_>) {
        crate::EVENTS.lock().unwrap().push("log".into());
        crate::LOGS.lock().unwrap().push(message.to_string());
    }
}
mod shell3 {
    use super::{Lock, VecDeque};
    #[derive(Clone)] pub struct MatrixTarget { pub slot: String, pub lifetime: u64 }
    struct State {
        lifetimes: Vec<(String, u64)>,
        echoes: Vec<(Option<String>, VecDeque<String>)>,
        generation: u64,
    }
    static SLOTS: Lock<State> = Lock::new(State { lifetimes: Vec::new(), echoes: Vec::new(), generation: 0 });
    fn matrix_slots() -> &'static Lock<State> { &SLOTS }
    pub struct MatrixSlots;
    impl MatrixSlots {
        @ECHO@
    }
    pub mod service {
        pub fn notify_work() {}
        @REPORT@
    }
    pub fn reset() -> MatrixTarget {
        *SLOTS.lock() = State {
            lifetimes: vec![("code".into(), 9)],
            echoes: vec![(Some("code".into()), VecDeque::from(["launch pending".into()]))],
            generation: 0,
        };
        MatrixTarget { slot: "code".into(), lifetime: 9 }
    }
    pub fn discard(target: &MatrixTarget) {
        let mut slots = SLOTS.lock();
        if crate::EXPECT_NOTICE_AT_DISCARD.load(std::sync::atomic::Ordering::SeqCst) {
            assert!(slots.echoes.iter().any(|(id, lines)| id.is_none() && !lines.is_empty()),
                "report the persistent bare-slot notice before deleting the failed launch slot");
        }
        slots.echoes.retain(|(id, _)| id.as_deref() != Some(target.slot.as_str()));
        slots.lifetimes.retain(|(id, lifetime)| id != &target.slot || *lifetime != target.lifetime);
        crate::EVENTS.lock().unwrap().push("discard".into());
    }
    pub fn default_lines() -> Vec<String> {
        SLOTS.lock().echoes.iter().find(|(id, _)| id.is_none())
            .map(|(_, lines)| lines.iter().cloned().collect()).unwrap_or_default()
    }
    pub fn launch_slot_exists() -> bool { !SLOTS.lock().lifetimes.is_empty() }
}
mod hv {
    #[derive(Debug)] pub enum StartError { CodexAlreadyRunning, Other }
    pub mod matrix { pub fn discard(target: &crate::shell3::MatrixTarget) { crate::shell3::discard(target); } }
    pub mod launcher {
        use alloc::string::String;
        pub struct Request { pub archive: String, pub frontend: Option<()> }
        pub trait LaunchObserver {
            fn finished(&self, target: &crate::shell3::MatrixTarget, vm: Option<u8>, error: Option<&str>, reason: Option<&str>);
        }
        pub fn message(error: super::StartError) -> String { match error @MAPPING@ }
        pub fn complete(request: Request, target: crate::shell3::MatrixTarget,
            observer: Option<Box<dyn LaunchObserver>>, vm: Option<u8>, error: Option<String>, reason: Option<String>) {
            @FINISH@;
            finish(vm, error, reason);
        }
    }
}

const CODEX_NOTICE: &str = "One Codex CLI is already running. Launching another is currently disabled.";
fn setup() -> shell3::MatrixTarget {
    EVENTS.lock().unwrap().clear(); LOGS.lock().unwrap().clear();
    EXPECT_NOTICE_AT_DISCARD.store(false, std::sync::atomic::Ordering::SeqCst);
    shell3::reset()
}
fn request(visible: bool) -> hv::launcher::Request {
    hv::launcher::Request { archive: "codex.bp".into(), frontend: visible.then_some(()) }
}

#[test]
fn deferred_codex_rejection_survives_the_failed_slot_being_discarded() {
    let target = setup();
    let error = hv::launcher::message(hv::StartError::CodexAlreadyRunning);
    assert_eq!(error, CODEX_NOTICE);
    EXPECT_NOTICE_AT_DISCARD.store(true, std::sync::atomic::Ordering::SeqCst);
    hv::launcher::complete(request(true), target, None, None, Some(error), None);
    assert_eq!(shell3::default_lines(), [CODEX_NOTICE]);
    assert!(!shell3::launch_slot_exists());
    assert!(LOGS.lock().unwrap()[0].contains(CODEX_NOTICE));
    assert_eq!(*EVENTS.lock().unwrap(), ["log", "discard"]);
}

#[test]
fn headless_rejection_stays_in_logs_without_a_default_slot_notice() {
    let target = setup();
    hv::launcher::complete(request(false), target, None, None,
        Some(hv::launcher::message(hv::StartError::CodexAlreadyRunning)), None);
    assert!(shell3::default_lines().is_empty());
    assert_eq!(LOGS.lock().unwrap().len(), 1);
    assert!(!shell3::launch_slot_exists());
}

#[test]
fn success_does_not_emit_a_failure_or_discard_the_live_slot() {
    let target = setup();
    hv::launcher::complete(request(true), target, None, Some(4), None, Some("user exit".into()));
    assert!(shell3::default_lines().is_empty());
    assert!(LOGS.lock().unwrap().is_empty());
    assert!(shell3::launch_slot_exists());
}

#[test]
fn cancelled_queued_launch_is_discarded_without_an_error_notice() {
    let target = setup();
    hv::launcher::complete(request(true), target, None, None, None, None);
    assert!(shell3::default_lines().is_empty());
    assert!(LOGS.lock().unwrap().is_empty());
    assert!(!shell3::launch_slot_exists());
}

#[test]
fn other_visible_admission_errors_also_reach_the_default_slot() {
    let target = setup();
    let error = hv::launcher::message(hv::StartError::Other);
    assert_eq!(error, "Other");
    hv::launcher::complete(request(true), target, None, None, Some(error), None);
    assert_eq!(shell3::default_lines(), ["Other"]);
}
'''


def main():
    launcher = (ROOT / 'src/hv/launcher.rs').read_text()
    service = (ROOT / 'src/shell3/service.rs').read_text()
    shell = (ROOT / 'src/shell3/shell3.rs').read_text()
    mapping = item(launcher, '.map_err(|error| match error {').split('match error ', 1)[1]
    source = HARNESS.replace('@FINISH@', item(launcher, 'let finish = |vm, error: Option<String>, reason: Option<String>|'))
    source = source.replace('@MAPPING@', mapping)
    source = source.replace('@REPORT@', item(service, 'pub(crate) fn report_launch_error('))
    source = source.replace('@ECHO@', item(shell, 'fn echo('))
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / 'rust-toolchain.toml').read_text())[1]
    with tempfile.TemporaryDirectory(prefix='trueos-launch-failure-') as directory:
        directory = Path(directory)
        path = directory / 'tests.rs'
        path.write_text(source)
        subprocess.run(['rustc', '+' + channel, '--edition=2024', '--test', str(path), '-o', str(directory / 'tests')], check=True)
        subprocess.run([str(directory / 'tests'), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
