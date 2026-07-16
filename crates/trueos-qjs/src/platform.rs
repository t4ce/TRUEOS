#![cfg(feature = "trueos")]

pub mod sys {
    #[inline]
    pub fn write_stream(stream: u32, bytes: &[u8]) {
        v::vsys::write_stream(stream, bytes);
    }

    #[inline]
    pub fn write_stdout(bytes: &[u8]) {
        write_stream(1, bytes);
    }

    #[inline]
    pub fn write_stderr(bytes: &[u8]) {
        write_stream(2, bytes);
    }

    #[inline]
    pub fn poll_once() {
        v::vsys::poll_once();
    }
}

pub mod ui {
    #[inline]
    pub fn signal_hosted_browser_dirty(content_id: u32, flags: u32) {
        let _ = (content_id, flags);
    }
}
