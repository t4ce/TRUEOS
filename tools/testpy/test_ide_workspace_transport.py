#!/usr/bin/env python3
"""Run the unchanged IDE router and its async-FS read adapter with host VMs."""

import json
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
mod allcaps { pub mod hv { pub const VM_ID_LIMIT: usize = 20; } }
mod allocators {
    pub fn with_host_alloc_domain_strong<R>(call: impl FnOnce() -> R) -> R { call() }
}
mod r {
    #[allow(dead_code)]
    #[path = @CABI_CODES@] pub mod cabi_codes;
    pub mod services { pub(crate) use crate::ide_workspace_service; }
    pub mod io {
      pub mod async_fs_cabi {
        pub fn canonical_file_key(path: &str) -> Result<String, i32> {
            Ok(if path.starts_with("trueosfs:disc") { path.to_owned() }
                else { format!("trueosfs:disc1/{path}") })
        }
      }
      pub mod env {
        pub fn resolve_fs_path(path: &str, _: bool) -> Option<String> {
            let mut normalized = Vec::new();
            for part in path.trim().split('/') {
                match part {
                    "" | "." => {},
                    ".." => return None,
                    part if part.contains('\0') => return None,
                    part => normalized.push(part),
                }
            }
            let path = normalized.join("/");
            if path.is_empty() { return None; }
            Some(if path.starts_with("common/") { format!("apps/{path}") } else { path })
        }
    } }
}
mod shell3 {
    pub struct MatrixTarget(pub u8);
    pub struct MatrixLease(pub String);
    impl MatrixLease {
        pub fn name(&self) -> &str { &self.0 }
        pub fn lifetime_generation(&self) -> u64 { 9 }
    }
    pub fn matrix_target_slot_lease(target: &MatrixTarget) -> MatrixLease {
        MatrixLease(format!("edit-{}", target.0))
    }
}
mod hv {
    use std::sync::Mutex;
    #[derive(Clone, Copy)]
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
    pub struct Vm { pub state: State, pub run: u64, pub instance: Option<[u8; 16]> }
    pub static VMS: Mutex<[Vm; 20]> = Mutex::new([Vm {
        state: State::live(), run: 1, instance: Some([1; 16]),
    }; 20]);
    pub struct Identity { pub instance: [u8; 16] }
    pub fn vm_run_generation(vm: u8) -> Option<u64> {
        VMS.lock().unwrap().get(vm as usize).map(|vm| vm.run)
    }
    pub fn vm_state(vm: u8) -> State { VMS.lock().unwrap()[vm as usize].state }
    pub fn blueprint_instance_identity(vm: u8) -> Option<Identity> {
        VMS.lock().unwrap().get(vm as usize)?.instance.map(|instance| Identity { instance })
    }
    pub fn blueprint_console_target(vm: u8) -> Option<crate::shell3::MatrixTarget> {
        Some(crate::shell3::MatrixTarget(vm))
    }
    pub fn with_guest_broker_context<R>(_: u8, call: impl FnOnce() -> R) -> R { call() }
}

