#!/usr/bin/env python3
"""Exercise the production AccessKeyDB adapter with a host filesystem backend."""

from pathlib import Path
import json
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
use std::collections::BTreeMap;
use std::sync::{Mutex, atomic::{AtomicBool, AtomicUsize, Ordering}};
use trueos_crypto::{AccountId, KeyHandle, KeyRef, ProviderId};
use trueos_fs::{RecordKey, access::{AccessKeyDb, ContainerId, ContainerPolicy, GrantScope}};

static WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());
macro_rules! log_warn {
    (target: $target:expr; $($args:tt)*) => {
        $crate::WARNINGS.lock().unwrap().push(format!("{}: {}", $target, format_args!($($args)*)))
    };
}
pub(crate) use log_warn;
mod allcaps { pub mod hv { pub const VM_ID_LIMIT: usize = 4; } }
mod allocators {
    pub fn with_host_alloc_domain_strong<T>(f: impl FnOnce() -> T) -> T { f() }
}
mod hv { pub mod blueprint {
    pub fn app_fs_root_for_archive(archive: &str, _: &[u8]) -> String {
        format!("apps/{}", archive.strip_suffix(".bp").unwrap_or(archive))
    }
} }
mod disc { pub mod block {
    #[derive(Clone, Copy)] pub struct DeviceHandle(pub u32);
} }
mod r { pub mod fs { pub mod trueosfs {
    #[derive(Clone, Copy)] pub struct NodeInfo { pub record_key: trueos_fs::RecordKey }
    pub async fn node_info_async(disk: crate::disc::block::DeviceHandle, path: &str)
        -> Result<Option<NodeInfo>, &'static str> {
        crate::READS.fetch_add(1, crate::Ordering::Relaxed);
        std::future::poll_fn(|cx| {
            if crate::YIELD_ONCE.swap(false, crate::Ordering::Relaxed) {
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            } else { std::task::Poll::Ready(()) }
        }).await;
        crate::NODES.lock().unwrap().get(&(disk.0, path.to_owned()))
            .copied().unwrap_or(Ok(None)).map(|node| node.map(|record_key| NodeInfo { record_key }))
    }
} } }
#[path = "__ADAPTER__"] mod access_key_db;

type Node = Result<Option<RecordKey>, &'static str>;
static NODES: Mutex<BTreeMap<(u32, String), Node>> = Mutex::new(BTreeMap::new());
static READS: AtomicUsize = AtomicUsize::new(0);
static YIELD_ONCE: AtomicBool = AtomicBool::new(false);
const ALICE: AccountId = AccountId::new(1);
const BOB: AccountId = AccountId::new(2);

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(result) = future.as_mut().poll(&mut context) { return result; }
    }
}
fn reset() {
    for vm in 0..allcaps::hv::VM_ID_LIMIT { access_key_db::clear_vm(vm as u8); }
    access_key_db::configure_policy(|db| *db = AccessKeyDb::new());
    WARNINGS.lock().unwrap().clear(); NODES.lock().unwrap().clear();
    READS.store(0, Ordering::Relaxed); YIELD_ONCE.store(false, Ordering::Relaxed);
}
fn container() -> ContainerId { ContainerId::new("notes", "team") }
fn register() {
    access_key_db::configure_policy(|db| {
        db.register_container(container(), ContainerPolicy::Shared).unwrap();
        db.add_member(&container(), ALICE).unwrap();
        db.add_member(&container(), BOB).unwrap();
        db.register_key(RecordKey::Ffa);
    });
}
fn key(value: u8) -> RecordKey {
    RecordKey::Key(KeyRef::new(ProviderId::new([1; 16]).unwrap(), KeyHandle::new([value; 32]).unwrap()))
}
fn audit(vm: u8, disk: u32, path: &str, operation: &'static str, create: bool) {
    block_on(access_key_db::audit_path(vm, disc::block::DeviceHandle(disk), path, operation, create));
}
fn messages() -> Vec<String> { WARNINGS.lock().unwrap().clone() }

