//! Authenticated, encrypted persistence for Shell2 command submissions.
//!
//! Recording is off unless `crypt` has a verified 2FA session for the same
//! shell backend. Plaintext is encrypted immediately; the pending queue and
//! TRUEOSFS only receive fixed-size ChaCha20-Poly1305 records.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use trueos_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;
use zeroize::Zeroizing;

pub(crate) const PATH: &str = "user_input_record.v1.enc";

const FLUSH_INTERVAL_SECS: u64 = 120;
const PENDING_CAP: usize = 256;
const COMMAND_BYTES: usize = 192;
const HEADER_BYTES: usize = 32;
const PLAINTEXT_BYTES: usize = 40 + COMMAND_BYTES;
const TAG_BYTES: usize = 16;
const RECORD_BYTES: usize = HEADER_BYTES + PLAINTEXT_BYTES + TAG_BYTES;
const FORMAT_VERSION: u8 = 1;
const ALGORITHM_CHACHA20_POLY1305: u8 = 1;

type EncryptedRecord = Zeroizing<Vec<u8>>;

static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static PENDING: Mutex<VecDeque<EncryptedRecord>> = Mutex::new(VecDeque::new());

/// Capture one already-redacted command if this shell backend owns the active
/// authenticated session. No plaintext is retained by this module.
pub(crate) fn capture(scope_id: u8, text: &str) {
    let Some(record) = encrypt_record(scope_id, text) else {
        return;
    };

    let mut pending = PENDING.lock();
    if pending.len() >= PENDING_CAP {
        let _ = pending.pop_front();
    }
    pending.push_back(record);
}

fn encrypt_record(scope_id: u8, text: &str) -> Option<EncryptedRecord> {
    let context = crate::crypt::authenticated_user_input_record_key(scope_id)?;

    let mut nonce_bytes = [0u8; 12];
    if !crate::tyche::fill_bytes(&mut nonce_bytes) {
        return None;
    }

    let sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::AcqRel);
    let mut header = [0u8; HEADER_BYTES];
    header[0..4].copy_from_slice(b"TUIR");
    header[4] = FORMAT_VERSION;
    header[5] = ALGORITHM_CHACHA20_POLY1305;
    header[6..8].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
    header[8..12].copy_from_slice(&(RECORD_BYTES as u32).to_le_bytes());
    header[12..20].copy_from_slice(&sequence.to_le_bytes());
    header[20..32].copy_from_slice(&nonce_bytes);

    let mut plaintext = Zeroizing::new([0u8; PLAINTEXT_BYTES]);
    plaintext[0..8].copy_from_slice(&embassy_time_driver::now().to_le_bytes());
    plaintext[8..16].copy_from_slice(&context.account.raw().to_le_bytes());
    plaintext[16..24].copy_from_slice(&context.challenge_sequence.to_le_bytes());
    plaintext[24..32].copy_from_slice(&context.authenticated_at_ticks.to_le_bytes());
    let text_len = text.len().min(COMMAND_BYTES);
    plaintext[32..34].copy_from_slice(&(text_len as u16).to_le_bytes());
    plaintext[34] = context.scope_id;
    plaintext[40..40 + text_len].copy_from_slice(&text.as_bytes()[..text_len]);

    let cipher = ChaCha20Poly1305::new_from_slice(context.key_bytes()).ok()?;
    let tag = cipher
        .encrypt_in_place_detached(
            Nonce::from_slice(&nonce_bytes),
            header.as_slice(),
            plaintext.as_mut_slice(),
        )
        .ok()?;

    let mut record = Zeroizing::new(Vec::with_capacity(RECORD_BYTES));
    record.extend_from_slice(&header);
    record.extend_from_slice(plaintext.as_slice());
    record.extend_from_slice(tag.as_slice());
    Some(record)
}

fn take_pending() -> Vec<EncryptedRecord> {
    PENDING.lock().drain(..).collect()
}

fn restore_pending(entries: Vec<EncryptedRecord>) {
    if entries.is_empty() {
        return;
    }

    let mut pending = PENDING.lock();
    for entry in entries.into_iter().rev() {
        pending.push_front(entry);
    }
    while pending.len() > PENDING_CAP {
        let _ = pending.pop_back();
    }
}

async fn append_and_verify(payload: &[u8]) -> bool {
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::log!("user-input-record: write deferred reason=no-root\n");
        return false;
    };

    match crate::r::fs::trueosfs::file_append_async(disk, PATH, payload).await {
        Ok(true) => {}
        Ok(false) => {
            crate::log!("user-input-record: write failed phase=append\n");
            return false;
        }
        Err(error) => {
            crate::log!("user-input-record: write failed phase=append err={:?}\n", error);
            return false;
        }
    }

    let info = match crate::r::fs::trueosfs::file_info_async(disk, PATH).await {
        Ok(Some(info)) => info,
        Ok(None) => {
            crate::log!("user-input-record: verify failed reason=missing\n");
            return false;
        }
        Err(error) => {
            crate::log!("user-input-record: verify failed phase=info err={:?}\n", error);
            return false;
        }
    };
    if info.data_len < payload.len() as u64 {
        crate::log!("user-input-record: verify failed reason=short-file\n");
        return false;
    }

    let offset = info.data_len - payload.len() as u64;
    let mut readback = Zeroizing::new(Vec::new());
    readback.resize(payload.len(), 0);
    match crate::r::fs::trueosfs::file_read_range_async(disk, PATH, offset, readback.as_mut_slice())
        .await
    {
        Ok(Some(got)) if got == payload.len() && readback.as_slice() == payload => true,
        Ok(Some(got)) => {
            crate::log!(
                "user-input-record: verify failed phase=read got={} expected={}\n",
                got,
                payload.len()
            );
            false
        }
        Ok(None) => {
            crate::log!("user-input-record: verify failed phase=read reason=missing\n");
            false
        }
        Err(error) => {
            crate::log!("user-input-record: verify failed phase=read err={:?}\n", error);
            false
        }
    }
}

async fn flush_once() {
    let entries = take_pending();
    if entries.is_empty() {
        return;
    }

    let record_count = entries.len();
    let mut payload = Zeroizing::new(Vec::with_capacity(record_count * RECORD_BYTES));
    for entry in entries.iter() {
        payload.extend_from_slice(entry.as_slice());
    }

    if append_and_verify(payload.as_slice()).await {
        crate::log!(
            "user-input-record: write verified records={} bytes={}\n",
            record_count,
            payload.len()
        );
    } else {
        restore_pending(entries);
    }
}

#[trueos_executor::task]
pub(crate) async fn writer_task() {
    crate::log_info!(target: "service"; "user-input-record: writer online default=off auth=cry-2fa encryption=chacha20-poly1305\n");
    loop {
        flush_once().await;
        Timer::after(EmbassyDuration::from_secs(FLUSH_INTERVAL_SECS)).await;
    }
}
