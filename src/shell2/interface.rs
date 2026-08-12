#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalHandoffOwner {
    raw: u32,
    local_session_generation: u64,
}

impl TerminalHandoffOwner {
    pub(crate) const STREAM_KIND: u32 = 1 << 31;

    pub(crate) const fn blueprint(vm_id: u8) -> Self {
        Self {
            raw: vm_id as u32 + 1,
            local_session_generation: 0,
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const fn stream(session_id: u32) -> Self {
        Self {
            raw: Self::STREAM_KIND | (session_id & !Self::STREAM_KIND),
            local_session_generation: 0,
        }
    }

    /// Bind this owner token to one incarnation of a recyclable local shell.
    /// Net-shell ownership remains transport-global and uses generation zero.
    pub(crate) const fn for_local_session(self, generation: u64) -> Self {
        Self {
            raw: self.raw,
            local_session_generation: generation,
        }
    }

    pub(crate) const fn local_session_generation(self) -> Option<u64> {
        if self.local_session_generation == 0 {
            None
        } else {
            Some(self.local_session_generation)
        }
    }

    pub(crate) const fn raw(self) -> u32 {
        self.raw
    }
}

pub(crate) trait ShellIo2: Sync {
    fn output_mask(&self) -> super::OutputMask {
        0
    }

    fn transport_scope(&self) -> u8 {
        0
    }

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

    /// Give an attached terminal byte-for-byte ownership of this backend.
    ///
    /// The shell task remains alive while a handoff is active, but it neither
    /// consumes input nor paints chrome. Backends without a raw terminal path
    /// keep the default unsupported implementation.
    fn claim_terminal_handoff(&self, _owner: TerminalHandoffOwner) -> bool {
        false
    }

    fn release_terminal_handoff(&self, _owner: TerminalHandoffOwner) {}

    fn terminal_handoff_active(&self) -> bool {
        false
    }

    fn supports_terminal_handoff(&self) -> bool {
        false
    }

    fn terminal_handoff_read(&self, _owner: TerminalHandoffOwner, _out: &mut [u8]) -> usize {
        0
    }

    fn terminal_handoff_readable_len(&self, _owner: TerminalHandoffOwner) -> usize {
        0
    }

    fn terminal_handoff_write(&self, _owner: TerminalHandoffOwner, _bytes: &[u8]) -> bool {
        false
    }
}
