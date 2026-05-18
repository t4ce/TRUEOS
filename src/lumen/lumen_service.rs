use alloc::{format, string::String as AllocString};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::shell2::{OUTPUT_UART1_MASK, matrix_target_for_slot_name, print_matrix_target_line};

const SERVICE_SLOT: &str = "LUM";
const CHAT_AI_NAME: &str = "lumen";
const CHAT_READY_HELLO: &str = "hi, I am lumen. Ask me from the lumen prompt.";
const CHAT_BUSY_TEXT: &str = "lumen busy, still working on the previous prompt.";

static SERVICE_SESSION_ID: AtomicU64 = AtomicU64::new(0);
static SERVICE_LOADING: AtomicBool = AtomicBool::new(false);
static SERVICE_ONLINE: AtomicBool = AtomicBool::new(false);
static SERVICE_OWNED_SESSION: AtomicU64 = AtomicU64::new(0);
static SERVICE_WORKER_DONE: AtomicBool = AtomicBool::new(false);
static CHAT_HELLO_SESSION_ID: AtomicU64 = AtomicU64::new(0);
static SERVICE_PROMPT_RUNNING: AtomicBool = AtomicBool::new(false);
static PENDING_LUMEN_PROMPTS: Mutex<alloc::vec::Vec<PendingLumenPrompt>> =
    Mutex::new(alloc::vec::Vec::new());

struct PendingLumenPrompt {
    prompt: AllocString,
}

pub(crate) fn is_online() -> bool {
    SERVICE_ONLINE.load(Ordering::Acquire)
}

pub(crate) fn is_prompt_running() -> bool {
    SERVICE_PROMPT_RUNNING.load(Ordering::Acquire)
}

pub(crate) fn remote_work_capacity() -> u32 {
    if !is_online() || SERVICE_LOADING.load(Ordering::Acquire) || is_prompt_running() {
        return 0;
    }
    crate::lumen::burn_baby::online_worker_count().min(u32::MAX as usize) as u32
}

pub(crate) fn mark_prompt_running(reason: &'static str) -> bool {
    match SERVICE_PROMPT_RUNNING.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
    {
        Ok(_) => {
            crate::log!("lumen-service: prompt running reason={}\n", reason);
            true
        }
        Err(_) => false,
    }
}

pub(crate) fn mark_prompt_complete(reason: &'static str) {
    let was_busy = SERVICE_PROMPT_RUNNING.swap(false, Ordering::AcqRel);
    if was_busy {
        crate::log!("lumen-service: prompt complete reason={}\n", reason);
    }
}

pub(crate) fn mark_online(session_id: u64) {
    if SERVICE_OWNED_SESSION.load(Ordering::Acquire) != session_id {
        return;
    }
    SERVICE_SESSION_ID.store(session_id, Ordering::Release);
    SERVICE_LOADING.store(false, Ordering::Release);
    SERVICE_ONLINE.store(true, Ordering::Release);
    emit_ready_hello(session_id);
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
    CHAT_HELLO_SESSION_ID
        .compare_exchange(session_id, 0, Ordering::AcqRel, Ordering::Acquire)
        .ok();
    clear_pending_prompts("offline");
}

fn lumen_target() -> crate::shell2::MatrixTarget {
    matrix_target_for_slot_name(OUTPUT_UART1_MASK, SERVICE_SLOT)
}

fn print_lumen_line(text: &str) {
    let target = lumen_target();
    print_matrix_target_line(&target, text);
}

fn print_lumen_message(user: &str, text: &str) {
    print_lumen_line(format!("{}: {}", user, text).as_str());
}

fn queue_pending_prompt(prompt: &str, reason: &str) {
    let mut pending = PENDING_LUMEN_PROMPTS.lock();
    if !pending.is_empty() {
        crate::log!(
            "lumen-service: rejected extra pending prompt reason={} pending={} bytes={}\n",
            reason,
            pending.len(),
            prompt.len()
        );
        return;
    }
    pending.push(PendingLumenPrompt {
        prompt: AllocString::from(prompt),
    });
    crate::log!(
        "lumen-service: buffered prompt reason={} pending={} bytes={}\n",
        reason,
        pending.len(),
        prompt.len()
    );
}

fn clear_pending_prompts(reason: &str) {
    let mut pending = PENDING_LUMEN_PROMPTS.lock();
    let count = pending.len();
    pending.clear();
    SERVICE_PROMPT_RUNNING.store(false, Ordering::Release);
    if count != 0 {
        crate::log!("lumen-service: cleared pending prompts reason={} count={}\n", reason, count);
    }
}

fn flush_pending(session_id: u64) {
    let mut pending = PENDING_LUMEN_PROMPTS.lock();
    let queued = core::mem::take(&mut *pending);
    drop(pending);
    if !queued.is_empty() {
        crate::log!(
            "lumen-service: flushing pending prompts session={} count={}\n",
            session_id,
            queued.len()
        );
    }
    for prompt in queued {
        if !crate::lumen::push_lumen_chat_prompt(session_id, prompt.prompt.as_str(), None) {
            queue_pending_prompt(prompt.prompt.as_str(), "flush-missed-session");
            break;
        }
    }
}

