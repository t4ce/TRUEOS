use crate::vcabi;

pub const DEFAULT_TIMEOUT_MS: u32 = 20_000;

pub const ERR_BAD_UTF8: i32 = -1;
pub const ERR_IO: i32 = -2;
pub const ERR_BAD_PARAM: i32 = -4;
pub const ERR_NOT_FOUND: i32 = -8;
pub const ERR_TIMEOUT: i32 = -14;

pub const ERR_DNS: i32 = -111;
pub const ERR_CONNECT: i32 = -112;
pub const ERR_TLS: i32 = -13;
pub const ERR_SMTP: i32 = -12;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct MailOp {
    id: u32,
}

impl MailOp {
    pub const fn from_raw(id: u32) -> Option<Self> {
        if id == 0 { None } else { Some(Self { id }) }
    }

    pub const fn id(self) -> u32 {
        self.id
    }

    pub fn result(self) -> Result<Option<()>, i32> {
        match unsafe { vcabi::trueos_cabi_smtp_result(self.id) } {
            0 => Ok(Some(())),
            ERR_NOT_FOUND => Ok(None),
            rc => Err(rc),
        }
    }

    pub fn wait(self, timeout_ms: u64) -> Result<(), i32> {
        match unsafe { vcabi::trueos_cabi_smtp_wait(self.id, timeout_ms) } {
            0 => Ok(()),
            rc => Err(rc),
        }
    }

    pub fn discard(self) {
        unsafe {
            vcabi::trueos_cabi_smtp_discard(self.id);
        }
    }
}

pub fn send_text_start(to: &str, subject: &str, body: &str) -> Result<MailOp, i32> {
    send_text_start_with_timeout(to, subject, body, DEFAULT_TIMEOUT_MS)
}

pub fn configure_password(password: &str) -> Result<(), i32> {
    configure_account("", password, "")
}

pub fn configure_account(user: &str, password: &str, from: &str) -> Result<(), i32> {
    match unsafe {
        vcabi::trueos_cabi_smtp_configure_account(
            user.as_ptr(),
            user.len(),
            password.as_ptr(),
            password.len(),
            from.as_ptr(),
            from.len(),
        )
    } {
        0 => Ok(()),
        rc => Err(rc),
    }
}

pub fn password_configured() -> bool {
    unsafe { vcabi::trueos_cabi_smtp_password_configured() != 0 }
}

pub fn send_text_start_with_timeout(
    to: &str,
    subject: &str,
    body: &str,
    timeout_ms: u32,
) -> Result<MailOp, i32> {
    let id = unsafe {
        vcabi::trueos_cabi_smtp_send_text_start(
            to.as_ptr(),
            to.len(),
            subject.as_ptr(),
            subject.len(),
            body.as_ptr(),
            body.len(),
            timeout_ms,
        )
    };
    MailOp::from_raw(id).ok_or(ERR_BAD_PARAM)
}

pub fn send_text_blocking(
    to: &str,
    subject: &str,
    body: &str,
    wait_timeout_ms: u64,
) -> Result<(), i32> {
    let op = send_text_start(to, subject, body)?;
    let result = op.wait(wait_timeout_ms);
    op.discard();
    result
}
