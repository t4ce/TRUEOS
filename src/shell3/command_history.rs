//! Session-local recall, with Cry-gated sharing and encrypted persistence.
use alloc::{string::String, vec::Vec};
use spin::Mutex;
use zeroize::{Zeroize, Zeroizing};

pub(crate) const HISTORY_RECALL_CAP: usize = 10;
struct Entry { account: u64, text: Zeroizing<String>, count: u64 }
struct LiveHistory { entries: Vec<Entry>, generation: u64 }
static LIVE: Mutex<LiveHistory> = Mutex::new(LiveHistory { entries: Vec::new(), generation: 0 });

type SessionIdentity = (u64, u64);

struct HistoryOwner {
    frontend: u64,
    scope: u8,
    identity: SessionIdentity,
}

static OWNER: Mutex<Option<HistoryOwner>> = Mutex::new(None);

pub(super) fn bind_verified_login(frontend: u64, scope: u8, identity: SessionIdentity) {
    let mut owner = OWNER.lock();
    if crate::crypt::authenticated_history_identity(scope) == Some(identity) {
        *owner = Some(HistoryOwner { frontend, scope, identity });
    }
}

fn owned_session(frontend: u64, scope: u8) -> Option<SessionIdentity> {
    let owner = OWNER.lock();
    let owner = owner.as_ref().filter(|owner| owner.frontend == frontend && owner.scope == scope)?;
    crate::crypt::authenticated_history_identity(scope).filter(|identity| *identity == owner.identity)
}

/// Disk entries arrive newest first. Preserve commands entered while the
/// reader was running and prepend only distinct older entries.
pub(crate) fn restore_commands(account: u64, commands: Vec<Zeroizing<String>>) {
    let mut live = LIVE.lock();
    live.entries.retain(|entry| entry.account == account);
    let available = HISTORY_RECALL_CAP.saturating_sub(live.entries.len());
    if available == 0 {
        return;
    }
    let older: Vec<_> = commands.into_iter()
        .filter(|text| !live.entries.iter().any(|entry| entry.account == account && entry.text.as_str() == text.as_str()))
        .take(available)
        .map(|text| Entry { account, text, count: 1 })
        .collect();
    if !older.is_empty() {
        live.entries.splice(..0, older.into_iter().rev());
        live.generation = live.generation.wrapping_add(1);
    }
}

pub(super) struct CommandHistory {
    pub(super) entries: Vec<String>,
    pub(super) cursor: Option<usize>,
    pub(super) draft: String,
    scope_id: u8,
    frontend: Option<u64>,
    session: Option<SessionIdentity>,
    generation: u64,
    recording: bool,
}

impl Default for CommandHistory {
    fn default() -> Self {
        Self { entries: Vec::new(), cursor: None, draft: String::new(), scope_id: super::matrix_target::TRANSPORT_LOCAL_SCOPE,
            frontend: None, session: None, generation: 0, recording: true }
    }
}

// Redact by shape even when a credential is malformed or has extra tokens.
fn redacted(line: &str) -> String {
    let mut words = line.split_whitespace();
    let first = words.next().unwrap_or("");
    if first.trim_start_matches('§').eq_ignore_ascii_case("cry") {
        let action = words.next().unwrap_or("");
        if (action.eq_ignore_ascii_case("login") || action.eq_ignore_ascii_case("unlock")) && words.next().is_some() {
            return alloc::format!("cry {action} ******");
        }
        if action.eq_ignore_ascii_case("ssh") {
            let operation = words.next().unwrap_or("");
            if (operation.eq_ignore_ascii_case("add") || operation.eq_ignore_ascii_case("remove")) && words.next().is_some() {
                return alloc::format!("cry ssh {operation} ******");
            }
        }
    }
    if !first.is_empty() && first.bytes().all(|byte| byte.is_ascii_digit()) { return String::from("******"); }
    String::from(line)
}

