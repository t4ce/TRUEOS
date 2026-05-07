use alloc::{format, string::String as AllocString};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::shell2::print_matrix_target_line;

const SERVICE_SLOT: &str = "LUM";
const CHAT_ROOM: &str = "lobby";
const CHAT_AI_NAME: &str = "lumen";
const CHAT_READY_HELLO: &str = "hi, I am lumen. Mention lumen to talk to me.";
const CHAT_BUSY_TEXT: &str = "lumen busy, still working on the previous prompt.";
const CHAT_HTTP_TIMEOUT_MS: u32 = 5_000;
const CHAT_HTTP_MAX_RX: usize = 128 * 1024;

static SERVICE_SESSION_ID: AtomicU64 = AtomicU64::new(0);
static SERVICE_LOADING: AtomicBool = AtomicBool::new(false);
static SERVICE_ONLINE: AtomicBool = AtomicBool::new(false);
static SERVICE_OWNED_SESSION: AtomicU64 = AtomicU64::new(0);
static CHAT_HELLO_SESSION_ID: AtomicU64 = AtomicU64::new(0);
static CHAT_STATEMENT_ID: AtomicU64 = AtomicU64::new(1);
static SERVICE_PROMPT_RUNNING: AtomicBool = AtomicBool::new(false);
static PENDING_CHATROOM: Mutex<alloc::vec::Vec<PendingChatroomPrompt>> =
    Mutex::new(alloc::vec::Vec::new());

struct PendingChatroomPrompt {
    prompt: AllocString,
    statement: AllocString,
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

fn queue_pending_prompt(prompt: &str, statement: &str, reason: &str) {
    let mut pending = PENDING_CHATROOM.lock();
    if !pending.is_empty() {
        crate::log!(
            "lumen-service: rejected extra pending chatroom prompt reason={} pending={} bytes={}\n",
            reason,
            pending.len(),
            prompt.len()
        );
        return;
    }
    pending.push(PendingChatroomPrompt {
        prompt: AllocString::from(prompt),
        statement: AllocString::from(statement),
    });
    crate::log!(
        "lumen-service: buffered chatroom prompt reason={} pending={} bytes={}\n",
        reason,
        pending.len(),
        prompt.len()
    );
}

fn clear_pending_prompts(reason: &str) {
    let mut pending = PENDING_CHATROOM.lock();
    let count = pending.len();
    pending.clear();
    SERVICE_PROMPT_RUNNING.store(false, Ordering::Release);
    if count != 0 {
        crate::log!(
            "lumen-service: cleared pending chatroom prompts reason={} count={}\n",
            reason,
            count
        );
    }
}

fn flush_pending(session_id: u64) {
    let mut pending = PENDING_CHATROOM.lock();
    let queued = core::mem::take(&mut *pending);
    drop(pending);
    if !queued.is_empty() {
        crate::log!(
            "lumen-service: flushing pending chatroom prompts session={} count={}\n",
            session_id,
            queued.len()
        );
    }
    for prompt in queued {
        if !crate::lumen::push_lumen_chat_prompt(
            session_id,
            prompt.prompt.as_str(),
            Some(prompt.statement.as_str()),
        ) {
            queue_pending_prompt(
                prompt.prompt.as_str(),
                prompt.statement.as_str(),
                "flush-missed-session",
            );
            break;
        }
    }
}

pub(crate) fn submit_chatroom_mention(prompt: &str) -> bool {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return false;
    }

    if !mark_prompt_running("chatroom-submit") {
        crate::log!("lumen-service: rejected chatroom prompt reason=busy bytes={}\n", prompt.len());
        let _ = post_chat_message(CHAT_AI_NAME, CHAT_BUSY_TEXT);
        return false;
    }

    let session_id = SERVICE_SESSION_ID.load(Ordering::Acquire);
    let statement = next_chat_statement_tag();
    if session_id != 0 && is_online() {
        if crate::lumen::push_lumen_chat_prompt(session_id, prompt, Some(statement.as_str())) {
            submit_chat_statement_placeholder(statement.as_str());
            crate::log!(
                "lumen-service: accepted chatroom prompt session={} bytes={}\n",
                session_id,
                prompt.len()
            );
            return true;
        }
        queue_pending_prompt(prompt, statement.as_str(), "online-missed-session");
        return false;
    }

    if SERVICE_LOADING.load(Ordering::Acquire) {
        submit_chat_statement_placeholder(statement.as_str());
        queue_pending_prompt(prompt, statement.as_str(), "warming");
    } else {
        crate::log!("lumen-service: dropped chatroom prompt; service offline\n");
        mark_prompt_complete("offline-drop");
    }
    false
}

fn form_push_encoded(out: &mut AllocString, value: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value.as_bytes().iter().copied() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            b' ' => out.push('+'),
            other => {
                out.push('%');
                out.push(char::from(HEX[(other >> 4) as usize]));
                out.push(char::from(HEX[(other & 0x0f) as usize]));
            }
        }
    }
}

