extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use crate::vcabi;
pub use crate::vcabi::{
    GamepadControlCommand, GamepadControlDeviceInfo, GamepadControlSnapshot,
    KeyboardControlCommand, KeyboardControlDeviceInfo, MouseMotionCommand, MouseMotionCursorInfo,
    TrueosHidCursorEvent, TrueosHidHutCombo, TrueosHidHutKeyboardState, TrueosHidHutMouseState,
    TrueosHidHutTabletState, TrueosHidKeyboardSample, TrueosHidMouseSample, TrueosHidTabletSample,
    TrueosMouseState, TrueosTabletEvent,
};

pub const MOUSE_MOTION_OPCODE_TELEPORT: u8 = 1;
pub const MOUSE_MOTION_OPCODE_STROKE: u8 = 2;
pub const MOUSE_MOTION_OPCODE_BUTTONS: u8 = 3;
pub const MOUSE_MOTION_OPCODE_WHEEL: u8 = 4;
pub const MOUSE_MOTION_PATH_LINE: u8 = 0;
pub const MOUSE_MOTION_PATH_QUADRATIC: u8 = 1;
pub const MOUSE_MOTION_PATH_CUBIC: u8 = 2;
pub const MOUSE_MOTION_EASING_LINEAR: u8 = 0;
pub const MOUSE_MOTION_EASING_FAST_LINEAR: u8 = 1;
pub const MOUSE_MOTION_EASING_NATURAL: u8 = 2;
pub const MOUSE_MOTION_FLAG_CLEAR_QUEUE: u8 = 1 << 0;
pub const KEYBOARD_CONTROL_OPCODE_STROKE: u8 = 1;
pub const KEYBOARD_CONTROL_OPCODE_DOWN: u8 = 2;
pub const KEYBOARD_CONTROL_OPCODE_UP: u8 = 3;
pub const KEYBOARD_CONTROL_OPCODE_WAIT: u8 = 4;
pub const KEYBOARD_CONTROL_FLAG_CLEAR_QUEUE: u8 = 1 << 0;
pub const GAMEPAD_CONTROL_OPCODE_SET: u8 = 1;
pub const GAMEPAD_CONTROL_OPCODE_TWEEN: u8 = 2;
pub const GAMEPAD_CONTROL_OPCODE_WAIT: u8 = 3;
pub const GAMEPAD_CONTROL_EASING_LINEAR: u8 = 0;
pub const GAMEPAD_CONTROL_EASING_NATURAL: u8 = 1;
pub const GAMEPAD_CONTROL_FLAG_CLEAR_QUEUE: u8 = 1 << 0;
pub const INPUT_COMBO_COLOR_AUTO: i32 = -1;
pub const INPUT_COMBO_FLAG_AUTO_ASSIGNED: u8 = 1 << 0;

/// Origin of an input collection. This describes the persona producing input,
/// independently of whether each member device is physical or virtual.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum InputComboSourceKind {
    #[default]
    Unknown = 0,
    Human = 1,
    Ai = 2,
    Remote = 3,
}

/// Stable visual identity assigned to an [`InputCombo`].
///
/// The values are palette slots rather than packed pixels, so every VLayer
/// consumer resolves the same identity into the representation it needs.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum InputComboColor {
    #[default]
    Coral = 0,
    Azure = 1,
    Mint = 2,
    Amber = 3,
    Violet = 4,
    Orange = 5,
    Cyan = 6,
    Lavender = 7,
    Lime = 8,
    Pink = 9,
    Cobalt = 10,
    Green = 11,
    Yellow = 12,
    Purple = 13,
    Rose = 14,
    Ice = 15,
}

impl InputComboColor {
    pub const COUNT: u8 = 16;

    pub const fn from_index(index: u8) -> Self {
        match index % Self::COUNT {
            0 => Self::Coral,
            1 => Self::Azure,
            2 => Self::Mint,
            3 => Self::Amber,
            4 => Self::Violet,
            5 => Self::Orange,
            6 => Self::Cyan,
            7 => Self::Lavender,
            8 => Self::Lime,
            9 => Self::Pink,
            10 => Self::Cobalt,
            11 => Self::Green,
            12 => Self::Yellow,
            13 => Self::Purple,
            14 => Self::Rose,
            _ => Self::Ice,
        }
    }

