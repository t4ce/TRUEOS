//! Real Tokio experiments; carrier correctness is tested separately against
//! TRUEOS ThreadTask. These reveal runtime queueing and cancellation semantics.
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn a_cpu_bound_async_task_stalls_other_tasks_on_a_current_thread_runtime() {
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let mark = Instant::now();
    let busy = rt.spawn(async move {
        while mark.elapsed() < Duration::from_millis(50) { core::hint::spin_loop(); }
    });
    let observed = rt.spawn(async move { mark.elapsed() });
    let ready_wait = rt.block_on(async { busy.await.unwrap(); observed.await.unwrap() });
    assert!(ready_wait >= Duration::from_millis(50));
    eprintln!("tokio-experiment worker_threads=1 task=async-cpu busy_ms=50 ready_wait_us={}", ready_wait.as_micros());
}

#[test]
fn bounded_blocking_pool_queues_excess_work_and_abort_does_not_stop_started_work() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time().max_blocking_threads(1).build().unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first = rt.spawn_blocking(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        7
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let (queued_tx, queued_rx) = mpsc::channel();
    let queued_at = Instant::now();
    let queued = rt.spawn_blocking(move || { queued_tx.send(()).unwrap(); 9 });
    // The pool's admitted worker is executing a synchronous closure. A second
    // closure cannot start, while the independent runtime still runs timers.
    rt.block_on(async { tokio::time::sleep(Duration::from_millis(25)).await });
    assert_eq!(queued_rx.try_recv(), Err(mpsc::TryRecvError::Empty));
    first.abort();
    assert!(!first.is_finished(), "abort incorrectly stopped a started blocking closure");
    release_tx.send(()).unwrap();
    assert_eq!(rt.block_on(first).unwrap(), 7);
    assert_eq!(rt.block_on(queued).unwrap(), 9);
    queued_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    eprintln!("tokio-experiment max_blocking_threads=1 queued_wait_us={} started_abort=continued timer=progressed", queued_at.elapsed().as_micros());
}

#[test]
fn additional_blocking_capacity_allows_a_peer_before_the_first_job_completes() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .max_blocking_threads(2).build().unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first = rt.spawn_blocking(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let (peer_tx, peer_rx) = mpsc::channel();
    let peer = rt.spawn_blocking(move || { peer_tx.send(()).unwrap(); });
    let progressed = peer_rx.recv_timeout(Duration::from_secs(2));
    release_tx.send(()).unwrap();
    rt.block_on(first).unwrap();
    rt.block_on(peer).unwrap();
    progressed.unwrap();
    eprintln!("tokio-experiment max_blocking_threads=2 peer_progress=before-first-job-completes");
}
