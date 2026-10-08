//! Kernel SSH transport: public-key login and RFC 4256 authenticator-code recovery.
//! The BSP commits cry's replay counter; APs own the SSH runner and Shell3.
use super::tty::Terminal;
use alloc::{string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use sunset::{ChanData, ChanHandle, Event, Runner, ServEvent, Server, SignKey};
use zeroize::Zeroizing;

const INPUT_LIMIT: usize = 64 * 1024;
const AUTH_TIMEOUT_MS: u64 = 60_000;

pub(super) struct AuthCompletion {
    result: spin::Mutex<Option<bool>>,
    cancelled: AtomicBool,
}
pub(super) struct AuthRequest {
    username: String,
    code: Zeroizing<String>,
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
    let Some(request) = queue.drain(1).pop() else {
        return;
    };
    if request.completion.cancelled.load(Ordering::Acquire) {
        return;
    }
    let success = match crate::crypt::prepare_remote_login(&request.username, &request.code) {
        Ok(report) => {
            let sequence = report.challenge_sequence;
            let result = match crate::crypt::prepare_persistence(sequence) {
                Ok(plan) => match crate::shell2::cmds::cry::write_persistence(&plan).await {
                    Ok(()) => crate::crypt::complete_persisted_remote_login(plan).is_ok(),
                    Err(_) => false,
                },
                Err(_) => false,
            };
            if !result {
                crate::crypt::abort_pending_login(sequence);
            }
            result
        }
        Err(_) => false,
    };
    *request.completion.result.lock() = Some(success);
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
    shell_requested: bool,
    pub finished: bool,
    pub closed: bool,
}

impl Session {
    pub fn new() -> Result<Self, crate::crypt::CryError> {
        let seed = crate::crypt::ssh_host_seed()?;
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
            shell_requested: false,
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

    pub fn wants_shell(&self) -> bool {
        self.shell_requested && !self.closed
    }

    pub fn pump(&mut self, terminal: Option<&mut Terminal>) {
        if self.closed {
            return;
        }
        if !self.shell_requested && trueos_time::Instant::now() >= self.deadline {
            self.closed = true;
            return;
        }
        if self.drive(terminal).is_err() {
            self.closed = true;
        }
    }

    fn drive(&mut self, mut terminal: Option<&mut Terminal>) -> sunset::Result<()> {
        if let Some(completion) = self.auth.as_ref() {
            let result = completion.result.lock().take();
            let Some(success) = result else {
                return Ok(());
            };
            self.auth = None;
            self.runner.finish_deferred_auth(success)?;
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
                            h.set_auth_methods(false, true)?;
                            h.reject()?;
                        }
                        // Sunset's response event also carries the RFC 4256 code.
                        // Actual password authentication is disabled above.
                        ServEvent::PasswordAuth(h) => {
                            let completion = Arc::new(AuthCompletion {
                                result: spin::Mutex::new(None),
                                cancelled: AtomicBool::new(false),
                            });
                            let request = AuthRequest {
                                username: String::from(h.username()?),
                                code: Zeroizing::new(String::from(h.password()?)),
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
                            let accepted = match h.pubkey()? {
                                sunset::PubKey::Ed25519(key) => {
                                    crate::crypt::ssh_key_allowed(h.username()?, &key.key.0)
                                }
                                _ => false,
                            };
                            if accepted {
                                h.allow()?;
                            } else {
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
                            if self
                                .channel
                                .as_ref()
                                .is_some_and(|c| c.num() == h.channel())
                            {
                                h.succeed()?;
                            } else {
                                h.fail()?;
                            }
                        }
                        ServEvent::SessionShell(h) => {
                            if self.authenticated
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
                            tty.input(b"\x03\x04");
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
