//! Reproduce scoped fanout alongside persistent background threads.
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const BACKGROUND: usize = 8;
const SCOPED: usize = 24;
thread_local! { static TOKEN: Cell<usize> = const { Cell::new(0) }; }

#[cfg(target_os = "trueos")]
unsafe extern "C" {
    fn trueos_cabi_wls_current_slot() -> u32;
}

#[derive(Default)]
struct Gate {
    release: Mutex<bool>,
    condition: Condvar,
    ready: AtomicUsize,
    #[cfg(target_os = "trueos")]
    slots: Mutex<Vec<u32>>,
}

fn record(state: &str, round: usize, elapsed: Duration) {
    let message = format!(
        "thread_fanout: {state} round={round} background={BACKGROUND} scoped={SCOPED} elapsed_ms={}",
        elapsed.as_millis()
    );
    #[cfg(target_os = "trueos")]
    trueos::logl::log(trueos::logl::level::INFO, format_args!("{message}"));
    #[cfg(not(target_os = "trueos"))]
    println!("{message}");
}

fn worker(gate: &Gate, token: usize) {
    TOKEN.with(|value| {
        assert_eq!(value.get(), 0);
        value.set(token);
    });
    #[cfg(target_os = "trueos")]
    let slot = {
        let slot = unsafe { trueos_cabi_wls_current_slot() };
        assert!(slot < 4096);
        gate.slots.lock().unwrap().push(slot);
        slot
    };
    gate.ready.fetch_add(1, Ordering::Release);
    let released = gate.release.lock().unwrap();
    let (released, timeout) = gate
        .condition
        .wait_timeout_while(released, Duration::from_secs(10), |released| !*released)
        .unwrap();
    assert!(*released && !timeout.timed_out(), "fanout release deadline");
    drop(released);
    for _ in 0..3 {
        std::thread::yield_now();
    }
    TOKEN.with(|value| assert_eq!(value.get(), token, "thread-local value aliased"));
    #[cfg(target_os = "trueos")]
    assert_eq!(unsafe { trueos_cabi_wls_current_slot() }, slot, "live identity moved");
}

fn round(index: usize) {
    let start = Instant::now();
    let gate = Arc::new(Gate::default());
    let mut background = Vec::new();
    for id in 1..=BACKGROUND {
        let gate = gate.clone();
        background.push(
            std::thread::Builder::new()
                .spawn(move || worker(&gate, id))
                .unwrap(),
        );
    }
    std::thread::scope(|scope| {
        for id in BACKGROUND + 1..=BACKGROUND + SCOPED {
            let gate = &gate;
            // Deliberately exercise Scope::spawn: this is the API reported by
            // the real application's scoped.rs:206 failure.
            scope.spawn(move || worker(gate, id));
            if id == 20 {
                record("TWENTY_LIVE_ADMITTED", index, start.elapsed());
            }
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while gate.ready.load(Ordering::Acquire) != BACKGROUND + SCOPED {
            assert!(Instant::now() < deadline, "fanout readiness deadline");
            std::thread::sleep(Duration::from_millis(1));
        }
        #[cfg(target_os = "trueos")]
        {
            let mut slots = gate.slots.lock().unwrap();
            assert_eq!(slots.len(), BACKGROUND + SCOPED);
            slots.sort_unstable();
            slots.dedup();
            assert_eq!(slots.len(), BACKGROUND + SCOPED, "simultaneous WLS identities aliased");
        }
        *gate.release.lock().unwrap() = true;
        gate.condition.notify_all();
    });
    for thread in background {
        thread.join().unwrap();
    }
    record("PASS", index, start.elapsed());
}

fn main() {
    let start = Instant::now();
    record("BEGIN", 0, start.elapsed());
    for index in 1..=3 {
        round(index);
    }
    record("ALL_PASS", 3, start.elapsed());
}
