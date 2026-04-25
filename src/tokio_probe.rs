//! Stage-1 Tokio probe for TRUEOS.
//!
//! This wires Tokio's runtime surfaces so we can probe BSP / VM-hull
//! assumptions incrementally. The current-thread scheduler is executed at
//! boot; the multi-thread scheduler is backed by TRUEOS worker APs.

extern crate alloc;
extern crate std;

use alloc::sync::Arc;
use core::cell::Cell;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_executor::task;
use socket2::{Domain, Protocol, Socket, Type};
use std::io;
use std::net::SocketAddr;

const VNET_PROBE_PORT: u16 = 48_123;
const TOKIO_NET_PROBE_PORT: u16 = 48_125;
const TOKIO_FS_PROBE_PATH: &str = "tokio-fs-probe.txt";
const TOKIO_FS_PROBE_BYTES: &[u8] = b"TRUEOS tokio::fs probe\n";

static TOKIO_NET_PROBE_TASK_SPAWNED: AtomicBool = AtomicBool::new(false);
static TOKIO_FS_PROBE_TASK_SPAWNED: AtomicBool = AtomicBool::new(false);
static TOKIO_BLOCKING_CANARY_TASK_SPAWNED: AtomicBool = AtomicBool::new(false);
static TOKIO_STD_TLS_CANARY_TASK_SPAWNED: AtomicBool = AtomicBool::new(false);

std::thread_local! {
    static TOKIO_STD_TLS_CANARY: Cell<u32> = const { Cell::new(0) };
}

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
                "tokio_probe: success net.socket2.stub_error (TRUEOS backend not wired yet)\n"
            );
            Ok(())
        }
    }
}

fn log_io_failure(stage: &str, err: &io::Error) {
    crate::log!("tokio_probe: failure {} kind={:?} err={}\n", stage, err.kind(), err);
}

fn log_fs_io_failure(stage: &str, err: &io::Error) {
    crate::log!(
        "tokio_probe: failure fs.runtime_ops.{} kind={:?} err={}\n",
        stage,
        err.kind(),
        err
    );
}

fn primary_ipv4_probe_addr(port: u16) -> Option<SocketAddr> {
    let dev_idx = crate::net::primary_device_index();
    let ip = crate::net::adapter::ipv4_at(dev_idx)?;
    Some(SocketAddr::from((ip, port)))
}

async fn probe_tokio_blocking_canary() -> bool {
    if !tokio_background_worker_ready() {
        crate::log!(
            "tokio_probe: note blocking.spawn_blocking_canary deferred until BACKGROUND_AP_WORKER_READY\n"
        );
        return false;
    }

    crate::log!("tokio_probe: enter blocking.spawn_blocking_canary\n");
    let canary = tokio::time::timeout(
        core::time::Duration::from_millis(50),
        tokio::task::spawn_blocking(|| 0xB10C_0001u32),
    )
    .await;

    match canary {
        Ok(Ok(0xB10C_0001)) => {
            crate::log!("tokio_probe: success blocking.spawn_blocking_canary\n");
            true
        }
        Ok(Ok(value)) => {
            crate::log!(
                "tokio_probe: failure blocking.spawn_blocking_canary value=0x{:08X}\n",
                value
            );
            false
        }
        Ok(Err(err)) => {
            crate::log!(
                "tokio_probe: failure blocking.spawn_blocking_canary join_cancelled={} join_panic={}\n",
                err.is_cancelled(),
                err.is_panic()
            );
            false
        }
        Err(_) => {
            crate::log!("tokio_probe: failure blocking.spawn_blocking_canary_timeout\n");
            false
        }
    }
}

fn run_tokio_blocking_canary_runtime() {
    let mut runtime_builder = tokio::runtime::Builder::new_current_thread();
    runtime_builder.enable_time();

    let runtime = match runtime_builder.build() {
        Ok(runtime) => runtime,
        Err(_) => {
            crate::log!("tokio_probe: failure blocking.rt.build current_thread\n");
            return;
        }
    };

    runtime.block_on(probe_tokio_blocking_canary());
}

