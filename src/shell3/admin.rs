//! Persistent account and disk menus over the existing OS services.
use super::{
    helper::{self, Action, Input, Screen},
    tui::{self, Frontend},
};
use crate::disc::block::DeviceHandle;
use crate::{
    crypt,
    shell2::{
        self, MatrixTarget,
        cmds::{cry, disc, format as disk_format, tlb_helper},
    },
};
use alloc::{
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use spin::Mutex;
use trueos_time::{Duration, Timer};
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Cry,
    Disc,
}
impl Kind {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "cry" => Some(Self::Cry),
            "disc" => Some(Self::Disc),
            _ => None,
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Cry => "CRY  account & keys",
            Self::Disc => "DISC  disk devices",
        }
    }
}
pub(super) fn recognizes(name: &str) -> bool {
    Kind::parse(name).is_some()
}
pub(super) fn start(name: &str, frontend: Frontend) -> Result<(), String> {
    let kind = Kind::parse(name).ok_or("Unknown admin helper.")?;
    let Some((target, spawner)) = helper::admit(name, frontend)? else {
        return Ok(());
    };
    let token = match menu_task(kind, target.clone()) {
        Ok(token) => token,
        Err(_) => {
            tui::cancel_native_attach(&target);
            return Err("Admin menu task pool is full.".into());
        }
    };
    spawner.spawn(token);
    Ok(())
}