    pub const fn rgba(self) -> [u8; 4] {
        match self {
            Self::Coral => [255, 64, 64, 255],
            Self::Azure => [32, 168, 255, 255],
            Self::Mint => [32, 224, 128, 255],
            Self::Amber => [255, 190, 32, 255],
            Self::Violet => [220, 80, 255, 255],
            Self::Orange => [255, 112, 32, 255],
            Self::Cyan => [32, 224, 224, 255],
            Self::Lavender => [152, 112, 255, 255],
            Self::Lime => [192, 240, 48, 255],
            Self::Pink => [255, 64, 176, 255],
            Self::Cobalt => [64, 112, 255, 255],
            Self::Green => [48, 192, 96, 255],
            Self::Yellow => [255, 224, 64, 255],
            Self::Purple => [176, 80, 224, 255],
            Self::Rose => [255, 128, 160, 255],
            Self::Ice => [96, 224, 255, 255],
        }
    }
}

/// Address of one independently clocked input device.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct InputEndpoint {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
}

impl InputEndpoint {
    pub const fn new(controller_id: u32, slot_id: u32, ep_target: u32) -> Self {
        Self {
            controller_id,
            slot_id,
            ep_target,
        }
    }
}

/// Capability-backed virtual cursor. Motion is accepted and clocked by the
/// kernel mouse-motion service; this object cannot inject a HID event directly.
#[derive(Debug)]
pub struct VCursor {
    info: MouseMotionCursorInfo,
    open: bool,
}

impl VCursor {
    pub fn request(label: &str) -> Result<Self, i32> {
        let mut info = MouseMotionCursorInfo::default();
        let rc = unsafe {
            vcabi::trueos_cabi_mouse_motion_cursor_request(label.as_ptr(), label.len(), &mut info)
        };
        if rc != 0 {
            return Err(rc);
        }
        Ok(Self { info, open: true })
    }

    pub const fn handle(&self) -> u64 {
        self.info.handle
    }

    pub const fn slot_id(&self) -> u32 {
        self.info.slot_id
    }

    pub const fn input_endpoint(&self) -> InputEndpoint {
        InputEndpoint::new(0, self.info.slot_id, 0)
    }

    pub fn submit(&self, command: MouseMotionCommand) -> Result<(), i32> {
        if !self.open {
            return Err(-3);
        }
        let rc = unsafe { vcabi::trueos_cabi_mouse_motion_submit(self.info.handle, &command) };
        if rc == 0 { Ok(()) } else { Err(rc) }
    }

    pub fn submit_json(&self, json: &str) -> Result<(), i32> {
        if !self.open || json.is_empty() {
            return Err(-1);
        }
        let rc = unsafe {
            vcabi::trueos_cabi_mouse_motion_submit_json(self.info.handle, json.as_ptr(), json.len())
        };
        if rc < 0 { Err(rc) } else { Ok(()) }
    }

    pub fn teleport(&self, x: i32, y: i32) -> Result<(), i32> {
        self.submit(MouseMotionCommand {
            opcode: MOUSE_MOTION_OPCODE_TELEPORT,
            flags: MOUSE_MOTION_FLAG_CLEAR_QUEUE,
            x,
            y,
            ..MouseMotionCommand::default()
        })
    }

    pub fn idle(&self) -> Result<bool, i32> {
        if !self.open {
            return Err(-3);
        }
        match unsafe { vcabi::trueos_cabi_mouse_motion_cursor_idle(self.info.handle) } {
            0 => Ok(false),
            1 => Ok(true),
            rc => Err(rc),
        }
    }

    pub fn close(mut self) -> Result<(), i32> {
        let result = self.close_inner();
        self.open = false;
        result
    }

    fn close_inner(&mut self) -> Result<(), i32> {
        if !self.open {
            return Ok(());
        }
        let rc = unsafe { vcabi::trueos_cabi_mouse_motion_cursor_release(self.info.handle) };
        if rc == 0 {
            self.open = false;
            Ok(())
        } else {
            Err(rc)
        }
    }
}

