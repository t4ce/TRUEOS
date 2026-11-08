//! One-time identity selection for human UI4 shells. Credential restoration is
//! asynchronous, so an early shell keeps waiting for the saved boot account.
use trueos_crypto::AccountId;

#[derive(Default)]
pub(super) struct Bootstrap {
    complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Bind(AccountId),
    Challenge,
}

impl Bootstrap {
    pub(super) fn poll(&mut self, current: Option<AccountId>) -> Option<Action> {
        if current.is_some() {
            self.complete();
        }
        if self.complete {
            return None;
        }
        if let Some(account) = super::ssh_boot::local_account_without_proof() {
            return Some(Action::Bind(account));
        }
        if !super::ssh_boot::DEV_SKIP_SSH_ACCOUNT_PROOF
            && crate::crypt::local_shell_account().is_some()
        {
            return Some(Action::Challenge);
        }
        None
    }

    /// Commit only after the exact frontend accepts the binding or challenge.
    /// Later sign-out must not silently reapply the development identity.
    pub(super) fn complete(&mut self) {
        self.complete = true;
    }
}
