use alloc::string::String as AllocString;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::shell2::{MatrixTarget, print_matrix_target_line};

const SERVICE_SLOT: &str = "LUM";

static SERVICE_SESSION_ID: AtomicU64 = AtomicU64::new(0);
static SERVICE_LOADING: AtomicBool = AtomicBool::new(false);
static SERVICE_ONLINE: AtomicBool = AtomicBool::new(false);
static SERVICE_OWNED_SESSION: AtomicU64 = AtomicU64::new(0);
static PENDING: Mutex<alloc::vec::Vec<(MatrixTarget, AllocString)>> =
    Mutex::new(alloc::vec::Vec::new());

pub(crate) fn is_online() -> bool {
    SERVICE_ONLINE.load(Ordering::Acquire)
}

pub(crate) fn mark_online(session_id: u64) {
    if SERVICE_OWNED_SESSION.load(Ordering::Acquire) != session_id {
        return;
    }
    SERVICE_SESSION_ID.store(session_id, Ordering::Release);
    SERVICE_LOADING.store(false, Ordering::Release);
    SERVICE_ONLINE.store(true, Ordering::Release);
    flush_pending(session_id);
}

pub(crate) fn mark_offline(session_id: u64) {
    if SERVICE_OWNED_SESSION.load(Ordering::Acquire) != session_id {
        return;
    }
    SERVICE_ONLINE.store(false, Ordering::Release);
    SERVICE_SESSION_ID
        .compare_exchange(session_id, 0, Ordering::AcqRel, Ordering::Acquire)
        .ok();
}

fn flush_pending(session_id: u64) {
    let mut pending = PENDING.lock();
    let queued = core::mem::take(&mut *pending);
    drop(pending);
    for (target, prompt) in queued {
        let _ =
            crate::shell2::cmds::bench_ai::push_lumen_prompt(session_id, &target, prompt.as_str());
    }
}

pub(crate) fn submit_chat(target: MatrixTarget, prompt: &str) {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return;
    }

    let session_id = SERVICE_SESSION_ID.load(Ordering::Acquire);
    if session_id != 0 && is_online() {
        if crate::shell2::cmds::bench_ai::push_lumen_prompt(session_id, &target, prompt) {
            print_matrix_target_line(&target, "lumen: thinking...");
            return;
        }
    }

    if SERVICE_LOADING.load(Ordering::Acquire) {
        PENDING
            .lock()
            .push((target.clone(), AllocString::from(prompt)));
        print_matrix_target_line(&target, "lumen: warming; queued prompt");
    } else {
        print_matrix_target_line(&target, "lumen: service offline");
    }
}

#[embassy_executor::task]
pub async fn lumen_service_task() {
    let target =
        crate::shell2::matrix_target_for_slot_name(crate::shell2::OUTPUT_UART1_MASK, SERVICE_SLOT);
    let session_id = crate::shell2::cmds::bench::bench_session_start();
    SERVICE_OWNED_SESSION.store(session_id, Ordering::Release);
    SERVICE_SESSION_ID.store(session_id, Ordering::Release);
    SERVICE_LOADING.store(true, Ordering::Release);
    SERVICE_ONLINE.store(false, Ordering::Release);

    print_matrix_target_line(&target, "lumen-service: warming model from TRUEOSFS");
    crate::shell2::cmds::bench_ai::run_lumen_session(target.clone(), session_id).await;

    SERVICE_LOADING.store(false, Ordering::Release);
    SERVICE_ONLINE.store(false, Ordering::Release);
    SERVICE_SESSION_ID.store(0, Ordering::Release);
    SERVICE_OWNED_SESSION.store(0, Ordering::Release);
    crate::shell2::cmds::bench::bench_session_finish(session_id);
    print_matrix_target_line(&target, "lumen-service: stopped");

    loop {
        Timer::after(EmbassyDuration::from_secs(60)).await;
    }
}
