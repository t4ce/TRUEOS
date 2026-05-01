use alloc::{format, string::String as AllocString, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use serde::Deserialize;
use spin::Mutex;

use crate::shell2::{MatrixTarget, print_matrix_target_line};

const SERVICE_SLOT: &str = "LUM";
const CHAT_ROOM: &str = "lobby";
const CHAT_AI_NAME: &str = "lumen";
const CHAT_READY_HELLO: &str = "hi, I am lumen. Mention lumen to talk to me.";
const CHAT_HTTP_TIMEOUT_MS: u32 = 5_000;
const CHAT_HTTP_MAX_RX: usize = 128 * 1024;

static SERVICE_SESSION_ID: AtomicU64 = AtomicU64::new(0);
static SERVICE_LOADING: AtomicBool = AtomicBool::new(false);
static SERVICE_ONLINE: AtomicBool = AtomicBool::new(false);
static SERVICE_OWNED_SESSION: AtomicU64 = AtomicU64::new(0);
static CHAT_HELLO_SESSION_ID: AtomicU64 = AtomicU64::new(0);
static PENDING: Mutex<alloc::vec::Vec<(MatrixTarget, AllocString)>> =
    Mutex::new(alloc::vec::Vec::new());

#[derive(Deserialize)]
struct ChatMessagesResponse {
    messages: Vec<ChatMessage>,
}

#[derive(Deserialize)]
struct ChatMessage {
    id: u64,
    user: AllocString,
    text: AllocString,
}

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

fn chat_post_body(user: &str, text: &str) -> AllocString {
    let mut body = AllocString::from("user=");
    form_push_encoded(&mut body, user);
    body.push_str("&text=");
    form_push_encoded(&mut body, text);
    body
}

fn chat_name_mentioned(text: &str) -> bool {
    text.to_ascii_lowercase().contains(CHAT_AI_NAME)
}

fn post_chat_message(user: &str, text: &str) -> bool {
    let Some(port) = crate::r::net::srv::chat::current_port() else {
        return false;
    };
    let url = chat_message_url(port, None);
    let body = chat_post_body(user, text);
    matches!(
        crate::t::block_on_io(crate::t::net::http::post_http_body_hyper(
            url.as_str(),
            "application/x-www-form-urlencoded",
            body.as_bytes(),
            CHAT_HTTP_TIMEOUT_MS,
            CHAT_HTTP_MAX_RX,
        )),
        Ok(Ok(_))
    )
}

pub(crate) fn submit_chat_answer(answer: &str) {
    let answer = answer.trim();
    if answer.is_empty() {
        let _ = post_chat_message(CHAT_AI_NAME, "<empty>");
    } else {
        let _ = post_chat_message(CHAT_AI_NAME, answer);
    }
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

async fn lumen_chat_watch_loop(session_id: u64) {
    let mut since = 0u64;
    loop {
        if SERVICE_OWNED_SESSION.load(Ordering::Acquire) != session_id {
            return;
        }
        if !is_online() {
            Timer::after(EmbassyDuration::from_millis(250)).await;
            continue;
        }
        let Some(port) = crate::r::net::srv::chat::current_port() else {
            Timer::after(EmbassyDuration::from_millis(500)).await;
            continue;
        };
        let url = chat_message_url(port, Some(since));
        let body = match crate::t::block_on_io(crate::t::net::http::fetch_http_body_hyper(
            url.as_str(),
            CHAT_HTTP_TIMEOUT_MS,
            CHAT_HTTP_MAX_RX,
        )) {
            Ok(Ok(body)) => body,
            _ => {
                Timer::after(EmbassyDuration::from_millis(1000)).await;
                continue;
            }
        };
        let Ok(response) = serde_json::from_slice::<ChatMessagesResponse>(body.as_slice()) else {
            Timer::after(EmbassyDuration::from_millis(1000)).await;
            continue;
        };
        for message in response.messages {
            since = since.max(message.id);
            if message.user.trim().eq_ignore_ascii_case(CHAT_AI_NAME) {
                continue;
            }
            if chat_name_mentioned(message.text.as_str()) {
                let prompt = format!("{}: {}", message.user.trim(), message.text.trim());
                if crate::shell2::cmds::bench_ai::push_lumen_chat_prompt(
                    session_id,
                    prompt.as_str(),
                ) {
                    crate::log!("lumen-service: queued chat prompt id={}\n", message.id);
                }
            }
        }
        Timer::after(EmbassyDuration::from_millis(1000)).await;
    }
}

#[embassy_executor::task(pool_size = 1)]
async fn lumen_chat_watch_task(session_id: u64) {
    lumen_chat_watch_loop(session_id).await;
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

    if let Ok(token) = lumen_chat_watch_task(session_id) {
        if let Some(spawner) = crate::workers::pick_background_spawner() {
            spawner.spawn(token);
        }
    }

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
