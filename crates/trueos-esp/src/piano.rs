use v::vnet as api;

pub const TRUEOS_PIANO_UDP_PORT: u16 = 10;
pub const PIANO_FRAME_MAGIC: &[u8; 4] = b"TPNO";
pub const PIANO_FRAME_VERSION: u8 = 1;
pub const PIANO_KEY_COUNT: usize = 96;
pub const PIANO_MASK_BYTES: usize = PIANO_KEY_COUNT / 8;
pub const PIANO_DELTA_BYTES: usize = PIANO_KEY_COUNT * 2;
pub const PIANO_FRAME_LEN: usize = 14 + PIANO_MASK_BYTES + PIANO_DELTA_BYTES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PianoFrame {
    pub module_id: u8,
    pub base_note: u8,
    pub key_count: u8,
    pub seq: u16,
    pub t_ms: u32,
    pub touch_mask: [u8; PIANO_MASK_BYTES],
    pub deltas: [i16; PIANO_KEY_COUNT],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PianoNoteEvent {
    pub key_index: u8,
    pub note: u8,
    pub delta: i16,
    pub velocity: u8,
}

pub struct PianoUdpReceiver {
    udp_handle: Option<api::NetHandle>,
    prev_mask: [u8; PIANO_MASK_BYTES],
}

impl Default for PianoUdpReceiver {
    fn default() -> Self {
        Self::new()
    }
}

impl PianoUdpReceiver {
    pub const fn new() -> Self {
        Self {
            udp_handle: None,
            prev_mask: [0; PIANO_MASK_BYTES],
        }
    }

    pub const fn bootstrap_command(&self) -> api::Command {
        api::Command::OpenUdp {
            port: TRUEOS_PIANO_UDP_PORT,
        }
    }

    pub fn handle(&self) -> Option<api::NetHandle> {
        self.udp_handle
    }

    pub fn bind(&mut self, handle: api::NetHandle) {
        self.udp_handle = Some(handle);
        self.prev_mask = [0; PIANO_MASK_BYTES];
    }

    pub fn unbind(&mut self, handle: api::NetHandle) -> bool {
        if self.udp_handle != Some(handle) {
            return false;
        }
        self.udp_handle = None;
        self.prev_mask = [0; PIANO_MASK_BYTES];
        true
    }

    pub fn on_packet<F>(&mut self, handle: api::NetHandle, data: &[u8], mut on_note_down: F) -> bool
    where
        F: FnMut(PianoNoteEvent),
    {
        if self.udp_handle != Some(handle) {
            return false;
        }

        let Some(frame) = parse_piano_frame(data) else {
            return false;
        };

        for key_index in 0..PIANO_KEY_COUNT {
            let now_down = mask_bit(&frame.touch_mask, key_index);
            let was_down = mask_bit(&self.prev_mask, key_index);
            if now_down && !was_down {
                let note = frame.base_note.saturating_add(key_index as u8);
                on_note_down(PianoNoteEvent {
                    key_index: key_index as u8,
                    note,
                    delta: frame.deltas[key_index],
                    velocity: delta_to_velocity(frame.deltas[key_index]),
                });
            }
        }

        self.prev_mask = frame.touch_mask;
        true
    }
}

pub fn parse_piano_frame(data: &[u8]) -> Option<PianoFrame> {
    if data.len() != PIANO_FRAME_LEN {
        return None;
    }
    if &data[0..4] != PIANO_FRAME_MAGIC {
        return None;
    }
    if data[4] != PIANO_FRAME_VERSION {
        return None;
    }

    let module_id = data[5];
    let base_note = data[6];
    let key_count = data[7];
    if usize::from(key_count) != PIANO_KEY_COUNT {
        return None;
    }

    let seq = u16::from_le_bytes([data[8], data[9]]);
    let t_ms = u32::from_le_bytes([data[10], data[11], data[12], data[13]]);

    let mask_start = 14;
    let delta_start = mask_start + PIANO_MASK_BYTES;
    let mut touch_mask = [0u8; PIANO_MASK_BYTES];
    touch_mask.copy_from_slice(&data[mask_start..delta_start]);

    let mut deltas = [0i16; PIANO_KEY_COUNT];
    for (idx, delta) in deltas.iter_mut().enumerate() {
        let off = delta_start + (idx * 2);
        *delta = i16::from_le_bytes([data[off], data[off + 1]]);
    }

    Some(PianoFrame {
        module_id,
        base_note,
        key_count,
        seq,
        t_ms,
        touch_mask,
        deltas,
    })
}

#[inline]
fn mask_bit(mask: &[u8; PIANO_MASK_BYTES], key_index: usize) -> bool {
    (mask[key_index / 8] & (1 << (key_index % 8))) != 0
}

fn delta_to_velocity(delta: i16) -> u8 {
    let delta = delta.max(0) as u16;
    let scaled = 32 + (delta.min(96) * 95 / 96);
    scaled.min(127) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frame_and_emits_edges() {
        let mut data = [0u8; PIANO_FRAME_LEN];
        data[0..4].copy_from_slice(PIANO_FRAME_MAGIC);
        data[4] = PIANO_FRAME_VERSION;
        data[6] = 24;
        data[7] = PIANO_KEY_COUNT as u8;
        data[14] = 0b0000_0010;
        let delta_off = 14 + PIANO_MASK_BYTES + 2;
        data[delta_off..delta_off + 2].copy_from_slice(&64i16.to_le_bytes());

        let frame = parse_piano_frame(&data).expect("frame");
        assert_eq!(frame.base_note, 24);
        assert_eq!(frame.deltas[1], 64);

        let mut rx = PianoUdpReceiver::new();
        rx.bind(api::NetHandle(7));
        let mut seen = None;
        assert!(rx.on_packet(api::NetHandle(7), &data, |event| seen = Some(event)));
        assert_eq!(
            seen,
            Some(PianoNoteEvent {
                key_index: 1,
                note: 25,
                delta: 64,
                velocity: 95,
            })
        );

        seen = None;
        assert!(rx.on_packet(api::NetHandle(7), &data, |event| seen = Some(event)));
        assert_eq!(seen, None);
    }
}