enum Field {
    SetupUsername,
    UnlockUsername,
    UnlockKey(String),
    LoginCode,
    SshPublicKey,
    SshCode { key: [u8; 32], remove: bool },
    RamSize,
    FormatSure(DeviceHandle),
}
impl Field {
    fn label(&self) -> &'static str {
        match self {
            Self::SetupUsername | Self::UnlockUsername => "Username",
            Self::UnlockKey(_) => "Recovery key",
            Self::LoginCode | Self::SshCode { .. } => "Authenticator code",
            Self::SshPublicKey => "OpenSSH public key",
            Self::RamSize => "Size (e.g. 512MiB)",
            Self::FormatSure(_) => "Type sure to erase this disk",
        }
    }
    fn limit(&self) -> usize {
        match self {
            Self::SetupUsername | Self::UnlockUsername => 32,
            Self::UnlockKey(_) => 64,
            Self::LoginCode | Self::SshCode { .. } => 6,
            Self::SshPublicKey => 512,
            Self::RamSize => 24,
            Self::FormatSure(_) => 4,
        }
    }
    fn secret(&self) -> bool {
        matches!(self, Self::UnlockKey(_) | Self::LoginCode | Self::SshCode { .. })
    }
}
struct Form {
    value: Zeroizing<String>,
    input: Input,
    after_cr: bool,
}
impl Default for Form {
    fn default() -> Self {
        Self {
            value: Zeroizing::new(String::new()),
            input: Input::default(),
            after_cr: false,
        }
    }
}
impl Form {
    fn feed(&mut self, bytes: &[u8], limit: usize, now: u64) -> Option<Action> {
        for &byte in bytes {
            if byte == 27 || self.input.in_sequence() {
                for action in self.input.feed(&[byte], now) {
                    if matches!(action, Action::Quit | Action::Click(_)) {
                        return Some(action);
                    }
                }
                continue;
            }
            if byte == b'\n' && self.after_cr {
                self.after_cr = false;
                continue;
            }
            self.after_cr = byte == b'\r';
            match byte {
                b'\r' | b'\n' => return Some(Action::Choose),
                3 => return Some(Action::Quit),
                8 | 127 => {
                    self.value.pop();
                }
                32..=126 if self.value.len() < limit => self.value.push(byte as char),
                _ => {}
            }
        }
        self.input.timeout(now)
    }
}
enum Page {
    Home,
    Disks {
        format: bool,
        disks: Vec<tlb_helper::DiskChoice>,
    },
    Disk(DeviceHandle),
    RamSizes,
    Field(Field),
    Details(Vec<String>),
    SshKeys(Vec<[u8; 32]>),
    Enrollment(Zeroizing<Vec<String>>),
    Recovery(Zeroizing<String>),
    SavedRecovery {
        username: String,
        key: Zeroizing<String>,
    },
}
impl Page {
    fn sensitive(&self) -> bool {
        matches!(
            self,
            Self::Enrollment(_) | Self::Recovery(_) | Self::SavedRecovery { .. } | Self::Field(_)
        )
    }
}
struct Outcome {
    message: String,
    qr: Option<Zeroizing<Vec<String>>>,
    draft: Option<Arc<cry::AccountEnrollment>>,
    recovery: Option<(String, Zeroizing<String>)>,
}
impl Outcome {
    fn message(message: String) -> Self {
        Self {
            message,
            qr: None,
            draft: None,
            recovery: None,
        }
    }
}
struct Job {
    progress: String,
    result: Option<Result<Outcome, String>>,
}
type Reply = Arc<Mutex<Job>>;
enum Request {
    Setup(String),
    Enrollment,
    SaveAccount {
        draft: Arc<cry::AccountEnrollment>,
        code: Zeroizing<String>,
    },
    Login {
        scope: u8,
        code: Zeroizing<String>,
    },
    Unlock {
        username: String,
        key: Zeroizing<String>,
    },
    Ssh {
        code: Zeroizing<String>,
        key: [u8; 32],
        remove: bool,
    },
    Ramdisc(u64),
    Format(DeviceHandle),
}
impl Request {
    async fn run(self, reply: &Reply) -> Result<Outcome, String> {
        let enrollment = || {
            let enrollment = crypt::begin_totp_enrollment().map_err(cry::error_text)?;
            let qr = cry::enrollment_qr_lines(&enrollment.qr_payload)?;
            Ok(Outcome {
                message: "Scan the QR, then enter an authenticator code.".into(),
                qr: Some(qr),
                draft: None,
                recovery: None,
            })
        };
        match self {
            Self::Setup(username) => {
                if crypt::status().username.as_deref() == Some(username.as_str()) {
                    return Ok(Outcome::message(format!(
                        "Account {username} already exists. Choose Sign in."
                    )));
                }
                if !crypt::status().configured {
                    cry::ensure_account_name_available(&username).await?;
                    match crypt::setup_root_key(&username) {
                        Ok(_) => return enrollment(),
                        Err(crypt::CryError::AlreadyConfigured) => {}
                        Err(error) => return Err(cry::error_text(error)),
                    }
                }
                let draft = cry::create_account_enrollment(&username).await?;
                let qr = draft.qr_lines()?;
                Ok(Outcome {
                    message: format!("Enroll {username}; the automatic account stays active."),
                    qr: Some(qr),
                    draft: Some(draft),
                    recovery: None,
                })
            }
            Self::SaveAccount { draft, code } => match draft.save(&code).await {
                Ok(key) => Ok(Outcome {
                    message: format!(
                        "Saved account {}. Automatic account unchanged.",
                        draft.username
                    ),
                    qr: None,
                    draft: None,
                    recovery: Some((draft.username.clone(), key)),
                }),
                Err(error) => {
                    let qr = draft.qr_lines()?;
                    Ok(Outcome {
                        message: super::capture::error_result(&error),
                        qr: Some(qr),
                        draft: Some(draft),
                        recovery: None,
                    })
                }
            },
            Self::Enrollment => enrollment(),
            Self::Login { scope, code } => cry::login_for_scope(scope, &code)
                .await
                .map(Outcome::message),
            Self::Unlock { username, key } => cry::unlock_account(&username, &key)
                .await
                .map(Outcome::message),
            Self::Ssh { code, key, remove } => cry::change_ssh_key(&code, key, remove)
                .await
                .map(Outcome::message),
            Self::Ramdisc(bytes) => {
                let disk = disc::create_ramdisc_bytes(bytes).await?;
                Ok(Outcome::message(format!("RAM disk disc{} ready and mounted.", disk.id().raw())))
            }
            Self::Format(disk) => {
                disk_format::format_disk(disk, |line| reply.lock().progress = line.into())
                    .await
                    .map(Outcome::message)
            }
        }
    }
}
struct View {
    kind: Kind,
    page: Page,
    selected: usize,
    input: Input,
    form: Form,
    pending: Option<Reply>,
    draft: Option<Arc<cry::AccountEnrollment>>,
    result: String,
    screen: Screen,
    geometry: (usize, usize),
    qr_painted: bool,
    skip_lf: bool,
}
impl View {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            page: Page::Home,
            selected: 0,
            input: Input::default(),
            form: Form::default(),
            pending: None,
            draft: None,
            result: String::new(),
            screen: Screen::default(),
            geometry: (0, 0),
            qr_painted: false,
            skip_lf: false,
        }
    }
    fn set_page(&mut self, page: Page) {
        self.page = page;
        self.selected = 0;
        self.form = Form::default();
        self.input = Input::default();
        self.screen.invalidate();
        self.qr_painted = false;
    }
    fn problem(&mut self, message: &str) {
        self.result = super::capture::error_result(message);
    }
    fn scope(target: &MatrixTarget) -> u8 {
        tui::native_transport_scope(target).unwrap_or(shell2::TRANSPORT_LOCAL_SCOPE)
    }
    fn submit(&mut self, target: &MatrixTarget, request: Request) {
        if self.pending.is_some() {
            return;
        }
        let reply = Arc::new(Mutex::new(Job {
            progress: "Working…".into(),
            result: None,
        }));
        self.pending = Some(reply.clone());
        self.set_page(Page::Home);
        let work = helper::Work::new(target);
        let target = target.clone();
        crate::wait::spawn_local_detached(async move {
            let _work = work;
            Timer::after(Duration::from_millis(1)).await;
            let lease = shell2::matrix_target_slot_lease(&target);
            let result = if shell2::matrix_slot_is_live(&lease) {
                request.run(&reply).await
            } else {
                Err("Slot closed before work started.".into())
            };
            reply.lock().result = Some(result);
            super::service::notify_work();
        });
    }
    fn complete(&mut self) {
        let Some(reply) = &self.pending else {
            return;
        };
        let Some(result) = reply.lock().result.take() else {
            return;
        };
        self.pending = None;
        match result {
            Ok(outcome) => {
                self.result = outcome.message;
                if let Some(draft) = outcome.draft {
                    self.draft = Some(draft);
                }
                if let Some((username, key)) = outcome.recovery {
                    self.draft = None;
                    self.set_page(Page::SavedRecovery { username, key });
                } else if let Some(qr) = outcome.qr {
                    self.set_page(Page::Enrollment(qr));
                } else {
                    self.set_page(Page::Home);
                }
            }
            Err(error) => {
                self.problem(&error);
                self.set_page(Page::Home);
            }
        }
    }
    fn labels(&self) -> Vec<String> {
        if self.pending.is_some() {
            return alloc::vec!["Return".into()];
        }
        let labels: &[&str] = match &self.page {
            Page::Home if self.kind == Kind::Cry => &[
                "Account status",
                "Create account",
                "Set up 2FA",
                "Sign in",
                "Unlock account",
                "Show recovery key",
                "Manage SSH keys",
                "Sign out",
                "Return",
            ],
            Page::Home => &["List disks", "Format a disk", "Create a RAM disk", "Return"],
            Page::Disk(_) => &["Format this disk", "Back", "Return"],
            Page::RamSizes => &[
                "128 MiB",
                "512 MiB",
                "1 GiB",
                "Custom size",
                "Back",
                "Return",
            ],
            Page::Details(_) | Page::Recovery(_) | Page::SavedRecovery { .. } => {
                &["Back", "Return"]
            }
            Page::SshKeys(_) => &["Add an SSH key", "Back", "Return"],
            Page::Field(_) | Page::Enrollment(_) => &[],
            Page::Disks { disks, .. } => {
                let mut labels: Vec<String> = disks
                    .iter()
                    .map(|disk| {
                        format!(
                            "disc{}  {}  {}  {}",
                            disk.raw_id(),
                            disk.label_text(),
                            disk.size_text(),
                            disk.mode_text()
                        )
                    })
                    .collect();
                labels.extend(["Refresh".into(), "Back".into(), "Return".into()]);
                return labels;
            }
        };
        let mut result: Vec<String> = labels.iter().map(|label| String::from(*label)).collect();
        if matches!(self.page, Page::Home) && self.kind == Kind::Cry && self.draft.is_some() {
            result[1] = "Resume account creation".into();
            result.insert(result.len() - 1, "Close unfinished enrollment".into());
        }
        if let Page::SshKeys(keys) = &self.page {
            for (index, key) in keys.iter().enumerate() {
                result.insert(index, format!("Remove {}", crypt::ssh_key_fingerprint(key)));
            }
        }
        result
    }
    fn account_lines(scope: u8) -> Vec<String> {
        let state = crypt::status();
        alloc::vec![
            format!(
                "Automatic account: {} · 2FA: {:?}",
                state.username.as_deref().unwrap_or("not set up"),
                state.two_factor
            ),
            format!(
                "Session: {} · Clock: {}",
                if state
                    .session
                    .is_some_and(|session| session.scope_id == scope)
                {
                    "signed in"
                } else {
                    "signed out"
                },
                if state.totp_clock.is_some() {
                    "synchronized"
                } else {
                    "waiting for network time"
                }
            ),
            format!("Persistence: {:?}", state.persistence)
        ]
    }
    fn body(&self, target: &MatrixTarget) -> Vec<String> {
        match &self.page {
            Page::Home if self.kind == Kind::Cry => Self::account_lines(Self::scope(target)),
            Page::Disks { format, disks } => alloc::vec![
                if *format {
                    "Choose a disk to format.".into()
                } else {
                    "Choose a disk to inspect.".into()
                },
                format!("{} disk(s)", disks.len())
            ],
            Page::Disk(disk) | Page::Field(Field::FormatSure(disk)) => {
                let info = disk.info();
                alloc::vec![
                    format!(
                        "disc{} · {} · {}",
                        info.id.raw(),
                        info.label.as_deref().unwrap_or("-"),
                        if info.writable {
                            "writable"
                        } else {
                            "read-only"
                        }
                    ),
                    format!("{} blocks × {} bytes", info.block_count, info.block_size),
                    if matches!(self.page, Page::Field(Field::FormatSure(_))) {
                        "Formatting destroys all data on this disk.".into()
                    } else {
                        "Choose an operation.".into()
                    }
                ]
            }
            Page::RamSizes => alloc::vec!["Choose a size. Creates and mounts TRUEOSFS.".into()],
            Page::Details(lines) => lines.clone(),
            Page::SshKeys(keys) => alloc::vec![
                format!("{} authorized SSH key(s)", keys.len()),
                "Adding or removing a key requires a fresh authenticator code.".into()
            ],
            Page::Recovery(key) => alloc::vec![
                "Keep this recovery key outside TRUEOSFS:".into(),
                key.to_string()
            ],
            Page::SavedRecovery { username, key } => alloc::vec![
                format!("Recovery key for saved account {username}:"),
                key.to_string()
            ],
            Page::Field(Field::SshCode { key, remove }) => alloc::vec![format!(
                "{} SSH key {}",
                if *remove { "Remove" } else { "Add" },
                crypt::ssh_key_fingerprint(key)
            )],
            Page::Field(_) => alloc::vec!["Enter the value below.".into()],
            Page::Enrollment(_) => {
                alloc::vec![if let Some(draft) = &self.draft {
                    if draft.code_verified() {
                        "Code verified. Press Enter to retry saving this account.".into()
                    } else {
                        format!("Scan for {}, then enter its authenticator code.", draft.username)
                    }
                } else {
                    "Scan with your authenticator, then enter its code.".into()
                }]
            }
            _ => alloc::vec!["Choose an operation.".into()],
        }
    }
    fn choose(&mut self, target: &MatrixTarget) {
        let labels = self.labels();
        let Some(label) = labels.get(self.selected) else {
            return;
        };
        if label == "Return" {
            self.leave(target);
            return;
        }
        if self.pending.is_some() {
            return;
        }
        if label == "Back" {
            self.set_page(Page::Home);
            return;
        }
        let scope = Self::scope(target);
        match &self.page {
            Page::Home if self.kind == Kind::Cry => match self.selected {
                0 => self.set_page(Page::Details(Self::account_lines(scope))),
                1 => {
                    if let Some(draft) = &self.draft {
                        match draft.qr_lines() {
                            Ok(qr) => self.set_page(Page::Enrollment(qr)),
                            Err(error) => self.problem(&error),
                        }
                    } else {
                        self.set_page(Page::Field(Field::SetupUsername));
                    }
                }
                2 => {
                    if self.draft.is_some() {
                        self.result = "Resume or close the unfinished enrollment first.".into();
                    } else if crypt::status().two_factor == crypt::CryTwoFactorState::Active {
                        self.result = "2FA is already active. Choose Sign in.".into();
                    } else {
                        self.submit(target, Request::Enrollment);
                    }
                }
                3 => self.set_page(Page::Field(Field::LoginCode)),
                4 => self.set_page(Page::Field(Field::UnlockUsername)),
                5 => {
                    if let Some(key) = crypt::authenticated_recovery_key(scope) {
                        self.set_page(Page::Recovery(Zeroizing::new(cry::full_hex(&*key))));
                    } else {
                        self.problem("A verified 2FA session is required.");
                    }
                }
                6 => self.set_page(Page::SshKeys(crypt::ssh_authorized_keys())),
                7 => {
                    crypt::logout(scope);
                    self.result = "Signed out.".into();
                }
                8 if self.draft.is_some() => {
                    self.draft = None;
                    self.result = "Enrollment closed.".into();
                    self.selected = 0;
                }
                _ => {}
            },
            Page::Home => match self.selected {
                0 | 1 => self.set_page(Page::Disks {
                    format: self.selected == 1,
                    disks: tlb_helper::collect_top_level_disk_choices(),
                }),
                2 => self.set_page(Page::RamSizes),
                _ => {}
            },
            Page::Disks { format, disks } => {
                if let Some(disk) = disks.get(self.selected) {
                    let disk = disk.handle;
                    if *format {
                        self.format_form(disk);
                    } else {
                        self.set_page(Page::Disk(disk));
                    }
                } else if self.selected == disks.len() {
                    let format = *format;
                    self.set_page(Page::Disks {
                        format,
                        disks: tlb_helper::collect_top_level_disk_choices(),
                    });
                }
            }
            Page::Disk(disk) => self.format_form(*disk),
            Page::RamSizes => match self.selected {
                0 => self.submit(target, Request::Ramdisc(128 * 1024 * 1024)),
                1 => self.submit(target, Request::Ramdisc(512 * 1024 * 1024)),
                2 => self.submit(target, Request::Ramdisc(1024 * 1024 * 1024)),
                3 => self.set_page(Page::Field(Field::RamSize)),
                _ => {}
            },
            Page::SshKeys(keys) => {
                if let Some(key) = keys.get(self.selected) {
                    self.set_page(Page::Field(Field::SshCode {
                        key: *key,
                        remove: true,
                    }));
                } else {
                    self.set_page(Page::Field(Field::SshPublicKey));
                }
            }
            _ => {}
        }
    }
    fn format_form(&mut self, disk: DeviceHandle) {
        if disk.info().writable {
            self.set_page(Page::Field(Field::FormatSure(disk)));
        } else {
            self.problem("Selected disk is read-only.");
        }
    }
    fn submit_form(&mut self, target: &MatrixTarget) {
        let value = core::mem::replace(&mut self.form.value, Zeroizing::new(String::new()));
        let page = core::mem::replace(&mut self.page, Page::Home);
        let scope = Self::scope(target);
        match page {
            Page::Field(Field::SetupUsername) => match crypt::canonical_username(&value) {
                Ok(name) => self.submit(target, Request::Setup(name)),
                Err(error) => {
                    self.problem(&cry::error_text(error));
                    self.set_page(Page::Field(Field::SetupUsername));
                }
            },
            Page::Field(Field::UnlockUsername) => match crypt::canonical_username(&value) {
                Ok(name) => self.set_page(Page::Field(Field::UnlockKey(name))),
                Err(error) => {
                    self.problem(&cry::error_text(error));
                    self.set_page(Page::Field(Field::UnlockUsername));
                }
            },
            Page::Field(Field::UnlockKey(username)) => {
                if cry::parse_recovery_key(&value).is_some() {
                    self.submit(
                        target,
                        Request::Unlock {
                            username,
                            key: value,
                        },
                    );
                } else {
                    self.problem("Recovery key must be 64 hexadecimal digits.");
                    self.set_page(Page::Field(Field::UnlockKey(username)));
                }
            }
            Page::Enrollment(qr) => {
                let valid = value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_digit());
                if let Some(draft) = self.draft.clone() {
                    if valid || draft.code_verified() {
                        self.submit(target, Request::SaveAccount { draft, code: value });
                    } else {
                        self.problem("Enter a 6-digit authenticator code.");
                        self.set_page(Page::Enrollment(qr));
                    }
                } else if valid {
                    self.submit(target, Request::Login { scope, code: value });
                } else {
                    self.problem("Enter a 6-digit authenticator code.");
                    self.set_page(Page::Enrollment(qr));
                }
            }
            Page::Field(Field::LoginCode) => {
                if value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_digit()) {
                    self.submit(target, Request::Login { scope, code: value });
                } else {
                    self.problem("Enter a 6-digit authenticator code.");
                    self.set_page(Page::Field(Field::LoginCode));
                }
            }
            Page::Field(Field::SshPublicKey) => match crypt::parse_ssh_public_key(&value) {
                Ok(key) => self.set_page(Page::Field(Field::SshCode { key, remove: false })),
                Err(error) => {
                    self.problem(&cry::error_text(error));
                    self.set_page(Page::Field(Field::SshPublicKey));
                }
            },
            Page::Field(Field::SshCode { key, remove }) => {
                if value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_digit()) {
                    self.submit(
                        target,
                        Request::Ssh {
                            code: value,
                            key,
                            remove,
                        },
                    );
                } else {
                    self.problem("Enter a 6-digit authenticator code.");
                    self.set_page(Page::Field(Field::SshCode { key, remove }));
                }
            }
            Page::Field(Field::RamSize) => {
                if let Some(bytes) = disc::parse_size_bytes(&value).filter(|bytes| *bytes != 0) {
                    self.submit(target, Request::Ramdisc(bytes));
                } else {
                    self.problem("Invalid RAM disk size.");
                    self.set_page(Page::Field(Field::RamSize));
                }
            }
            Page::Field(Field::FormatSure(disk)) => {
                if value.eq_ignore_ascii_case("sure") {
                    self.submit(target, Request::Format(disk));
                } else {
                    self.result = "Format cancelled.".into();
                    self.set_page(Page::Disk(disk));
                }
            }
            _ => self.set_page(Page::Home),
        }
    }
    fn leave(&mut self, target: &MatrixTarget) {
        if self.page.sensitive() {
            self.set_page(Page::Home);
            self.screen
                .paint(target, &[], self.geometry.0, self.geometry.1);
        }
        tui::native_return(target);
    }
    fn input(&mut self, target: &MatrixTarget, mut bytes: &[u8], now: u64, rows: usize) {
        if self.skip_lf && !bytes.is_empty() {
            if bytes[0] == b'\n' {
                bytes = &bytes[1..];
            }
            self.skip_lf = false;
        }
        if bytes.last() == Some(&b'\r') {
            self.skip_lf = true;
        }
        let form = match &self.page {
            Page::Field(field) => Some(field.limit()),
            Page::Enrollment(_) => Some(6),
            _ => None,
        };
        if let Some(limit) = form {
            match self.form.feed(bytes, limit, now) {
                Some(Action::Choose) => self.submit_form(target),
                Some(Action::Quit) => self.leave(target),
                Some(Action::Click(row)) if row == rows.saturating_sub(1) => self.leave(target),
                _ => {}
            };
            return;
        }
        let mut actions = self.input.feed(bytes, now);
        if let Some(action) = self.input.timeout(now) {
            actions.push(action);
        }
        for action in actions {
            let labels = self.labels();
            match action {
                Action::Up if !labels.is_empty() => {
                    self.selected = (self.selected + labels.len() - 1) % labels.len()
                }
                Action::Down if !labels.is_empty() => {
                    self.selected = (self.selected + 1) % labels.len()
                }
                Action::Choose => self.choose(target),
                Action::Quit => {
                    self.leave(target);
                    break;
                }
                Action::Click(row) if row == rows.saturating_sub(1) => {
                    self.leave(target);
                    break;
                }
                Action::Click(row) => {
                    let start = self.menu_start(rows);
                    let visible = rows.saturating_sub(start + 3);
                    let offset = self.selected.saturating_sub(visible.saturating_sub(1));
                    if row >= start && row < start + visible && row - start + offset < labels.len()
                    {
                        self.selected = row - start + offset;
                        self.choose(target);
                    }
                }
                _ => {}
            }
            if !tui::native_visible(target) {
                break;
            }
        }
    }
    fn menu_start(&self, rows: usize) -> usize {
        let count = match &self.page {
            Page::Home if self.kind == Kind::Cry => 3,
            Page::Disks { .. }
            | Page::Recovery(_)
            | Page::SavedRecovery { .. }
            | Page::SshKeys(_) => 2,
            Page::Disk(_) | Page::Field(Field::FormatSure(_)) => 3,
            Page::Details(lines) => lines.len().min(4),
            _ => 1,
        };
        (2 + count).min(rows.saturating_sub(4))
    }
    fn paint(&mut self, target: &MatrixTarget, cols: usize, rows: usize, now: u64) {
        if rows < 5 || cols == 0 {
            return;
        }
        if self.geometry != (cols, rows) {
            self.geometry = (cols, rows);
            self.qr_painted = false;
        }
        if matches!(self.page, Page::Enrollment(_))
            && self.draft.is_none()
            && crypt::status().two_factor != crypt::CryTwoFactorState::Pending
        {
            self.set_page(Page::Home);
        }
        if matches!(self.page, Page::Recovery(_))
            && !crypt::has_authenticated_two_factor_session(Self::scope(target))
        {
            self.set_page(Page::Home);
            self.problem("The 2FA session ended.");
        }
        let mut lines = Zeroizing::new(alloc::vec![String::new();rows]);
        lines[0] = self.kind.title().into();
        let body = Zeroizing::new(self.body(target));
        for (index, line) in body.iter().take(4).enumerate() {
            if index + 2 < rows - 3 {
                lines[index + 2] = line.clone();
            }
        }
        let labels = self.labels();
        self.selected = self.selected.min(labels.len().saturating_sub(1));
        let start = self.menu_start(rows);
        let visible = rows.saturating_sub(start + 3);
        let offset = self.selected.saturating_sub(visible.saturating_sub(1));
        for (index, label) in labels.iter().skip(offset).take(visible).enumerate() {
            lines[start + index] = format!(
                "  {} {}",
                if offset + index == self.selected {
                    "›"
                } else {
                    " "
                },
                label
            );
        }
        let field = match &self.page {
            Page::Field(field) => Some((field.label(), field.secret())),
            Page::Enrollment(_) => Some(("Authenticator code", true)),
            _ => None,
        };
        if let Some((label, secret)) = field {
            lines[rows - 3] = format!(
                "{label}: {}",
                if secret {
                    "•".repeat(self.form.value.len())
                } else {
                    self.form.value.to_string()
                }
            );
        } else if let Some(reply) = &self.pending {
            lines[rows - 3] = format!("{} {}", helper::spinner(now), reply.lock().progress);
        }
        lines[rows - 2] = self.result.clone();
        lines[rows - 1] = if field.is_some() {
            "Enter submit   Esc Return (input is not recorded)".into()
        } else {
            "↑/↓ j/k select   Enter choose   Esc/q Return   Mouse choose".into()
        };
        let mut qr_fits = false;
        if matches!(self.page, Page::Recovery(_) | Page::SavedRecovery { .. }) && cols < 64 {
            lines[3] = "Resize to 64 columns to show the entire recovery key.".into();
        }
        if let Page::Enrollment(qr) = &self.page {
            let width = qr.first().map_or(0, |line| line.chars().count());
            qr_fits = width <= cols && qr.len() + 6 <= rows;
            if qr_fits {
                for (index, line) in qr.iter().enumerate() {
                    lines[index + 3] = line.clone();
                }
            } else {
                lines[3] = format!(
                    "Resize to at least {width} × {} cells to show the complete QR.",
                    qr.len() + 6
                );
            }
        }
        self.screen.paint(target, &lines, cols, rows);
        if qr_fits && !self.qr_painted {
            if let Page::Enrollment(qr) = &self.page {
                for (index, line) in qr.iter().enumerate() {
                    tui::native_write(
                        target,
                        format!("\x1b[{};1H\x1b[30;47m{}\x1b[0m", index + 4, line).as_bytes(),
                    );
                }
            }
            self.qr_painted = true;
        }
        // Clear temporary copies after painting the terminal surface.
        for line in lines.iter_mut() {
            line.zeroize();
        }
    }
}
#[trueos_executor::task(pool_size = 2)]
async fn menu_task(kind: Kind, target: MatrixTarget) {
    let mut view = View::new(kind);
    while let Some((mut bytes, _)) = tui::native_read(&target) {
        view.complete();
        if !tui::native_visible(&target) {
            if view.page.sensitive() {
                view.set_page(Page::Home);
                let (cols, rows) = view.geometry;
                view.screen.paint(&target, &[], cols, rows);
            }
            bytes.zeroize();
            Timer::after(Duration::from_millis(20)).await;
            continue;
        }
        let Some(surface) = tui::surface(&target) else {
            break;
        };
        let now = crate::chronos::monotonic_nanos();
        view.input(&target, &bytes, now, surface.rows as usize);
        bytes.zeroize();
        if tui::native_visible(&target) {
            view.paint(&target, surface.cols as usize, surface.rows as usize, now);
        }
        Timer::after(Duration::from_millis(20)).await;
    }
}
