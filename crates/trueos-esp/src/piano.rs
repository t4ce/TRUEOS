use v::vnet as api;

pub const TRUEOS_PIANO_UDP_PORT: u16 = 9696;
pub const PIANO_FRAME_MAGIC: &[u8; 4] = b"TPNO";
pub const PIANO_FRAME_VERSION: u8 = 1;
pub const PIANO_STATE_FRAME_VERSION: u8 = 2;
pub const PIANO_KEY_COUNT: usize = 96;
pub const PIANO_TEXT_BASE_NOTE: u8 = 36;
pub const PIANO_MASK_BYTES: usize = PIANO_KEY_COUNT / 8;
pub const PIANO_DELTA_BYTES: usize = PIANO_KEY_COUNT * 2;
pub const PIANO_FRAME_LEN: usize = 14 + PIANO_MASK_BYTES + PIANO_DELTA_BYTES;
pub const PIANO_STATE_FRAME_LEN: usize = 14 + PIANO_MASK_BYTES;
const PIANO_ATTACK_FULL_RISE_DELTA: i16 = 48;
const PIANO_ATTACK_BONUS_MAX: u8 = 10;
const PIANO_DELTA_NOISE_FLOOR: i16 = 8;
const PIANO_DELTA_ACTIVE_MIN: i16 = 12;
const PIANO_DELTA_VELOCITY_MAX: i16 = 100;

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
pub enum PianoNoteEventKind {
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PianoNoteEvent {
    pub kind: PianoNoteEventKind,
    pub key_index: u8,
    pub note: u8,
    pub delta: i16,
    pub velocity: u8,
}

pub struct PianoUdpReceiver {
    udp_handle: Option<api::NetHandle>,
    prev_mask: [u8; PIANO_MASK_BYTES],
    prev_deltas: [i16; PIANO_KEY_COUNT],
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
            prev_deltas: [0; PIANO_KEY_COUNT],
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
        self.prev_deltas = [0; PIANO_KEY_COUNT];
    }

    pub fn unbind(&mut self, handle: api::NetHandle) -> bool {
        if self.udp_handle != Some(handle) {
            return false;
        }
        self.udp_handle = None;
        self.prev_mask = [0; PIANO_MASK_BYTES];
        self.prev_deltas = [0; PIANO_KEY_COUNT];
        true
    }

    pub fn on_packet<F>(&mut self, handle: api::NetHandle, data: &[u8], mut on_note_event: F) -> bool
    where
        F: FnMut(PianoNoteEvent),
    {
        if self.udp_handle != Some(handle) {
            return false;
        }

        let Some(frame) = parse_piano_frame(data) else {
            return false;
        };

        let mut active_mask = [0u8; PIANO_MASK_BYTES];
        for key_index in 0..PIANO_KEY_COUNT {
            let was_down = mask_bit(&self.prev_mask, key_index);
            let delta = frame.deltas[key_index];
            let now_down = key_active(&frame.touch_mask, key_index, delta, was_down);
            if now_down {
                set_mask_bit(&mut active_mask, key_index);
            }
            if now_down != was_down {
                let note = frame.base_note.saturating_add(key_index as u8);
                let velocity = if now_down {
                    delta_to_velocity_with_attack(delta, self.prev_deltas[key_index])
                } else {
                    delta_to_velocity(delta)
                };
                on_note_event(PianoNoteEvent {
                    kind: if now_down {
                        PianoNoteEventKind::Down
                    } else {
                        PianoNoteEventKind::Up
                    },
                    key_index: key_index as u8,
                    note,
                    delta,
                    velocity,
                });
            }
        }

        self.prev_mask = active_mask;
        self.prev_deltas = frame.deltas;
        true
    }
}