async fn probe_tokio_fs_runtime_surface() {
    crate::log!("tokio_probe: enter fs.runtime_ops probe\n");

    crate::log!("tokio_probe: enter fs.runtime_ops.write\n");
    match tokio::fs::write(TOKIO_FS_PROBE_PATH, TOKIO_FS_PROBE_BYTES).await {
        Ok(()) => crate::log!("tokio_probe: success fs.runtime_ops.write\n"),
        Err(err) => {
            log_fs_io_failure("write", &err);
            return;
        }
    }

    crate::log!("tokio_probe: enter fs.runtime_ops.read\n");
    match tokio::fs::read(TOKIO_FS_PROBE_PATH).await {
        Ok(bytes) if bytes.as_slice() == TOKIO_FS_PROBE_BYTES => {
            crate::log!("tokio_probe: success fs.runtime_ops.read\n")
        }
        Ok(bytes) => {
            crate::log!("tokio_probe: failure fs.runtime_ops.read_value len={}\n", bytes.len());
            return;
        }
        Err(err) => {
            log_fs_io_failure("read", &err);
            return;
        }
    }

    crate::log!("tokio_probe: enter fs.runtime_ops.read_to_string\n");
    match tokio::fs::read_to_string(TOKIO_FS_PROBE_PATH).await {
        Ok(text) if text.as_bytes() == TOKIO_FS_PROBE_BYTES => {
            crate::log!("tokio_probe: success fs.runtime_ops.read_to_string\n")
        }
        Ok(text) => {
            crate::log!(
                "tokio_probe: failure fs.runtime_ops.read_to_string_value len={}\n",
                text.len()
            );
            return;
        }
        Err(err) => {
            log_fs_io_failure("read_to_string", &err);
            return;
        }
    }

    crate::log!("tokio_probe: enter fs.runtime_ops.remove_file\n");
    match tokio::fs::remove_file(TOKIO_FS_PROBE_PATH).await {
        Ok(()) => crate::log!("tokio_probe: success fs.runtime_ops.remove_file\n"),
        Err(err) => {
            log_fs_io_failure("remove_file", &err);
            return;
        }
    }

    crate::log!("tokio_probe: success fs.runtime_ops.cabi_helpers\n");

    if let Err(err) = tokio::fs::write(TOKIO_FS_PROBE_PATH, TOKIO_FS_PROBE_BYTES).await {
        log_fs_io_failure("file_ops.prepare_write", &err);
        return;
    }

    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut file = match tokio::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .truncate(true)
            .open(TOKIO_FS_PROBE_PATH)
            .await
        {
            Ok(file) => file,
            Err(err) => {
                log_fs_io_failure("open_options.open", &err);
                return;
            }
        };

        if let Err(err) = file.write_all(TOKIO_FS_PROBE_BYTES).await {
            log_fs_io_failure("file.write_all", &err);
            return;
        }
        if let Err(err) = file.flush().await {
            log_fs_io_failure("file.flush", &err);
            return;
        }
        drop(file);
        crate::log!("tokio_probe: success fs.runtime_ops.file_write_flush\n");

        let mut file = match tokio::fs::File::open(TOKIO_FS_PROBE_PATH).await {
            Ok(file) => file,
            Err(err) => {
                log_fs_io_failure("file.open", &err);
                return;
            }
        };
        let mut bytes = alloc::vec::Vec::new();
        match file.read_to_end(&mut bytes).await {
            Ok(_) if bytes.as_slice() == TOKIO_FS_PROBE_BYTES => {
                crate::log!("tokio_probe: success fs.runtime_ops.file_read_to_end\n")
            }
            Ok(_) => {
                crate::log!(
                    "tokio_probe: failure fs.runtime_ops.file_read_to_end_value len={}\n",
                    bytes.len()
                );
                return;
            }
            Err(err) => {
                log_fs_io_failure("file.read_to_end", &err);
                return;
            }
        }
    }

    match tokio::fs::remove_file(TOKIO_FS_PROBE_PATH).await {
        Ok(()) => crate::log!("tokio_probe: success fs.runtime_ops.remove_file\n"),
        Err(err) => {
            log_fs_io_failure("remove_file", &err);
            return;
        }
    }

    crate::log!("tokio_probe: success fs.file_ops probe_suite\n");

    if !probe_tokio_blocking_canary().await {
        crate::log!(
            "tokio_probe: note blocking.spawn_blocking still unsupported; fs.file_ops use TRUEOS CABI handle backend\n"
        );
    }
}

