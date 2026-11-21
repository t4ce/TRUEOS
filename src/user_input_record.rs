//! Authenticated, encrypted persistence for Shell3 command submissions.
//!
//! Recording is off unless `crypt` has a verified 2FA session for the same
//! shell backend. Plaintext is encrypted immediately; the pending queue and
//! TRUEOSFS only receive fixed-size ChaCha20-Poly1305 records.

mod frame;

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use embassy_sync::signal::Signal;
use spin::Mutex;
use trueos_time::{Duration as EmbassyDuration, Instant, with_deadline};
use zeroize::Zeroizing;

use self::frame::{
    ALGORITHM_CHACHA20_POLY1305, FORMAT_VERSION, HEADER_BYTES, PLAINTEXT_BYTES, RECORD_BYTES,
};

pub(crate) const PATH: &str = "user_input_record.v1.enc";

const FLUSH_INTERVAL_SECS: u64 = 120;
const PENDING_CAP: usize = 256;
const RESTORE_SCAN_RECORDS: u64 = 256;

type EncryptedRecord = Zeroizing<Vec<u8>>;

static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static PENDING: Mutex<VecDeque<EncryptedRecord>> = Mutex::new(VecDeque::new());

static RESTORE: Signal<crate::wait::EmbassySpinRawMutex, RestoreRequest> = Signal::new();

struct RestoreRequest {
    scope: u8,
    identity: (u64, u64),
}

pub(crate) fn request_restore(scope: u8, identity: (u64, u64)) {
    if crate::crypt::authenticated_history_identity(scope) == Some(identity) {
        RESTORE.signal(RestoreRequest { scope, identity });
    }
}

async fn restore_history(request: RestoreRequest) {
    use crate::r::fs::trueosfs;

    let RestoreRequest { scope, identity } = request;
    if crate::crypt::authenticated_history_identity(scope) != Some(identity) {
        return;
    }
    let Some(context) = crate::crypt::authenticated_user_input_record_key(scope) else {
        return;
    };
    if (context.account.raw(), context.challenge_sequence) != identity {
        return;
    }
    let Some(disk) = trueosfs::primary_root_handle() else {
        return;
    };
    let Ok(Some(handle)) = trueosfs::file_read_open_async(disk, PATH).await else {
        return;
    };
    let records = handle.data_len() / RECORD_BYTES as u64;
    let count = records.min(RESTORE_SCAN_RECORDS);
    if count == 0 {
        return;
    }
    let mut bytes = alloc::vec![0; count as usize * RECORD_BYTES];
    let offset = (records - count) * RECORD_BYTES as u64;
    if !matches!(
        trueosfs::file_read_handle_range_async(handle, offset, &mut bytes).await,
        Ok(Some(got)) if got == bytes.len()
    ) {
        return;
    }

    let Ok(cipher) = ChaCha20Poly1305::new_from_slice(context.key_bytes()) else {
        return;
    };
    let mut commands: Vec<Zeroizing<alloc::string::String>> = Vec::new();
    for record in bytes.rchunks_exact(RECORD_BYTES) {
        let Some(text) = frame::decode(&cipher, record, identity.0) else {
            continue;
        };
        if commands.iter().any(|entry| entry.as_str() == text.as_str()) {
            continue;
        }
        commands.push(text);
        if commands.len() == crate::shell3::HISTORY_RECALL_CAP {
            break;
        }
    }
    // A logout, another login, or a remount may have raced the disk read.
    if trueosfs::primary_root_handle() == Some(disk)
        && trueosfs::file_read_handle_is_current(handle)
        && crate::crypt::authenticated_history_identity(scope) == Some(identity)
    {
        crate::shell3::restore_commands(identity.0, commands);
    }
}

/// Capture one already-redacted command if this shell backend owns the active
/// authenticated session. No plaintext is retained by this module.
pub(crate) fn capture(scope_id: u8, identity: (u64, u64), text: &str) {
    if !frame::recallable(text) {
        return;
    }
    let Some(record) = encrypt_record(scope_id, identity, text) else {
        return;
    };

    let mut pending = PENDING.lock();
    if pending.len() >= PENDING_CAP {
        let _ = pending.pop_front();
    }
    pending.push_back(record);
}

fn encrypt_record(scope_id: u8, identity: (u64, u64), text: &str) -> Option<EncryptedRecord> {
    let context = crate::crypt::authenticated_user_input_record_key(scope_id)?;
    if (context.account.raw(), context.challenge_sequence) != identity {
        return None;
    }

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
    let text_len = text.len();
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
            crate::log!(
                "user-input-record: write failed phase=append err={:?}\n",
                error
            );
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
            crate::log!(
                "user-input-record: verify failed phase=info err={:?}\n",
                error
            );
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
            crate::log!(
                "user-input-record: verify failed phase=read err={:?}\n",
                error
            );
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
    let interval = EmbassyDuration::from_secs(FLUSH_INTERVAL_SECS);
    let mut next_flush = Instant::now();
    loop {
        if Instant::now() >= next_flush {
            flush_once().await;
            next_flush = Instant::now() + interval;
        }
        if let Ok(request) = with_deadline(next_flush, RESTORE.wait()).await {
            restore_history(request).await;
        }
    }
}
