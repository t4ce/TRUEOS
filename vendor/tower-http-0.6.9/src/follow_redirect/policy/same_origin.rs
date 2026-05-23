use super::{eq_origin, Action, Attempt, Policy};
use ::core::fmt;

/// A redirection [`Policy`] that stops cross-origin redirections.
#[derive(Clone, Copy, Default)]
pub struct SameOrigin {
    _priv: (),
}

impl SameOrigin {
    /// Create a new [`SameOrigin`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl fmt::Debug for SameOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SameOrigin").finish()
    }
}

impl<B, E> Policy<B, E> for SameOrigin {
    fn redirect(&mut self, attempt: &Attempt<'_>) -> Result<Action, E> {
        if eq_origin(attempt.previous(), attempt.location()) {
            Ok(Action::Follow)
        } else {
            Ok(Action::Stop)
        }
    }
}