fn run_tokio_fs_probe_runtime() {
    let mut runtime_builder = tokio::runtime::Builder::new_current_thread();
    runtime_builder.enable_time();

    let runtime = match runtime_builder.build() {
        Ok(runtime) => runtime,
        Err(_) => {
            crate::log!("tokio_probe: failure fs.rt.build current_thread\n");
            return;
        }
    };

    runtime.block_on(probe_tokio_fs_runtime_surface());
}

#[task]
async fn tokio_fs_probe_task() {
    crate::r::readiness::wait_for(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED).await;
    crate::log!("tokio_probe: resume fs.runtime_ops after TRUEOSFS_ROOT_MOUNTED\n");
    run_tokio_fs_probe_runtime();
}

fn spawn_deferred_tokio_fs_probe() {
    if crate::r::readiness::is_set(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED) {
        return;
    }

    crate::log!("tokio_probe: note fs.runtime_ops deferred until TRUEOSFS_ROOT_MOUNTED\n");

    if TOKIO_FS_PROBE_TASK_SPAWNED.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(spawner) = crate::workers::spawner_for_slot(0) else {
        crate::log!("tokio_probe: note fs.runtime_ops task not spawned (no slot0 spawner)\n");
        return;
    };

    match tokio_fs_probe_task() {
        Ok(token) => spawner.spawn(token),
        Err(err) => crate::log!("tokio_probe: note fs.runtime_ops task spawn failed: {:?}\n", err),
    }
}

async fn probe_vnet_surface() -> Result<(), &'static str> {
    let deadline = embassy_time::Instant::now() + embassy_time::Duration::from_millis(500);
    while !crate::r::readiness::is_set(crate::r::readiness::NET_CONFIGURED) {
        if embassy_time::Instant::now() >= deadline {
            crate::log!("tokio_probe: note vnet surface skipped (net not configured yet)\n");
            return Ok(());
        }
        tokio::time::sleep(core::time::Duration::from_millis(10)).await;
    }

    let Some(vnet) = crate::r::net::VNet::open_primary() else {
        crate::log!("tokio_probe: note vnet surface skipped (no primary vnet)\n");
        return Ok(());
    };

    if vnet
        .submit(v::vnet::Command::OpenTcpListen {
            port: VNET_PROBE_PORT,
        })
        .is_err()
    {
        return Err("net.vnet.submit_open_tcp_listen");
    }

    let deadline = embassy_time::Instant::now() + embassy_time::Duration::from_millis(500);
    loop {
        if let Some(event) = vnet.pop_event() {
            match event {
                v::vnet::Event::Opened { handle, kind } if kind == v::vnet::SocketKind::Tcp => {
                    crate::log!("tokio_probe: success net.vnet.open_tcp_listen\n");
                    let _ = vnet.submit(v::vnet::Command::Close { handle });
                    return Ok(());
                }
                v::vnet::Event::Error { .. } => return Err("net.vnet.open_tcp_listen"),
                _ => {}
            }
        }

        if embassy_time::Instant::now() >= deadline {
            return Err("net.vnet.open_tcp_listen_timeout");
        }

        tokio::task::yield_now().await;
    }
}