impl Drop for VCursor {
    fn drop(&mut self) {
        let _ = self.close_inner();
    }
}

/// Capability-backed virtual keyboard. Key programs are clocked by the kernel
/// and appear as one coherent AI keyboard in HUT and the keyboard event ring.
#[derive(Debug)]
pub struct VKeyboard {
    info: KeyboardControlDeviceInfo,
    open: bool,
}

impl VKeyboard {
    pub fn request(label: &str) -> Result<Self, i32> {
        let mut info = KeyboardControlDeviceInfo::default();
        let rc = unsafe {
            vcabi::trueos_cabi_keyboard_control_request(label.as_ptr(), label.len(), &mut info)
        };
        if rc != 0 {
            return Err(rc);
        }
        Ok(Self { info, open: true })
    }

    pub const fn handle(&self) -> u64 {
        self.info.handle
    }

    pub const fn slot_id(&self) -> u32 {
        self.info.slot_id
    }

    pub const fn input_endpoint(&self) -> InputEndpoint {
        InputEndpoint::new(0, self.info.slot_id, 0)
    }

    pub fn submit(&self, command: KeyboardControlCommand) -> Result<(), i32> {
        if !self.open {
            return Err(-3);
        }
        let rc = unsafe { vcabi::trueos_cabi_keyboard_control_submit(self.info.handle, &command) };
        if rc == 0 { Ok(()) } else { Err(rc) }
    }

    pub fn type_text(&self, text: &str, interval_ms: u32, clear_queue: bool) -> Result<usize, i32> {
        if !self.open || text.is_empty() {
            return Err(-1);
        }
        let rc = unsafe {
            vcabi::trueos_cabi_keyboard_control_submit_text(
                self.info.handle,
                text.as_ptr(),
                text.len(),
                interval_ms,
                u32::from(clear_queue),
            )
        };
        if rc < 0 { Err(rc) } else { Ok(rc as usize) }
    }

    pub fn submit_json(&self, json: &str) -> Result<usize, i32> {
        if !self.open || json.is_empty() {
            return Err(-1);
        }
        let rc = unsafe {
            vcabi::trueos_cabi_keyboard_control_submit_json(
                self.info.handle,
                json.as_ptr(),
                json.len(),
            )
        };
        if rc < 0 { Err(rc) } else { Ok(rc as usize) }
    }

    pub fn idle(&self) -> Result<bool, i32> {
        if !self.open {
            return Err(-3);
        }
        match unsafe { vcabi::trueos_cabi_keyboard_control_idle(self.info.handle) } {
            0 => Ok(false),
            1 => Ok(true),
            rc => Err(rc),
        }
    }

    pub fn close(mut self) -> Result<(), i32> {
        let result = self.close_inner();
        self.open = false;
        result
    }

    fn close_inner(&mut self) -> Result<(), i32> {
        if !self.open {
            return Ok(());
        }
        let rc = unsafe { vcabi::trueos_cabi_keyboard_control_release(self.info.handle) };
        if rc == 0 {
            self.open = false;
            Ok(())
        } else {
            Err(rc)
        }
    }
}

impl Drop for VKeyboard {
    fn drop(&mut self) {
        let _ = self.close_inner();
    }
}

/// Capability-backed virtual gamepad. Its mediated state is ready for a future
/// UI/game consumer without granting callers a raw HID injection endpoint.
#[derive(Debug)]
pub struct VGamepad {
    info: GamepadControlDeviceInfo,
    open: bool,
}

impl VGamepad {
    pub fn request(label: &str) -> Result<Self, i32> {
        let mut info = GamepadControlDeviceInfo::default();
        let rc = unsafe {
            vcabi::trueos_cabi_gamepad_control_request(label.as_ptr(), label.len(), &mut info)
        };
        if rc != 0 {
            return Err(rc);
        }
        Ok(Self { info, open: true })
    }

    pub const fn handle(&self) -> u64 {
        self.info.handle
    }

    pub const fn slot_id(&self) -> u32 {
        self.info.slot_id
    }

