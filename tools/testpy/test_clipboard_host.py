#!/usr/bin/env python3
"""Exercise the actual clipboard authority and Blueprint adapter without booting."""
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code, unused_variables)]
extern crate alloc;
extern crate self as embassy_time_driver;
extern crate self as trueos_vm;
pub const TICK_HZ: u64 = 1_000;
pub fn now() -> u64 { CLOCK.fetch_add(100, std::sync::atomic::Ordering::SeqCst) }
static CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
mod crypt {
    pub static AUTH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    pub fn has_authenticated_two_factor_session(scope: u8) -> bool {
        scope == 2 && AUTH.load(std::sync::atomic::Ordering::SeqCst)
    }
}
mod shell2 {
    use alloc::sync::Arc;
    pub const TRANSPORT_LOCAL_SCOPE: u8 = 2;
    #[derive(Clone, Debug, Eq, PartialEq)] pub struct MatrixSlotLease(u64);
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct MatrixSlotAttachmentId(u64);
    pub enum MatrixSlotAttachmentError { TargetExpired, Capacity }
    pub trait MatrixSlotAttachment: Send + Sync { fn on_matrix_slot_freed(&self, lease: &MatrixSlotLease); }
    pub struct MatrixTarget;
    static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    static ATTACHMENTS: std::sync::Mutex<Vec<(MatrixSlotLease, MatrixSlotAttachmentId, Arc<dyn MatrixSlotAttachment>)>> = std::sync::Mutex::new(Vec::new());
    pub fn matrix_target_slot_lease(_: &MatrixTarget) -> MatrixSlotLease { MatrixSlotLease(GENERATION.load(std::sync::atomic::Ordering::SeqCst)) }
    pub fn matrix_target_auth_scope(_: &MatrixTarget) -> Option<u8> { Some(2) }
    pub fn matrix_slot_is_live(lease: &MatrixSlotLease) -> bool { lease.0 == GENERATION.load(std::sync::atomic::Ordering::SeqCst) }
    pub fn attach_matrix_slot_resource(lease: &MatrixSlotLease, resource: Arc<dyn MatrixSlotAttachment>) -> Result<MatrixSlotAttachmentId, MatrixSlotAttachmentError> {
        if !matrix_slot_is_live(lease) { return Err(MatrixSlotAttachmentError::TargetExpired); }
        let mut attachments = ATTACHMENTS.lock().unwrap();
        let id = MatrixSlotAttachmentId(attachments.len() as u64 + 1);
        attachments.push((lease.clone(), id, resource)); Ok(id)
    }
    pub fn detach_matrix_slot_resource(lease: &MatrixSlotLease, id: MatrixSlotAttachmentId) -> bool {
        let mut attachments = ATTACHMENTS.lock().unwrap();
        let len = attachments.len(); attachments.retain(|(l, i, _)| l != lease || *i != id); len != attachments.len()
    }
    pub fn free_slot() {
        GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let attachments = std::mem::take(&mut *ATTACHMENTS.lock().unwrap());
        for (lease, _, resource) in attachments { resource.on_matrix_slot_freed(&lease); }
    }
}
mod hv {
    pub fn current_hull_guest_context_vm_id() -> Option<u8> { None }
    pub fn current_guest_execution_context_vm_id() -> Option<u8> { Some(1) }
    pub fn blueprint_console_target(_: u8) -> Option<crate::shell2::MatrixTarget> { Some(crate::shell2::MatrixTarget) }
}
pub mod vmcall {
    pub const OP_BP_CLIPBOARD_COMMAND_V1: u32 = 0x230;
    pub const STATUS_OK: u32 = 0;
    pub fn call_with_payload(_: u32, _: u64, _: u64, _: &[u8], _: &mut [u8]) -> (u32, u64) { panic!("host tests must not enter guest transport") }
}
mod r { pub mod services { #[path = SERVICE_PATH] pub mod clipboard_service; } }
mod ui4 {
    pub mod blueprint_text {
        use alloc::vec::Vec;
        use spin::Mutex;
        #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum WindowOwner { Vm(u8), Kernel }
        #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct WindowId(pub u32);
        pub const ERROR_INVALID: i32 = -100;
        pub const ERROR_CONTEXT: i32 = -101;
        pub const ERROR_NOT_FOUND: i32 = -102;
        pub const ERROR_UI4: i32 = -103;
        pub struct Surface { pub owner: WindowOwner, pub window: WindowId }
        pub static SURFACES: Mutex<Vec<Surface>> = Mutex::new(Vec::new());
        pub fn blueprint_owner() -> Option<WindowOwner> { Some(WindowOwner::Vm(1)) }
        pub fn surface_mut(surfaces: &mut [Surface], owner: WindowOwner, window: u32) -> Option<&mut Surface> { surfaces.iter_mut().find(|s| s.owner == owner && s.window.0 == window) }
        #[path = ADAPTER_PATH] pub mod clipboard_api;
        pub fn close_window(owner: WindowOwner, window: WindowId) { clipboard_api::release_window(owner, window); }
    }
}
#[cfg(test)] mod integration {
    use super::*;
    use ui4::blueprint_text::{self as bp, clipboard_api as api, WindowOwner, WindowId};
    fn command(window: u32, action: u32, kind: u32, text: &[u8], output: &mut [u8]) -> i32 {
        unsafe { api::trueos_cabi_clipboard_command_v1(window, action, kind, text.as_ptr(), text.len(), output.as_mut_ptr(), output.len()) }
    }
    fn setup() {
        api::release_owner(WindowOwner::Vm(1));
        shell2::free_slot();
        crypt::AUTH.store(false, std::sync::atomic::Ordering::SeqCst);
        *bp::SURFACES.lock() = vec![bp::Surface { owner: WindowOwner::Vm(1), window: WindowId(7) }];
    }
    #[test] fn rejected_secure_copy_never_replaces_plain_clip_and_poll_cannot_request_paste() {
        setup();
        assert_eq!(command(7, 1, 1, b"plain", &mut []), 0);
        assert_eq!(command(7, 1, 2, b"secret", &mut []), -12);
        assert_eq!(command(7, 2, 1, b"", &mut []), 0);
        let mut output = [0u8; 512];
        assert_eq!(command(7, 3, 1, b"", &mut output), 0);
        api::trusted_paste(WindowOwner::Vm(1), WindowId(7));
        assert_eq!(command(7, 3, 1, b"", &mut output), 5);
        assert_eq!(&output[..5], b"plain");
        assert_eq!(command(7, 3, 1, b"", &mut output), 0);
    }
    #[test] fn secure_copy_and_paste_remain_typed_and_authenticated() {
        setup(); crypt::AUTH.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(command(7, 1, 2, b"secret", &mut []), 0);
        assert_eq!(command(7, 2, 1, b"", &mut []), 0);
        let mut output = [0u8; 512];
        api::trusted_paste(WindowOwner::Vm(1), WindowId(7));
        assert_eq!(command(7, 3, 1, b"", &mut output), -14);
        assert_eq!(output, [0; 512]);
        assert_eq!(command(7, 2, 2, b"", &mut []), 0);
        crypt::AUTH.store(false, std::sync::atomic::Ordering::SeqCst);
        api::trusted_paste(WindowOwner::Vm(1), WindowId(7));
        assert_eq!(command(7, 3, 2, b"", &mut output), -12);
        crypt::AUTH.store(true, std::sync::atomic::Ordering::SeqCst);
        api::trusted_paste(WindowOwner::Vm(1), WindowId(7));
        assert_eq!(command(7, 3, 2, b"", &mut output), 6);
        assert_eq!(&output[..6], b"secret");
    }
    #[test] fn delivery_cannot_cross_window_owner_focus_or_retired_slot() {
        setup(); crypt::AUTH.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(command(7, 1, 2, b"secret", &mut []), 0);
        assert_eq!(command(7, 2, 2, b"", &mut []), 0);
        let mut output = [0u8; 512];
        assert_eq!(command(8, 3, 2, b"", &mut output), bp::ERROR_NOT_FOUND);
        api::trusted_paste(WindowOwner::Vm(2), WindowId(7));
        api::trusted_paste(WindowOwner::Vm(1), WindowId(8));
        assert_eq!(command(7, 3, 2, b"", &mut output), 0);
        api::trusted_paste(WindowOwner::Vm(1), WindowId(7));
        shell2::free_slot();
        assert_eq!(command(7, 3, 2, b"", &mut output), -8);
        assert_eq!(command(7, 2, 2, b"", &mut []), 0);
        assert_eq!(command(7, 3, 2, b"", &mut output), 0);
        bp::close_window(WindowOwner::Vm(1), WindowId(7));
        assert_eq!(command(7, 3, 2, b"", &mut output), -8);
    }
}
'''

def main():
    with tempfile.TemporaryDirectory(prefix="trueos-clipboard-tests-") as directory:
        root = Path(directory)
        (root / "src").mkdir()
        (root / "Cargo.toml").write_text('''[package]
name = "trueos-clipboard-host-tests"
version = "0.1.0"
edition = "2024"
[dependencies]
heapless = "=0.9.3"
spin = "=0.10.1"
zeroize = { version = "1", features = ["alloc"] }
serde_json = { version = "1", default-features = false, features = ["alloc"] }
[workspace]
''')
        source = HARNESS.replace('SERVICE_PATH', json.dumps(str(ROOT / 'src/r/services/clipboard_service.rs'))).replace('ADAPTER_PATH', json.dumps(str(ROOT / 'src/ui4/blueprint_text/clipboard_api.rs')))
        (root / 'src/lib.rs').write_text(source)
        subprocess.run(['cargo', '+nightly-2026-07-10', 'test', '--offline', '--quiet', '--', '--test-threads=1'], cwd=root, check=True)

if __name__ == '__main__':
    main()