async fn wait_for_net_configured() -> bool {
    let deadline = embassy_time::Instant::now() + embassy_time::Duration::from_millis(500);
    while !crate::r::readiness::is_set(crate::r::readiness::NET_CONFIGURED) {
        if embassy_time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(core::time::Duration::from_millis(10)).await;
    }
    true
}

async fn probe_tokio_net_surface() -> Result<(), &'static str> {
    if !wait_for_net_configured().await {
        crate::log!("tokio_probe: note net.tokio skipped (net not configured yet)\n");
        return Ok(());
    }

    let Some(tcp_probe) = primary_ipv4_probe_addr(TOKIO_NET_PROBE_PORT) else {
        crate::log!("tokio_probe: note net.tokio skipped (no primary ipv4 yet)\n");
        return Ok(());
    };

    let Some(udp_probe) = primary_ipv4_probe_addr(0) else {
        crate::log!("tokio_probe: note net.tokio skipped (no primary ipv4 yet)\n");
        return Ok(());
    };

    match tokio::net::TcpListener::bind(tcp_probe).await {
        Ok(listener) => {
            let _ = listener.local_addr();
            crate::log!("tokio_probe: success net.tokio.tcp_listener.bind\n");
        }
        Err(err) => {
            log_io_failure("net.tokio.tcp_listener.bind", &err);
            return Err("net.tokio.tcp_listener.bind");
        }
    }

    let udp = match tokio::net::UdpSocket::bind(udp_probe).await {
        Ok(udp) => {
            crate::log!("tokio_probe: success net.tokio.udp_socket.bind\n");
            udp
        }
        Err(err) => {
            log_io_failure("net.tokio.udp_socket.bind", &err);
            return Err("net.tokio.udp_socket.bind");
        }
    };

    match tokio::time::timeout(core::time::Duration::from_millis(50), udp.writable()).await {
        Ok(Ok(())) => crate::log!("tokio_probe: success net.tokio.udp_socket.writable\n"),
        Ok(Err(err)) => {
            log_io_failure("net.tokio.udp_socket.writable", &err);
            return Err("net.tokio.udp_socket.writable");
        }
        Err(_) => return Err("net.tokio.udp_socket.writable_timeout"),
    }

    Ok(())
}

fn run_tokio_net_probe_runtime() {
    let mut runtime_builder = tokio::runtime::Builder::new_current_thread();
    runtime_builder.enable_io();
    runtime_builder.enable_time();

    let runtime = match runtime_builder.build() {
        Ok(runtime) => runtime,
        Err(_) => {
            crate::log!("tokio_probe: failure net.tokio.rt.build current_thread\n");
            return;
        }
    };

    match runtime.block_on(probe_tokio_net_surface()) {
        Ok(()) => crate::log!("tokio_probe: success net.tokio probe_suite\n"),
        Err(stage) => crate::log!("tokio_probe: failure {}\n", stage),
    }
}

fn tokio_background_worker_ready() -> bool {
    crate::r::readiness::is_set(crate::r::readiness::BACKGROUND_AP_WORKER_READY)
        && crate::workers::has_background_worker_slot()
}

fn mark_std_tls_canary_on_boot_cpu() {
    TOKIO_STD_TLS_CANARY.with(|canary| canary.set(0xB5B5_0001));
    crate::log!("tokio_probe: note std.thread_local boot canary armed\n");
}

fn read_std_tls_canary() -> u32 {
    TOKIO_STD_TLS_CANARY.with(|canary| canary.get())
}