pub(crate) fn submit_lumen_prompt(prompt: &str) -> bool {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return false;
    }

    if !mark_prompt_running("lumen-submit") {
        crate::log!("lumen-service: rejected prompt reason=busy bytes={}\n", prompt.len());
        print_lumen_message(CHAT_AI_NAME, CHAT_BUSY_TEXT);
        return false;
    }

    let session_id = SERVICE_SESSION_ID.load(Ordering::Acquire);
    if session_id != 0 && is_online() {
        if crate::lumen::push_lumen_chat_prompt(session_id, prompt, None) {
            crate::log!(
                "lumen-service: accepted prompt session={} bytes={}\n",
                session_id,
                prompt.len()
            );
            return true;
        }
        queue_pending_prompt(prompt, "online-missed-session");
        return false;
    }

    if SERVICE_LOADING.load(Ordering::Acquire) {
        queue_pending_prompt(prompt, "warming");
    } else {
        crate::log!("lumen-service: dropped prompt; service offline\n");
        mark_prompt_complete("offline-drop");
    }
    false
}

pub(crate) fn submit_chat_answer(answer: &str) {
    let answer = answer.trim();
    if answer.is_empty() {
        print_lumen_message(CHAT_AI_NAME, "<empty>");
    } else {
        print_lumen_message(CHAT_AI_NAME, answer);
    }
}

pub(crate) fn submit_chat_statement_delta(statement: &str, delta: &str) {
    let _ = statement;
    let delta = delta.trim_start_matches('\0');
    if delta.is_empty() {
        return;
    }
    print_lumen_message(CHAT_AI_NAME, delta);
}

fn emit_ready_hello(session_id: u64) {
    if CHAT_HELLO_SESSION_ID.swap(session_id, Ordering::AcqRel) == session_id {
        return;
    }

    let target = lumen_target();
    print_matrix_target_line(&target, format!("{}: {}", CHAT_AI_NAME, CHAT_READY_HELLO).as_str());
}

#[embassy_executor::task(pool_size = 1)]
async fn lumen_service_worker_task(target: crate::shell2::MatrixTarget, session_id: u64) {
    let cpu_slot = crate::percpu::current_slot();
    let lapic_id = crate::percpu::current_lapic_id_via_cpuid();
    crate::log!(
        "lumen-service: worker start session={} cpu_slot={} lapic={}\n",
        session_id,
        cpu_slot,
        lapic_id
    );
    crate::lumen::run_lumen_session(target, session_id).await;
    SERVICE_WORKER_DONE.store(true, Ordering::Release);
    crate::log!("lumen-service: worker done session={}\n", session_id);
}

#[embassy_executor::task]
pub async fn lumen_service_task() {
    let cpu_slot = crate::percpu::current_slot();
    let lapic_id = crate::percpu::current_lapic_id_via_cpuid();
    crate::log!("lumen-service: task start cpu_slot={} lapic={}\n", cpu_slot, lapic_id);

    let target =
        crate::shell2::matrix_target_for_slot_name(crate::shell2::OUTPUT_UART1_MASK, SERVICE_SLOT);
    let session_id = crate::shell2::cmds::bench::bench_session_start();
    SERVICE_OWNED_SESSION.store(session_id, Ordering::Release);
    SERVICE_SESSION_ID.store(session_id, Ordering::Release);
    SERVICE_LOADING.store(true, Ordering::Release);
    SERVICE_ONLINE.store(false, Ordering::Release);
    SERVICE_WORKER_DONE.store(false, Ordering::Release);

    print_matrix_target_line(&target, "lumen-service: warming model from TRUEOSFS");
    let spawned_worker = match crate::workers::pick_background_spawner_with_slot() {
        Some((slot, kind, spawner)) => {
            match lumen_service_worker_task(target.clone(), session_id) {
                Ok(token) => {
                    crate::log!(
                        "lumen-service: coordinator handoff session={} target_slot={} core_kind={} work=run-lumen-session\n",
                        session_id,
                        slot,
                        kind
                    );
                    spawner.spawn(token);
                    true
                }
                Err(err) => {
                    crate::log_warn!(
                        target: "lumen";
                        "lumen-service: worker spawn failed session={} err={:?}\n",
                        session_id,
                        err
                    );
                    false
                }
            }
        }
        None => {
            crate::log_warn!(
                target: "lumen";
                "lumen-service: worker spawn skipped session={} reason=no-background-worker\n",
                session_id
            );
            false
        }
    };

    if spawned_worker {
        loop {
            if SERVICE_WORKER_DONE.load(Ordering::Acquire) {
                break;
            }
            if SERVICE_OWNED_SESSION.load(Ordering::Acquire) != session_id {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(500)).await;
        }
    }

    SERVICE_LOADING.store(false, Ordering::Release);
    SERVICE_ONLINE.store(false, Ordering::Release);
    SERVICE_SESSION_ID.store(0, Ordering::Release);
    SERVICE_OWNED_SESSION.store(0, Ordering::Release);
    clear_pending_prompts("service-stopped");
    crate::shell2::cmds::bench::bench_session_finish(session_id);
    print_matrix_target_line(&target, "lumen-service: stopped");

    loop {
        Timer::after(EmbassyDuration::from_secs(60)).await;
    }
}
