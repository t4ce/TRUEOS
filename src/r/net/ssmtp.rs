#![allow(dead_code)]

extern crate alloc;

use alloc::{format, string::String, vec, vec::Vec};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SSmtpState {
    Greeting,
    Auth,
    MailTransaction,
    Data,
    Closing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SSmtpAuthStage {
    None,
    Username,
    Password,
}

#[derive(Debug, Clone)]
pub struct SSmtpReply {
    pub code: u16,
    pub lines: Vec<String>,
}

impl SSmtpReply {
    pub fn single(code: u16, line: impl Into<String>) -> Self {
        Self {
            code,
            lines: vec![line.into()],
        }
    }

    pub fn multi(code: u16, lines: Vec<String>) -> Self {
        Self { code, lines }
    }

    pub fn encode(&self) -> String {
        if self.code == 0 || self.lines.is_empty() {
            return String::new();
        }

        let mut out = String::new();
        for (idx, line) in self.lines.iter().enumerate() {
            let sep = if idx + 1 == self.lines.len() {
                ' '
            } else {
                '-'
            };
            out.push_str(format!("{}{}{}\r\n", self.code, sep, line).as_str());
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct SSmtpMessage {
    pub mail_from: String,
    pub rcpt_to: Vec<String>,
    pub data: String,
}

#[derive(Debug)]
pub struct SSmtpSession {
    pub state: SSmtpState,
    pub tls_active: bool,
    pub authenticated: bool,
    pub helo_name: Option<String>,
    pub auth_user: Option<String>,
    pub mail_from: Option<String>,
    pub rcpt_to: Vec<String>,
    pub last_message: Option<SSmtpMessage>,
    auth_stage: SSmtpAuthStage,
    pending_auth_user: Option<String>,
    data_lines: Vec<String>,
}

impl SSmtpSession {
    pub fn new() -> Self {
        Self {
            state: SSmtpState::Greeting,
            tls_active: false,
            authenticated: false,
            helo_name: None,
            auth_user: None,
            mail_from: None,
            rcpt_to: Vec::new(),
            last_message: None,
            auth_stage: SSmtpAuthStage::None,
            pending_auth_user: None,
            data_lines: Vec::new(),
        }
    }

    pub fn banner(&self) -> SSmtpReply {
        SSmtpReply::single(220, "trueos.local ESMTP ready")
    }

    pub fn reset_transaction(&mut self) {
        self.mail_from = None;
        self.rcpt_to.clear();
        self.data_lines.clear();
        self.state = SSmtpState::Greeting;
    }

    pub fn ehlo(&mut self, domain: &str) -> SSmtpReply {
        self.helo_name = Some(String::from(domain));
        self.state = SSmtpState::MailTransaction;

        let mut lines = vec![format!("trueos.local greets {}", domain)];
        if !self.tls_active {
            lines.push(String::from("STARTTLS"));
        }
        lines.push(String::from("AUTH LOGIN"));
        lines.push(String::from("SIZE 10485760"));
        SSmtpReply::multi(250, lines)
    }

    pub fn starttls(&mut self) -> SSmtpReply {
        if self.tls_active {
            return SSmtpReply::single(503, "Bad sequence of commands");
        }

        self.tls_active = true;
        self.helo_name = None;
        self.auth_stage = SSmtpAuthStage::None;
        self.state = SSmtpState::Greeting;
        SSmtpReply::single(220, "Ready to start TLS")
    }

    pub fn auth_login(&mut self) -> SSmtpReply {
        if !self.tls_active {
            return SSmtpReply::single(
                538,
                "Encryption required for requested authentication mechanism",
            );
        }

        self.auth_stage = SSmtpAuthStage::Username;
        self.state = SSmtpState::Auth;
        SSmtpReply::single(334, "VXNlcm5hbWU6")
    }

    pub fn auth_input(&mut self, line: &str) -> SSmtpReply {
        match self.auth_stage {
            SSmtpAuthStage::Username => {
                let username = base64_decode(line).unwrap_or_default();
                if username.is_empty() {
                    self.auth_stage = SSmtpAuthStage::None;
                    self.state = SSmtpState::Greeting;
                    return SSmtpReply::single(535, "Authentication credentials invalid");
                }

                self.pending_auth_user = Some(username);
                self.auth_stage = SSmtpAuthStage::Password;
                SSmtpReply::single(334, "UGFzc3dvcmQ6")
            }
            SSmtpAuthStage::Password => {
                let password = base64_decode(line).unwrap_or_default();
                self.auth_stage = SSmtpAuthStage::None;
                if password.is_empty() {
                    self.state = SSmtpState::Greeting;
                    return SSmtpReply::single(535, "Authentication credentials invalid");
                }

                self.authenticated = true;
                self.auth_user = self.pending_auth_user.take();
                self.state = SSmtpState::MailTransaction;
                SSmtpReply::single(235, "2.7.0 Authentication successful")
            }
            SSmtpAuthStage::None => SSmtpReply::single(503, "Bad sequence of commands"),
        }
    }

    pub fn mail_from(&mut self, addr: &str) -> SSmtpReply {
        if !self.authenticated {
            return SSmtpReply::single(530, "Authentication required");
        }

        self.mail_from = Some(String::from(addr));
        self.rcpt_to.clear();
        self.data_lines.clear();
        self.state = SSmtpState::MailTransaction;
        SSmtpReply::single(250, "2.1.0 Ok")
    }

    pub fn rcpt_to(&mut self, addr: &str) -> SSmtpReply {
        if self.mail_from.is_none() {
            return SSmtpReply::single(503, "Need MAIL FROM first");
        }

        self.rcpt_to.push(String::from(addr));
        SSmtpReply::single(250, "2.1.5 Ok")
    }

    pub fn data_begin(&mut self) -> SSmtpReply {
        if self.mail_from.is_none() || self.rcpt_to.is_empty() {
            return SSmtpReply::single(503, "Need RCPT TO first");
        }

        self.data_lines.clear();
        self.state = SSmtpState::Data;
        SSmtpReply::single(354, "End data with <CR><LF>.<CR><LF>")
    }

    pub fn data_line(&mut self, line: &str) -> SSmtpReply {
        if self.state != SSmtpState::Data {
            return SSmtpReply::single(503, "Bad sequence of commands");
        }

        if line == "." {
            let msg = SSmtpMessage {
                mail_from: self.mail_from.clone().unwrap_or_default(),
                rcpt_to: self.rcpt_to.clone(),
                data: self.data_lines.join("\r\n"),
            };
            self.last_message = Some(msg);
            self.reset_transaction();
            return SSmtpReply::single(250, "2.0.0 Message accepted for delivery");
        }

        let unstuffed = line.strip_prefix("..").unwrap_or(line);
        self.data_lines.push(String::from(unstuffed));
        SSmtpReply::single(0, "")
    }

    pub fn noop(&self) -> SSmtpReply {
        SSmtpReply::single(250, "2.0.0 Ok")
    }

    pub fn rset(&mut self) -> SSmtpReply {
        self.reset_transaction();
        SSmtpReply::single(250, "2.0.0 Ok")
    }

    pub fn quit(&mut self) -> SSmtpReply {
        self.state = SSmtpState::Closing;
        SSmtpReply::single(221, "2.0.0 Bye")
    }

    pub fn handle_line(&mut self, line: &str) -> SSmtpReply {
        if self.state == SSmtpState::Data {
            return self.data_line(line);
        }
        if self.auth_stage != SSmtpAuthStage::None {
            return self.auth_input(line);
        }

        let trimmed = line.trim();
        let upper = trimmed.to_ascii_uppercase();
        if upper == "AUTH LOGIN" {
            return self.auth_login();
        }
        if upper == "STARTTLS" {
            return self.starttls();
        }
        if upper == "DATA" {
            return self.data_begin();
        }
        if upper == "RSET" {
            return self.rset();
        }
        if upper == "NOOP" {
            return self.noop();
        }
        if upper == "QUIT" {
            return self.quit();
        }
        if let Some(domain) = trimmed.strip_prefix("EHLO ") {
            return self.ehlo(domain.trim());
        }
        if let Some(domain) = trimmed.strip_prefix("HELO ") {
            return self.ehlo(domain.trim());
        }
        if let Some(rest) = trimmed.strip_prefix("MAIL FROM:") {
            return self.mail_from(strip_path(rest));
        }
        if let Some(rest) = trimmed.strip_prefix("RCPT TO:") {
            return self.rcpt_to(strip_path(rest));
        }

        SSmtpReply::single(502, "Command not implemented")
    }
}

fn strip_path(input: &str) -> &str {
    input.trim().trim_matches('<').trim_matches('>')
}

fn base64_decode(input: &str) -> Option<String> {
    let mut buf = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0u32;

    for ch in input.bytes() {
        let val = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\r' | b'\n' => continue,
            _ => return None,
        } as u32;

        acc = (acc << 6) | val;
        bits += 6;
        while bits >= 8 {
            bits -= 8;
            buf.push(((acc >> bits) & 0xff) as u8);
        }
    }

    String::from_utf8(buf).ok()
}