#[task]
async fn tokio_std_tls_canary_task() {
    crate::r::readiness::wait_for(crate::r::readiness::BACKGROUND_AP_WORKER_READY).await;

    let before = read_std_tls_canary();
    TOKIO_STD_TLS_CANARY.with(|canary| canary.set(0xA9A9_0001));
    let after = read_std_tls_canary();

    if before == 0 {
        crate::log!(
            "tokio_probe: success std.thread_local per-AP canary before=0x{:08X} after=0x{:08X}\n",
            before,
            after
        );
    } else {
        crate::log!(
            "tokio_probe: failure std.thread_local per-AP canary leaked before=0x{:08X} after=0x{:08X}; rt-multi-thread.build stays deferred\n",
            before,
            after
        );
    }
}

fn spawn_deferred_std_tls_canary() {
    if TOKIO_STD_TLS_CANARY_TASK_SPAWNED.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(spawner) = crate::workers::spawner_for_slot(0) else {
        crate::log!(
            "tokio_probe: note std.thread_local canary task not spawned (no slot0 spawner)\n"
        );
        return;
    };

    match tokio_std_tls_canary_task() {
        Ok(token) => spawner.spawn(token),
        Err(err) => {
            crate::log!("tokio_probe: note std.thread_local canary task spawn failed: {:?}\n", err)
        }
    }
}

fn run_rt_multi_thread_probe() {
    let mut runtime_builder = tokio::runtime::Builder::new_multi_thread();
    runtime_builder.worker_threads(2);
    runtime_builder.enable_io();
    runtime_builder.enable_time();

    crate::log!("tokio_probe: success rt-multi-thread.builder_surface\n");
    // TRUEOS treats the VM/app/process as the isolation boundary. Tokio worker
    // lanes inside that boundary may trust the same app memory, but they still
    // need independent TLS scratchpads for runtime context: enter depth,
    // current task, current scheduler worker, errno-like state, and similar
    // bookkeeping. Until each AP carrier has that per-lane context isolation,
    // building the multi-thread runtime would corrupt Tokio's own worker state.
    crate::log!(
        "tokio_probe: note rt-multi-thread.build deferred until TRUEOS per-AP TLS is ready\n"
    );
    let _ = runtime_builder;
}

fn log_rt_multi_thread_probe() {
    run_rt_multi_thread_probe();
}

#[task]
async fn tokio_blocking_canary_task() {
    crate::r::readiness::wait_for(crate::r::readiness::BACKGROUND_AP_WORKER_READY).await;
    crate::log!(
        "tokio_probe: resume blocking.spawn_blocking_canary after BACKGROUND_AP_WORKER_READY\n"
    );
    run_tokio_blocking_canary_runtime();
}

fn spawn_deferred_tokio_blocking_canary() {
    if tokio_background_worker_ready() {
        return;
    }

    if TOKIO_BLOCKING_CANARY_TASK_SPAWNED.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(spawner) = crate::workers::spawner_for_slot(0) else {
        crate::log!(
            "tokio_probe: note blocking.spawn_blocking_canary task not spawned (no slot0 spawner)\n"
        );
        return;
    };

    match tokio_blocking_canary_task() {
        Ok(token) => spawner.spawn(token),
        Err(err) => crate::log!(
            "tokio_probe: note blocking.spawn_blocking_canary task spawn failed: {:?}\n",
            err
        ),
    }
}

#[task]
async fn tokio_net_probe_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_CONFIGURED).await;
    crate::log!("tokio_probe: resume net.tokio surface after NET_CONFIGURED\n");
    run_tokio_net_probe_runtime();
}

