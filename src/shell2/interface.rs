pub(crate) trait ShellIo2 {
    fn write_str(&self, s: &str);
    fn write_fmt(&self, args: core::fmt::Arguments<'_>);
    fn write_char(&self, ch: char);
    fn write_byte(&self, b: u8) {
        self.write_char(b as char);
    }
}

pub(crate) trait ShellBackend2: ShellIo2 {
    fn init(&self) {}
    fn read_byte(&self) -> Option<u8>;
}
