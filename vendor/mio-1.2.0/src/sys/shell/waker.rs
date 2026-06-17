use crate::io;
use crate::sys::Selector;
use crate::Token;

#[derive(Debug)]
pub struct Waker {
    selector: Selector,
    token: Token,
}

impl Waker {
    pub fn new(selector: &Selector, token: Token) -> io::Result<Waker> {
        Ok(Waker {
            selector: selector.try_clone()?,
            token,
        })
    }

    pub fn wake(&self) -> io::Result<()> {
        self.selector.push_waker_event(self.token)?;
        crate::zkvm_net::selector_wake(self.selector.id());
        Ok(())
    }
}
