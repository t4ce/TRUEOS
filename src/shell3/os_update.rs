use trueos_executor::Spawner;
use trueos_time::{Duration as EmbassyDuration, Timer};

pub(crate) const REMOTE_UPDATE_URL: &str = "https://trueos.eu/TrueOS.tar.lz4";
pub(crate) const LAN_UPDATE_URL: &str = "http://192.168.178.111:8080/TrueOS.tar.lz4";

use crate::shell3::{MatrixTarget, matrix_target_print_line as print_matrix_target_line};
use crate::shell3::matrix_target_interrupted;

pub(crate) fn submit_live_update_to_target(spawner: &Spawner, target: MatrixTarget) {
    submit_live_update_from_url_to_target(spawner, target, REMOTE_UPDATE_URL);
}

pub(crate) fn submit_live_update_from_url_to_target(
    spawner: &Spawner,
    target: MatrixTarget,
    url: &'static str,
) {
    print_matrix_target_line(
        &target,
        "update live: step=01/20 command-accepted mode=RAM-only disk-install=disabled",
    );

    match live_update_command_task(target.clone(), url) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {

            print_matrix_target_line(&target, "update live: spawn failed");
        }
    }
}

#[trueos_executor::task]
async fn live_update_command_task(target: MatrixTarget, url: &'static str) {
    let task_target = target.clone();
    async move {
        Timer::after(EmbassyDuration::from_millis(1)).await;

        let log = |line: &str| {
            print_matrix_target_line(&task_target, line);
        };
        let interrupted = || matrix_target_interrupted(&task_target);

        log("update live: step=02a/20 waiting for IPv4 network");
        crate::r::readiness::wait_for(crate::r::readiness::NET_V4_CONFIGURED).await;
        if interrupted() {
            log("update live: interrupted before download");
            return;
        }
        log("update live: step=02b/20 readiness-satisfied net=v4; checkpoint storage is conditional");

        log(alloc::format!("update live: step=03/20 download-begin {}", url).as_str());
        let payload = match crate::surfer::html_shack::fetch_bytes_via_pool(
            url,
            120_000,
            crate::update_bundle::MAX_COMPRESSED_BYTES,
        )
        .await
        {
            Ok(fetch) => fetch.bytes,
            Err(error) => {
                log(alloc::format!("update live: download failed ({})", error).as_str());
                return;
            }
        };
        if interrupted() {
            log("update live: interrupted after download");
            return;
        }
        log(alloc::format!(
            "update live: step=04a/20 download-complete bytes={}",
            payload.len(),
        )
        .as_str());

        let kernel = match crate::update_bundle::kernel_from_archive(payload).await {
            Ok(kernel) => kernel,
            Err(error) => {
                log(alloc::format!("update live: extract failed ({error})").as_str());
                return;
            }
        };
        log("update live: step=04b/20 native-tar-LZ4-verified");
        if interrupted() {
            log("update live: interrupted before candidate staging");
            return;
        }
        log(alloc::format!(
            "update live: step=05/20 candidate-ELF-extracted bytes={} disk-install=skipped",
            kernel.len(),
        )
        .as_str());
        // The download may run on an administration AP, but the swap parks
        // every AP. Hand ownership to the BSP before beginning that protocol.
        let Some(bsp) = crate::workers::spawner_for_slot(0) else {
            log("update live: failed (BSP executor unavailable)");
            return;
        };
        match live_update_swap_task(kernel, task_target.clone()) {
            Ok(token) => bsp.spawn(token),
            Err(_) => log("update live: failed (swap task pool exhausted)"),
        }
    }
    .await;
}

#[trueos_executor::task]
async fn live_update_swap_task(kernel: alloc::vec::Vec<u8>, target: MatrixTarget) {
    let spawner = unsafe { Spawner::for_current_executor().await };
    match crate::live_update::stage_and_swap(
        kernel,
        spawner,
        target.clone(),
        crate::live_update::NonReplicatableVmPolicy::DiscardAtCommit,
    ).await {
        Ok(never) => match never {},
        Err(error) => {
            let line = alloc::format!("update live: failed ({})", error);
            print_matrix_target_line(&target, line.as_str());
            crate::log_important!(target: "global"; "{}\n", line);
        }
    }
}
