//! Input devices owned by one authenticated SSH RDP subsystem session.
//! Transport admission belongs to SSH; source labels never authenticate input.
use alloc::vec::Vec;
use heapless::Vec as BoundedVec;
use spin::Mutex;
use trueos_crypto::AccountId;
use super::tui::{self, RemoteInputAuthority};
use crate::r::services::{keyboard_control_service as keyboard, mouse_motion_service as mouse};
use crate::usb2::hid::hut::{self, HidDeviceKind, HidSourceKind};

#[derive(Clone, Copy)]
struct Devices {
    authority: RemoteInputAuthority,
    combo: u32,
    keyboard: u32,
    cursor: u32,
}
static DEVICES: Mutex<BoundedVec<Devices, 16>> = Mutex::new(BoundedVec::new());

/// Resolve only devices this trusted SSH bridge actually allocated. A kernel
/// station or a Remote label alone is insufficient to obtain human authority.
pub(crate) fn device_authority(kind: HidDeviceKind, slot: u32) -> Option<(RemoteInputAuthority, u32)> {
    let entry = DEVICES.lock().iter().find(|entry| match kind {
        HidDeviceKind::Keyboard => entry.keyboard == slot,
        HidDeviceKind::Mouse => entry.cursor == slot,
        _ => false,
    }).copied()?;
    tui::remote_input_authority_valid(entry.authority).then_some((entry.authority, entry.combo))
}

pub(crate) struct InputSession {
    authority: RemoteInputAuthority,
    keyboard: keyboard::KeyboardControlDevice,
    cursor: mouse::MouseControlCursor,
    combo: u32,
    x: i32,
    y: i32,
    buttons: u32,
    device_id: u16,
    sequences: [Option<u32>; 5],
}