#[test] fn unknown_identity_and_container_warn_without_interrupting_the_caller() {
    reset();
    access_key_db::bind_launch(0, "notes.bp", "apps/notes_team", None);
    NODES.lock().unwrap().insert((7, "shared".into()), Ok(Some(key(1))));
    audit(0, 7, "shared", "read", false);
    assert_eq!(access_key_db::principal_for_vm(0), None);
    assert_eq!(messages().len(), 2);
    assert!(messages().iter().all(|line| line.contains("reason=MissingIdentity") && line.contains("mode=warn-only")));
    access_key_db::bind_launch(1, "notes.bp", "apps/notes_team", Some(ALICE));
    audit(1, 7, "shared", "write", false);
    assert!(messages().last().unwrap().contains("reason=UnknownContainer"));
    assert_eq!(READS.load(Ordering::Relaxed), 2);
}

#[test] fn missing_reads_create_defaults_and_metadata_errors_preserve_progress() {
    reset(); register();
    access_key_db::bind_launch(0, "notes.bp", "apps/notes_team", Some(ALICE));
    audit(0, 7, "missing", "read", false);
    assert!(messages().is_empty());
    audit(0, 7, "missing", "write", true);
    assert!(messages()[0].contains("key=Some(Ffa)"));
    assert!(messages()[0].contains("reason=MissingGrant"));
    NODES.lock().unwrap().insert((7, "broken".into()), Err("backend-offline"));
    audit(0, 7, "broken", "read", false);
    assert!(messages()[1].contains("reason=metadata-unavailable"));
    assert!(messages()[1].contains("backend-offline"));
    audit(0, 7, "", "metadata", false);
    access_key_db::clear_vm(0);
    audit(0, 7, "unbound", "read", false);
    assert_eq!(READS.load(Ordering::Relaxed), 3);
}

#[test] fn resolved_disk_and_launch_account_determine_the_audited_identity() {
    reset(); register();
    access_key_db::configure_policy(|db| {
        db.grant(ALICE, GrantScope::Container(container()), RecordKey::Ffa).unwrap();
        db.register_key(key(2));
    });
    access_key_db::bind_launch(0, "notes.bp", "apps/notes_team", Some(ALICE));
    access_key_db::bind_launch(1, "notes.bp", "apps/notes_team", Some(BOB));
    NODES.lock().unwrap().insert((7, "shared".into()), Ok(Some(RecordKey::Ffa)));
    NODES.lock().unwrap().insert((8, "shared".into()), Ok(Some(key(2))));
    audit(0, 7, "shared", "read", false);
    assert!(messages().is_empty());
    audit(1, 7, "shared", "read", false);
    assert!(messages()[0].contains("account=Some(2)"));
    assert!(messages()[0].contains("vm=1"));
    audit(0, 8, "shared", "read", false);
    assert_eq!(messages().len(), 2);
    assert!(messages()[1].contains("account=Some(1)"));
    assert!(messages()[1].contains("key=Some(Key("));
}

#[test] fn implicit_container_zero_keeps_the_standard_file_key_independent() {
    reset();
    let default = ContainerId::default_for_app("notes");
    assert_eq!(default.id(), "0");
    assert_eq!(default.workspace_root(), "apps/notes");
    assert_eq!(container().workspace_root(), "apps/notes_team");
    access_key_db::configure_policy(|db| {
        db.register_container(default.clone(), ContainerPolicy::Private { owner: ALICE }).unwrap();
        db.add_member(&default, ALICE).unwrap();
    });
    access_key_db::bind_launch(0, "notes.bp", "apps/notes", Some(ALICE));
    assert!(messages().is_empty());
    NODES.lock().unwrap().insert((7, "apps/notes/file".into()), Ok(Some(RecordKey::Ffa)));
    audit(0, 7, "apps/notes/file", "read", false);
    assert!(messages()[0].contains("reason=MissingGrant"));
    assert!(messages()[0].contains("key=Some(Ffa)"));
    // Container zero is an installation identity, not a grant of the standard
    // file key. Its explicit grant must not spill into a second installation.
    access_key_db::configure_policy(|db| {
        db.grant(ALICE, GrantScope::Container(default), RecordKey::Ffa).unwrap();
        db.register_container(container(), ContainerPolicy::Private { owner: ALICE }).unwrap();
        db.add_member(&container(), ALICE).unwrap();
    });
    audit(0, 7, "apps/notes/file", "read", false);
    assert_eq!(messages().len(), 1);
    access_key_db::bind_launch(1, "notes.bp", "apps/notes_team", Some(ALICE));
    audit(1, 7, "apps/notes/file", "read", false);
    assert_eq!(messages().len(), 2);
    assert!(messages()[1].contains("vm=1"));
    assert!(messages()[1].contains("reason=MissingGrant"));
}

