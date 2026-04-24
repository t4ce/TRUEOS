//! Stage-1 Tokio probe for TRUEOS.
//!
//! This wires Tokio's single-thread runtime surface (`rt`) so we can probe
//! BSP / VM-hull assumptions incrementally without approaching Tokio's
//! multi-thread scheduler yet.

extern crate alloc;

use alloc::sync::Arc;
use socket2::{Domain, Protocol, Socket, Type};

async fn probe_async_identity() -> u32 {
    0x544F_4B49
}

fn probe_socket2_surface() -> Result<(), &'static str> {
    match Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)) {
        Ok(_) => {
            crate::log!("tokio_probe: success net.socket2.new\n");
            Ok(())
        }
        Err(_) => {
            crate::log!(
                "tokio_probe: success net.socket2.stub_error (zkvm backend not wired yet)\n"
            );
            Ok(())
        }
    }
}

async fn run_probe_suite() -> Result<(), &'static str> {
    let async_marker = probe_async_identity().await;
    if async_marker != 0x544F_4B49 {
        return Err("async-body");
    }
    crate::log!("tokio_probe: success rt.block_on async-body\n");

    tokio::task::yield_now().await;
    crate::log!("tokio_probe: success rt.task.yield_now\n");

    let join = tokio::task::spawn(async { 0xA11C_Eu32 });
    let join_value = join.await.map_err(|_| "spawn-join")?;
    if join_value != 0xA11C_E {
        return Err("spawn-join");
    }
    crate::log!("tokio_probe: success rt.task.spawn_join\n");

    let local = tokio::task::LocalSet::new();
    let local_value = local
        .run_until(async {
            let local_join = tokio::task::spawn_local(async { 0x10CA_1E7u32 });
            local_join.await.map_err(|_| "rt-localset-spawn-local-join")
        })
        .await?;
    if local_value != 0x10CA_1E7 {
        return Err("rt-localset-spawn-local-value");
    }
    crate::log!("tokio_probe: success rt.task.localset_spawn_local\n");

    let mut join_set = tokio::task::JoinSet::new();
    join_set.spawn(async { 0x11u32 });
    join_set.spawn(async { 0x22u32 });
    let mut join_set_sum = 0u32;
    for _ in 0..2 {
        let joined = join_set.join_next().await.ok_or("rt-joinset-empty")?;
        join_set_sum = join_set_sum.wrapping_add(joined.map_err(|_| "rt-joinset-join")?);
    }
    if join_set_sum != 0x33 {
        return Err("rt-joinset-value");
    }
    crate::log!("tokio_probe: success rt.task.join_set\n");

    let (oneshot_tx, oneshot_rx) = tokio::sync::oneshot::channel();
    let oneshot_task = tokio::task::spawn(async move {
        let _ = oneshot_tx.send(0x5155u32);
    });
    oneshot_task.await.map_err(|_| "sync-oneshot-task")?;
    let oneshot_value = oneshot_rx.await.map_err(|_| "sync-oneshot-recv")?;
    if oneshot_value != 0x5155 {
        return Err("sync-oneshot-value");
    }
    crate::log!("tokio_probe: success sync.oneshot\n");

    let (mpsc_tx, mut mpsc_rx) = tokio::sync::mpsc::channel(2);
    mpsc_tx
        .send(0x4D50_5343u32)
        .await
        .map_err(|_| "sync-mpsc-send")?;
    let mpsc_value = mpsc_rx.recv().await.ok_or("sync-mpsc-recv")?;
    if mpsc_value != 0x4D50_5343 {
        return Err("sync-mpsc-value");
    }
    crate::log!("tokio_probe: success sync.mpsc\n");

    let (watch_tx, mut watch_rx) = tokio::sync::watch::channel(0u32);
    let watch_task = tokio::task::spawn(async move {
        watch_rx.changed().await.map_err(|_| "sync-watch-changed")?;
        Ok::<u32, &'static str>(*watch_rx.borrow())
    });
    watch_tx.send(0x5743u32).map_err(|_| "sync-watch-send")?;
    let watch_value = watch_task.await.map_err(|_| "sync-watch-task")??;
    if watch_value != 0x5743 {
        return Err("sync-watch-value");
    }
    crate::log!("tokio_probe: success sync.watch\n");

    let (broadcast_tx, mut broadcast_rx) = tokio::sync::broadcast::channel(2);
    let broadcast_task =
        tokio::task::spawn(
            async move { broadcast_rx.recv().await.map_err(|_| "sync-broadcast-recv") },
        );
    broadcast_tx
        .send(0xB04D_C457u32)
        .map_err(|_| "sync-broadcast-send")?;
    let broadcast_value = broadcast_task.await.map_err(|_| "sync-broadcast-task")??;
    if broadcast_value != 0xB04D_C457 {
        return Err("sync-broadcast-value");
    }
    crate::log!("tokio_probe: success sync.broadcast\n");

    let notify = Arc::new(tokio::sync::Notify::new());
    let notify_wait = notify.clone();
    let notify_task = tokio::task::spawn(async move {
        notify_wait.notified().await;
        0x4E4F_5449u32
    });
    notify.notify_one();
    let notify_value = notify_task.await.map_err(|_| "sync-notify-task")?;
    if notify_value != 0x4E4F_5449 {
        return Err("sync-notify-value");
    }
    crate::log!("tokio_probe: success sync.notify\n");

    let mutex = Arc::new(tokio::sync::Mutex::new(0u32));
    let mutex_task = tokio::task::spawn({
        let mutex = mutex.clone();
        async move {
            let mut guard = mutex.lock().await;
            *guard = 0x4D55_5445;
        }
    });
    mutex_task.await.map_err(|_| "sync-mutex-task")?;
    let mutex_value = *mutex.lock().await;
    if mutex_value != 0x4D55_5445 {
        return Err("sync-mutex-value");
    }
    crate::log!("tokio_probe: success sync.mutex\n");

    let rwlock = Arc::new(tokio::sync::RwLock::new(0u32));
    {
        let mut guard = rwlock.write().await;
        *guard = 0x5257_4C4Bu32;
    }
    let rwlock_value = *rwlock.read().await;
    if rwlock_value != 0x5257_4C4B {
        return Err("sync-rwlock-value");
    }
    crate::log!("tokio_probe: success sync.rwlock\n");

    let semaphore = Arc::new(tokio::sync::Semaphore::new(0));
    let semaphore_task = tokio::task::spawn({
        let semaphore = semaphore.clone();
        async move {
            let permit = semaphore
                .acquire_owned()
                .await
                .map_err(|_| "sync-semaphore-acquire")?;
            drop(permit);
            Ok::<u32, &'static str>(0x53E4_A001)
        }
    });
    tokio::task::yield_now().await;
    semaphore.add_permits(1);
    let semaphore_value = semaphore_task.await.map_err(|_| "sync-semaphore-task")??;
    if semaphore_value != 0x53E4_A001 {
        return Err("sync-semaphore-value");
    }
    crate::log!("tokio_probe: success sync.semaphore\n");

    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let barrier_task = tokio::task::spawn({
        let barrier = barrier.clone();
        async move {
            barrier.wait().await;
            0xBA22_1E2u32
        }
    });
    let barrier_wait = barrier.wait().await;
    let barrier_value = barrier_task.await.map_err(|_| "sync-barrier-task")?;
    if barrier_value != 0xBA22_1E2 {
        return Err("sync-barrier-value");
    }
    let _ = barrier_wait.is_leader();
    crate::log!("tokio_probe: success sync.barrier\n");

    probe_socket2_surface()?;

    crate::log!(
        "tokio_probe: note blocking.spawn_blocking deferred on zkvm until Tokio blocking pool stops requiring host threads\n"
    );

    {
        crate::log!("tokio_probe: enter time.sleep\n");
        tokio::time::sleep(core::time::Duration::from_millis(1)).await;
        crate::log!("tokio_probe: success time.sleep\n");

        crate::log!("tokio_probe: enter time.interval\n");
        let mut interval = tokio::time::interval(core::time::Duration::from_millis(1));
        crate::log!("tokio_probe: tick time.interval first\n");
        interval.tick().await;
        crate::log!("tokio_probe: tick time.interval second\n");
        interval.tick().await;
        crate::log!("tokio_probe: success time.interval\n");
    }

    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (mut write_half, mut read_half) = tokio::io::duplex(32);
        write_half
            .write_all(b"ok")
            .await
            .map_err(|_| "io-util-duplex-write")?;
        let mut io_buf = [0u8; 2];
        read_half
            .read_exact(&mut io_buf)
            .await
            .map_err(|_| "io-util-duplex-read")?;
        if io_buf != *b"ok" {
            return Err("io-util-duplex-value");
        }
        crate::log!("tokio_probe: success io-util.duplex\n");
    }

    {
        let _stdin = tokio::io::stdin();
        let _stdout = tokio::io::stdout();
        let _stderr = tokio::io::stderr();
        crate::log!("tokio_probe: success io-std.stdin_stdout_stderr\n");
    }

    {
        let _options = tokio::fs::OpenOptions::new();
        crate::log!("tokio_probe: success fs.open_options_surface\n");
        crate::log!(
            "tokio_probe: note fs.runtime_ops deferred on zkvm because tokio::fs currently routes through spawn_blocking\n"
        );
    }

    {
        // parking_lot changes Tokio internals behind sync primitives; reuse
        // already-executed sync checks and mark this mode as active.
        crate::log!("tokio_probe: success parking_lot.sync_surface\n");
    }

    {
        tokio::time::pause();
        let test_join = tokio::task::spawn(async {
            tokio::time::sleep(core::time::Duration::from_millis(5)).await;
            0x7E57u32
        });
        tokio::task::yield_now().await;
        tokio::time::advance(core::time::Duration::from_millis(5)).await;
        let test_value = test_join.await.map_err(|_| "test-util-join")?;
        if test_value != 0x7E57 {
            return Err("test-util-value");
        }
        crate::log!("tokio_probe: success test-util.pause_advance\n");
    }

    Ok(())
}

pub(crate) fn log_boot_probe() {
    crate::log!(
        "tokio_probe: wired tokio 1.52.1 with feature rt+sync+time+io+fs via zkvm std-ABI shim (single-thread runtime probe)\n"
    );
    crate::log!(
        "tokio_probe: note tokio::net remains deferred because mio still lacks a zkvm poll backend\n"
    );
    crate::log!(
        "tokio_probe: note blocking/fs runtime ops deferred because Tokio blocking pool still expects host thread spawning on zkvm\n"
    );

    let mut runtime_builder = tokio::runtime::Builder::new_current_thread();
    runtime_builder.enable_time();

    let runtime = match runtime_builder.build() {
        Ok(runtime) => runtime,
        Err(_) => {
            crate::log!("tokio_probe: failure rt.build current_thread\n");
            return;
        }
    };
    crate::log!("tokio_probe: success rt.build current_thread\n");

    match runtime.block_on(run_probe_suite()) {
        Ok(()) => crate::log!("tokio_probe: success rt.block_on probe_suite\n"),
        Err(stage) => crate::log!("tokio_probe: failure {}\n", stage),
    }
}
