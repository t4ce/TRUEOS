extern crate alloc;

include!("../cabi_codes.rs");

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use core::sync::atomic::{AtomicU32, Ordering};

use spin::Mutex;

use crate::r::net::cli::smtp::{SmtpClient, SmtpError};

const SMTP_BODY_MAX: usize = 64 * 1024;
const SMTP_DEFAULT_TIMEOUT_MS: u32 = 20_000;

static CABI_SMTP_SEQ: AtomicU32 = AtomicU32::new(1);
static CABI_SMTP_RESULTS: Mutex<BTreeMap<u32, Option<i32>>> = Mutex::new(BTreeMap::new());

fn smtp_error_to_code(err: SmtpError) -> i32 {
    match err {
        SmtpError::DnsFailed => NET_ERR_TIMEOUT_DNS,
        SmtpError::ConnectFailed => NET_ERR_TIMEOUT_CONNECT,
        SmtpError::Timeout => NET_ERR_TIMEOUT,
        SmtpError::TlsFailed => NET_ERR_TLS,
        SmtpError::AuthFailed | SmtpError::ReplyError(_, _) | SmtpError::Protocol => NET_ERR_HTTP,
        SmtpError::Io | SmtpError::Closed => FS_ERR_IO,
    }
}

fn monotonic_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

fn sanitize_header(value: &str) -> String {
    let mut out = String::new();
    for ch in value.trim().chars() {
        if ch != '\r' && ch != '\n' {
            out.push(ch);
        }
    }
    out
}

fn first_recipient(to: &str) -> Option<&str> {
    to.split(|ch| ch == ',' || ch == ';')
        .map(str::trim)
        .find(|part| !part.is_empty() && part.contains('@'))
}

fn build_message(from: &str, to: &str, subject: &str, body: &str) -> String {
    format!(
        "From: <{}>\r\nTo: <{}>\r\nSubject: {}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=US-ASCII\r\nContent-Transfer-Encoding: 7bit\r\nX-Mailer: TRUEOS Blueprint\r\n\r\n{}",
        from,
        to,
        sanitize_header(subject),
        body
    )
}

async fn send_text_mail(to: String, subject: String, body: String, timeout_ms: u32) -> i32 {
    if body.trim().is_empty() || body.len() > SMTP_BODY_MAX {
        return FS_ERR_BAD_PARAM;
    }
    let Some(to) = first_recipient(to.as_str()).map(String::from) else {
        return FS_ERR_BAD_PARAM;
    };

    let Ok((user, pass, from)) = crate::r::net::mail_config::runtime_smtp_account().await else {
        return FS_ERR_NOT_FOUND;
    };

    let timeout_ms = timeout_ms.max(1);
    let wire = build_message(from.as_str(), to.as_str(), subject.as_str(), body.as_str());
    crate::log!(
        "smtp-cabi: send begin to={} subject_bytes={} body_bytes={}\n",
        to.as_str(),
        subject.len(),
        body.len()
    );

    let result = async {
        let mut client = SmtpClient::connect(timeout_ms).await?;
        client
            .auth_login(user.as_str(), pass.as_str(), timeout_ms)
            .await?;
        client
            .send_mail(from.as_str(), &[to.as_str()], wire.as_str(), timeout_ms)
            .await?;
        let _ = client.quit(5_000).await;
        Ok::<(), SmtpError>(())
    }
    .await;

    match result {
        Ok(()) => {
            crate::log!("smtp-cabi: send ok to={}\n", to.as_str());
            0
        }
        Err(err) => {
            crate::log!("smtp-cabi: send failed to={} err={:?}\n", to.as_str(), err);
            smtp_error_to_code(err)
        }
    }
}

fn spawn_send_text(op_id: u32, to: String, subject: String, body: String, timeout_ms: u32) {
    crate::wait::spawn_local_detached(async move {
        let rc = send_text_mail(to, subject, body, timeout_ms).await;
        if let Some(slot) = CABI_SMTP_RESULTS.lock().get_mut(&op_id) {
            *slot = Some(rc);
        }
    });
}

unsafe fn abi_str<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if ptr.is_null() || len == 0 {
        return None;
    }
    core::str::from_utf8(unsafe { core::slice::from_raw_parts(ptr, len) }).ok()
}

unsafe fn optional_abi_str<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if ptr.is_null() || len == 0 {
        None
    } else {
        unsafe { abi_str(ptr, len) }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_smtp_configure_account(
    user_ptr: *const u8,
    user_len: usize,
    pass_ptr: *const u8,
    pass_len: usize,
    from_ptr: *const u8,
    from_len: usize,
) -> i32 {
    let Some(pass) = (unsafe { abi_str(pass_ptr, pass_len) }) else {
        return FS_ERR_BAD_PARAM;
    };
    let user = unsafe { optional_abi_str(user_ptr, user_len) }
        .unwrap_or(crate::allports::mail::ACCOUNT_EMAIL);
    let from = unsafe { optional_abi_str(from_ptr, from_len) };
    match crate::r::net::mail_config::set_runtime_override(user, pass, from) {
        Ok(()) => 0,
        Err(_) => FS_ERR_BAD_PARAM,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_smtp_password_configured() -> i32 {
    if crate::r::net::mail_config::runtime_password_configured() {
        1
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_smtp_send_text_start(
    to_ptr: *const u8,
    to_len: usize,
    subject_ptr: *const u8,
    subject_len: usize,
    body_ptr: *const u8,
    body_len: usize,
    timeout_ms: u32,
) -> u32 {
    let Some(to) = (unsafe { abi_str(to_ptr, to_len) }) else {
        return 0;
    };
    let Some(subject) = (unsafe { abi_str(subject_ptr, subject_len) }) else {
        return 0;
    };
    let Some(body) = (unsafe { abi_str(body_ptr, body_len) }) else {
        return 0;
    };
    if body.len() > SMTP_BODY_MAX {
        return 0;
    }

    let op_id = CABI_SMTP_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_SMTP_RESULTS.lock().insert(op_id, None);
    spawn_send_text(
        op_id,
        String::from(to),
        String::from(subject),
        String::from(body),
        timeout_ms.max(SMTP_DEFAULT_TIMEOUT_MS),
    );
    op_id
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_smtp_result(op_id: u32) -> i32 {
    match CABI_SMTP_RESULTS.lock().get(&op_id) {
        Some(Some(rc)) => *rc,
        Some(None) | None => FS_ERR_NOT_FOUND,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_smtp_discard(op_id: u32) -> i32 {
    CABI_SMTP_RESULTS.lock().remove(&op_id);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_smtp_wait(op_id: u32, timeout_ms: u64) -> i32 {
    if op_id == 0 {
        return FS_ERR_BAD_PARAM;
    }
    let start = monotonic_ms();
    loop {
        let rc = trueos_cabi_smtp_result(op_id);
        if rc != FS_ERR_NOT_FOUND {
            return rc;
        }
        if timeout_ms == 0 || monotonic_ms().saturating_sub(start) >= timeout_ms {
            return FS_ERR_TIMEOUT;
        }
        crate::wait::spin_step();
    }
}