impl InputSession {
    pub(crate) fn new(account: AccountId) -> Result<Self, &'static str> {
        let keyboard = keyboard::request_keyboard(keyboard::KeyboardControlPrincipal::Kernel, "ssh-rdp-keyboard")
            .map_err(|_| "RDP keyboard unavailable")?;
        let cursor = match mouse::request_cursor(mouse::MouseControlPrincipal::Kernel, "ssh-rdp-cursor", None) {
            Ok(cursor) => cursor,
            Err(_) => {
                let _ = keyboard::release_keyboard(keyboard::KeyboardControlPrincipal::Kernel, keyboard.handle);
                return Err("RDP cursor unavailable");
            }
        };
        let combo = hut::request_combo(HidSourceKind::Remote, "SSH RDP", None);
        let Some(combo_id) = combo.as_ref().filter(|combo|
            hut::bind_combo_keyboard(combo.combo_id, 0, keyboard.slot_id, 0)
                && hut::bind_combo_mouse(combo.combo_id, 0, cursor.slot_id, 0)).map(|combo| combo.combo_id) else {
            if let Some(combo) = combo { hut::remove_combo(combo.combo_id); }
            let _ = mouse::release_cursor(mouse::MouseControlPrincipal::Kernel, cursor.handle);
            let _ = keyboard::release_keyboard(keyboard::KeyboardControlPrincipal::Kernel, keyboard.handle);
            return Err("RDP input combo unavailable");
        };
        let authority = tui::open_rdp_input_session(account);
        let (width, height) = dimensions();
        let session = Self {
            authority, keyboard, cursor, combo: combo_id,
            x: (width / 2) as i32, y: (height / 2) as i32, buttons: 0,
            device_id: 1, sequences: [None; 5],
        };
        DEVICES.lock().push(Devices {
            authority, combo: session.combo, keyboard: keyboard.slot_id, cursor: cursor.slot_id,
        }).map_err(|_| "RDP session capacity reached")?;
        // Establish the paired pointer route before the first keyboard report.
        session.pointer(None, 0)?;
        Ok(session)
    }

    pub(crate) fn id(&self) -> u64 { self.authority.session_id().expect("native SSH input session") }

    pub(crate) fn input(&mut self, packet: &[u8]) -> Result<(), &'static str> {
        if !tui::remote_input_authority_valid(self.authority) { return Err("RDP session expired"); }
        if packet.len() < 16 || &packet[..4] != b"THID" || packet[4] != 1
            || packet[14..16] != [0, 0] { return Err("invalid RDP input header"); }
        let kind = packet[5] as usize;
        let flags = u16::from_le_bytes(packet[6..8].try_into().unwrap());
        let sequence = u32::from_le_bytes(packet[8..12].try_into().unwrap());
        let payload = &packet[16..];
        let valid = match kind { 1 => payload.len() == 4, 2 => payload.len() == 8,
            3 => payload.len() == 16, 4 => payload.is_empty(), _ => false };
        if !valid { return Err("invalid RDP input payload"); }
        if self.sequences[kind].is_some_and(|previous| sequence.wrapping_sub(previous) as i32 <= 0) { return Ok(()); }
        self.sequences[kind] = Some(sequence);
        self.device_id = u16::from_le_bytes(packet[12..14].try_into().unwrap());
        match kind {
            1 => {
                let dx = i16::from(payload[1] as i8);
                let dy = i16::from(payload[2] as i8);
                self.move_relative(dx, dy);
                self.buttons = u32::from(payload[0]);
                self.pointer(Some((dx, dy)), i16::from(payload[3] as i8))?;
            }
            2 => {
                if payload[1] != 0 { return Err("invalid RDP keyboard report"); }
                keyboard::submit_report(keyboard::KeyboardControlPrincipal::Kernel, self.keyboard.handle,
                    payload[0], payload[2..8].try_into().unwrap())
                    .map_err(|_| "RDP keyboard report rejected")?;
            }
            3 => {
                let x = u32::from_le_bytes(payload[..4].try_into().unwrap());
                let y = u32::from_le_bytes(payload[4..8].try_into().unwrap());
                let relative = flags & (1 << 15) != 0;
                let delta = if relative {
                    let dx = i16::try_from(x as i32).map_err(|_| "RDP relative X out of range")?;
                    let dy = i16::try_from(y as i32).map_err(|_| "RDP relative Y out of range")?;
                    self.move_relative(dx, dy);
                    Some((dx, dy))
                } else {
                    let (width, height) = dimensions();
                    self.x = (u64::from(x.min(65535)) * u64::from(width - 1) / 65535) as i32;
                    self.y = (u64::from(y.min(65535)) * u64::from(height - 1) / 65535) as i32;
                    None
                };
                self.buttons = u32::from_le_bytes(payload[8..12].try_into().unwrap());
                let wheel = i16::from_le_bytes(payload[12..14].try_into().unwrap());
                self.pointer(delta, wheel)?;
            }
            4 if flags & 1 != 0 => {
                if let Some(frame) = crate::ui4::center_snapped_frame_for_source(self.source()) {
                    let _ = crate::ui4::set_window_center_snapped_mouse(frame.owner, frame.window, false);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn source(&self) -> crate::ui4::Ui4CursorSource {
        crate::ui4::Ui4CursorSource { controller_id: 0, slot_id: self.cursor.slot_id,
            ep_target: 0, hid_kind: crate::r::cursor::HID_KIND_MOUSE }
    }
    fn move_relative(&mut self, dx: i16, dy: i16) {
        let (width, height) = dimensions();
        self.x = self.x.saturating_add(i32::from(dx)).clamp(0, (width - 1) as i32);
        self.y = self.y.saturating_add(i32::from(dy)).clamp(0, (height - 1) as i32);
    }
    fn pointer(&self, relative: Option<(i16, i16)>, wheel: i16) -> Result<(), &'static str> {
        mouse::submit_remote_report(mouse::MouseControlPrincipal::Kernel, self.cursor.handle,
            self.x, self.y, self.buttons, wheel, relative).map_err(|_| "RDP pointer report rejected")
    }
    pub(crate) fn control(&self) -> Vec<u8> {
        let enabled = tui::remote_input_authority_valid(self.authority)
            && crate::ui4::center_snapped_frame_for_source(self.source()).is_some();
        let mut reply = Vec::from(&b"THIC\x01\x00\x00\x00"[..]);
        reply[5] = u8::from(enabled);
        reply[6..8].copy_from_slice(&self.device_id.to_le_bytes());
        reply
    }
}

fn dimensions() -> (u32, u32) {
    let (width, height) = crate::ui4::output_dimensions().unwrap_or((320, 200));
    (width.max(1).min(i32::MAX as u32), height.max(1).min(i32::MAX as u32))
}

impl Drop for InputSession {
    fn drop(&mut self) {
        let session = self.id();
        tui::close_rdp_input_session(session);
        DEVICES.lock().retain(|entry| entry.authority.session_id() != Some(session));
        let _ = keyboard::release_keyboard(keyboard::KeyboardControlPrincipal::Kernel, self.keyboard.handle);
        let _ = mouse::release_cursor(mouse::MouseControlPrincipal::Kernel, self.cursor.handle);
        hut::remove_combo(self.combo);
    }
}
