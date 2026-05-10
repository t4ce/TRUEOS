use alloc::format;
use alloc::string::String as AllocString;
use core::sync::atomic::{AtomicU32, Ordering};

use super::{
    MatrixTarget, ShellBackend2, matrix, matrix_target_for_backend, print_matrix_target_line,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatPromptMode {
    Default,
    Add,
}

impl ChatPromptMode {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Default => Self::Add,
            Self::Add => Self::Default,
        }
    }
}

static NEXT_CHAT_CHANNEL: AtomicU32 = AtomicU32::new(1);
pub(crate) const DEFAULT_CHAT_SLOT: &str = "LUM";
const DEFAULT_CHAT_ROOM: &str = "lobby";
const SHELL_CHAT_USER: &str = "shell";
const CHAT_HTTP_TIMEOUT_MS: u32 = 5_000;
const CHAT_HTTP_MAX_RX: usize = 16 * 1024;

fn channel_slot_label(slot_id: &matrix::MatrixSlotId) -> AllocString {
    if slot_id.is_empty() || slot_id.as_str() == DEFAULT_CHAT_SLOT {
        AllocString::from("Room lobby")
    } else if let Some(id) = slot_id.as_str().strip_prefix("AI") {
        format!("channel{}", id)
    } else {
        format!("channel{}", slot_id.as_str())
    }
}

pub(crate) fn ensure_default_channel(output_mask: u8) -> matrix::MatrixSlotId {
    let slot_id = matrix::active_slot_id(output_mask);
    if slot_id.is_empty() {
        matrix::switch_active_slot(output_mask, DEFAULT_CHAT_SLOT)
    } else {
        slot_id
    }
}

pub(crate) fn active_channel_status(output_mask: u8) -> AllocString {
    let mut slot_id = matrix::active_slot_id(output_mask);
    if slot_id.is_empty() {
        slot_id = matrix::slot_id_from_name(DEFAULT_CHAT_SLOT);
    }
    let mut out = channel_slot_label(&slot_id);
    out.push_str("(§");
    out.push_str(slot_id.as_str());
    out.push(')');
    out
}

fn create_channel(io: &'static dyn ShellBackend2, current: &MatrixTarget) {
    let id = NEXT_CHAT_CHANNEL.fetch_add(1, Ordering::Relaxed);
    let requested = format!("AI{}", id);
    let slot = matrix::switch_active_slot(current.output_mask, requested.as_str());
    let target = MatrixTarget {
        output_mask: current.output_mask,
        slot_id: slot.clone(),
    };
    print_matrix_target_line(
        &target,
        format!("chat: created channel{} (§{})", id, slot.as_str()).as_str(),
    );
    let _ = io;
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

fn form_body(user: &str, text: &str) -> AllocString {
    let mut body = AllocString::from("user=");
    form_push_encoded(&mut body, user);
    body.push_str("&text=");
    form_push_encoded(&mut body, text);
    body
}

fn chat_url(port: u16, room: &str) -> AllocString {
    format!("http://127.0.0.1:{}/api/rooms/{}/messages", port, room)
}

#[embassy_executor::task(pool_size = 4)]
async fn shell_chat_post_task(target: MatrixTarget, text: AllocString) {
    let Some(port) = crate::tst_chatserver::current_port() else {
        print_matrix_target_line(&target, "chat: service offline");
        return;
    };
    let url = chat_url(port, DEFAULT_CHAT_ROOM);
    let body = form_body(SHELL_CHAT_USER, text.as_str());
    let result = crate::t::block_on_io(crate::t::net::http::post_http_body_hyper(
        url.as_str(),
        "application/x-www-form-urlencoded",
        body.as_bytes(),
        CHAT_HTTP_TIMEOUT_MS,
        CHAT_HTTP_MAX_RX,
    ));
    match result {
        Ok(Ok(_)) => {
            print_matrix_target_line(&target, format!("{}: {}", SHELL_CHAT_USER, text).as_str())
        }
        Ok(Err(err)) => {
            print_matrix_target_line(&target, format!("chat: post failed: {:?}", err).as_str())
        }
        Err(_) => print_matrix_target_line(&target, "chat: runtime build failed"),
    }
}

pub(crate) fn submit(
    io: &'static dyn ShellBackend2,
    mode: ChatPromptMode,
    target: &MatrixTarget,
    line: &str,
) {
    let trimmed = line.trim();
    if mode == ChatPromptMode::Add || trimmed == "+" {
        create_channel(io, target);
        return;
    }
    if trimmed.is_empty() {
        return;
    }

    ensure_default_channel(target.output_mask);
    let active_target = matrix_target_for_backend(io);
    let text = AllocString::from(trimmed);
    match shell_chat_post_task(active_target.clone(), text.clone()) {
        Ok(token) => {
            if let Some(spawner) = crate::workers::pick_background_spawner() {
                spawner.spawn(token);
            } else {
                print_matrix_target_line(&active_target, "chat: no worker spawner");
            }
        }
        Err(_) => print_matrix_target_line(&active_target, "chat: submit busy"),
    }
}