#[path = @SOURCE@] mod ide_workspace_service;

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
    fn lock_operations() -> MutexGuard<'static, BTreeMap<u32, Operation>> { OPERATIONS.lock().unwrap() }
    fn start_completed_read_inner(owner: u32, bytes: Vec<u8>) -> i32 {
        let mut operations = lock_operations();
        if operations.len() >= CAPACITY.load(Ordering::SeqCst) { return FS_ERR_NO_SPACE; }
        let id = SEQUENCE.fetch_add(1, Ordering::SeqCst);
        operations.insert(id, Operation { owner, state: OperationState::Read(bytes) });
        id as i32
    }
    @RESERVED_READ@

    #[test]
    fn exported_transport_preserves_messages_at_capacity_and_binds_vm_incarnations() {
        use crate::ide_workspace_service as service;
        use crate::r::cabi_codes::*;
        use serde_json::{Value, json};
        const OWNER: u32 = 0x8000_0000;
        const CLIENT: u32 = 0x8000_0001;
        let write = |owner, path, payload: Value| {
            service::write_for_owner(owner, path, &serde_json::to_vec(&payload).unwrap())
        };
        let registration = json!({"version":1,"project":"demo","database":"common/ide/projects.redb"});
        assert_eq!(write(0, "vFile:ide/register", registration.clone()), Err(FS_ERR_NOT_FOUND));
        write(OWNER, "vFile:ide/register", registration.clone()).unwrap();
        assert_eq!(write(CLIENT, "vFile:ide/register", json!({"version":1,"project":"other",
            "database":"/apps/common/ide/projects.redb"})), Err(FS_ERR_ALREADY_EXISTS));
        let descriptors: Value = serde_json::from_slice(&service::read_for_owner(CLIENT, "vFile:ide/discover").unwrap()).unwrap();
        let endpoint = descriptors[0]["endpoint"].as_u64().unwrap();
        assert_eq!(descriptors[0]["matrix_slot"], "edit-0");
        assert_eq!(descriptors[0]["matrix_lifetime"], 9);
        write(CLIENT, "vFile:ide/request", json!({"endpoint":endpoint,"request_id":1,"command":{"op":"context"}})).unwrap();
        CAPACITY.store(0, Ordering::SeqCst);
        assert_eq!(start_ide_workspace_read(OWNER, "vFile:ide/poll"), FS_ERR_NO_SPACE);
        CAPACITY.store(64, Ordering::SeqCst);
        let operation = start_ide_workspace_read(OWNER, "vFile:ide/poll");
        assert!(operation > 0);
        let OperationState::Read(delivery) = lock_operations().remove(&(operation as u32)).unwrap().state;
        let mut reply: Value = serde_json::from_slice(&delivery).unwrap();
        reply.as_object_mut().unwrap().remove("command");
        reply["reply"] = json!({"status":"ok"});
        write(OWNER, "vFile:ide/reply", reply.clone()).unwrap();
        CAPACITY.store(0, Ordering::SeqCst);
        assert_eq!(start_ide_workspace_read(CLIENT, "vFile:ide/reply"), FS_ERR_NO_SPACE);
        CAPACITY.store(64, Ordering::SeqCst);
        let result: Value = serde_json::from_slice(&service::read_for_owner(CLIENT, "vFile:ide/reply").unwrap()).unwrap();
        assert_eq!(result, reply);
        write(CLIENT, "vFile:ide/request", json!({"endpoint":endpoint,"request_id":2,"command":{"op":"context"}})).unwrap();
        service::write_for_owner(CLIENT, "vFile:ide/cancel", b"").unwrap();
        assert!(service::read_for_owner(OWNER, "vFile:ide/poll").unwrap().is_empty());
        write(CLIENT, "vFile:ide/request", json!({"endpoint":endpoint,"request_id":3,"command":{}})).unwrap();
        {
            let mut vms = crate::hv::VMS.lock().unwrap();
            vms[0].run = 2;
            vms[0].instance = Some([2;16]);
        }
        assert_eq!(service::read_for_owner(CLIENT, "vFile:ide/discover").unwrap(), b"[]");
        let result: Value = serde_json::from_slice(&service::read_for_owner(CLIENT, "vFile:ide/reply").unwrap()).unwrap();
        assert_eq!(result["reply"]["code"], "unavailable");
        write(OWNER, "vFile:ide/register", registration.clone()).unwrap();
        crate::hv::VMS.lock().unwrap()[0].state.paused = true;
        assert_eq!(service::read_for_owner(CLIENT, "vFile:ide/discover").unwrap(), b"[]");
        assert_eq!(write(CLIENT, "vFile:ide/register", registration), Err(FS_ERR_ALREADY_EXISTS));
        crate::hv::VMS.lock().unwrap()[0].state.paused = false;
        service::release_vm(0, 2);
        assert!(service::is_read("vFile:ide/reply") && service::is_write("vFile:ide/reply"));
        assert!(!service::is_read("vFile:ide/register") && !service::is_write("vFile:ide/discover"));
        assert_eq!(service::write_for_owner(CLIENT, "vFile:ide/cancel", b"{}"), Err(FS_ERR_BAD_PARAM));
        assert_eq!(service::write_for_owner(CLIENT, "vFile:ide/request", &vec![b'x';service::MAX_FRAME_BYTES+1]), Err(FS_ERR_TOO_LARGE));
    }
}
'''


def main() -> None:
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())
    assert channel
    adapter = (ROOT / "src/r/io/async_fs_cabi.rs").read_text()
    start = adapter.index("fn start_ide_workspace_read(")
    stop = adapter.index("\nfn start_codex_input_read(", start)
    with tempfile.TemporaryDirectory(prefix="trueos-ide-router-") as directory:
        directory = Path(directory)
        (directory / "Cargo.toml").write_text('''[package]
name = "trueos-ide-router-tests"
version = "0.1.0"
edition = "2024"
[workspace]
[lib]
path = "harness.rs"
[dependencies]
serde = { version = "1.0", default-features = false, features = ["alloc", "derive"] }
serde_json = { version = "1.0", default-features = false, features = ["alloc"] }
''')
        (directory / "harness.rs").write_text(HARNESS
            .replace("@SOURCE@", json.dumps(str(ROOT / "src/r/services/ide_workspace_service.rs")))
            .replace("@CABI_CODES@", json.dumps(str(ROOT / "src/r/cabi_codes.rs")))
            .replace("@RESERVED_READ@", adapter[start:stop]))
        subprocess.run(["cargo", "+" + channel.group(1), "test", "--offline", "--quiet",
                        "--target", "x86_64-unknown-linux-gnu", "--", "--test-threads=1"],
                       cwd=directory, check=True)


if __name__ == "__main__":
    main()
