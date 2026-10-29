#!/usr/bin/env python3
"""Test the production Codex whole-message mailbox with host VM/time adapters.

The actual service is linked unchanged. Its inline model tests verify message,
scope, incarnation and readiness semantics; the adapter exercise additionally
checks the exported service functions against unavailable/paused/wrong-app VMs.
"""

import json
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "src/r/services/codex_input_service.rs"

HARNESS = r'''extern crate alloc;
extern crate self as spin;

pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}

mod allcaps { pub mod hv { pub const VM_ID_LIMIT: usize = 4; } }
mod allocators {
    pub fn with_host_alloc_domain_strong<R>(call: impl FnOnce() -> R) -> R { call() }
}
mod r {
    #[allow(dead_code)]
    #[path = @CABI_CODES@] pub mod cabi_codes;
    pub mod input_timing {
        pub static NOW: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        pub fn now_ms() -> u64 { NOW.load(std::sync::atomic::Ordering::SeqCst) }
    }
    pub mod services { pub(crate) use crate::codex_input_service; }
}
mod hv {
    use std::sync::Mutex;
    #[derive(Clone, Copy, Default)]
    pub struct State {
        pub supported: bool, pub running: bool, pub starting: bool,
        pub stop_requested: bool, pub pause_requested: bool,
        pub paused: bool, pub restore_inflight: bool,
    }
    impl State { pub const fn live() -> Self {
        Self { supported: true, running: true, starting: false, stop_requested: false,
            pause_requested: false, paused: false, restore_inflight: false }
    } }
    #[derive(Clone, Copy)]
    pub struct Vm {
        pub state: State, pub run: u64, pub instance: Option<[u8; 16]>, pub archive: &'static str,
    }
    pub static VMS: Mutex<[Vm; 4]> = Mutex::new([Vm {
        state: State::live(), run: 1, instance: Some([1; 16]), archive: "codex.bp",
    }; 4]);
    pub struct Identity { pub instance: [u8; 16] }
    pub fn vm_run_generation(vm: u8) -> Option<u64> {
        VMS.lock().unwrap().get(vm as usize).map(|vm| vm.run)
    }
    pub fn vm_state(vm: u8) -> State {
        VMS.lock().unwrap().get(vm as usize).map(|vm| vm.state).unwrap_or_default()
    }
    pub fn blueprint_current_run_archive(vm: u8, run: u64) -> Option<String> {
        VMS.lock().unwrap().get(vm as usize).filter(|vm| vm.run == run)
            .map(|vm| vm.archive.to_owned())
    }
    pub fn blueprint_instance_identity(vm: u8) -> Option<Identity> {
        VMS.lock().unwrap().get(vm as usize)?.instance.map(|instance| Identity { instance })
    }
}

#[path = @SOURCE@] mod codex_input_service;

mod async_fs_adapter {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
    use std::sync::{Mutex, MutexGuard};
    use crate::r::cabi_codes::{FS_ERR_NO_SPACE, FS_ERR_NOT_FOUND};
    enum OperationState { Read(Vec<u8>) }
    struct Operation { owner: u32, state: OperationState }
    static OPERATIONS: Mutex<BTreeMap<u32, Operation>> = Mutex::new(BTreeMap::new());
    static CAPACITY: AtomicUsize = AtomicUsize::new(64);
    static SEQUENCE: AtomicU32 = AtomicU32::new(1);
    fn lock_operations() -> MutexGuard<'static, BTreeMap<u32, Operation>> {
        OPERATIONS.lock().unwrap()
    }
    fn start_completed_read_inner(owner: u32, bytes: Vec<u8>) -> i32 {
        let mut operations = lock_operations();
        if operations.len() >= CAPACITY.load(Ordering::SeqCst) { return FS_ERR_NO_SPACE; }
        let id = SEQUENCE.fetch_add(1, Ordering::SeqCst);
        operations.insert(id, Operation { owner, state: OperationState::Read(bytes) });
        id as i32
    }
    @RESERVED_READ@

    #[test]
    fn production_async_fs_adapter_reserves_capacity_before_consuming_message() {
        const OWNER: u32 = 0x8000_0000;
        const PATH: &str = "vFile:codex-input/new";
        crate::r::input_timing::NOW.store(10_000, Ordering::SeqCst);
        {
            let mut vms = crate::hv::VMS.lock().unwrap();
            vms[0].run = 10;
            vms[0].instance = Some([10; 16]);
        }
        assert!(start_codex_input_read(OWNER, PATH) > 0);
        let target = crate::codex_input_service::current_target().unwrap();
        let text = "accepted whole text\n§ unchanged";
        crate::codex_input_service::submit(&target, text).unwrap();
        CAPACITY.store(0, Ordering::SeqCst);
        assert_eq!(start_codex_input_read(OWNER, PATH), FS_ERR_NO_SPACE);
        CAPACITY.store(64, Ordering::SeqCst);
        let id = start_codex_input_read(OWNER, PATH);
        assert!(id > 0);
        {
            let operations = lock_operations();
            let OperationState::Read(bytes) = &operations.get(&(id as u32)).unwrap().state;
            assert_eq!(bytes, text.as_bytes());
        }
        let id = start_codex_input_read(OWNER, PATH);
        {
            let operations = lock_operations();
            let OperationState::Read(bytes) = &operations.get(&(id as u32)).unwrap().state;
            assert!(bytes.is_empty());
        }
        crate::codex_input_service::read_for_owner(OWNER, "vFile:codex-input/offline").unwrap();
        crate::r::input_timing::NOW.store(0, Ordering::SeqCst);
    }
}

#[test]
fn public_boundary_requires_unique_live_exact_app_and_current_receiver() {
    use codex_input_service::{SubmitError, current_target, is_target_live, read_for_owner, submit};
    use r::cabi_codes::{FS_ERR_BAD_PARAM, FS_ERR_NOT_FOUND};
    use std::sync::atomic::Ordering;
    const OWNER: u32 = 0x8000_0000;
    const PATH: &str = "vFile:codex-input/new";
    {
        let mut vms = hv::VMS.lock().unwrap();
        vms[0].run = 1;
        vms[0].instance = Some([1; 16]);
    }
    assert!(current_target().is_none());
    assert_eq!(read_for_owner(0, PATH), Err(FS_ERR_NOT_FOUND));
    assert_eq!(read_for_owner(OWNER | 999, PATH), Err(FS_ERR_NOT_FOUND));
    assert_eq!(read_for_owner(OWNER, "vFile:codex-input/not-a-uuid"), Err(FS_ERR_BAD_PARAM));
    for archive in ["codex-check.bp", "my-codex.bp", "codex.bp.backup", "edit.bp"] {
        hv::VMS.lock().unwrap()[0].archive = archive;
        assert_eq!(read_for_owner(OWNER, PATH), Err(FS_ERR_NOT_FOUND));
        assert!(current_target().is_none());
    }
    hv::VMS.lock().unwrap()[0].archive = "codex.bp";
    assert!(read_for_owner(OWNER, PATH).unwrap().is_empty());
    let target = current_target().unwrap();
    for bit in 0..7 {
        let mut state = hv::State::live();
        match bit {
            0 => state.supported = false, 1 => state.running = false,
            2 => state.starting = true, 3 => state.stop_requested = true,
            4 => state.pause_requested = true, 5 => state.paused = true,
            _ => state.restore_inflight = true,
        }
        hv::VMS.lock().unwrap()[0].state = state;
        assert!(!is_target_live(&target));
        assert!(current_target().is_none());
        assert_eq!(submit(&target, "cannot send"), Err(SubmitError::Unavailable));
        assert_eq!(read_for_owner(OWNER, PATH), Err(FS_ERR_NOT_FOUND));
    }
    hv::VMS.lock().unwrap()[0].state = hv::State::live();
    hv::VMS.lock().unwrap()[0].instance = None;
    assert!(!is_target_live(&target));
    hv::VMS.lock().unwrap()[0].instance = Some([1; 16]);
    let text = "§ typed user input\nsecond line";
    submit(&target, text).unwrap();
    assert_eq!(read_for_owner(OWNER, PATH).unwrap(), text.as_bytes());
    assert!(read_for_owner(OWNER, PATH).unwrap().is_empty());
    // A second valid VM makes new context clicks ambiguous, while a frame
    // already bound to the first receiver keeps its exact destination.
    read_for_owner(OWNER | 1, PATH).unwrap();
    assert!(current_target().is_none());
    assert!(is_target_live(&target));
    submit(&target, "still first VM").unwrap();
    assert!(current_target().is_none(), "queued input must not make the other VM uniquely selected");
    assert!(read_for_owner(OWNER | 1, PATH).unwrap().is_empty());
    assert_eq!(read_for_owner(OWNER, PATH).unwrap(), b"still first VM");
    read_for_owner(OWNER | 1, "vFile:codex-input/offline").unwrap();
    r::input_timing::NOW.store(2_000, Ordering::SeqCst);
    assert!(!is_target_live(&target));
    assert!(current_target().is_none());
    read_for_owner(OWNER, PATH).unwrap();
    let renewed = current_target().unwrap();
    assert_ne!(target, renewed);
    hv::VMS.lock().unwrap()[0].run = 2;
    assert!(!is_target_live(&renewed));
    assert_eq!(submit(&renewed, "old run"), Err(SubmitError::Unavailable));
    assert!(read_for_owner(OWNER, PATH).unwrap().is_empty());
    let new_run = current_target().unwrap();
    assert_ne!(renewed, new_run);
    read_for_owner(OWNER, "vFile:codex-input/offline").unwrap();
    assert!(!is_target_live(&new_run));
    assert!(current_target().is_none());
}
'''


def main() -> None:
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel, "repository Rust toolchain channel missing"
    adapter = (ROOT / "src/r/io/async_fs_cabi.rs").read_text()
    start = adapter.index("fn start_codex_input_read(")
    stop = adapter.index("\npub(crate) fn start_read(", start)
    reserved_read = adapter[start:stop]
    with tempfile.TemporaryDirectory(prefix="trueos-codex-input-") as directory:
        directory = Path(directory)
        harness = directory / "harness.rs"
        harness.write_text(HARNESS.replace("@SOURCE@", json.dumps(str(SOURCE)))
                           .replace("@CABI_CODES@", json.dumps(str(ROOT / "src/r/cabi_codes.rs")))
                           .replace("@RESERVED_READ@", reserved_read))
        binary = directory / "tests"
        subprocess.run(["rustc", "+" + channel.group(1), "--edition=2024", "--test",
                        str(harness), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