fn chat_message_url(port: u16, since: Option<u64>) -> AllocString {
    match since {
        Some(since) => {
            format!("http://127.0.0.1:{}/api/rooms/{}/messages?since={}", port, CHAT_ROOM, since)
        }
        None => format!("http://127.0.0.1:{}/api/rooms/{}/messages", port, CHAT_ROOM),
    }
}

fn chat_post_body(user: &str, text: &str, statement: Option<&str>) -> AllocString {
    let mut body = AllocString::from("user=");
    form_push_encoded(&mut body, user);
    body.push_str("&text=");
    form_push_encoded(&mut body, text);
    if let Some(statement) = statement {
        body.push_str("&statement=");
        form_push_encoded(&mut body, statement);
    }
    body
}

fn post_chat_message(user: &str, text: &str) -> bool {
    post_chat_statement(user, None, text)
}

fn post_chat_statement(user: &str, statement: Option<&str>, text: &str) -> bool {
    let local_ok = match statement {
        Some(statement) => crate::r::net::srv::chat::post_local_statement_volatile(
            CHAT_ROOM, user, statement, text,
        ),
        None => crate::r::net::srv::chat::post_local_message_volatile(CHAT_ROOM, user, text),
    };
    if local_ok {
        crate::log!("lumen-service: inserted chat message user={} bytes={}\n", user, text.len());
        return true;
    }

    let Some(port) = crate::r::net::srv::chat::current_port() else {
        crate::log!("lumen-service: chat post failed; no chat port\n");
        return false;
    };
    let url = chat_message_url(port, None);
    let body = chat_post_body(user, text, statement);
    let ok = matches!(
        crate::t::block_on_io(crate::t::net::http::post_http_body_hyper(
            url.as_str(),
            "application/x-www-form-urlencoded",
            body.as_bytes(),
            CHAT_HTTP_TIMEOUT_MS,
            CHAT_HTTP_MAX_RX,
        )),
        Ok(Ok(_))
    );
    if !ok {
        crate::log!(
            "lumen-service: chat post failed via http user={} bytes={}\n",
            user,
            text.len()
        );
    }
    ok
}

pub(crate) fn submit_chat_answer(answer: &str) {
    let answer = answer.trim();
    if answer.is_empty() {
        let _ = post_chat_message(CHAT_AI_NAME, "<empty>");
    } else {
        let _ = post_chat_message(CHAT_AI_NAME, answer);
    }
}

pub(crate) fn next_chat_statement_tag() -> AllocString {
    let id = CHAT_STATEMENT_ID.fetch_add(1, Ordering::AcqRel);
    format!("lumen:{}", id)
}

pub(crate) fn submit_chat_statement_delta(statement: &str, delta: &str) {
    let delta = delta.trim_start_matches('\0');
    if delta.is_empty() {
        return;
    }
    let _ = post_chat_statement(CHAT_AI_NAME, Some(statement), delta);
}

fn submit_chat_statement_placeholder(statement: &str) {
    let _ = post_chat_statement(CHAT_AI_NAME, Some(statement), "");
}

fn emit_ready_hello(session_id: u64) {
    if CHAT_HELLO_SESSION_ID.swap(session_id, Ordering::AcqRel) == session_id {
        return;
    }

    let target =
        crate::shell2::matrix_target_for_slot_name(crate::shell2::OUTPUT_UART1_MASK, SERVICE_SLOT);
    print_matrix_target_line(&target, format!("{}: {}", CHAT_AI_NAME, CHAT_READY_HELLO).as_str());

    match lumen_chat_ready_hello_task(session_id) {
        Ok(token) => {
            if let Some(spawner) = crate::workers::pick_background_spawner() {
                spawner.spawn(token);
            } else if !post_chat_message(CHAT_AI_NAME, CHAT_READY_HELLO) {
                print_matrix_target_line(&target, "lumen-service: chat hello post failed");
            }
        }
        Err(_) => {
            if !post_chat_message(CHAT_AI_NAME, CHAT_READY_HELLO) {
                print_matrix_target_line(&target, "lumen-service: chat hello busy");
            }
        }
    }
}

async fn post_ready_hello_loop(session_id: u64) {
    for _ in 0..40 {
        if SERVICE_OWNED_SESSION.load(Ordering::Acquire) != session_id || !is_online() {
            return;
        }
        if post_chat_message(CHAT_AI_NAME, CHAT_READY_HELLO) {
            crate::log!("lumen-service: announced chat ready as {}\n", CHAT_AI_NAME);
            return;
        }
        Timer::after(EmbassyDuration::from_millis(500)).await;
    }
    crate::log!("lumen-service: chat ready announce skipped; chat service unavailable\n");
}

#[embassy_executor::task(pool_size = 1)]
async fn lumen_chat_ready_hello_task(session_id: u64) {
    post_ready_hello_loop(session_id).await;
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

    print_matrix_target_line(&target, "lumen-service: warming model from TRUEOSFS");
    crate::lumen::run_lumen_session(target.clone(), session_id).await;

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
