use super::{Action, Attempt, Policy};

/// A redirection [`Policy`] that limits the number of successive redirections.
#[derive(Clone, Copy, Debug)]
pub struct Limited {
    remaining: usize,
}

impl Limited {
    /// Create a new [`Limited`] with a limit of `max` redirections.
    pub fn new(max: usize) -> Self {
        Limited { remaining: max }
    }
}

impl Default for Limited {
    /// Returns the default [`Limited`] with a limit of `20` redirections.
    fn default() -> Self {
        // This is the (default) limit of Firefox and the Fetch API.
        // https://hg.mozilla.org/mozilla-central/file/6264f13d54a1caa4f5b60303617a819efd91b8ee/modules/libpref/init/all.js#l1371
        // https://fetch.spec.whatwg.org/#http-redirect-fetch
        Limited::new(20)
    }
}

impl<B, E> Policy<B, E> for Limited {
    fn redirect(&mut self, _: &Attempt<'_>) -> Result<Action, E> {
        if self.remaining > 0 {
            self.remaining -= 1;
            Ok(Action::Follow)
        } else {
            Ok(Action::Stop)
        }
    }
}