    pub const fn input_endpoint(&self) -> InputEndpoint {
        InputEndpoint::new(0, self.info.slot_id, 0)
    }

    pub fn submit(&self, command: GamepadControlCommand) -> Result<(), i32> {
        if !self.open {
            return Err(-3);
        }
        let rc = unsafe { vcabi::trueos_cabi_gamepad_control_submit(self.info.handle, &command) };
        if rc == 0 { Ok(()) } else { Err(rc) }
    }

    pub fn submit_json(&self, json: &str) -> Result<usize, i32> {
        if !self.open || json.is_empty() {
            return Err(-1);
        }
        let rc = unsafe {
            vcabi::trueos_cabi_gamepad_control_submit_json(
                self.info.handle,
                json.as_ptr(),
                json.len(),
            )
        };
        if rc < 0 { Err(rc) } else { Ok(rc as usize) }
    }

    pub fn idle(&self) -> Result<bool, i32> {
        if !self.open {
            return Err(-3);
        }
        match unsafe { vcabi::trueos_cabi_gamepad_control_idle(self.info.handle) } {
            0 => Ok(false),
            1 => Ok(true),
            rc => Err(rc),
        }
    }

    pub fn snapshot(&self) -> Result<GamepadControlSnapshot, i32> {
        if !self.open {
            return Err(-3);
        }
        let mut snapshot = GamepadControlSnapshot::default();
        let rc =
            unsafe { vcabi::trueos_cabi_gamepad_control_snapshot(self.info.handle, &mut snapshot) };
        if rc == 0 { Ok(snapshot) } else { Err(rc) }
    }

    pub fn close(mut self) -> Result<(), i32> {
        let result = self.close_inner();
        self.open = false;
        result
    }

    fn close_inner(&mut self) -> Result<(), i32> {
        if !self.open {
            return Ok(());
        }
        let rc = unsafe { vcabi::trueos_cabi_gamepad_control_release(self.info.handle) };
        if rc == 0 {
            self.open = false;
            Ok(())
        } else {
            Err(rc)
        }
    }
}

impl Drop for VGamepad {
    fn drop(&mut self) {
        let _ = self.close_inner();
    }
}

/// VLayer identity for a collection of independently operating input devices.
///
/// A combo may contain at most one device of each supported class. Binding a
/// member never changes how that device is clocked or owned; it only gives UI
/// and routing services a shared persona, color, and keyboard/cursor relation.
/// RDP can therefore request its virtual devices independently and bind them
/// into one combo after allocation.
#[derive(Clone, Debug)]
pub struct InputCombo {
    info: TrueosHidHutCombo,
}

impl InputCombo {
    pub fn request(
        label: &str,
        source_kind: InputComboSourceKind,
        color: Option<InputComboColor>,
    ) -> Result<Self, i32> {
        if label.is_empty() {
            return Err(-1);
        }
        let mut info = TrueosHidHutCombo::default();
        let requested_color = color
            .map(|value| i32::from(value as u8))
            .unwrap_or(INPUT_COMBO_COLOR_AUTO);
        let rc = unsafe {
            vcabi::trueos_cabi_input_combo_request(
                source_kind as u8,
                requested_color,
                label.as_ptr(),
                label.len(),
                &mut info,
            )
        };
        if rc == 0 {
            Ok(Self { info })
        } else {
            Err(rc)
        }
    }

    pub const fn id(&self) -> u32 {
        self.info.combo_id
    }

    pub const fn color(&self) -> InputComboColor {
        InputComboColor::from_index(self.info.color_id)
    }

    pub const fn info(&self) -> TrueosHidHutCombo {
        self.info
    }

    pub fn refresh(&mut self) -> Result<(), i32> {
        let Some(info) = hid_hut_combos()
            .into_iter()
            .find(|combo| combo.combo_id == self.info.combo_id)
        else {
            return Err(-3);
        };
        self.info = info;
        Ok(())
    }

    pub fn set_color(&mut self, color: InputComboColor) -> Result<(), i32> {
        let rc = unsafe {
            vcabi::trueos_cabi_input_combo_set_color(self.info.combo_id, color as u8)
        };
        if rc != 0 {
            return Err(rc);
        }
        self.info.color_id = color as u8;
        Ok(())
    }

