extern crate alloc;
extern crate std;

use alloc::boxed::Box;
use core::sync::atomic::{AtomicU16, Ordering};
use std::{io, net::SocketAddr};

use axum::{Json, Router, routing::get};
use embassy_time::{Duration as EmbassyDuration, Timer};
use serde::Serialize;

use crate::allports::services::AXUM_BOOT_TCP_PORT;

const AXUM_BOOT_BLOCKING_LANE_RETRY_MS: u64 = 1000;

static AXUM_BOOT_PORT: AtomicU16 = AtomicU16::new(0);

pub fn current_port() -> Option<u16> {
    match AXUM_BOOT_PORT.load(Ordering::Acquire) {
        0 => None,
        port => Some(port),
    }
}

#[derive(Serialize)]
struct AxumBootStatus {
    ok: bool,
    service: &'static str,
    port: u16,
    readiness: u32,
}

async fn root() -> &'static str {
    "trueos axum boot ok\n"
}

async fn status() -> Json<AxumBootStatus> {
    Json(AxumBootStatus {
        ok: true,
        service: "axum-boot",
        port: AXUM_BOOT_TCP_PORT,
        readiness: crate::r::readiness::mask(),
    })
}

fn primary_ipv4_addr(port: u16) -> Option<SocketAddr> {
    let dev_idx = crate::net::primary_device_index();
    let ip = crate::net::adapter::ipv4_at(dev_idx)?;
    Some(SocketAddr::from((ip, port)))
}

async fn axum_boot_runtime() -> Result<(), io::Error> {
    let app = Router::new()
        .route("/", get(root))
        .route("/json", get(status));

    loop {
        let Some(addr) = primary_ipv4_addr(AXUM_BOOT_TCP_PORT) else {
            crate::log!("axum-boot: waiting for primary ipv4\n");
            tokio::time::sleep(core::time::Duration::from_millis(100)).await;
            continue;
        };

        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                crate::log!("axum-boot: bind {} failed kind={:?} err={}\n", addr, err.kind(), err);
                tokio::time::sleep(core::time::Duration::from_millis(1000)).await;
                continue;
            }
        };

        AXUM_BOOT_PORT.store(addr.port(), Ordering::Release);
        crate::log!("axum-boot: listening on http://{}/\n", addr);
        return axum::serve(listener, app).await;
    }
}

fn run_axum_boot_runtime() -> Result<(), io::Error> {
    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_io();
    builder.enable_time();
    let runtime = builder.build()?;
    runtime.block_on(axum_boot_runtime())
}

#[embassy_executor::task]
pub async fn axum_boot_service_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_V4_CONFIGURED).await;
    crate::log!("axum-boot: launching after NET_V4_CONFIGURED\n");

    loop {
        let rc = crate::trueos_tokio_worker::spawn_blocking_job_with_purpose(
            Box::new(|| {
                if let Err(err) = run_axum_boot_runtime() {
                    AXUM_BOOT_PORT.store(0, Ordering::Release);
                    crate::log!("axum-boot: runtime failed {:?}\n", err);
                }
            }),
            "axum-boot-runtime",
        );
        if rc == 0 {
            crate::log!("axum-boot: submitted Tokio runtime to blocking lane\n");
            core::future::pending::<()>().await;
        }
        crate::log!(
            "axum-boot: blocking lane unavailable rc={} retry={}ms\n",
            rc,
            AXUM_BOOT_BLOCKING_LANE_RETRY_MS
        );
        Timer::after(EmbassyDuration::from_millis(AXUM_BOOT_BLOCKING_LANE_RETRY_MS)).await;
    }
}
