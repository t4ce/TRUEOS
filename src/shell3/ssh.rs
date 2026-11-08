//! Kernel SSH transport: public-key login and RFC 4256 authenticator-code recovery.
//! The BSP commits cry's replay counter; APs own the SSH runner and Shell3.
use super::tty::Terminal;
use alloc::{string::String, sync::Arc, vec::Vec};
use zeroize::Zeroizing;
use core::sync::atomic::{AtomicBool, Ordering};
use sunset::{ChanData, ChanHandle, Event, Runner, ServEvent, Server, SignKey};

const INPUT_LIMIT: usize = 64 * 1024;
const AUTH_TIMEOUT_MS: u64 = 60_000;

#[derive(Clone, Copy)]
enum AuthOutcome { Denied, Verified(trueos_crypto::AccountId) }

pub(super) struct AuthCompletion {
    result: spin::Mutex<Option<AuthOutcome>>,
    cancelled: AtomicBool,
}
pub(super) struct AuthRequest {
    username: String,
    code: Zeroizing<String>,
    deadline: trueos_time::Instant,
    completion: Arc<AuthCompletion>,
}
static AUTH_REQUESTS: spin::Once<&'static crate::net::adapter::NetQueue<AuthRequest>> =
    spin::Once::new();

pub(super) fn init() {
    AUTH_REQUESTS.call_once(|| crate::net::adapter::NetQueue::new_leaked("ssh-cry-auth", 64));
}

/// Called by the BSP authentication worker; never block an AP executor on IO.
pub(super) async fn service_auth() {
    let Some(queue) = AUTH_REQUESTS.get() else {
        return;
    };
    for request in queue.drain(1) {
        if request.completion.cancelled.load(Ordering::Acquire)
            || trueos_time::Instant::now() >= request.deadline
        {
            *request.completion.result.lock() = Some(AuthOutcome::Denied);
            continue;
        }
        let result = super::cry::remote_login(&request.username, &request.code).await;
        let outcome = match result {
            Ok(account) if !request.completion.cancelled.load(Ordering::Acquire)
                && trueos_time::Instant::now() < request.deadline => AuthOutcome::Verified(account),
            Ok(_) => AuthOutcome::Denied,
            Err(reason) => {
                // Reason contains a policy/storage category, never the code.
                crate::log_warn!(target: "service"; "shell3-ssh: authenticator rejected reason={}\n", reason);
                AuthOutcome::Denied
            }
        };
        *request.completion.result.lock() = Some(outcome);
    }
}

#[trueos_executor::task]
pub(super) async fn auth_task() {
    loop {
        service_auth().await;
        trueos_time::Timer::after(trueos_time::Duration::from_millis(10)).await;
    }
}

pub(super) struct Session {
    runner: Runner<'static, Server>,
    host_key: SignKey,
    channel: Option<ChanHandle>,
    received: Vec<u8>,
    auth: Option<Arc<AuthCompletion>>,
    deadline: trueos_time::Instant,
    authenticated: bool,
    access_account: Option<trueos_crypto::AccountId>,
    shell_requested: bool,
    size: Option<(usize, usize)>,
    pub finished: bool,
    pub closed: bool,
}

impl Session {
    pub fn new() -> Result<Self, crate::crypt::CryError> {
        let seed = super::ssh_boot::host_seed()?;
        let mut runner = Runner::new_server_owned();
        runner
            .set_auth_methods(false, true)
            .map_err(|_| crate::crypt::CryError::NotConfigured)?;
        runner
            .enable_keyboard_interactive()
            .map_err(|_| crate::crypt::CryError::NotConfigured)?;
        Ok(Self {
            runner,
            host_key: SignKey::Ed25519(ed25519_dalek::SigningKey::from_bytes(&seed)),
            channel: None,
            received: Vec::new(),
            auth: None,
            deadline: trueos_time::Instant::now()
                + trueos_time::Duration::from_millis(AUTH_TIMEOUT_MS),
            authenticated: false,
            access_account: None,
            shell_requested: false,
            size: None,
            closed: false,
            finished: false,
        })
    }

    pub fn input(&mut self, bytes: &[u8]) {
        if self.received.len().saturating_add(bytes.len()) > INPUT_LIMIT {
            self.closed = true;
        } else {
            self.received.extend_from_slice(bytes);
        }
    }

    /// A verified enrolled SSH key may establish an account. Development
    /// none-auth grants terminal access with no account identity.
    pub fn access_account(&self) -> Option<trueos_crypto::AccountId> {
        self.authenticated.then_some(self.access_account).flatten()
    }

    pub fn wants_shell(&self) -> bool {
        self.shell_requested && !self.closed
    }

    pub fn size(&self) -> Option<(usize, usize)> {
        self.size
    }

    pub fn pump(&mut self, terminal: Option<&mut Terminal>) {
        if self.closed {
            return;
        }
        if !self.shell_requested && trueos_time::Instant::now() >= self.deadline {
            self.closed = true;
            if let Some(completion) = &self.auth {
                completion.cancelled.store(true, Ordering::Release);
            }
            return;
        }
        if self.drive(terminal).is_err() {
            self.closed = true;
        }
    }

