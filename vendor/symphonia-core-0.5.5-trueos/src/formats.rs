use alloc::boxed::Box;

#[derive(Clone)]
pub struct Packet {
    track_id: u32,
    pub ts: u64,
    pub dur: u64,
    pub trim_start: u32,
    pub trim_end: u32,
    pub data: Box<[u8]>,
}

impl Packet {
    pub fn new_from_slice(track_id: u32, ts: u64, dur: u64, buf: &[u8]) -> Self {
        Packet { track_id, ts, dur, trim_start: 0, trim_end: 0, data: Box::from(buf) }
    }

    pub fn new_from_boxed_slice(track_id: u32, ts: u64, dur: u64, data: Box<[u8]>) -> Self {
        Packet { track_id, ts, dur, trim_start: 0, trim_end: 0, data }
    }

    pub fn track_id(&self) -> u32 {
        self.track_id
    }

    pub fn ts(&self) -> u64 {
        self.ts
    }

    pub fn dur(&self) -> u64 {
        self.dur
    }

    pub fn buf(&self) -> &[u8] {
        &self.data
    }
}
