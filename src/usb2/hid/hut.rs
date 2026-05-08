use heapless::{String, Vec};
use spin::Mutex;

const MAX_HID_HUT_MICE: usize = 32;
const MAX_HID_HUT_TABLETS: usize = 32;
const MAX_HID_HUT_KEYBOARDS: usize = 32;
const MAX_HID_HUT_COMBOS: usize = 32;
pub const HID_HUT_SOURCE_TAG_MAX: usize = 32;
const HID_SOURCE_TAG_MAX: usize = HID_HUT_SOURCE_TAG_MAX;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HidSourceKind {
    Unknown = 0,
    Human = 1,
    Ai = 2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HidMouseState {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub x: f64,
    pub y: f64,
    pub buttons_down: u32,
    pub combo_id: u32,
    pub source_kind: HidSourceKind,
    pub source_tag: String<HID_SOURCE_TAG_MAX>,
    pub virtual_device: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HidTabletState {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub x: f64,
    pub y: f64,
    pub x_raw: u16,
    pub y_raw: u16,
    pub buttons_down: u32,
    pub report_id: u8,
    pub combo_id: u32,
    pub source_kind: HidSourceKind,
    pub source_tag: String<HID_SOURCE_TAG_MAX>,
    pub virtual_device: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HidKeyboardState {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub modifiers: u8,
    pub keys: [u8; 6],
    pub ascii: [u8; 6],
    pub combo_id: u32,
    pub source_kind: HidSourceKind,
    pub source_tag: String<HID_SOURCE_TAG_MAX>,
    pub virtual_device: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HidCombo {
    pub combo_id: u32,
    pub source_kind: HidSourceKind,
    pub source_tag: String<HID_SOURCE_TAG_MAX>,
    pub mouse_controller_id: u32,
    pub mouse_slot_id: u32,
    pub mouse_ep_target: u32,
    pub keyboard_controller_id: u32,
    pub keyboard_slot_id: u32,
    pub keyboard_ep_target: u32,
    pub tablet_controller_id: u32,
    pub tablet_slot_id: u32,
    pub tablet_ep_target: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TrueosHidHutMouseState {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub buttons_down: u32,
    pub combo_id: u32,
    pub source_kind: u8,
    pub virtual_device: u8,
    pub source_tag_len: u8,
    pub reserved0: u8,
    pub source_tag: [u8; HID_HUT_SOURCE_TAG_MAX],
    pub x: f64,
    pub y: f64,
}

impl Default for TrueosHidHutMouseState {
    fn default() -> Self {
        Self {
            controller_id: 0,
            slot_id: 0,
            ep_target: 0,
            buttons_down: 0,
            combo_id: 0,
            source_kind: 0,
            virtual_device: 0,
            source_tag_len: 0,
            reserved0: 0,
            source_tag: [0; HID_HUT_SOURCE_TAG_MAX],
            x: 0.0,
            y: 0.0,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TrueosHidHutTabletState {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub x_raw: u16,
    pub y_raw: u16,
    pub buttons_down: u32,
    pub report_id: u8,
    pub source_kind: u8,
    pub virtual_device: u8,
    pub source_tag_len: u8,
    pub combo_id: u32,
    pub source_tag: [u8; HID_HUT_SOURCE_TAG_MAX],
    pub x: f64,
    pub y: f64,
}

impl Default for TrueosHidHutTabletState {
    fn default() -> Self {
        Self {
            controller_id: 0,
            slot_id: 0,
            ep_target: 0,
            x_raw: 0,
            y_raw: 0,
            buttons_down: 0,
            report_id: 0,
            source_kind: 0,
            virtual_device: 0,
            source_tag_len: 0,
            combo_id: 0,
            source_tag: [0; HID_HUT_SOURCE_TAG_MAX],
            x: 0.0,
            y: 0.0,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TrueosHidHutKeyboardState {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub combo_id: u32,
    pub modifiers: u8,
    pub source_kind: u8,
    pub virtual_device: u8,
    pub source_tag_len: u8,
    pub keys: [u8; 6],
    pub ascii: [u8; 6],
    pub source_tag: [u8; HID_HUT_SOURCE_TAG_MAX],
}

impl Default for TrueosHidHutKeyboardState {
    fn default() -> Self {
        Self {
            controller_id: 0,
            slot_id: 0,
            ep_target: 0,
            combo_id: 0,
            modifiers: 0,
            source_kind: 0,
            virtual_device: 0,
            source_tag_len: 0,
            keys: [0; 6],
            ascii: [0; 6],
            source_tag: [0; HID_HUT_SOURCE_TAG_MAX],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosHidHutCombo {
    pub combo_id: u32,
    pub source_kind: u8,
    pub source_tag_len: u8,
    pub reserved0: u16,
    pub source_tag: [u8; HID_HUT_SOURCE_TAG_MAX],
    pub mouse_controller_id: u32,
    pub mouse_slot_id: u32,
    pub mouse_ep_target: u32,
    pub keyboard_controller_id: u32,
    pub keyboard_slot_id: u32,
    pub keyboard_ep_target: u32,
    pub tablet_controller_id: u32,
    pub tablet_slot_id: u32,
    pub tablet_ep_target: u32,
}

#[derive(Clone, Debug)]
struct HidHutState {
    mice: Vec<HidMouseState, MAX_HID_HUT_MICE>,
    tablets: Vec<HidTabletState, MAX_HID_HUT_TABLETS>,
    keyboards: Vec<HidKeyboardState, MAX_HID_HUT_KEYBOARDS>,
    combos: Vec<HidCombo, MAX_HID_HUT_COMBOS>,
}

impl HidHutState {
    const fn new() -> Self {
        Self {
            mice: Vec::new(),
            tablets: Vec::new(),
            keyboards: Vec::new(),
            combos: Vec::new(),
        }
    }
}

#[derive(Clone)]
struct ResolvedBinding {
    combo_id: u32,
    source_kind: HidSourceKind,
    source_tag: String<HID_SOURCE_TAG_MAX>,
}

static HID_HUT: Mutex<HidHutState> = Mutex::new(HidHutState::new());

fn normalized_tag(value: &str) -> String<HID_SOURCE_TAG_MAX> {
    let mut out = String::new();
    for ch in value.chars() {
        if out.push(ch).is_err() {
            break;
        }
    }
    out
}

fn resolve_mouse_binding(
    state: &HidHutState,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    fallback_kind: HidSourceKind,
    fallback_tag: &str,
) -> ResolvedBinding {
    if let Some(combo) = state.combos.iter().find(|combo| {
        combo.mouse_controller_id == controller_id
            && combo.mouse_slot_id == slot_id
            && combo.mouse_ep_target == ep_target
    }) {
        return ResolvedBinding {
            combo_id: combo.combo_id,
            source_kind: combo.source_kind,
            source_tag: combo.source_tag.clone(),
        };
    }
    ResolvedBinding {
        combo_id: 0,
        source_kind: fallback_kind,
        source_tag: normalized_tag(fallback_tag),
    }
}

fn resolve_tablet_binding(
    state: &HidHutState,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    fallback_kind: HidSourceKind,
    fallback_tag: &str,
) -> ResolvedBinding {
    if let Some(combo) = state.combos.iter().find(|combo| {
        combo.tablet_controller_id == controller_id
            && combo.tablet_slot_id == slot_id
            && combo.tablet_ep_target == ep_target
    }) {
        return ResolvedBinding {
            combo_id: combo.combo_id,
            source_kind: combo.source_kind,
            source_tag: combo.source_tag.clone(),
        };
    }
    ResolvedBinding {
        combo_id: 0,
        source_kind: fallback_kind,
        source_tag: normalized_tag(fallback_tag),
    }
}

fn resolve_keyboard_binding(
    state: &HidHutState,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    fallback_kind: HidSourceKind,
    fallback_tag: &str,
) -> ResolvedBinding {
    if let Some(combo) = state.combos.iter().find(|combo| {
        combo.keyboard_controller_id == controller_id
            && combo.keyboard_slot_id == slot_id
            && combo.keyboard_ep_target == ep_target
    }) {
        return ResolvedBinding {
            combo_id: combo.combo_id,
            source_kind: combo.source_kind,
            source_tag: combo.source_tag.clone(),
        };
    }
    ResolvedBinding {
        combo_id: 0,
        source_kind: fallback_kind,
        source_tag: normalized_tag(fallback_tag),
    }
}

pub fn upsert_mouse_state(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    x: f64,
    y: f64,
    buttons_down: u32,
    source_kind: HidSourceKind,
    source_tag: &str,
    virtual_device: bool,
) {
    let mut guard = HID_HUT.lock();
    let binding =
        resolve_mouse_binding(&guard, controller_id, slot_id, ep_target, source_kind, source_tag);
    if let Some(existing) = guard.mice.iter_mut().find(|mouse| {
        mouse.controller_id == controller_id
            && mouse.slot_id == slot_id
            && mouse.ep_target == ep_target
    }) {
        existing.x = x;
        existing.y = y;
        existing.buttons_down = buttons_down;
        existing.combo_id = binding.combo_id;
        existing.source_kind = binding.source_kind;
        existing.source_tag = binding.source_tag.clone();
        existing.virtual_device = virtual_device;
        return;
    }

    let next = HidMouseState {
        controller_id,
        slot_id,
        ep_target,
        x,
        y,
        buttons_down,
        combo_id: binding.combo_id,
        source_kind: binding.source_kind,
        source_tag: binding.source_tag,
        virtual_device,
    };
    if guard.mice.push(next.clone()).is_ok() {
        return;
    }
    if !guard.mice.is_empty() {
        let last = guard.mice.len() - 1;
        guard.mice[last] = next;
    }
}

pub fn upsert_tablet_state(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    x: f64,
    y: f64,
    x_raw: u16,
    y_raw: u16,
    buttons_down: u32,
    report_id: u8,
    source_kind: HidSourceKind,
    source_tag: &str,
    virtual_device: bool,
) {
    let mut guard = HID_HUT.lock();
    let binding =
        resolve_tablet_binding(&guard, controller_id, slot_id, ep_target, source_kind, source_tag);
    if let Some(existing) = guard.tablets.iter_mut().find(|tablet| {
        tablet.controller_id == controller_id
            && tablet.slot_id == slot_id
            && tablet.ep_target == ep_target
    }) {
        existing.x = x;
        existing.y = y;
        existing.x_raw = x_raw;
        existing.y_raw = y_raw;
        existing.buttons_down = buttons_down;
        existing.report_id = report_id;
        existing.combo_id = binding.combo_id;
        existing.source_kind = binding.source_kind;
        existing.source_tag = binding.source_tag.clone();
        existing.virtual_device = virtual_device;
        return;
    }

    let next = HidTabletState {
        controller_id,
        slot_id,
        ep_target,
        x,
        y,
        x_raw,
        y_raw,
        buttons_down,
        report_id,
        combo_id: binding.combo_id,
        source_kind: binding.source_kind,
        source_tag: binding.source_tag,
        virtual_device,
    };
    if guard.tablets.push(next.clone()).is_ok() {
        return;
    }
    if !guard.tablets.is_empty() {
        let last = guard.tablets.len() - 1;
        guard.tablets[last] = next;
    }
}

pub fn upsert_keyboard_state(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    modifiers: u8,
    keys: [u8; 6],
    ascii: [u8; 6],
    source_kind: HidSourceKind,
    source_tag: &str,
    virtual_device: bool,
) {
    let mut guard = HID_HUT.lock();
    let binding = resolve_keyboard_binding(
        &guard,
        controller_id,
        slot_id,
        ep_target,
        source_kind,
        source_tag,
    );
    if let Some(existing) = guard.keyboards.iter_mut().find(|keyboard| {
        keyboard.controller_id == controller_id
            && keyboard.slot_id == slot_id
            && keyboard.ep_target == ep_target
    }) {
        existing.modifiers = modifiers;
        existing.keys = keys;
        existing.ascii = ascii;
        existing.combo_id = binding.combo_id;
        existing.source_kind = binding.source_kind;
        existing.source_tag = binding.source_tag.clone();
        existing.virtual_device = virtual_device;
        return;
    }

    let next = HidKeyboardState {
        controller_id,
        slot_id,
        ep_target,
        modifiers,
        keys,
        ascii,
        combo_id: binding.combo_id,
        source_kind: binding.source_kind,
        source_tag: binding.source_tag,
        virtual_device,
    };
    if guard.keyboards.push(next.clone()).is_ok() {
        return;
    }
    if !guard.keyboards.is_empty() {
        let last = guard.keyboards.len() - 1;
        guard.keyboards[last] = next;
    }
}

pub fn upsert_combo(combo_id: u32, source_kind: HidSourceKind, source_tag: &str) -> bool {
    if combo_id == 0 {
        return false;
    }
    let mut guard = HID_HUT.lock();
    if let Some(existing) = guard
        .combos
        .iter_mut()
        .find(|combo| combo.combo_id == combo_id)
    {
        existing.source_kind = source_kind;
        existing.source_tag = normalized_tag(source_tag);
        return true;
    }

    let next = HidCombo {
        combo_id,
        source_kind,
        source_tag: normalized_tag(source_tag),
        mouse_controller_id: 0,
        mouse_slot_id: 0,
        mouse_ep_target: 0,
        keyboard_controller_id: 0,
        keyboard_slot_id: 0,
        keyboard_ep_target: 0,
        tablet_controller_id: 0,
        tablet_slot_id: 0,
        tablet_ep_target: 0,
    };
    guard.combos.push(next).is_ok()
}

pub fn bind_combo_mouse(combo_id: u32, controller_id: u32, slot_id: u32, ep_target: u32) -> bool {
    if combo_id == 0 {
        return false;
    }
    let mut guard = HID_HUT.lock();
    let Some(combo) = guard
        .combos
        .iter_mut()
        .find(|combo| combo.combo_id == combo_id)
    else {
        return false;
    };
    combo.mouse_controller_id = controller_id;
    combo.mouse_slot_id = slot_id;
    combo.mouse_ep_target = ep_target;
    true
}

pub fn bind_combo_keyboard(
    combo_id: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
) -> bool {
    if combo_id == 0 {
        return false;
    }
    let mut guard = HID_HUT.lock();
    let Some(combo) = guard
        .combos
        .iter_mut()
        .find(|combo| combo.combo_id == combo_id)
    else {
        return false;
    };
    combo.keyboard_controller_id = controller_id;
    combo.keyboard_slot_id = slot_id;
    combo.keyboard_ep_target = ep_target;
    true
}

pub fn bind_combo_tablet(combo_id: u32, controller_id: u32, slot_id: u32, ep_target: u32) -> bool {
    if combo_id == 0 {
        return false;
    }
    let mut guard = HID_HUT.lock();
    let Some(combo) = guard
        .combos
        .iter_mut()
        .find(|combo| combo.combo_id == combo_id)
    else {
        return false;
    };
    combo.tablet_controller_id = controller_id;
    combo.tablet_slot_id = slot_id;
    combo.tablet_ep_target = ep_target;
    true
}

pub fn remove_slot(controller_id: u32, slot_id: u32) -> bool {
    let mut guard = HID_HUT.lock();
    let mut removed = false;

    let mut idx = 0usize;
    while idx < guard.mice.len() {
        if guard.mice[idx].controller_id == controller_id && guard.mice[idx].slot_id == slot_id {
            let _ = guard.mice.remove(idx);
            removed = true;
        } else {
            idx += 1;
        }
    }

    let mut idx = 0usize;
    while idx < guard.keyboards.len() {
        if guard.keyboards[idx].controller_id == controller_id
            && guard.keyboards[idx].slot_id == slot_id
        {
            let _ = guard.keyboards.remove(idx);
            removed = true;
        } else {
            idx += 1;
        }
    }

    let mut idx = 0usize;
    while idx < guard.tablets.len() {
        if guard.tablets[idx].controller_id == controller_id
            && guard.tablets[idx].slot_id == slot_id
        {
            let _ = guard.tablets.remove(idx);
            removed = true;
        } else {
            idx += 1;
        }
    }

    removed
}

pub fn mice_snapshot() -> Vec<HidMouseState, MAX_HID_HUT_MICE> {
    HID_HUT.lock().mice.clone()
}

pub fn tablets_snapshot() -> Vec<HidTabletState, MAX_HID_HUT_TABLETS> {
    HID_HUT.lock().tablets.clone()
}

pub fn keyboards_snapshot() -> Vec<HidKeyboardState, MAX_HID_HUT_KEYBOARDS> {
    HID_HUT.lock().keyboards.clone()
}

pub fn combos_snapshot() -> Vec<HidCombo, MAX_HID_HUT_COMBOS> {
    HID_HUT.lock().combos.clone()
}

#[inline]
fn copy_source_tag(
    out: &mut [u8; HID_HUT_SOURCE_TAG_MAX],
    value: &String<HID_SOURCE_TAG_MAX>,
) -> u8 {
    *out = [0; HID_HUT_SOURCE_TAG_MAX];
    let bytes = value.as_bytes();
    let len = core::cmp::min(bytes.len(), HID_HUT_SOURCE_TAG_MAX);
    out[..len].copy_from_slice(&bytes[..len]);
    len as u8
}

pub fn read_mice_snapshot(out: &mut [TrueosHidHutMouseState]) -> usize {
    let snapshot = mice_snapshot();
    let mut wrote = 0usize;
    for mouse in snapshot.iter().take(out.len()) {
        let mut next = TrueosHidHutMouseState {
            controller_id: mouse.controller_id,
            slot_id: mouse.slot_id,
            ep_target: mouse.ep_target,
            buttons_down: mouse.buttons_down,
            combo_id: mouse.combo_id,
            source_kind: mouse.source_kind as u8,
            virtual_device: u8::from(mouse.virtual_device),
            source_tag_len: 0,
            reserved0: 0,
            source_tag: [0; HID_HUT_SOURCE_TAG_MAX],
            x: mouse.x,
            y: mouse.y,
        };
        next.source_tag_len = copy_source_tag(&mut next.source_tag, &mouse.source_tag);
        out[wrote] = next;
        wrote += 1;
    }
    wrote
}

pub fn read_tablets_snapshot(out: &mut [TrueosHidHutTabletState]) -> usize {
    let snapshot = tablets_snapshot();
    let mut wrote = 0usize;
    for tablet in snapshot.iter().take(out.len()) {
        let mut next = TrueosHidHutTabletState {
            controller_id: tablet.controller_id,
            slot_id: tablet.slot_id,
            ep_target: tablet.ep_target,
            x_raw: tablet.x_raw,
            y_raw: tablet.y_raw,
            buttons_down: tablet.buttons_down,
            report_id: tablet.report_id,
            source_kind: tablet.source_kind as u8,
            virtual_device: u8::from(tablet.virtual_device),
            source_tag_len: 0,
            combo_id: tablet.combo_id,
            source_tag: [0; HID_HUT_SOURCE_TAG_MAX],
            x: tablet.x,
            y: tablet.y,
        };
        next.source_tag_len = copy_source_tag(&mut next.source_tag, &tablet.source_tag);
        out[wrote] = next;
        wrote += 1;
    }
    wrote
}

pub fn read_keyboards_snapshot(out: &mut [TrueosHidHutKeyboardState]) -> usize {
    let snapshot = keyboards_snapshot();
    let mut wrote = 0usize;
    for keyboard in snapshot.iter().take(out.len()) {
        let mut next = TrueosHidHutKeyboardState {
            controller_id: keyboard.controller_id,
            slot_id: keyboard.slot_id,
            ep_target: keyboard.ep_target,
            combo_id: keyboard.combo_id,
            modifiers: keyboard.modifiers,
            source_kind: keyboard.source_kind as u8,
            virtual_device: u8::from(keyboard.virtual_device),
            source_tag_len: 0,
            keys: keyboard.keys,
            ascii: keyboard.ascii,
            source_tag: [0; HID_HUT_SOURCE_TAG_MAX],
        };
        next.source_tag_len = copy_source_tag(&mut next.source_tag, &keyboard.source_tag);
        out[wrote] = next;
        wrote += 1;
    }
    wrote
}

pub fn read_combos_snapshot(out: &mut [TrueosHidHutCombo]) -> usize {
    let snapshot = combos_snapshot();
    let mut wrote = 0usize;
    for combo in snapshot.iter().take(out.len()) {
        let mut next = TrueosHidHutCombo {
            combo_id: combo.combo_id,
            source_kind: combo.source_kind as u8,
            source_tag_len: 0,
            reserved0: 0,
            source_tag: [0; HID_HUT_SOURCE_TAG_MAX],
            mouse_controller_id: combo.mouse_controller_id,
            mouse_slot_id: combo.mouse_slot_id,
            mouse_ep_target: combo.mouse_ep_target,
            keyboard_controller_id: combo.keyboard_controller_id,
            keyboard_slot_id: combo.keyboard_slot_id,
            keyboard_ep_target: combo.keyboard_ep_target,
            tablet_controller_id: combo.tablet_controller_id,
            tablet_slot_id: combo.tablet_slot_id,
            tablet_ep_target: combo.tablet_ep_target,
        };
        next.source_tag_len = copy_source_tag(&mut next.source_tag, &combo.source_tag);
        out[wrote] = next;
        wrote += 1;
    }
    wrote
}