#[test] fn noncanonical_workspace_cannot_borrow_container_zero_grants() {
    reset();
    let default = ContainerId::default_for_app("notes");
    access_key_db::configure_policy(|db| {
        db.register_container(default.clone(), ContainerPolicy::Private { owner: ALICE }).unwrap();
        db.add_member(&default, ALICE).unwrap();
        db.register_key(RecordKey::Ffa);
        db.grant(ALICE, GrantScope::Container(default), RecordKey::Ffa).unwrap();
    });
    for invalid in ["apps/notes_0", "apps/notes_", "apps/notes_.", "apps/notes_..", "apps/notes_team/nested", "apps/notes/team", "apps/other"] {
        access_key_db::bind_launch(0, "notes.bp", "apps/notes", Some(ALICE));
        assert_eq!(access_key_db::principal_for_vm(0), Some(ALICE));
        WARNINGS.lock().unwrap().clear();
        access_key_db::bind_launch(0, "notes.bp", invalid, Some(ALICE));
        assert_eq!(access_key_db::principal_for_vm(0), None);
        assert_eq!(messages().len(), 1);
        assert!(messages()[0].contains("reason=invalid-container-path"));
        assert!(messages()[0].contains("mode=warn-only"));
        audit(0, 7, "shared", "read", false);
    }
    assert_eq!(READS.load(Ordering::Relaxed), 0);
}

#[test] fn another_shared_member_cannot_replace_the_frozen_resume_identity() {
    reset(); register();
    access_key_db::configure_policy(|db| {
        db.grant(ALICE, GrantScope::Container(container()), RecordKey::Ffa).unwrap();
        db.register_key(key(2));
        db.grant(BOB, GrantScope::Container(container()), key(2)).unwrap();
    });
    access_key_db::bind_launch(0, "notes.bp", "apps/notes_team", Some(ALICE));
    access_key_db::audit_resume(0, Some(ALICE));
    assert!(messages().is_empty());
    access_key_db::audit_resume(0, Some(BOB));
    access_key_db::audit_resume(0, Some(BOB));
    assert_eq!(messages().len(), 1);
    assert!(messages()[0].contains("reason=PrincipalMismatch"));
    assert!(messages()[0].contains("account=Some(1) actor=Some(2) op=resume"));
    access_key_db::audit_resume(0, None);
    assert_eq!(messages().len(), 2);
    assert!(messages()[1].contains("reason=MissingIdentity"));
    assert!(messages()[1].contains("actor=None"));
    assert_eq!(access_key_db::principal_for_vm(0), Some(ALICE));
    access_key_db::audit_key(0, RecordKey::Ffa, "read");
    assert_eq!(messages().len(), 2);
    access_key_db::audit_key(0, key(2), "read");
    assert_eq!(messages().len(), 3);
    assert!(messages()[2].contains("reason=MissingGrant"));
    assert!(messages()[2].contains("account=Some(1)"));
}

#[test] fn warnings_are_bounded_and_policy_changes_reopen_diagnostics() {
    reset(); register();
    access_key_db::bind_launch(0, "notes.bp", "apps/notes_team", Some(ALICE));
    access_key_db::audit_key(0, key(1), "read");
    access_key_db::audit_key(0, key(1), "read");
    assert_eq!(messages().len(), 1);
    for value in 2..=100 { access_key_db::audit_key(0, key(value), "read"); }
    assert_eq!(messages().len(), 32);
    access_key_db::audit_key(0, key(1), "read");
    assert_eq!(messages().len(), 32);
    access_key_db::configure_policy(|_| {});
    access_key_db::audit_key(0, RecordKey::Ffa, "write");
    assert_eq!(messages().len(), 33);
    access_key_db::configure_policy(|db| {
        db.grant(ALICE, GrantScope::Container(container()), RecordKey::Ffa).unwrap();
    });
    access_key_db::audit_key(0, RecordKey::Ffa, "write");
    assert_eq!(messages().len(), 33);
}

