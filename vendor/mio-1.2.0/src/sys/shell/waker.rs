use crate::sys::Selector;
use crate::Token;
use std::io;

#[derive(Debug)]
pub struct Waker {}

impl Waker {
    pub fn new(_: &Selector, _: Token) -> io::Result<Waker> {
        unsupported_io!("mio zkvm poll waker registration is not wired yet");
    }

    pub fn wake(&self) -> io::Result<()> {
        unsupported_io!("mio zkvm poll wake signalling is not wired yet");
    }
}
