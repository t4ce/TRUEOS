#!/usr/bin/env python3
"""Exercise the production Codex VM limit, including racing launchers."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = ROOT / "src/hv/codex_admission.rs"
    hv = (ROOT / "src/hv/mod.rs").read_text()
    admission = hv.split("fn start_with_mode(", 1)[1].split("\npub fn stop(", 1)[0]
    # Pair the production limiter tests with the lifecycle serialization that
    # keeps a retiring non-Codex occupant from releasing the next incarnation.
    assert admission.index("let _control = vm.lifecycle_control.lock();") < admission.index("CODEX_ADMISSION.claim(vm_id)") < admission.index("matrix::bind(")
    offline = hv.split("fn publish_vm_offline(", 1)[1].split("\nfn contains_bytes(", 1)[0]
    assert offline.index("let _control = vm.lifecycle_control.lock();") < offline.index("CODEX_ADMISSION.release_vm(vm_id)")
    harness = r'''
#[path = "@SOURCE@"] mod admission;
use admission::Limit;

#[test]
fn preparation_reserves_the_only_slot() {
    let limit = Limit::new();
    let preparation = limit.claim(1).unwrap();
    assert!(limit.claim(2).is_err());
    drop(preparation);
    assert!(limit.claim(2).is_ok());
}

#[test]
fn failed_start_releases_its_reservation() {
    let limit = Limit::new();
    let launch = || -> Result<(), ()> {
        let _rollback = limit.claim(1)?;
        Err(())
    };
    assert!(launch().is_err());
    assert!(limit.claim(2).is_ok());
}

#[test]
fn successful_launch_holds_until_its_own_final_offline() {
    let limit = Limit::new();
    limit.claim(1).unwrap().commit();
    assert!(limit.claim(1).is_err());
    assert!(limit.claim(2).is_err());
    limit.release_vm(2);
    assert!(limit.claim(2).is_err());
    limit.release_vm(1);
    limit.claim(2).unwrap().commit();
    assert!(limit.claim(3).is_err());
}

#[test]
fn stale_rollback_cannot_release_a_reused_vm_id() {
    let limit = Limit::new();
    let old = limit.claim(1).unwrap();
    limit.release_vm(1);
    limit.claim(1).unwrap().commit();
    drop(old);
    assert!(limit.claim(2).is_err());
}

#[test]
fn old_non_codex_offline_cannot_release_the_next_codex_reservation() {
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;
    let limit = Arc::new(Limit::new());
    let lifecycle = Arc::new(Mutex::new(()));
    // The old non-Codex occupant has made `running=false` visible, but still
    // holds lifecycle_control until its final offline publication completes.
    let old_publication = lifecycle.lock().unwrap();
    let (attempted, attempts) = mpsc::channel();
    let (admitted, admissions) = mpsc::channel();
    std::thread::scope(|scope| {
        let (next_limit, next_lifecycle) = (limit.clone(), lifecycle.clone());
        scope.spawn(move || {
            attempted.send(()).unwrap();
            let permit = {
                // Match the production boundary asserted against hv/mod.rs
                // above; the limiter itself is the unchanged production code.
                let _control = next_lifecycle.lock().unwrap();
                next_limit.claim(1).unwrap()
            };
            permit.commit();
            admitted.send(()).unwrap();
        });
        attempts.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(admissions.try_recv(), Err(mpsc::TryRecvError::Empty)));
        // The pending Codex cannot own a ticket while the old publication
        // holds this VM's lifecycle lock. A different VM can still reserve it.
        let unrelated = limit.claim(2).expect("new launch has not claimed early");
        drop(unrelated);
        limit.release_vm(1);
        drop(old_publication);
        admissions.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(limit.claim(2).is_err(), "old teardown must not clear the new owner");
    });
    limit.release_vm(1);
    assert!(limit.claim(2).is_ok());
}

#[test]
fn concurrent_launchers_admit_exactly_one() {
    use std::sync::{Arc, Barrier, atomic::{AtomicUsize, Ordering}};
    for _ in 0..32 {
        let limit = Arc::new(Limit::new());
        let begin = Arc::new(Barrier::new(16));
        let finish = Arc::new(Barrier::new(16));
        let admitted = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for vm in 0..16 {
                let (limit, begin, finish, admitted) =
                    (limit.clone(), begin.clone(), finish.clone(), admitted.clone());
                scope.spawn(move || {
                    begin.wait();
                    let permit = limit.claim(vm);
                    if permit.is_ok() { admitted.fetch_add(1, Ordering::SeqCst); }
                    finish.wait();
                    drop(permit);
                });
            }
        });
        assert_eq!(admitted.load(Ordering::SeqCst), 1);
        assert!(limit.claim(0).is_ok());
    }
}
'''.replace("@SOURCE@", str(source))
    with tempfile.TemporaryDirectory(prefix="trueos-codex-single-instance-") as directory:
        path = Path(directory)
        rust = path / "tests.rs"
        rust.write_text(harness)
        for optimization in ("0", "3"):
            binary = path / f"tests-o{optimization}"
            subprocess.run(["rustc", "--edition=2024", "--test", "-C", f"opt-level={optimization}", str(rust), "-o", str(binary)], check=True)
            subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
