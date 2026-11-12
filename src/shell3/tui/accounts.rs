//! Account bindings are per frontend, never per transport or current login.
use alloc::vec::Vec;
use trueos_crypto::AccountId;

pub(super) struct Bindings {
    human: Vec<(u64, AccountId, u64)>,
    guest: Vec<(u64, Option<AccountId>)>,
    next_human_epoch: u64,
}
impl Bindings {
    pub(super) const fn new() -> Self {
        Self {
            human: Vec::new(),
            guest: Vec::new(),
            next_human_epoch: 0,
        }
    }
    pub(super) fn account(&self, frontend: u64) -> Option<AccountId> {
        if let Some((_, account)) = self.guest.iter().find(|entry| entry.0 == frontend) {
            return *account;
        }
        self.human
            .iter()
            .find(|entry| entry.0 == frontend)
            .map(|entry| entry.1)
    }
    /// An inherited account is not a human session. Reauthentication issues a
    /// new epoch even for the same account, so old control grants stay retired.
    pub(super) fn human_session(&self, frontend: u64) -> Option<(AccountId, u64)> {
        self.human.iter().find(|entry| entry.0 == frontend)
            .map(|entry| (entry.1, entry.2))
    }
    pub(super) fn is_guest(&self, frontend: u64) -> bool {
        self.guest.iter().any(|entry| entry.0 == frontend)
    }
    pub(super) fn bind_human(&mut self, frontend: u64, account: Option<AccountId>) -> bool {
        if self.is_guest(frontend) {
            return false;
        }
        self.human.retain(|entry| entry.0 != frontend);
        if let Some(account) = account {
            self.next_human_epoch = self.next_human_epoch.checked_add(1)
                .expect("human frontend session epoch exhausted");
            self.human.push((frontend, account, self.next_human_epoch));
        }
        true
    }
    pub(super) fn bind_guest(&mut self, frontend: u64, account: Option<AccountId>) {
        if self.is_guest(frontend) {
            return;
        }
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
    #[test]
    fn anonymous_ssh_cannot_borrow_another_connections_login() {
        let mut bindings = Bindings::new();
        assert!(bindings.bind_human(10, Some(ALICE)));
        assert_eq!(bindings.account(10), Some(ALICE));
        assert_eq!(bindings.account(11), None);
    }
    #[test]
    fn separate_verified_sessions_keep_their_own_accounts() {
        let mut bindings = Bindings::new();
        bindings.bind_human(10, Some(ALICE));
        bindings.bind_human(11, Some(BOB));
        assert_eq!(bindings.account(10), Some(ALICE));
        assert_eq!(bindings.account(11), Some(BOB));
        bindings.bind_human(10, None);
        assert_eq!(bindings.account(10), None);
        assert_eq!(bindings.account(11), Some(BOB));
    }
    #[test]
    fn guest_principal_survives_another_human_login_and_rebind_attempt() {
        let mut bindings = Bindings::new();
        bindings.bind_guest(20, Some(ALICE));
        bindings.bind_human(10, Some(BOB));
        assert!(!bindings.bind_human(20, Some(BOB)));
        bindings.bind_guest(20, Some(BOB));
        assert_eq!(bindings.account(20), Some(ALICE));
    }
    #[test]
    fn anonymous_guest_cannot_upgrade_itself_after_login() {
        let mut bindings = Bindings::new();
        bindings.bind_guest(20, None);
        bindings.bind_human(10, Some(ALICE));
        assert!(!bindings.bind_human(20, Some(ALICE)));
        assert_eq!(bindings.account(20), None);
    }
    #[test]
    fn detached_frontend_drops_identity_before_reuse() {
        let mut bindings = Bindings::new();
        bindings.bind_human(10, Some(ALICE));
        bindings.remove(10);
        assert_eq!(bindings.account(10), None);
        bindings.bind_guest(10, None);
        assert_eq!(bindings.account(10), None);
    }
    #[test]
    fn same_account_reauthentication_never_revives_old_control_grant() {
        let mut bindings = Bindings::new();
        bindings.bind_human(10, Some(ALICE));
        let original = bindings.human_session(10).unwrap();
        bindings.bind_human(10, None);
        assert_eq!(bindings.human_session(10), None);
        bindings.bind_human(10, Some(ALICE));
        let reauthenticated = bindings.human_session(10).unwrap();
        assert_eq!(original.0, reauthenticated.0);
        assert_ne!(original.1, reauthenticated.1);
        bindings.remove(10);
        bindings.bind_human(10, Some(ALICE));
        assert_ne!(bindings.human_session(10), Some(reauthenticated));
    }
    #[test]
    fn inherited_account_never_supplies_a_human_session() {
        let mut bindings = Bindings::new();
        bindings.bind_guest(20, Some(ALICE));
        assert_eq!(bindings.account(20), Some(ALICE));
        assert_eq!(bindings.human_session(20), None);
        assert!(!bindings.bind_human(20, Some(ALICE)));
        assert_eq!(bindings.human_session(20), None);
    }
}