#[test] fn persisted_owner_grants_are_reused_by_two_instances_and_never_by_another_account() {
    reset();
    let root = "apps/owned-first";
    access_key_db::grant_owned_workspace("owned-first.bp", root, ALICE, key(8)).unwrap();
    access_key_db::grant_owned_workspace("owned-first.bp", root, ALICE, key(8)).unwrap();
    access_key_db::bind_launch(0, "owned-first.bp", root, Some(ALICE));
    access_key_db::bind_launch(1, "owned-first.bp", root, Some(ALICE));
    access_key_db::bind_launch(2, "owned-first.bp", root, Some(BOB));
    for vm in [0, 1] {
        assert_eq!(access_key_db::authorize_record(vm, key(8)), Ok(()));
        assert_eq!(access_key_db::authorize_protected_workspace(vm, key(8)), Ok(()));
        assert_eq!(access_key_db::workspace_for_vm(vm).as_deref(), Some(root));
        assert!(access_key_db::authorize_protected_workspace(vm, RecordKey::Ffa).is_err());
        assert!(access_key_db::authorize_protected_workspace(vm, key(9)).is_err());
    }
    assert!(access_key_db::authorize_record(2, key(8)).is_err());
    assert!(access_key_db::authorize_protected_workspace(2, key(8)).is_err());
    assert!(access_key_db::grant_owned_workspace("owned-first.bp", root, BOB, key(8)).is_err());
    assert!(access_key_db::grant_owned_workspace("owned-first.bp", root, ALICE, key(9)).is_err());
    assert!(access_key_db::grant_owned_workspace("owned-first.bp", root, ALICE, RecordKey::Ffa).is_err());
    assert_eq!(access_key_db::authorize_vm_control(0, Some(ALICE)), Ok(()));
    assert!(access_key_db::authorize_vm_control(0, Some(BOB)).is_err());
    assert!(access_key_db::authorize_vm_control(0, None).is_err());
    assert_eq!(access_key_db::authorize_record(3, RecordKey::Ffa), Ok(()));
    access_key_db::clear_vm(0);
    assert!(access_key_db::authorize_record(0, key(8)).is_err());
}

#[test] fn awaiting_old_metadata_cannot_attribute_a_warning_to_a_new_principal() {
    reset(); register();
    access_key_db::bind_launch(0, "notes.bp", "apps/notes_team", Some(ALICE));
    NODES.lock().unwrap().insert((7, "shared".into()), Ok(Some(RecordKey::Ffa)));
    YIELD_ONCE.store(true, Ordering::Relaxed);
    let mut audit = std::pin::pin!(access_key_db::audit_path(0, disc::block::DeviceHandle(7), "shared", "read", false));
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(audit.as_mut().poll(&mut context).is_pending());
    access_key_db::clear_vm(0);
    access_key_db::bind_launch(0, "notes.bp", "apps/notes_team", Some(BOB));
    assert!(audit.as_mut().poll(&mut context).is_ready());
    assert!(messages().is_empty());
    assert_eq!(access_key_db::principal_for_vm(0), Some(BOB));
}
'''


def main():
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-access-key-adapter-") as temporary:
        directory = Path(temporary)
        manifest = '\n'.join([
            '[package]', 'name = "trueos-access-key-adapter-tests"', 'version = "0.0.0"',
            'edition = "2024"', '[lib]', 'path = "tests.rs"', '[dependencies]',
            f'trueos-fs = {{ path = {json.dumps(str(ROOT / "crates/trueos-fs"))} }}',
            f'trueos-crypto = {{ path = {json.dumps(str(ROOT / "crates/trueos-crypto"))} }}',
            'spin = "0.10"', '',
        ])
        (directory / "Cargo.toml").write_text(manifest)
        (directory / "tests.rs").write_text(HARNESS.replace("__ADAPTER__", str(ROOT / "src/access_key_db.rs")))
        # Run outside the kernel workspace to avoid its custom target/build-std
        # configuration. Dependencies and adapter source are still production.
        subprocess.run([
            "cargo", "+" + channel, "test", "--offline", "--lib", "--target",
            "x86_64-unknown-linux-gnu", "--", "--test-threads=1",
        ], cwd=directory, check=True)


if __name__ == "__main__":
    main()
