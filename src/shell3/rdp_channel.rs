//! Binary desktop records inside the existing authenticated SSH channel.
use alloc::vec::Vec;

const MAX_RECORD: usize = 65_536;
const HEADER: usize = 5;

pub(crate) struct Channel {
    account: trueos_crypto::AccountId,
    input: Option<super::rdp::InputSession>,
    received: Vec<u8>,
    pub output: Vec<u8>,
    view: bool,
    control: Vec<u8>,
}

impl Channel {
    pub fn new(account: trueos_crypto::AccountId) -> Self {
        Self { account, input: None, received: Vec::new(), output: Vec::new(), view: false, control: Vec::new() }
    }

    pub fn initialized(&self) -> bool { self.input.is_some() }

    fn record(&mut self, kind: u8, payload: &[u8]) {
        self.output.push(kind);
        self.output.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        self.output.extend_from_slice(payload);
    }

    pub fn receive(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        if self.received.len().saturating_add(bytes.len()) > MAX_RECORD + HEADER { return Err("RDP input overflow"); }
        self.received.extend_from_slice(bytes);
        while self.received.len() >= HEADER {
            let kind = self.received[0];
            let len = u32::from_le_bytes(self.received[1..5].try_into().unwrap()) as usize;
            if len > MAX_RECORD { return Err("RDP record too large"); }
            if self.received.len() < HEADER + len { break; }
            let payload = self.received[HEADER..HEADER + len].to_vec();
            self.received.drain(..HEADER + len);
            match kind {
                1 if self.input.is_none() && payload.len() == 2 && payload[0] == 1 && payload[1] <= 1 => {
                    let input = super::rdp::InputSession::new(self.account)?;
                    self.view = payload[1] == 1;
                    if self.view && !crate::ui4::rdp_media_subscribe(input.id()) { return Err("RDP viewer already active"); }
                    let (width, height) = crate::ui4::output_dimensions().unwrap_or((1, 1));
                    let mut ready = [0; 8];
                    ready[..4].copy_from_slice(&width.to_le_bytes());
                    ready[4..].copy_from_slice(&height.to_le_bytes());
                    self.input = Some(input);
                    self.record(0x81, &ready);
                }
                2 if self.input.is_some() => self.input.as_mut().unwrap().input(&payload)?,
                _ => return Err("unexpected RDP record"),
            }
        }
        Ok(())
    }

    pub fn poll(&mut self) {
        if !self.output.is_empty() { return; }
        let Some(input) = &self.input else { return; };
        let id = input.id();
        let control = input.control();
        if control != self.control {
            self.control = control.clone();
            self.record(0x82, &control);
        } else if self.view {
            if let Some(packet) = crate::ui4::rdp_media_take_packet(id) { self.record(0x83, &packet); }
        }
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        if self.view {
            if let Some(input) = &self.input { crate::ui4::rdp_media_unsubscribe(input.id()); }
        }
        // InputSession's destructor revokes the grant and releases held keys.
    }
}
