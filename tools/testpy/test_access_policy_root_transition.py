#!/usr/bin/env python3
"""Keep broker readiness separate from storage discovery and authorization.

Reuse the production-backed workspace fixture with a changing root selector.
No production policy code is replaced: only mount discovery and storage I/O
are fixtures. An early rootless load must not cache an accountless decision.
"""
import json
from pathlib import Path
import re
import subprocess
import tempfile

from test_owned_workspace_provision import HARNESS, ROOT


TESTS = r'''
static ROOT_AVAILABLE: AtomicBool = AtomicBool::new(true);

fn transition_subject() -> RecordKey {
    let key = access_key_store::software_key([0x71; 32]).unwrap();
    access_key_db::bind_launch(1, "termdir.bp", "apps/termdir", Some(ALICE));
    key
}

#[test]
fn root_transition_later_mount_loads_grants_after_rootless_start() {
    reset();
    let key = transition_subject();
    let image = access_key_store::edit_admin(&[], &access_key_store::AdminChange::Grant {
        account: ALICE, app: "termdir".into(), key, enabled: true,
    }).unwrap();
    FILES.lock().unwrap().insert(access_key_workspace::CONFIGURATION_DB_PATH.into(), image);
    ROOT_AVAILABLE.store(false, Ordering::SeqCst);
    let early = run(access_key_workspace::load_admin_catalog()).unwrap();
    assert!(early.grants.is_empty());
    assert!(EVENTS.lock().unwrap().is_empty(), "no disk read without a discovered root");
    assert!(access_key_db::authorize_record(1, key).is_err());

    ROOT_AVAILABLE.store(true, Ordering::SeqCst);
    let later = run(access_key_workspace::load_admin_catalog()).unwrap();
    assert_eq!(later.grants, vec![(ALICE, String::from("termdir"), key)]);
    assert_eq!(*EVENTS.lock().unwrap(), vec!["read"]);
    assert_eq!(access_key_db::authorize_record(1, key), Ok(()));
    access_key_db::bind_launch(2, "termdir.bp", "apps/termdir", Some(BOB));
    assert!(access_key_db::authorize_record(2, key).is_err());
}

#[test]
fn root_transition_missing_catalog_does_not_grant_keyed_access() {
    reset(); ROOT_AVAILABLE.store(true, Ordering::SeqCst);
    let key = transition_subject();
    let empty = run(access_key_workspace::load_admin_catalog()).unwrap();
    assert!(empty.grants.is_empty());
    assert_eq!(*EVENTS.lock().unwrap(), vec!["read"]);
    assert!(access_key_db::authorize_record(1, key).is_err());
    assert!(FILES.lock().unwrap().is_empty(), "loading does not create a catalogue");
}

#[test]
fn root_transition_corrupt_catalog_and_io_failure_remain_errors() {
    reset(); ROOT_AVAILABLE.store(true, Ordering::SeqCst);
    let key = transition_subject();
    FILES.lock().unwrap().insert(access_key_workspace::CONFIGURATION_DB_PATH.into(), b"corrupt".to_vec());
    assert!(matches!(run(access_key_workspace::load_admin_catalog()),
        Err(r::io::kfs::FsError::Device(disc::block::Error::Corrupted))));
    assert!(access_key_db::authorize_record(1, key).is_err());
    FAIL_READ.store(true, Ordering::SeqCst);
    assert!(matches!(run(access_key_workspace::load_admin_catalog()),
        Err(r::io::kfs::FsError::Device(disc::block::Error::Io))));
    assert!(access_key_db::authorize_record(1, key).is_err());
    assert_eq!(*EVENTS.lock().unwrap(), vec!["read", "read"]);
}
'''


def main():
    old = "pub fn primary_root_handle()->Option<DeviceHandle>{Some(DeviceHandle)}"
    assert HARNESS.count(old) == 1
    harness = HARNESS.replace(old, "pub fn primary_root_handle()->Option<DeviceHandle>{ROOT_AVAILABLE.load(Ordering::SeqCst).then_some(DeviceHandle)}")
    harness = (harness + TESTS).replace("@ROOT@", str(ROOT))
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-access-policy-root-") as temporary:
        project = Path(temporary)
        (project / "lib.rs").write_text(harness)
        dependencies = "".join(
            f'{name}={{path={json.dumps(str(path))}' + (',features=["std"]' if name == "trueos-redb" else '') + '}\n'
            for name, path in (
                ("trueos-fs", ROOT / "crates/trueos-fs"),
                ("trueos-crypto", ROOT / "crates/trueos-crypto"),
                ("trueos-redb", ROOT.parent / "TRUEOS-Blueprints/crates/trueos-redb"),
            )
        )
        (project / "Cargo.toml").write_text(
            '[package]\nname="trueos-access-policy-root-tests"\nversion="0.0.0"\nedition="2024"\n'
            '[lib]\npath="lib.rs"\n[workspace]\n[dependencies]\n' + dependencies
            + 'spin="0.10"\nsha2="0.10.9"\nembassy-sync="0.8"\n'
        )
        subprocess.run([
            "cargo", "+" + channel, "test", "--offline", "--target", "x86_64-unknown-linux-gnu",
            "--target-dir", str(ROOT / "tgt/access-policy-root-tests-host"),
            "root_transition_", "--", "--test-threads=1",
        ], cwd=project, check=True)


if __name__ == "__main__":
    main()
