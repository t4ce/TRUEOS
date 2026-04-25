pub(crate) trait ShellIo2 {
    // Raw terminal/backend writes bypass the shell transcript. Normal command
    // output should go through `print_shell_line` or a command session target.
    fn raw_write_str(&self, s: &str);
    fn raw_write_fmt(&self, args: core::fmt::Arguments<'_>);
    fn raw_write_char(&self, ch: char);
    fn raw_write_byte(&self, b: u8) {
        self.raw_write_char(b as char);
    }
}

pub(crate) trait ShellBackend2: ShellIo2 {
    fn init(&self) {}
    fn read_byte(&self) -> Option<u8>;
}