    fn drive(&mut self, mut terminal: Option<&mut Terminal>) -> sunset::Result<()> {
        if let Some(completion) = self.auth.as_ref() {
            let result = completion.result.lock().take();
            let Some(outcome) = result else {
                return Ok(());
            };
            self.auth = None;
            self.access_account = match outcome {
                AuthOutcome::Denied => None,
                AuthOutcome::Verified(account) => Some(account),
            };
            self.runner.finish_deferred_auth(self.access_account.is_some())?;
        }
        // Bound per-poll CPU work; unread input and output stay queued.
        for _ in 0..64 {
            let count = self.runner.input(&self.received)?;
            self.received.drain(..count);
            let mut progressed = count != 0;
            match self.runner.progress()? {
                Event::None => {}
                Event::Progressed => progressed = true,
                Event::Serv(event) => {
                    progressed = true;
                    match event {
                        ServEvent::Hostkeys(h) => h.hostkeys(&[&self.host_key])?,
                        ServEvent::FirstAuth(mut h) => {
                            self.access_account = None;
                            if super::ssh_boot::allow_none()
                                && !crate::crypt::ssh_requires_account_proof(h.username()?) {
                                h.allow()?;
                            } else {
                                h.set_auth_methods(false, true)?;
                                h.reject()?;
                            }
                        }
                        // Sunset's response event also carries the RFC 4256 code.
                        // Actual password authentication is disabled above.
                        ServEvent::PasswordAuth(h) => {
                            self.access_account = None;
                            let username = h.username()?;
                            let code = h.password()?;
                            if crate::crypt::canonical_username(username).as_deref() != Ok(username)
                                || code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit())
                            {
                                h.reject()?;
                                continue;
                            }
                            let username = String::from(username);
                            let code = Zeroizing::new(String::from(code));
                            let completion = Arc::new(AuthCompletion {
                                result: spin::Mutex::new(None),
                                cancelled: AtomicBool::new(false),
                            });
                            let request = AuthRequest {
                                username,
                                code,
                                deadline: self.deadline,
                                completion: completion.clone(),
                            };
                            if AUTH_REQUESTS
                                .get()
                                .is_some_and(|queue| queue.push(request).is_ok())
                            {
                                h.defer();
                                self.auth = Some(completion);
                                return Ok(());
                            }
                            h.reject()?;
                        }
                        ServEvent::PubkeyAuth(h) => {
                            let account = match h.pubkey()? {
                                sunset::PubKey::Ed25519(key) => {
                                    crate::crypt::ssh_key_account(h.username()?, &key.key.0)
                                }
                                _ => None,
                            };
                            if let Some(account) = account {
                                let verified = h.signature_verified();
                                h.allow()?;
                                // A public-key probe can receive PK_OK but
                                // cannot bind an account without its signature.
                                self.access_account = verified.then_some(account);
                            } else {
                                self.access_account = None;
                                h.reject()?;
                            }
                        }
                        ServEvent::Authenticated => self.authenticated = true,
                        ServEvent::OpenSession(h) => {
                            if self.authenticated && self.channel.is_none() {
                                self.channel = Some(h.accept()?);
                            } else {
                                h.reject(sunset::ChanFail::SSH_OPEN_ADMINISTRATIVELY_PROHIBITED)?;
                            }
                        }
                        ServEvent::SessionPty(h) => {
                            let (columns, rows) = h.dimensions()?;
                            let initial = h.is_initial_request()?;
                            if self
                                .channel
                                .as_ref()
                                .is_some_and(|c| c.num() == h.channel())
                                && (initial || self.size.is_some())
                                && (20..=512).contains(&columns)
                                && (5..=256).contains(&rows)
                            {
                                self.size = Some((columns as usize, rows as usize));
                                if let Some(tty) = terminal.as_deref_mut() {
                                    tty.resize(columns as usize, rows as usize);
                                }
                                h.succeed()?;
                            } else {
                                h.fail()?;
                            }
                        }
                        ServEvent::SessionShell(h) => {
                            if self.authenticated
                                && self.size.is_some()
                                && !self.shell_requested
                                && self
                                    .channel
                                    .as_ref()
                                    .is_some_and(|c| c.num() == h.channel())
                            {
                                h.succeed()?;
                                self.shell_requested = true;
                            } else {
                                h.fail()?;
                            }
                        }
                        ServEvent::SessionExec(h) | ServEvent::SessionSubsystem(h) => h.fail()?,
                        ServEvent::SessionEnv(h) => h.fail()?,
                        ServEvent::Defunct => self.closed = true,
                        ServEvent::PollAgain => {}
                    }
                }
                Event::Cli(_) => return Err(sunset::Error::msg("unexpected SSH client event")),
            }
            if let Some(channel) = self.channel.as_ref() {
                if self.runner.is_channel_closed(channel) {
                    self.closed = true;
                    break;
                }
                if let Some(tty) = terminal.as_deref_mut() {
                    let mut bytes = [0; 1024];
                    let count = if self.runner.is_channel_eof(channel) {
                        if !tty.closing {
                            tty.input_eof();
                        }
                        0
                    } else {
                        self.runner
                            .read_channel(channel, ChanData::Normal, &mut bytes)?
                    };
                    if count != 0 {
                        tty.input(&bytes[..count]);
                        progressed = true;
                    }
                    if !tty.output.is_empty() {
                        let count =
                            self.runner
                                .write_channel(channel, ChanData::Normal, &tty.output)?;
                        tty.output.drain(..count);
                        progressed |= count != 0;
                    }
                }
            }
            if self.closed || !progressed {
                break;
            }
        }
        if !self.finished
            && terminal
                .as_ref()
                .is_some_and(|tty| tty.closing && tty.output.is_empty())
            && self.runner.output_buf().is_empty()
        {
            if let Some(channel) = self.channel.as_ref() {
                self.runner.send_session_exit(channel)?;
                self.finished = true;
            }
        }
        Ok(())
    }

    pub fn output(&mut self) -> &[u8] {
        self.runner.output_buf()
    }
    pub fn consume_output(&mut self, count: usize) {
        self.runner.consume_output(count);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(completion) = &self.auth {
            completion.cancelled.store(true, Ordering::Release);
        }
    }
}
