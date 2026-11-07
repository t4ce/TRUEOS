//! Account bindings are per frontend, never per transport or current login.
use alloc::vec::Vec;
use trueos_crypto::AccountId;

pub(super) struct Bindings {
    human: Vec<(u64, AccountId)>,
    guest: Vec<(u64, Option<AccountId>)>,
}
impl Bindings {
    pub(super) const fn new() -> Self { Self { human: Vec::new(), guest: Vec::new() } }
    pub(super) fn account(&self, frontend: u64) -> Option<AccountId> {
        if let Some((_, account)) = self.guest.iter().find(|entry| entry.0 == frontend) {
            return *account;
        }
        self.human.iter().find(|entry| entry.0 == frontend).map(|entry| entry.1)
    }
    pub(super) fn is_guest(&self, frontend: u64) -> bool {
        self.guest.iter().any(|entry| entry.0 == frontend)
    }
    pub(super) fn bind_human(&mut self, frontend: u64, account: Option<AccountId>) -> bool {
        if self.is_guest(frontend) { return false; }
        self.human.retain(|entry| entry.0 != frontend);
        if let Some(account) = account { self.human.push((frontend, account)); }
        true
    }
    pub(super) fn bind_guest(&mut self, frontend: u64, account: Option<AccountId>) {
        if self.is_guest(frontend) { return; }
        self.human.retain(|entry| entry.0 != frontend);
        self.guest.push((frontend, account));
    }
    pub(super) fn remove(&mut self, frontend: u64) {
        self.human.retain(|entry| entry.0 != frontend);
        self.guest.retain(|entry| entry.0 != frontend);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ALICE: AccountId = AccountId::new(1);
    const BOB: AccountId = AccountId::new(2);
    #[test] fn anonymous_ssh_cannot_borrow_another_connections_login() {
        let mut bindings = Bindings::new();
        assert!(bindings.bind_human(10, Some(ALICE)));
        assert_eq!(bindings.account(10), Some(ALICE));
        assert_eq!(bindings.account(11), None);
    }
    #[test] fn separate_verified_sessions_keep_their_own_accounts() {
        let mut bindings = Bindings::new();
        bindings.bind_human(10, Some(ALICE)); bindings.bind_human(11, Some(BOB));
        assert_eq!(bindings.account(10), Some(ALICE)); assert_eq!(bindings.account(11), Some(BOB));
        bindings.bind_human(10, None);
        assert_eq!(bindings.account(10), None); assert_eq!(bindings.account(11), Some(BOB));
    }
    #[test] fn guest_principal_survives_another_human_login_and_rebind_attempt() {
        let mut bindings = Bindings::new();
        bindings.bind_guest(20, Some(ALICE)); bindings.bind_human(10, Some(BOB));
        assert!(!bindings.bind_human(20, Some(BOB)));
        bindings.bind_guest(20, Some(BOB));
        assert_eq!(bindings.account(20), Some(ALICE));
    }
    #[test] fn anonymous_guest_cannot_upgrade_itself_after_login() {
        let mut bindings = Bindings::new(); bindings.bind_guest(20, None);
        bindings.bind_human(10, Some(ALICE)); assert!(!bindings.bind_human(20, Some(ALICE)));
        assert_eq!(bindings.account(20), None);
    }
    #[test] fn detached_frontend_drops_identity_before_reuse() {
        let mut bindings = Bindings::new(); bindings.bind_human(10, Some(ALICE)); bindings.remove(10);
        assert_eq!(bindings.account(10), None); bindings.bind_guest(10, None);
        assert_eq!(bindings.account(10), None);
    }
}