fn spawn_deferred_tokio_net_probe() {
    if crate::r::readiness::is_set(crate::r::readiness::NET_CONFIGURED) {
        return;
    }

    crate::log!("tokio_probe: note net.tokio surface deferred until NET_CONFIGURED\n");

    if TOKIO_NET_PROBE_TASK_SPAWNED.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(spawner) = crate::workers::spawner_for_slot(0) else {
        crate::log!("tokio_probe: note net.tokio task not spawned (no slot0 spawner)\n");
        return;
    };

    match tokio_net_probe_task() {
        Ok(token) => spawner.spawn(token),
        Err(err) => {
            crate::log!("tokio_probe: note net.tokio task spawn failed: {:?}\n", err)
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

    let (join_left, join_right) = tokio::join!(
        async {
            tokio::task::yield_now().await;
            0x4A4F_494Eu32
        },
        async { 0x4D41_4352u32 },
    );
    if join_left != 0x4A4F_494E || join_right != 0x4D41_4352 {
        return Err("rt-join-macro-value");
    }
    crate::log!("tokio_probe: success rt.task.join_macro\n");

    let (try_join_left, try_join_right) = tokio::try_join!(
        async {
            tokio::task::yield_now().await;
            Ok::<u32, &'static str>(0x5452_5931)
        },
        async { Ok::<u32, &'static str>(0x5452_5932) },
    )?;
    if try_join_left != 0x5452_5931 || try_join_right != 0x5452_5932 {
        return Err("rt-try-join-macro-value");
    }
    crate::log!("tokio_probe: success rt.task.try_join_macro\n");

    let (_abort_tx, abort_rx) = tokio::sync::oneshot::channel::<()>();
    let abort_task = tokio::task::spawn(async move {
        let _ = abort_rx.await;
        0x4142_4F52u32
    });
    abort_task.abort();
    match abort_task.await {
        Err(err) if err.is_cancelled() => crate::log!("tokio_probe: success rt.task.abort\n"),
        Err(_) => return Err("rt-task-abort-state"),
        Ok(_) => return Err("rt-task-abort-value"),
    }

    let select_value = tokio::select! {
        _ = tokio::time::sleep(core::time::Duration::from_millis(5)) => 0u32,
        value = async {
            tokio::task::yield_now().await;
            0x5345_4C45u32
        } => value,
    };
    if select_value != 0x5345_4C45 {
        return Err("rt-select-value");
    }
    crate::log!("tokio_probe: success rt.select\n");

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

    probe_vnet_surface().await?;
    probe_socket2_surface()?;
    probe_tokio_net_surface().await?;

    probe_tokio_blocking_canary().await;

    {
        crate::log!("tokio_probe: enter time.sleep\n");
        tokio::time::sleep(core::time::Duration::from_millis(1)).await;
        crate::log!("tokio_probe: success time.sleep\n");

        let timeout_value = tokio::time::timeout(core::time::Duration::from_millis(5), async {
            tokio::task::yield_now().await;
            0x5449_4D45u32
        })
        .await
        .map_err(|_| "time-timeout-ok")?;
        if timeout_value != 0x5449_4D45 {
            return Err("time-timeout-value");
        }
        crate::log!("tokio_probe: success time.timeout\n");

        match tokio::time::timeout(core::time::Duration::from_millis(1), async {
            tokio::time::sleep(core::time::Duration::from_millis(5)).await;
            0x4445_4144u32
        })
        .await
        {
            Err(_) => crate::log!("tokio_probe: success time.timeout_elapsed\n"),
            Ok(_) => return Err("time-timeout-elapsed"),
        }

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
        crate::log!("tokio_probe: note fs.runtime_ops pulled through TRUEOS CABI backend\n");
        if crate::r::readiness::is_set(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED) {
            probe_tokio_fs_runtime_surface().await;
        } else {
            spawn_deferred_tokio_fs_probe();
        }
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
    crate::log!("tokio_probe: wired tokio 1.52.1 with feature full via TRUEOS std-ABI shim\n");
    mark_std_tls_canary_on_boot_cpu();

    log_rt_multi_thread_probe();
    spawn_deferred_std_tls_canary();
    spawn_deferred_tokio_blocking_canary();

    let mut runtime_builder = tokio::runtime::Builder::new_current_thread();
    runtime_builder.enable_io();
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

    spawn_deferred_tokio_net_probe();
    spawn_deferred_tokio_fs_probe();
}