    pub fn bind_mouse_endpoint(&self, endpoint: InputEndpoint) -> Result<(), i32> {
        combo_bind_result(unsafe {
            vcabi::trueos_cabi_input_combo_bind_mouse(
                self.info.combo_id,
                endpoint.controller_id,
                endpoint.slot_id,
                endpoint.ep_target,
            )
        })
    }

    pub fn bind_keyboard_endpoint(&self, endpoint: InputEndpoint) -> Result<(), i32> {
        combo_bind_result(unsafe {
            vcabi::trueos_cabi_input_combo_bind_keyboard(
                self.info.combo_id,
                endpoint.controller_id,
                endpoint.slot_id,
                endpoint.ep_target,
            )
        })
    }

    pub fn bind_tablet_endpoint(&self, endpoint: InputEndpoint) -> Result<(), i32> {
        combo_bind_result(unsafe {
            vcabi::trueos_cabi_input_combo_bind_tablet(
                self.info.combo_id,
                endpoint.controller_id,
                endpoint.slot_id,
                endpoint.ep_target,
            )
        })
    }

    pub fn bind_gamepad_endpoint(&self, endpoint: InputEndpoint) -> Result<(), i32> {
        combo_bind_result(unsafe {
            vcabi::trueos_cabi_input_combo_bind_gamepad(
                self.info.combo_id,
                endpoint.controller_id,
                endpoint.slot_id,
                endpoint.ep_target,
            )
        })
    }

    pub fn bind_cursor(&self, cursor: &VCursor) -> Result<(), i32> {
        self.bind_mouse_endpoint(cursor.input_endpoint())
    }

    pub fn bind_keyboard(&self, keyboard: &VKeyboard) -> Result<(), i32> {
        self.bind_keyboard_endpoint(keyboard.input_endpoint())
    }

    pub fn bind_gamepad(&self, gamepad: &VGamepad) -> Result<(), i32> {
        self.bind_gamepad_endpoint(gamepad.input_endpoint())
    }

    /// Remove the collection identity. Member devices remain alive and
    /// independent; only their shared routing/color association is removed.
    pub fn remove(self) -> Result<(), i32> {
        combo_bind_result(unsafe { vcabi::trueos_cabi_input_combo_remove(self.info.combo_id) })
    }
}

#[inline]
fn combo_bind_result(rc: i32) -> Result<(), i32> {
    if rc == 0 { Ok(()) } else { Err(rc) }
}

#[inline]
pub fn mouse_poll() -> Option<TrueosMouseState> {
    let mut out = TrueosMouseState::default();
    let rc = unsafe { vcabi::trueos_cabi_mouse_poll(&mut out) };
    if rc == 0 { Some(out) } else { None }
}

#[inline]
pub fn qjs_mouse_pop() -> Option<TrueosMouseState> {
    let mut out = TrueosMouseState::default();
    let rc = unsafe { vcabi::trueos_cabi_qjs_mouse_pop(&mut out) };
    if rc == 0 { Some(out) } else { None }
}

#[inline]
pub fn pop_mouse_delta() -> Option<(u8, i8, i8, i8)> {
    let mut buttons = 0u8;
    let mut dx = 0i8;
    let mut dy = 0i8;
    let mut wheel = 0i8;
    let rc =
        unsafe { vcabi::trueos_cabi_input_pop_mouse(&mut buttons, &mut dx, &mut dy, &mut wheel) };
    if rc == 1 {
        Some((buttons, dx, dy, wheel))
    } else {
        None
    }
}

#[inline]
pub fn pop_tablet_event() -> Option<TrueosTabletEvent> {
    let mut out = TrueosTabletEvent::default();
    let rc = unsafe { vcabi::trueos_cabi_input_pop_tablet(&mut out) };
    if rc == 1 { Some(out) } else { None }
}

