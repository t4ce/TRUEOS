
#[test]
fn a_separate_carrier_runs_a_continuation_while_the_first_carrier_is_busy() {
    let _serial = SERIAL.lock().unwrap();
    use core::sync::atomic::AtomicBool;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use crate::workers::application_policy::{ThreadPlacement, ThreadRole};
    let policy = ThreadPlacement::from_raw(1, 2).unwrap();
    let foreground = policy.carrier(ThreadRole::Foreground, Some(7), &[2, 3, 4], 0).unwrap();
    let background = policy.carrier(ThreadRole::TokioPool, Some(7), &[2, 3, 4], 0).unwrap();
    assert_ne!(foreground, background);
    let original_slot = crate::percpu::current_slot();
    crate::percpu::set_slot(foreground as usize);
    let running = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicBool::new(false));
    let peer_running = running.clone();
    let peer_progress = progress.clone();
    let (finished_tx, finished_rx) = mpsc::channel();
    let second = std::thread::spawn(move || {
        crate::percpu::set_slot(background as usize);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !peer_running.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(peer_running.load(Ordering::Acquire), "first carrier never entered its continuation");
        let mut task = make_thread(61, move || {
            assert_eq!(crate::percpu::current_slot(), background as usize);
            peer_progress.store(true, Ordering::Release);
        });
        assert!(poll_thread(&mut task).is_ready());
        drop(task);
        finished_tx.send(()).unwrap();
    });
    let mark = Instant::now();
    let mut busy = make_thread(60, move || {
        running.store(true, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !progress.load(Ordering::Acquire) && Instant::now() < deadline {
            core::hint::spin_loop();
        }
        assert!(progress.load(Ordering::Acquire), "separate carrier failed to make progress during the busy poll");
    });
    assert!(poll_thread(&mut busy).is_ready());
    finished_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    second.join().unwrap();
    drop(busy);
    crate::percpu::set_slot(original_slot);
    eprintln!("scheduler-experiment placement=foreground-isolated foreground={} pool={} peer_progress=during-active-poll observed_us={}", foreground, background, mark.elapsed().as_micros());
}

#[test]
fn same_carrier_ready_latency_includes_the_unyielding_slice() {
    let _serial = SERIAL.lock().unwrap();
    use std::time::{Duration, Instant};
    let ran = Arc::new(AtomicUsize::new(0));
    let peer = ran.clone();
    let mut waiting = make_thread(61, move || {
        assert!(yield_now());
        peer.store(1, Ordering::Release);
    });
    assert!(crate::r::blocking::open_guest_jobs(61, 19));
    let owner = crate::r::blocking::reserve_guest_job(61).unwrap();
    owner.diagnostic.identify("std-thread", 61, 0);
    owner.diagnostic.role(1);
    owner.diagnostic.enable_progress_probe();
    waiting.0._admission._owner = Some(owner);
    waiting.0.workload_role = crate::workers::application_policy::ThreadRole::Foreground;
    waiting.0.timed = true;
    waiting.0.submitted_ms = crate::now();
    // The production wrapped waker records a real outstanding wake when this
    // first poll yields. Subsequent CPU work prevents that wake being serviced.
    assert!(poll_thread(&mut waiting).is_pending());
    let mark = Instant::now();
    let mut busy = make_thread(60, move || {
        while mark.elapsed() < Duration::from_millis(50) { core::hint::spin_loop(); }
        assert_eq!(ran.load(Ordering::Acquire), 0, "same-carrier peer was preemptively scheduled");
    });
    assert!(poll_thread(&mut busy).is_ready());
    let ready_wait = mark.elapsed();
    assert!(poll_thread(&mut waiting).is_ready());
    assert!(ready_wait >= Duration::from_millis(50));
    let live = diagnostics::progress_lines(61, 1).remove(0);
    assert!(live.contains("role=foreground"));
    let ready_ms = |line: &str| -> u64 {
        line.split_whitespace().find_map(|field| field.strip_prefix("ready_max_ms=")).unwrap().parse().unwrap()
    };
    assert!(ready_ms(&live) >= 50, "production wake-to-poll probe missed the injected stall: {live}");
    drop(waiting);
    assert!(diagnostics::progress_lines(61, 1).is_empty());
    let mut cursor = 1; // Carrier 0, foreground role.
    let retired = diagnostics::timing_lines(&mut cursor, 1).remove(0);
    assert!(retired.contains("scope=since-boot carrier=0 role=foreground"));
    assert!(ready_ms(&retired) >= 50, "retirement discarded the timing evidence: {retired}");
    eprintln!("scheduler-experiment placement=same-carrier busy_ms=50 ready_wait_us={}", ready_wait.as_micros());
}