impl CommandHistory {
    pub(super) fn for_frontend(frontend: u64) -> Self {
        let mut history = Self::default();
        history.frontend = Some(frontend);
        history
    }

    pub fn set_scope(&mut self, scope_id: u8) {
        self.scope_id = scope_id;
        self.session = None;
        self.wipe();
    }

    // SSH's transport adapter owns its submission boundary, preventing a
    // second encrypted record when it calls the same Shell3 command dispatcher.
    pub fn set_recording(&mut self, recording: bool) { self.recording = recording; }

    fn wipe(&mut self) {
        for entry in &mut self.entries { entry.zeroize(); }
        self.entries.clear();
        self.end_recall();
    }

    pub fn sync(&mut self) -> bool {
        let session = self.frontend.and_then(|frontend| owned_session(frontend, self.scope_id));
        let changed = session != self.session;
        if changed {
            self.wipe();
            self.session = session;
            self.generation = u64::MAX;
        }
        let Some((account, _)) = session else { return changed; };
        let live = LIVE.lock();
        if self.generation != live.generation {
            self.wipe();
            self.entries.extend(live.entries.iter().filter(|entry| entry.account == account).map(|entry| String::from(entry.text.as_str())));
            self.generation = live.generation;
        }
        changed
    }

    pub fn remember(&mut self, line: &str) {
        self.sync();
        if !self.recording || line.trim().is_empty() { return; }
        let text = Zeroizing::new(redacted(line));
        // Development SSH and UI4 need recall even without a Cry login.
        // These entries stay with this frontend and never reach the writer
        // or the account-shared cache. Authentication changes wipe them.
        let Some(identity @ (account, _)) = self.session else {
            if self.entries.iter().any(|entry| entry == text.as_str()) { return; }
            if self.entries.len() == HISTORY_RECALL_CAP {
                let mut oldest = self.entries.remove(0);
                oldest.zeroize();
            }
            self.entries.push(String::from(text.as_str()));
            return;
        };
        crate::user_input_record::capture(self.scope_id, identity, &text);
        {
            let mut live = LIVE.lock();
            if let Some(entry) = live.entries.iter_mut().find(|entry| entry.account == account && entry.text.as_str() == text.as_str()) {
                entry.count = entry.count.saturating_add(1);
                return;
            }
            if live.entries.len() == HISTORY_RECALL_CAP { live.entries.remove(0); }
            live.entries.push(Entry { account, text, count: 1 });
            live.generation = live.generation.wrapping_add(1);
        }
        self.sync();
    }

    pub fn recall(&mut self, up: bool, input: &str) -> Option<String> {
        let changed = self.sync();
        if changed && self.entries.is_empty() {
            return if input.is_empty() { None } else { Some(String::new()) };
        }
        if up && self.cursor.is_none() && !self.draft.is_empty() {
            return Some(core::mem::take(&mut self.draft));
        }
        if self.entries.is_empty() {
            if input.is_empty() { return None; }
            self.draft.zeroize();
            self.draft = String::from(input);
            return Some(String::new());
        }
        if up {
            if self.cursor.is_none() { self.draft.zeroize(); self.draft = String::from(input); }
            let index = self.cursor.map_or(self.entries.len() - 1, |i| i.saturating_sub(1));
            self.cursor = Some(index);
            Some(self.entries[index].clone())
        } else if let Some(index) = self.cursor {
            if index + 1 < self.entries.len() {
                self.cursor = Some(index + 1);
                Some(self.entries[index + 1].clone())
            } else {
                self.cursor = None;
                Some(core::mem::take(&mut self.draft))
            }
        } else if !input.is_empty() {
            self.draft.zeroize();
            self.draft = String::from(input);
            Some(String::new())
        } else { None }
    }

    pub fn end_recall(&mut self) { self.cursor = None; self.draft.zeroize(); }
}

impl Drop for CommandHistory { fn drop(&mut self) { self.wipe(); } }
