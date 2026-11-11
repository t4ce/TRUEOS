//! Pure command/input model for the native keys table.
use crate::access_key_store::{AdminChange, valid_app_name};
use alloc::{string::String, vec::Vec};
use trueos_crypto::AccountId;
use trueos_fs::RecordKey;

pub(super) fn command(
    text: &str,
    keys: &[RecordKey],
    names: &[(RecordKey, String)],
    user: Option<AccountId>,
) -> Result<Option<AdminChange>, &'static str> {
    let mut words = text.split_whitespace();
    let op = words
        .next()
        .ok_or("Enter grant, revoke, alias or refresh.")?;
    if op == "refresh" && words.next().is_none() {
        return Ok(None);
    }
    if !matches!(op, "grant" | "revoke" | "alias") {
        return Err("Use grant KEY APP [USER], revoke KEY APP [USER], alias KEY NAME or refresh.");
    }
    let selector = words.next().ok_or("Missing key row number or alias.")?;
    let key = selector
        .parse::<usize>()
        .ok()
        .and_then(|n| keys.get(n).copied())
        .or_else(|| {
            names
                .iter()
                .find(|(_, name)| name == selector)
                .map(|(key, _)| *key)
        })
        .ok_or("Unknown key. Use the row number shown in the table.")?;
    if op == "alias" {
        // Preserve spaces in names; row numbers always remain usable selectors.
        let name = words.collect::<Vec<_>>().join(" ");
        if name.is_empty()
            || name.len() > 64
            || name.chars().any(char::is_control)
            || name.parse::<usize>().is_ok()
        {
            return Err("Alias must be 1-64 bytes and cannot be only a number.");
        }
        if names
            .iter()
            .any(|(other, existing)| *other != key && existing == &name)
        {
            return Err("That alias or workspace name already selects another key.");
        }
        return Ok(Some(AdminChange::Alias { key, name }));
    }
    let app = words
        .next()
        .ok_or("Missing canonical app name, e.g. termdir.")?;
    if !valid_app_name(app) {
        return Err("Use the canonical lowercase app name, e.g. termdir (no apps/ prefix).");
    }
    let account = match words.next() {
        Some(raw) => AccountId::new(
            raw.parse()
                .map_err(|_| "USER must be a numeric account ID from the table.")?,
        ),
        None => user.ok_or("No signed-in terminal user. Append a USER ID from the table.")?,
    };
    if words.next().is_some() {
        return Err("Too many arguments; use grant KEY APP [USER].");
    }
    Ok(Some(AdminChange::Grant {
        account,
        app: app.into(),
        key,
        enabled: op == "grant",
    }))
}

#[derive(Debug, PartialEq)]
pub(super) enum Event {
    Submit(String),
    Scroll(isize),
    Quit,
}
#[derive(Default)]
pub(super) struct Input {
    text: Vec<u8>,
    sequence: Vec<u8>,
    escape_at: u64,
    after_cr: bool,
}
impl Input {
    pub(super) fn text(&self) -> String {
        String::from_utf8_lossy(&self.text).into_owned()
    }
    pub(super) fn feed(&mut self, bytes: &[u8], now: u64) -> Vec<Event> {
        let mut events = Vec::new();
        for &byte in bytes {
            if !self.sequence.is_empty() {
                if self.sequence.len() == 1 && !matches!(byte, b'[' | b'O') {
                    self.sequence.clear();
                    events.push(Event::Quit);
                    continue;
                }
                self.sequence.push(byte);
                if self.sequence.len() > 2 && (0x40..=0x7e).contains(&byte) {
                    match self.sequence.as_slice() {
                        b"\x1b[A" | b"\x1bOA" => events.push(Event::Scroll(-1)),
                        b"\x1b[B" | b"\x1bOB" => events.push(Event::Scroll(1)),
                        b"\x1b[5~" => events.push(Event::Scroll(-10)),
                        b"\x1b[6~" => events.push(Event::Scroll(10)),
                        _ => {} // Ignore mouse/focus/other terminal reports.
                    }
                    self.sequence.clear();
                } else if self.sequence.len() > 64 {
                    self.sequence.clear();
                }
                continue;
            }
            if byte == b'\n' && self.after_cr {
                self.after_cr = false;
                continue;
            }
            self.after_cr = byte == b'\r';
            match byte {
                27 => {
                    self.sequence.push(byte);
                    self.escape_at = now;
                }
                3 => events.push(Event::Quit),
                b'\r' | b'\n' => {
                    events.push(Event::Submit(self.text()));
                    self.text.clear();
                }
                8 | 127 => {
                    if let Some(last) = self.text.pop() {
                        if last & 0xc0 == 0x80 {
                            while self.text.last().is_some_and(|b| b & 0xc0 == 0x80) {
                                self.text.pop();
                            }
                            self.text.pop();
                        }
                    }
                }
                32..=255 if self.text.len() < 512 => self.text.push(byte),
                _ => {}
            }
        }
        events
    }
    pub(super) fn timeout(&mut self, now: u64) -> Option<Event> {
        if !self.sequence.is_empty() && now.saturating_sub(self.escape_at) >= 75_000_000 {
            let quit = self.sequence.len() == 1;
            self.sequence.clear();
            return quit.then_some(Event::Quit);
        }
        None
    }
}