pub fn parse_piano_frame(data: &[u8]) -> Option<PianoFrame> {
    if data.starts_with(b"piano ") || data == b"piano" {
        return parse_text_piano_frame(data);
    }

    if data.len() != PIANO_FRAME_LEN && data.len() != PIANO_STATE_FRAME_LEN {
        return None;
    }
    if &data[0..4] != PIANO_FRAME_MAGIC {
        return None;
    }
    let version = data[4];
    if version != PIANO_FRAME_VERSION && version != PIANO_STATE_FRAME_VERSION {
        return None;
    }
    if version == PIANO_FRAME_VERSION && data.len() != PIANO_FRAME_LEN {
        return None;
    }
    if version == PIANO_STATE_FRAME_VERSION && data.len() != PIANO_STATE_FRAME_LEN {
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
    if version == PIANO_FRAME_VERSION {
        for (idx, delta) in deltas.iter_mut().enumerate() {
            let off = delta_start + (idx * 2);
            *delta = i16::from_le_bytes([data[off], data[off + 1]]);
        }
    } else {
        for (idx, delta) in deltas.iter_mut().enumerate() {
            if mask_bit(&touch_mask, idx) {
                *delta = 64;
            }
        }
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

fn parse_text_piano_frame(data: &[u8]) -> Option<PianoFrame> {
    let text = core::str::from_utf8(data).ok()?.trim();
    let mut parts = text.split_whitespace();
    if parts.next()? != "piano" {
        return None;
    }

    let mut seq = 0u16;
    let mut mask = 0u128;
    let mut saw_mask = false;
    let mut deltas = [0i16; PIANO_KEY_COUNT];
    let mut saw_deltas = false;

    for part in parts {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "seq" => {
                seq = parse_u16_token(value)?;
            }
            "mask" => {
                mask = parse_mask_token(value)?;
                saw_mask = true;
            }
            "deltas" => {
                parse_deltas_token(value, &mut deltas)?;
                saw_deltas = true;
            }
            _ => {}
        }
    }

    if !saw_mask {
        return None;
    }

    let mut touch_mask = [0u8; PIANO_MASK_BYTES];
    for (idx, byte) in touch_mask.iter_mut().enumerate() {
        *byte = ((mask >> (idx * 8)) & 0xFF) as u8;
    }

    if !saw_deltas {
        for (idx, delta) in deltas.iter_mut().enumerate() {
            if mask_bit(&touch_mask, idx) {
                *delta = 64;
            }
        }
    }

    Some(PianoFrame {
        module_id: 0,
        base_note: PIANO_TEXT_BASE_NOTE,
        key_count: PIANO_KEY_COUNT as u8,
        seq,
        t_ms: 0,
        touch_mask,
        deltas,
    })
}

fn parse_u16_token(value: &str) -> Option<u16> {
    let parsed = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        u32::from_str_radix(value, 10).ok()?
    };
    Some(parsed as u16)
}

fn parse_mask_token(value: &str) -> Option<u128> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u128::from_str_radix(hex, 16).ok()
    } else {
        u128::from_str_radix(value, 10).ok()
    }
}

fn parse_i16_token(value: &str) -> Option<i16> {
    let parsed = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        i32::from_str_radix(hex, 16).ok()?
    } else {
        i32::from_str_radix(value, 10).ok()?
    };
    Some(parsed.clamp(i16::MIN as i32, i16::MAX as i32) as i16)
}

fn parse_deltas_token(value: &str, deltas: &mut [i16; PIANO_KEY_COUNT]) -> Option<()> {
    for (idx, token) in value.split(',').enumerate() {
        if idx >= PIANO_KEY_COUNT {
            return None;
        }
        if token.is_empty() {
            return None;
        }
        deltas[idx] = parse_i16_token(token)?;
    }
    Some(())
}

#[inline]
fn mask_bit(mask: &[u8; PIANO_MASK_BYTES], key_index: usize) -> bool {
    (mask[key_index / 8] & (1 << (key_index % 8))) != 0
}

#[inline]
fn set_mask_bit(mask: &mut [u8; PIANO_MASK_BYTES], key_index: usize) {
    mask[key_index / 8] |= 1 << (key_index % 8);
}

fn key_active(
    touch_mask: &[u8; PIANO_MASK_BYTES],
    key_index: usize,
    delta: i16,
    was_active: bool,
) -> bool {
    if !mask_bit(touch_mask, key_index) {
        return false;
    }

    if was_active {
        delta > PIANO_DELTA_NOISE_FLOOR
    } else {
        delta >= PIANO_DELTA_ACTIVE_MIN
    }
}

fn delta_to_velocity(delta: i16) -> u8 {
    if delta <= PIANO_DELTA_NOISE_FLOOR {
        return 0;
    }

    let delta = delta.clamp(PIANO_DELTA_ACTIVE_MIN, PIANO_DELTA_VELOCITY_MAX) as u16;
    let min = PIANO_DELTA_ACTIVE_MIN as u16;
    let max = PIANO_DELTA_VELOCITY_MAX as u16;
    let span = max - min;
    let pos = delta.saturating_sub(min);

    let linear_q8 = pos * 255 / span;
    let square_q8 = linear_q8 * linear_q8 / 255;
    let curve_q8 = (linear_q8 + square_q8) / 2;
    1 + (curve_q8 * 126 / 255) as u8
}

