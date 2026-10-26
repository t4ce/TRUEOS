//! Bounded, allocation-free loader transcript retained as one boot module.
//!
//! The module has a fixed size. Its length prefix changes as the loader emits
//! later messages, so a handoff prepared before the final boot stages still
//! exposes their text when the kernel reads the original retained allocation.

const MAGIC: &[u8; 8] = b"TOSBOOT1";
pub const CAPACITY: usize = 32 * 1024;
pub const HEADER_BYTES: usize = MAGIC.len() + 4;
pub const WIRE_BYTES: usize = HEADER_BYTES + CAPACITY;

#[repr(transparent)]
pub struct BootLog {
    wire: [u8; WIRE_BYTES],
}

impl BootLog {
    pub const fn new() -> Self {
        // Keep the loader's retained static in BSS instead of adding 32 KiB
        // of mostly zero data to its EFI file. The first append marks it live.
        Self { wire: [0; WIRE_BYTES] }
    }

    /// Retain the earliest messages. Clip full buffers on a UTF-8 boundary.
    pub fn append(&mut self, text: &str) {
        self.wire[..MAGIC.len()].copy_from_slice(MAGIC);
        let used = u32::from_le_bytes(self.wire[8..HEADER_BYTES].try_into().unwrap()) as usize;
        let mut length = text.len().min(CAPACITY - used);
        while !text.is_char_boundary(length) {
            length -= 1;
        }
        self.wire[HEADER_BYTES + used..HEADER_BYTES + used + length]
            .copy_from_slice(&text.as_bytes()[..length]);
        self.wire[8..HEADER_BYTES].copy_from_slice(&((used + length) as u32).to_le_bytes());
    }

    /// Include the complete fixed-size wire allocation in the boot module.
    pub fn payload_bytes(&self) -> &[u8] {
        &self.wire
    }
}

/// Read a retained transcript without trusting its length prefix or encoding.
pub fn text_from_blob(blob: &[u8]) -> Option<&str> {
    if blob.len() != WIRE_BYTES || blob.get(..MAGIC.len())? != MAGIC {
        return None;
    }
    let length = u32::from_le_bytes(blob.get(8..HEADER_BYTES)?.try_into().ok()?) as usize;
    if length > CAPACITY {
        return None;
    }
    core::str::from_utf8(blob.get(HEADER_BYTES..HEADER_BYTES + length)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_prefix_and_payload_are_validated_before_reading() {
        let mut log = BootLog::new();
        assert_eq!(text_from_blob(log.payload_bytes()), None);
        log.append("");
        assert_eq!(text_from_blob(log.payload_bytes()), Some(""));
        log.append("Selected display: 2560x1440\n");
        let mut wire = log.wire;
        assert_eq!(&wire[..8], b"TOSBOOT1");
        assert_eq!(u32::from_le_bytes(wire[8..12].try_into().unwrap()), 28);
        assert_eq!(text_from_blob(&wire), Some("Selected display: 2560x1440\n"));
        assert_eq!(text_from_blob(&wire[..WIRE_BYTES - 1]), None);
        wire[0] ^= 1;
        assert_eq!(text_from_blob(&wire), None);
        wire[0] ^= 1;
        wire[8..12].copy_from_slice(&((CAPACITY + 1) as u32).to_le_bytes());
        assert_eq!(text_from_blob(&wire), None);
        wire[8..12].copy_from_slice(&1u32.to_le_bytes());
        wire[HEADER_BYTES] = 0xff;
        assert_eq!(text_from_blob(&wire), None);
    }

    #[test]
    fn full_buffers_preserve_valid_utf8_and_cannot_overrun_the_wire() {
        let mut log = BootLog::new();
        let text = [b'x'; CAPACITY - 2];
        log.append(core::str::from_utf8(&text).unwrap());
        log.append("€"); // Three-byte scalar cannot fit into the remaining two bytes.
        assert_eq!(text_from_blob(log.payload_bytes()).unwrap().len(), CAPACITY - 2);
        assert_eq!(&log.payload_bytes()[WIRE_BYTES - 2..], &[0, 0]);
        log.append("é");
        let retained = text_from_blob(log.payload_bytes()).unwrap();
        assert_eq!(retained.len(), CAPACITY);
        assert!(retained.ends_with('é'));
        let full_wire = log.wire;
        log.append("unretained message");
        assert_eq!(log.wire, full_wire);
    }

    #[test]
    fn messages_after_handoff_preparation_keep_the_same_fixed_blob() {
        let mut log = BootLog::new();
        log.append("Monitor EDID: 128 bytes\n");
        let module_address = log.payload_bytes().as_ptr() as usize;
        let module_size = log.payload_bytes().len();
        log.append("ExitBootServices complete\n");
        log.append("CPU APIC 2: ready\n");
        assert_eq!(log.payload_bytes().as_ptr() as usize, module_address);
        assert_eq!(log.payload_bytes().len(), module_size);
        assert_eq!(module_size, WIRE_BYTES);
        assert_eq!(text_from_blob(log.payload_bytes()), Some(
            "Monitor EDID: 128 bytes\nExitBootServices complete\nCPU APIC 2: ready\n"));
    }
}
