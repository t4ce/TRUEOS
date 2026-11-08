//! Shared account label for pixel and terminal Shell3 titles.
use alloc::{format, string::String, vec, vec::Vec};
use trueos_crypto::AccountId;

use super::{MetaFmtStr, OPERATOR, RgbaColor};

#[derive(Default)]
pub(super) struct Identity {
    account: Option<AccountId>,
    username: Option<String>,
}

impl Identity {
    pub(super) fn refresh(&mut self, account: Option<AccountId>) -> bool {
        if self.account == account && (account.is_none() || self.username.is_some()) {
            return false;
        }
        let username = account.and_then(crate::crypt::account_username);
        let changed = self.username != username;
        self.account = account;
        self.username = username;
        changed
    }

    pub(super) fn left(&self, time: &str) -> Vec<MetaFmtStr> {
        let mut runs = vec![MetaFmtStr::new(format!("TrueOS {OPERATOR} {time}"))];
        if let Some(username) = &self.username {
            let mut display = username.clone();
            if let Some(first) = display.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            runs.push(MetaFmtStr::new(" "));
            runs.push(MetaFmtStr::new(display).color(RgbaColor::Cyan));
        }
        runs
    }
}