fn delta_to_velocity_with_attack(delta: i16, prev_delta: i16) -> u8 {
    let pressure_velocity = delta_to_velocity(delta);
    let rise = delta.saturating_sub(prev_delta).max(0);
    let attack_bonus = (rise.min(PIANO_ATTACK_FULL_RISE_DELTA) as u16
        * PIANO_ATTACK_BONUS_MAX as u16
        / PIANO_ATTACK_FULL_RISE_DELTA as u16) as u8;
    pressure_velocity.saturating_add(attack_bonus).min(127)
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
                kind: PianoNoteEventKind::Down,
                key_index: 1,
                note: 25,
                delta: 64,
                velocity: 69,
            })
        );

        seen = None;
        assert!(rx.on_packet(api::NetHandle(7), &data, |event| seen = Some(event)));
        assert_eq!(seen, None);

        data[14] = 0;
        assert!(rx.on_packet(api::NetHandle(7), &data, |event| seen = Some(event)));
        assert_eq!(
            seen,
            Some(PianoNoteEvent {
                kind: PianoNoteEventKind::Up,
                key_index: 1,
                note: 25,
                delta: 64,
                velocity: 59,
            })
        );
    }

    #[test]
    fn parses_compact_state_frame_for_two_boards() {
        let mut data = [0u8; PIANO_STATE_FRAME_LEN];
        data[0..4].copy_from_slice(PIANO_FRAME_MAGIC);
        data[4] = PIANO_STATE_FRAME_VERSION;
        data[6] = 36;
        data[7] = PIANO_KEY_COUNT as u8;

        // Board A uses bits 0..11, board B uses bits 12..23.
        data[14] = 0b0000_0001;
        data[15] = 0b0001_0000;

        let frame = parse_piano_frame(&data).expect("state frame");
        assert_eq!(frame.base_note, 36);
        assert!(mask_bit(&frame.touch_mask, 0));
        assert!(mask_bit(&frame.touch_mask, 12));
        assert_eq!(frame.deltas[0], 64);
    }

    #[test]
    fn parses_text_state_frame_from_current_sender() {
        let data = b"piano seq=42 mask=0x001001\n";
        let frame = parse_piano_frame(data).expect("text frame");
        assert_eq!(frame.seq, 42);
        assert_eq!(frame.base_note, PIANO_TEXT_BASE_NOTE);
        assert!(mask_bit(&frame.touch_mask, 0));
        assert!(mask_bit(&frame.touch_mask, 12));
        assert_eq!(frame.deltas[0], 64);
    }

    #[test]
    fn parses_text_state_frame_with_deltas() {
        let data =
            b"piano seq=42 mask=0x000040 deltas=0,0,1,0,4,0,81,3,0,0,0,0,0,0,0,2,0,0,0,0,0,0,0,0";
        let frame = parse_piano_frame(data).expect("text frame with deltas");
        assert_eq!(frame.seq, 42);
        assert!(mask_bit(&frame.touch_mask, 6));
        assert_eq!(frame.deltas[6], 81);
        assert_eq!(delta_to_velocity(frame.deltas[6]), 88);
        assert_eq!(frame.deltas[24], 0);
    }

    #[test]
    fn parses_text_state_frame_with_board_b_delta() {
        let data =
            b"piano seq=43 mask=0x080000 deltas=0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,86,0,0,0,0";
        let frame = parse_piano_frame(data).expect("board b text frame");
        assert_eq!(frame.seq, 43);
        assert!(mask_bit(&frame.touch_mask, 19));
        assert_eq!(frame.deltas[19], 86);
        assert_eq!(delta_to_velocity(frame.deltas[19]), 97);
    }

    #[test]
    fn note_on_velocity_gets_gentle_attack_bonus() {
        let mut rx = PianoUdpReceiver::new();
        rx.bind(api::NetHandle(7));

        let idle = b"piano seq=1 mask=0x000000 deltas=0,0,0,0,0,0,40";
        let mut seen = None;
        assert!(rx.on_packet(api::NetHandle(7), idle, |event| seen = Some(event)));
        assert_eq!(seen, None);

        let down = b"piano seq=2 mask=0x000040 deltas=0,0,0,0,0,0,45";
        assert!(rx.on_packet(api::NetHandle(7), down, |event| seen = Some(event)));
        assert_eq!(
            seen,
            Some(PianoNoteEvent {
                kind: PianoNoteEventKind::Down,
                key_index: 6,
                note: 42,
                delta: 45,
                velocity: 34,
            })
        );
    }

    #[test]
    fn held_delta_crossing_noise_floor_can_start_note() {
        let mut rx = PianoUdpReceiver::new();
        rx.bind(api::NetHandle(7));

        let below_floor = b"piano seq=1 mask=0x000040 deltas=0,0,0,0,0,0,9";
        let mut seen = None;
        assert!(rx.on_packet(api::NetHandle(7), below_floor, |event| seen = Some(event)));
        assert_eq!(seen, None);

        let active = b"piano seq=2 mask=0x000040 deltas=0,0,0,0,0,0,45";
        assert!(rx.on_packet(api::NetHandle(7), active, |event| seen = Some(event)));
        assert_eq!(
            seen,
            Some(PianoNoteEvent {
                kind: PianoNoteEventKind::Down,
                key_index: 6,
                note: 42,
                delta: 45,
                velocity: 40,
            })
        );
    }
}