#[test]
fn native_spawn_scopes_restore_nested_hints_without_leaking_to_a_sibling() {
    let _serial = SERIAL.lock().unwrap();
    assert_eq!(swap_native_scope(false, 1), None, "ordinary carrier has no continuation scope");
    let mut parent = make_thread(60, || {
        assert_eq!(swap_native_scope(false, 1), Some(0));
        assert_eq!(swap_native_scope(false, 2), Some(1));
        assert_eq!(swap_native_scope(true, 2), Some(0));
        assert!(yield_now());
        assert_eq!(swap_native_scope(false, 1), Some(2), "nested spawn hint lost across suspension");
        assert_eq!(swap_native_scope(true, 3), Some(2), "activity lost across suspension");
        assert!(yield_now());
        assert_eq!(swap_native_scope(false, 0), Some(1), "outer spawn hint failed to restore");
        assert_eq!(swap_native_scope(true, 2), Some(3));
        assert_eq!(swap_native_scope(true, 0), Some(2));
        assert_eq!(swap_native_scope(false, 99), None);
        assert_eq!(swap_native_scope(true, 4), None);
        assert_eq!(swap_native_scope(false, 0), Some(0), "invalid hint mutated the continuation");
        assert_eq!(swap_native_scope(true, 0), Some(0), "invalid activity mutated the continuation");
    });
    let mut sibling = make_thread(61, || {
        assert_eq!(swap_native_scope(false, 3), Some(0), "sibling inherited another continuation's spawn scope");
        assert_eq!(swap_native_scope(true, 1), Some(0));
        assert!(yield_now());
        assert_eq!(swap_native_scope(false, 0), Some(3));
        assert_eq!(swap_native_scope(true, 0), Some(1));
    });
    assert!(poll_thread(&mut parent).is_pending());
    assert!(poll_thread(&mut sibling).is_pending());
    assert_eq!(swap_native_scope(false, 1), None, "parked scope leaked onto carrier frames");
    assert!(poll_thread(&mut parent).is_pending());
    assert!(poll_thread(&mut sibling).is_ready());
    assert!(poll_thread(&mut parent).is_ready());
}

#[test]
fn native_child_scopes_remain_independent_on_simultaneously_active_carriers() {
    let _serial = SERIAL.lock().unwrap();
    use std::sync::mpsc;
    use std::time::Duration;
    let (parked_tx, parked_rx) = mpsc::channel();
    let mut releases = Vec::new();
    let mut carriers = Vec::new();
    for (slot, role, activity) in [(6usize, 1u32, 2u32), (7, 3, 3)] {
        let parked = parked_tx.clone();
        let (release_tx, release_rx) = mpsc::channel();
        releases.push(release_tx);
        carriers.push(std::thread::spawn(move || {
            crate::percpu::set_slot(slot);
            let mut task = make_thread(62, move || {
                assert_eq!(swap_native_scope(false, role), Some(0));
                assert_eq!(swap_native_scope(true, activity), Some(0));
                assert!(yield_now());
                assert_eq!(swap_native_scope(false, 0), Some(role), "another carrier overwrote this spawn scope");
                assert_eq!(swap_native_scope(true, 0), Some(activity), "another carrier overwrote this activity");
            });
            assert!(poll_thread(&mut task).is_pending());
            parked.send(slot).unwrap();
            release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(poll_thread(&mut task).is_ready());
        }));
    }
    let first = parked_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let second = parked_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_ne!(first, second);
    for release in releases { release.send(()).unwrap(); }
    for carrier in carriers { carrier.join().unwrap(); }
}