#[inline]
pub fn cursor_pos(cursor_id: u32) -> Result<(i32, i32), i32> {
    let mut x = 0i32;
    let mut y = 0i32;
    let rc = unsafe { vcabi::trueos_cabi_input_cursor_pos(cursor_id, &mut x, &mut y) };
    if rc != 0 {
        return Err(rc);
    }
    Ok((x, y))
}

#[inline]
pub fn cursor_buttons(cursor_id: u32) -> Result<u32, i32> {
    let mut buttons = 0u32;
    let rc = unsafe { vcabi::trueos_cabi_input_cursor_buttons(cursor_id, &mut buttons) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(buttons)
}

#[inline]
pub fn read_cursor_events_since(
    read_seq: u64,
    out_cap: u32,
) -> Result<(Vec<TrueosHidCursorEvent>, u64, u32), i32> {
    let mut events = vec![TrueosHidCursorEvent::default(); out_cap as usize];
    let mut next_seq = read_seq;
    let mut dropped = 0u32;
    let got = unsafe {
        vcabi::trueos_cabi_input_read_cursor_events_since(
            read_seq,
            events.as_mut_ptr(),
            out_cap,
            &mut next_seq,
            &mut dropped,
        )
    };
    if got == 0 && out_cap != 0 && dropped == 0 && next_seq == read_seq {
        return Ok((Vec::new(), next_seq, dropped));
    }
    events.truncate(got as usize);
    Ok((events, next_seq, dropped))
}

#[inline]
pub fn write_cursor(
    slot_id: u32,
    x: i32,
    y: i32,
    buttons_down: u32,
    wheel: i32,
    flags: u32,
) -> Result<(), i32> {
    // Compatibility endpoint. `slot_id` must belong to a VCursor allocated by
    // the mouse-motion service; arbitrary direct injection is rejected.
    let rc =
        unsafe { vcabi::trueos_cabi_input_write_cursor(slot_id, x, y, buttons_down, wheel, flags) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

#[inline]
pub fn hid_keyboard_read(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    out_cap: u32,
) -> (Vec<TrueosHidKeyboardSample>, u32) {
    let mut samples = vec![TrueosHidKeyboardSample::default(); out_cap as usize];
    let mut dropped = 0u32;
    let got = unsafe {
        vcabi::trueos_cabi_hid_keyboard_read(
            controller_id,
            slot_id,
            ep_target,
            samples.as_mut_ptr(),
            out_cap,
            &mut dropped,
        )
    };
    samples.truncate(got as usize);
    (samples, dropped)
}

#[inline]
pub fn hid_mouse_read(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    out_cap: u32,
) -> (Vec<TrueosHidMouseSample>, u32) {
    let mut samples = vec![TrueosHidMouseSample::default(); out_cap as usize];
    let mut dropped = 0u32;
    let got = unsafe {
        vcabi::trueos_cabi_hid_mouse_read(
            controller_id,
            slot_id,
            ep_target,
            samples.as_mut_ptr(),
            out_cap,
            &mut dropped,
        )
    };
    samples.truncate(got as usize);
    (samples, dropped)
}

#[inline]
pub fn hid_tablet_read(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    out_cap: u32,
) -> (Vec<TrueosHidTabletSample>, u32) {
    let mut samples = vec![TrueosHidTabletSample::default(); out_cap as usize];
    let mut dropped = 0u32;
    let got = unsafe {
        vcabi::trueos_cabi_hid_tablet_read(
            controller_id,
            slot_id,
            ep_target,
            samples.as_mut_ptr(),
            out_cap,
            &mut dropped,
        )
    };
    samples.truncate(got as usize);
    (samples, dropped)
}

#[inline]
pub fn hid_hut_upsert_combo(combo_id: u32, source_kind: u8, source_tag: &str) -> Result<(), i32> {
    let rc = unsafe {
        vcabi::trueos_cabi_hid_hut_upsert_combo(
            combo_id,
            source_kind,
            source_tag.as_ptr(),
            source_tag.len(),
        )
    };
    if rc == 0 { Ok(()) } else { Err(rc) }
}

#[inline]
pub fn hid_hut_bind_combo_mouse(
    combo_id: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
) -> Result<(), i32> {
    let rc = unsafe {
        vcabi::trueos_cabi_hid_hut_bind_combo_mouse(combo_id, controller_id, slot_id, ep_target)
    };
    if rc == 0 { Ok(()) } else { Err(rc) }
}

#[inline]
pub fn hid_hut_bind_combo_keyboard(
    combo_id: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
) -> Result<(), i32> {
    let rc = unsafe {
        vcabi::trueos_cabi_hid_hut_bind_combo_keyboard(combo_id, controller_id, slot_id, ep_target)
    };
    if rc == 0 { Ok(()) } else { Err(rc) }
}

#[inline]
pub fn hid_hut_bind_combo_tablet(
    combo_id: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
) -> Result<(), i32> {
    let rc = unsafe {
        vcabi::trueos_cabi_hid_hut_bind_combo_tablet(combo_id, controller_id, slot_id, ep_target)
    };
    if rc == 0 { Ok(()) } else { Err(rc) }
}

#[inline]
pub fn hid_hut_bind_combo_gamepad(
    combo_id: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
) -> Result<(), i32> {
    let rc = unsafe {
        vcabi::trueos_cabi_hid_hut_bind_combo_gamepad(
            combo_id,
            controller_id,
            slot_id,
            ep_target,
        )
    };
    if rc == 0 { Ok(()) } else { Err(rc) }
}

#[inline]
pub fn hid_hut_mice() -> Vec<TrueosHidHutMouseState> {
    let count = unsafe { vcabi::trueos_cabi_hid_hut_read_mice(core::ptr::null_mut(), 0) };
    let mut out = vec![TrueosHidHutMouseState::default(); count as usize];
    if count != 0 {
        let got = unsafe { vcabi::trueos_cabi_hid_hut_read_mice(out.as_mut_ptr(), count) };
        out.truncate(got as usize);
    }
    out
}

#[inline]
pub fn hid_hut_tablets() -> Vec<TrueosHidHutTabletState> {
    let count = unsafe { vcabi::trueos_cabi_hid_hut_read_tablets(core::ptr::null_mut(), 0) };
    let mut out = vec![TrueosHidHutTabletState::default(); count as usize];
    if count != 0 {
        let got = unsafe { vcabi::trueos_cabi_hid_hut_read_tablets(out.as_mut_ptr(), count) };
        out.truncate(got as usize);
    }
    out
}

#[inline]
pub fn hid_hut_keyboards() -> Vec<TrueosHidHutKeyboardState> {
    let count = unsafe { vcabi::trueos_cabi_hid_hut_read_keyboards(core::ptr::null_mut(), 0) };
    let mut out = vec![TrueosHidHutKeyboardState::default(); count as usize];
    if count != 0 {
        let got = unsafe { vcabi::trueos_cabi_hid_hut_read_keyboards(out.as_mut_ptr(), count) };
        out.truncate(got as usize);
    }
    out
}

#[inline]
pub fn hid_hut_combos() -> Vec<TrueosHidHutCombo> {
    let count = unsafe { vcabi::trueos_cabi_hid_hut_read_combos(core::ptr::null_mut(), 0) };
    let mut out = vec![TrueosHidHutCombo::default(); count as usize];
    if count != 0 {
        let got = unsafe { vcabi::trueos_cabi_hid_hut_read_combos(out.as_mut_ptr(), count) };
        out.truncate(got as usize);
    }
    out
}

#[inline]
pub fn write_keyboard_text(slot_id: u32, bytes: &[u8], flags: u32) -> Result<(), i32> {
    if slot_id == 0 || bytes.is_empty() {
        return Err(-1);
    }
    let rc = unsafe {
        vcabi::trueos_cabi_input_write_keyboard_text(slot_id, bytes.as_ptr(), bytes.len(), flags)
    };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

#[inline]
pub fn write_keyboard_key(
    slot_id: u32,
    codepoint: u32,
    key_code: u32,
    modifiers: u32,
    flags: u32,
) -> Result<(), i32> {
    if slot_id == 0 {
        return Err(-1);
    }
    let rc = unsafe {
        vcabi::trueos_cabi_input_write_keyboard_key(slot_id, codepoint, key_code, modifiers, flags)
    };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}
