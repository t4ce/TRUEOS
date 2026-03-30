#![allow(dead_code)]

extern crate alloc;

use alloc::{format, string::String, vec, vec::Vec};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SPop3State {
    Authorization,
    Transaction,
    Update,
}

#[derive(Debug, Clone)]
pub struct SPop3Reply {
    pub ok: bool,
    pub lines: Vec<String>,
}

impl SPop3Reply {
    pub fn ok(line: impl Into<String>) -> Self {
        Self {
            ok: true,
            lines: vec![line.into()],
        }
    }

    pub fn err(line: impl Into<String>) -> Self {
        Self {
            ok: false,
            lines: vec![line.into()],
        }
    }

    pub fn multi_ok(first: impl Into<String>, mut tail: Vec<String>) -> Self {
        let mut lines = vec![first.into()];
        lines.append(&mut tail);
        Self { ok: true, lines }
    }

    pub fn encode(&self) -> String {
        let mut out = String::new();
        let prefix = if self.ok { "+OK " } else { "-ERR " };
        if let Some(first) = self.lines.first() {
            out.push_str(prefix);
            out.push_str(first);
            out.push_str("\r\n");
        }
        if self.ok && self.lines.len() > 1 {
            for line in self.lines.iter().skip(1) {
                if line.starts_with('.') {
                    out.push('.');
                }
                out.push_str(line);
                out.push_str("\r\n");
            }
            out.push_str(".\r\n");
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct SPop3Message {
    pub from: String,
    pub subject: String,
    pub body: String,
    pub deleted: bool,
}

impl SPop3Message {
    pub fn size(&self) -> usize {
        self.wire().len()
    }

    pub fn wire(&self) -> String {
        format!("From: {}\r\nSubject: {}\r\n\r\n{}\r\n", self.from, self.subject, self.body)
    }

    pub fn top(&self, body_lines: u32) -> String {
        let mut out = format!("From: {}\r\nSubject: {}\r\n\r\n", self.from, self.subject);
        for line in self.body.lines().take(body_lines as usize) {
            out.push_str(line);
            out.push_str("\r\n");
        }
        out
    }
}

#[derive(Debug)]
pub struct SPop3Session {
    pub state: SPop3State,
    pub user: Option<String>,
    pub authenticated: bool,
    pub mailbox: Vec<SPop3Message>,
}

impl SPop3Session {
    pub fn new() -> Self {
        Self {
            state: SPop3State::Authorization,
            user: None,
            authenticated: false,
            mailbox: Vec::new(),
        }
    }

    pub fn greeting(&self) -> SPop3Reply {
        SPop3Reply::ok("trueos POP3 ready")
    }

    pub fn on_user(&mut self, user: &str) {
        self.user = Some(String::from(user));
    }

    pub fn on_auth_ok(&mut self) {
        self.authenticated = true;
        self.state = SPop3State::Transaction;
    }

    pub fn push_message(&mut self, from: &str, subject: &str, body: &str) {
        self.mailbox.push(SPop3Message {
            from: String::from(from),
            subject: String::from(subject),
            body: String::from(body),
            deleted: false,
        });
    }

    pub fn user(&mut self, user: &str) -> SPop3Reply {
        self.on_user(user);
        SPop3Reply::ok("User accepted")
    }

    pub fn pass(&mut self, password: &str) -> SPop3Reply {
        if self.user.is_none() {
            return SPop3Reply::err("USER required first");
        }
        if password.is_empty() {
            return SPop3Reply::err("Authentication failed");
        }

        self.on_auth_ok();
        SPop3Reply::ok("Authentication successful")
    }

    pub fn stat(&self) -> SPop3Reply {
        if !self.authenticated {
            return SPop3Reply::err("Not authenticated");
        }

        let count = self.mailbox.iter().filter(|m| !m.deleted).count();
        let bytes: usize = self
            .mailbox
            .iter()
            .filter(|m| !m.deleted)
            .map(SPop3Message::size)
            .sum();
        SPop3Reply::ok(format!("{} {}", count, bytes))
    }

    pub fn list(&self) -> SPop3Reply {
        if !self.authenticated {
            return SPop3Reply::err("Not authenticated");
        }

        let mut items = Vec::new();
        for (idx, msg) in self.mailbox.iter().enumerate() {
            if !msg.deleted {
                items.push(format!("{} {}", idx + 1, msg.size()));
            }
        }
        SPop3Reply::multi_ok("scan listing follows", items)
    }

    pub fn retr(&self, msg_id: u32) -> SPop3Reply {
        if !self.authenticated {
            return SPop3Reply::err("Not authenticated");
        }
        let Some(msg) = self.get_message(msg_id) else {
            return SPop3Reply::err("No such message");
        };

        let mut lines = vec![format!("{} octets", msg.size())];
        lines.extend(
            msg.wire()
                .split("\r\n")
                .filter(|l| !l.is_empty())
                .map(String::from),
        );
        SPop3Reply::multi_ok(lines.remove(0), lines)
    }

    pub fn top(&self, msg_id: u32, body_lines: u32) -> SPop3Reply {
        if !self.authenticated {
            return SPop3Reply::err("Not authenticated");
        }
        let Some(msg) = self.get_message(msg_id) else {
            return SPop3Reply::err("No such message");
        };

        let top = msg.top(body_lines);
        let mut lines = vec![String::from("top of message follows")];
        lines.extend(
            top.split("\r\n")
                .filter(|l| !l.is_empty())
                .map(String::from),
        );
        SPop3Reply::multi_ok(lines.remove(0), lines)
    }

    pub fn dele(&mut self, msg_id: u32) -> SPop3Reply {
        if !self.authenticated {
            return SPop3Reply::err("Not authenticated");
        }
        let Some(msg) = self.get_message_mut(msg_id) else {
            return SPop3Reply::err("No such message");
        };

        msg.deleted = true;
        SPop3Reply::ok("Message marked for deletion")
    }

    pub fn noop(&self) -> SPop3Reply {
        SPop3Reply::ok("Ok")
    }

    pub fn quit(&mut self) -> SPop3Reply {
        self.state = SPop3State::Update;
        SPop3Reply::ok("Bye")
    }

    pub fn handle_line(&mut self, line: &str) -> SPop3Reply {
        let trimmed = line.trim();
        let mut parts = trimmed.split_whitespace();
        let Some(cmd) = parts.next() else {
            return SPop3Reply::err("Empty command");
        };

        if cmd.eq_ignore_ascii_case("USER") {
            if let Some(user) = parts.next() {
                return self.user(user);
            }
            return SPop3Reply::err("Missing user");
        }
        if cmd.eq_ignore_ascii_case("PASS") {
            return self.pass(parts.next().unwrap_or(""));
        }
        if cmd.eq_ignore_ascii_case("STAT") {
            return self.stat();
        }
        if cmd.eq_ignore_ascii_case("LIST") {
            return self.list();
        }
        if cmd.eq_ignore_ascii_case("RETR") {
            let id = parts
                .next()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0);
            return self.retr(id);
        }
        if cmd.eq_ignore_ascii_case("TOP") {
            let id = parts
                .next()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0);
            let body_lines = parts
                .next()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0);
            return self.top(id, body_lines);
        }
        if cmd.eq_ignore_ascii_case("DELE") {
            let id = parts
                .next()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0);
            return self.dele(id);
        }
        if cmd.eq_ignore_ascii_case("NOOP") {
            return self.noop();
        }
        if cmd.eq_ignore_ascii_case("QUIT") {
            return self.quit();
        }

        SPop3Reply::err("Command not implemented")
    }

    fn get_message(&self, msg_id: u32) -> Option<&SPop3Message> {
        let idx = msg_id.checked_sub(1)? as usize;
        let msg = self.mailbox.get(idx)?;
        if msg.deleted { None } else { Some(msg) }
    }

    fn get_message_mut(&mut self, msg_id: u32) -> Option<&mut SPop3Message> {
        let idx = msg_id.checked_sub(1)? as usize;
        let msg = self.mailbox.get_mut(idx)?;
        if msg.deleted { None } else { Some(msg) }
    }
}
