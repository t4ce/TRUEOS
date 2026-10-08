// Network-only harnesses model a locked cry account. The real SSH/cry path is
// exercised by test_shell3_ssh.py with an OpenSSH client.
mod ssh {
    pub struct Session { pub closed: bool, pub finished: bool }
    impl Session {
        pub fn new() -> Result<Self, ()> { Err(()) }
        pub fn input(&mut self, _: &[u8]) { unreachable!() }
        pub fn pump(&mut self, _: Option<&mut super::tty::Terminal>) { unreachable!() }
        pub fn wants_shell(&self) -> bool { unreachable!() }
        pub fn size(&self) -> Option<(usize, usize)> { unreachable!() }
        pub fn output(&mut self) -> &[u8] { unreachable!() }
        pub fn consume_output(&mut self, _: usize) { unreachable!() }
    }
}
