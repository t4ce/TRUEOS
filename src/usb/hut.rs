use heapless::{String, Vec};
use spin::Mutex;

const MAX_HID_HUT_MICE: usize = 32;
const MAX_HID_HUT_KEYBOARDS: usize = 32;
const MAX_HID_HUT_COMBOS: usize = 32;
const HID_SOURCE_TAG_MAX: usize = 32;

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
}

#[derive(Clone, Debug)]
struct HidHutState {
    mice: Vec<HidMouseState, MAX_HID_HUT_MICE>,
    keyboards: Vec<HidKeyboardState, MAX_HID_HUT_KEYBOARDS>,
    combos: Vec<HidCombo, MAX_HID_HUT_COMBOS>,
}

impl HidHutState {
    const fn new() -> Self {
        Self {
            mice: Vec::new(),
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
    let binding = resolve_mouse_binding(
        &guard,
        controller_id,
        slot_id,
        ep_target,
        source_kind,
        source_tag,
    );
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
    if let Some(existing) = guard.combos.iter_mut().find(|combo| combo.combo_id == combo_id) {
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
    };
    guard.combos.push(next).is_ok()
}

pub fn bind_combo_mouse(combo_id: u32, controller_id: u32, slot_id: u32, ep_target: u32) -> bool {
    if combo_id == 0 {
        return false;
    }
    let mut guard = HID_HUT.lock();
    let Some(combo) = guard.combos.iter_mut().find(|combo| combo.combo_id == combo_id) else {
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
    let Some(combo) = guard.combos.iter_mut().find(|combo| combo.combo_id == combo_id) else {
        return false;
    };
    combo.keyboard_controller_id = controller_id;
    combo.keyboard_slot_id = slot_id;
    combo.keyboard_ep_target = ep_target;
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

    removed
}

pub fn mice_snapshot() -> Vec<HidMouseState, MAX_HID_HUT_MICE> {
    HID_HUT.lock().mice.clone()
}

pub fn keyboards_snapshot() -> Vec<HidKeyboardState, MAX_HID_HUT_KEYBOARDS> {
    HID_HUT.lock().keyboards.clone()
}

pub fn combos_snapshot() -> Vec<HidCombo, MAX_HID_HUT_COMBOS> {
    HID_HUT.lock().combos.clone()
}
