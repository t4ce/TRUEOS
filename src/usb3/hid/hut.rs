//! HID Usage Tables input state and input-combo management.

use core::sync::atomic::{AtomicU32, Ordering};
use heapless::{String, Vec};
use spin::Mutex;

const MAX_HID_HUT_MICE: usize = 32;
const MAX_HID_HUT_TABLETS: usize = 32;
const MAX_HID_HUT_KEYBOARDS: usize = 32;
const MAX_HID_HUT_GAMEPADS: usize = 32;
const MAX_HID_HUT_COMBOS: usize = 32;
pub const HID_HUT_MAX_DEVICES: usize =
    MAX_HID_HUT_MICE + MAX_HID_HUT_TABLETS + MAX_HID_HUT_KEYBOARDS + MAX_HID_HUT_GAMEPADS;
pub const HID_HUT_SOURCE_TAG_MAX: usize = 32;
const HID_SOURCE_TAG_MAX: usize = HID_HUT_SOURCE_TAG_MAX;
pub const INPUT_COMBO_FLAG_AUTO_ASSIGNED: u8 = v::vinput::INPUT_COMBO_FLAG_AUTO_ASSIGNED;
const AUTO_COMBO_ID_BASE: u32 = 0x4943_0000;
static NEXT_AUTO_COMBO_ID: AtomicU32 = AtomicU32::new(1);

pub use v::vinput::InputComboSourceKind;
pub type HidSourceKind = InputComboSourceKind;

/// The live device classes that can be moved between input buckets.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum HidDeviceKind {
    Mouse,
    Keyboard,
    Tablet,
    Gamepad,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct HidDeviceId {
    pub kind: HidDeviceKind,
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HidDevice {
    pub id: HidDeviceId,
    pub combo_id: u32,
    pub source_kind: HidSourceKind,
    pub source_tag: String<HID_SOURCE_TAG_MAX>,
    pub virtual_device: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ComboMoveError {
    DeviceMissing,
    ComboMissing,
    CursorOccupied,
    DeviceKindOccupied,
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
    pub key_down_bits: [u32; 8],
    pub combo_id: u32,
    pub source_kind: HidSourceKind,
    pub source_tag: String<HID_SOURCE_TAG_MAX>,
    pub virtual_device: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputCombo {
    pub combo_id: u32,
    pub source_kind: HidSourceKind,
    pub source_tag: String<HID_SOURCE_TAG_MAX>,
    pub color_id: u8,
    pub flags: u8,
    pub mouse_controller_id: u32,
    pub mouse_slot_id: u32,
    pub mouse_ep_target: u32,
    pub keyboard_controller_id: u32,
    pub keyboard_slot_id: u32,
    pub keyboard_ep_target: u32,
    pub tablet_controller_id: u32,
    pub tablet_slot_id: u32,
    pub tablet_ep_target: u32,
    pub gamepad_controller_id: u32,
    pub gamepad_slot_id: u32,
    pub gamepad_ep_target: u32,
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
    pub key_down_bits: [u32; 8],
    pub source_tag: [u8; HID_HUT_SOURCE_TAG_MAX],
}

const _: () = assert!(core::mem::size_of::<TrueosHidHutKeyboardState>() == 96);

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
            key_down_bits: [0; 8],
            source_tag: [0; HID_HUT_SOURCE_TAG_MAX],
        }
    }
}

#[derive(Clone, Debug)]
struct HidHutState {
    mice: Vec<HidMouseState, MAX_HID_HUT_MICE>,
    tablets: Vec<HidTabletState, MAX_HID_HUT_TABLETS>,
    keyboards: Vec<HidKeyboardState, MAX_HID_HUT_KEYBOARDS>,
    gamepads: Vec<HidDevice, MAX_HID_HUT_GAMEPADS>,
    combos: Vec<InputCombo, MAX_HID_HUT_COMBOS>,
}

impl HidHutState {
    const fn new() -> Self {
        Self {
            mice: Vec::new(),
            tablets: Vec::new(),
            keyboards: Vec::new(),
            gamepads: Vec::new(),
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ComboDeviceKind {
    Mouse,
    Keyboard,
    Tablet,
    Gamepad,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ComboEndpoint {
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
}

impl ComboEndpoint {
    const fn new(controller_id: u32, slot_id: u32, ep_target: u32) -> Self {
        Self {
            controller_id,
            slot_id,
            ep_target,
        }
    }
}

fn combo_endpoint(combo: &InputCombo, kind: ComboDeviceKind) -> ComboEndpoint {
    match kind {
        ComboDeviceKind::Mouse => ComboEndpoint::new(
            combo.mouse_controller_id,
            combo.mouse_slot_id,
            combo.mouse_ep_target,
        ),
        ComboDeviceKind::Keyboard => ComboEndpoint::new(
            combo.keyboard_controller_id,
            combo.keyboard_slot_id,
            combo.keyboard_ep_target,
        ),
        ComboDeviceKind::Tablet => ComboEndpoint::new(
            combo.tablet_controller_id,
            combo.tablet_slot_id,
            combo.tablet_ep_target,
        ),
        ComboDeviceKind::Gamepad => ComboEndpoint::new(
            combo.gamepad_controller_id,
            combo.gamepad_slot_id,
            combo.gamepad_ep_target,
        ),
    }
}

fn set_combo_endpoint(combo: &mut InputCombo, kind: ComboDeviceKind, endpoint: ComboEndpoint) {
    match kind {
        ComboDeviceKind::Mouse => {
            combo.mouse_controller_id = endpoint.controller_id;
            combo.mouse_slot_id = endpoint.slot_id;
            combo.mouse_ep_target = endpoint.ep_target;
        }
        ComboDeviceKind::Keyboard => {
            combo.keyboard_controller_id = endpoint.controller_id;
            combo.keyboard_slot_id = endpoint.slot_id;
            combo.keyboard_ep_target = endpoint.ep_target;
        }
        ComboDeviceKind::Tablet => {
            combo.tablet_controller_id = endpoint.controller_id;
            combo.tablet_slot_id = endpoint.slot_id;
            combo.tablet_ep_target = endpoint.ep_target;
        }
        ComboDeviceKind::Gamepad => {
            combo.gamepad_controller_id = endpoint.controller_id;
            combo.gamepad_slot_id = endpoint.slot_id;
            combo.gamepad_ep_target = endpoint.ep_target;
        }
    }
}

#[inline]
fn endpoint_is_empty(endpoint: ComboEndpoint) -> bool {
    endpoint.controller_id == 0 && endpoint.slot_id == 0 && endpoint.ep_target == 0
}

#[inline]
fn endpoint_matches(left: ComboEndpoint, right: ComboEndpoint) -> bool {
    left.controller_id == right.controller_id
        && left.slot_id == right.slot_id
        && left.ep_target == right.ep_target
}

/// Mouse and tablet each produce a cursor, so they share one bucket slot.
fn other_cursor_endpoint(combo: &InputCombo, kind: ComboDeviceKind) -> ComboEndpoint {
    match kind {
        ComboDeviceKind::Mouse => combo_endpoint(combo, ComboDeviceKind::Tablet),
        ComboDeviceKind::Tablet => combo_endpoint(combo, ComboDeviceKind::Mouse),
        _ => ComboEndpoint::new(0, 0, 0),
    }
}

fn combo_is_empty(combo: &InputCombo) -> bool {
    [
        ComboDeviceKind::Mouse,
        ComboDeviceKind::Keyboard,
        ComboDeviceKind::Tablet,
        ComboDeviceKind::Gamepad,
    ]
    .into_iter()
    .all(|kind| endpoint_is_empty(combo_endpoint(combo, kind)))
}

fn combo_contains_usb_slot(combo: &InputCombo, controller_id: u32, slot_id: u32) -> bool {
    [
        ComboDeviceKind::Mouse,
        ComboDeviceKind::Keyboard,
        ComboDeviceKind::Tablet,
        ComboDeviceKind::Gamepad,
    ]
    .into_iter()
    .map(|kind| combo_endpoint(combo, kind))
    .any(|endpoint| {
        !endpoint_is_empty(endpoint)
            && endpoint.controller_id == controller_id
            && endpoint.slot_id == slot_id
    })
}

fn next_combo_color(state: &HidHutState) -> u8 {
    for color_id in 0..v::vinput::InputComboColor::COUNT {
        if !state.combos.iter().any(|combo| combo.color_id == color_id) {
            return color_id;
        }
    }
    (state.combos.len() as u8) % v::vinput::InputComboColor::COUNT
}

fn next_combo_id(state: &HidHutState) -> Option<u32> {
    for _ in 0..(MAX_HID_HUT_COMBOS * 2) {
        let sequence = NEXT_AUTO_COMBO_ID.fetch_add(1, Ordering::AcqRel).max(1);
        let combo_id = AUTO_COMBO_ID_BASE | (sequence & 0x0000_FFFF);
        if combo_id != 0 && !state.combos.iter().any(|combo| combo.combo_id == combo_id) {
            return Some(combo_id);
        }
    }
    None
}

fn empty_combo(
    combo_id: u32,
    source_kind: HidSourceKind,
    source_tag: &str,
    color_id: u8,
    flags: u8,
) -> InputCombo {
    InputCombo {
        combo_id,
        source_kind,
        source_tag: normalized_tag(source_tag),
        color_id: color_id % v::vinput::InputComboColor::COUNT,
        flags,
        mouse_controller_id: 0,
        mouse_slot_id: 0,
        mouse_ep_target: 0,
        keyboard_controller_id: 0,
        keyboard_slot_id: 0,
        keyboard_ep_target: 0,
        tablet_controller_id: 0,
        tablet_slot_id: 0,
        tablet_ep_target: 0,
        gamepad_controller_id: 0,
        gamepad_slot_id: 0,
        gamepad_ep_target: 0,
    }
}

fn bind_device_in_state(
    state: &mut HidHutState,
    combo_id: u32,
    kind: ComboDeviceKind,
    endpoint: ComboEndpoint,
) -> bool {
    let Some(target_index) = state
        .combos
        .iter()
        .position(|combo| combo.combo_id == combo_id)
    else {
        return false;
    };
    if !endpoint_is_empty(other_cursor_endpoint(&state.combos[target_index], kind)) {
        return false;
    }
    let displaced_endpoint = combo_endpoint(&state.combos[target_index], kind);

    // A device belongs to at most one collection. This makes manual re-pairing
    // deterministic and prevents one keyboard from routing to two cursors.
    for combo in state.combos.iter_mut() {
        if endpoint_matches(combo_endpoint(combo, kind), endpoint) {
            set_combo_endpoint(combo, kind, ComboEndpoint::new(0, 0, 0));
        }
    }
    set_combo_endpoint(&mut state.combos[target_index], kind, endpoint);
    let source_kind = state.combos[target_index].source_kind;
    let source_tag = state.combos[target_index].source_tag.clone();
    match kind {
        ComboDeviceKind::Mouse => {
            if !endpoint_is_empty(displaced_endpoint)
                && !endpoint_matches(displaced_endpoint, endpoint)
                && let Some(mouse) = state.mice.iter_mut().find(|mouse| {
                    mouse.controller_id == displaced_endpoint.controller_id
                        && mouse.slot_id == displaced_endpoint.slot_id
                        && mouse.ep_target == displaced_endpoint.ep_target
                })
                && mouse.combo_id == combo_id
            {
                mouse.combo_id = 0;
            }
            if let Some(mouse) = state.mice.iter_mut().find(|mouse| {
                mouse.controller_id == endpoint.controller_id
                    && mouse.slot_id == endpoint.slot_id
                    && mouse.ep_target == endpoint.ep_target
            }) {
                mouse.combo_id = combo_id;
                mouse.source_kind = source_kind;
                mouse.source_tag = source_tag;
            }
        }
        ComboDeviceKind::Keyboard => {
            if !endpoint_is_empty(displaced_endpoint)
                && !endpoint_matches(displaced_endpoint, endpoint)
                && let Some(keyboard) = state.keyboards.iter_mut().find(|keyboard| {
                    keyboard.controller_id == displaced_endpoint.controller_id
                        && keyboard.slot_id == displaced_endpoint.slot_id
                        && keyboard.ep_target == displaced_endpoint.ep_target
                })
                && keyboard.combo_id == combo_id
            {
                keyboard.combo_id = 0;
            }
            if let Some(keyboard) = state.keyboards.iter_mut().find(|keyboard| {
                keyboard.controller_id == endpoint.controller_id
                    && keyboard.slot_id == endpoint.slot_id
                    && keyboard.ep_target == endpoint.ep_target
            }) {
                keyboard.combo_id = combo_id;
                keyboard.source_kind = source_kind;
                keyboard.source_tag = source_tag;
            }
        }
        ComboDeviceKind::Tablet => {
            if !endpoint_is_empty(displaced_endpoint)
                && !endpoint_matches(displaced_endpoint, endpoint)
                && let Some(tablet) = state.tablets.iter_mut().find(|tablet| {
                    tablet.controller_id == displaced_endpoint.controller_id
                        && tablet.slot_id == displaced_endpoint.slot_id
                        && tablet.ep_target == displaced_endpoint.ep_target
                })
                && tablet.combo_id == combo_id
            {
                tablet.combo_id = 0;
            }
            if let Some(tablet) = state.tablets.iter_mut().find(|tablet| {
                tablet.controller_id == endpoint.controller_id
                    && tablet.slot_id == endpoint.slot_id
                    && tablet.ep_target == endpoint.ep_target
            }) {
                tablet.combo_id = combo_id;
                tablet.source_kind = source_kind;
                tablet.source_tag = source_tag;
            }
        }
        ComboDeviceKind::Gamepad => {
            if !endpoint_is_empty(displaced_endpoint)
                && !endpoint_matches(displaced_endpoint, endpoint)
                && let Some(gamepad) = state.gamepads.iter_mut().find(|gamepad| {
                    gamepad.id.controller_id == displaced_endpoint.controller_id
                        && gamepad.id.slot_id == displaced_endpoint.slot_id
                        && gamepad.id.ep_target == displaced_endpoint.ep_target
                })
                && gamepad.combo_id == combo_id
            {
                gamepad.combo_id = 0;
            }
            if let Some(gamepad) = state.gamepads.iter_mut().find(|gamepad| {
                gamepad.id.controller_id == endpoint.controller_id
                    && gamepad.id.slot_id == endpoint.slot_id
                    && gamepad.id.ep_target == endpoint.ep_target
            }) {
                gamepad.combo_id = combo_id;
                gamepad.source_kind = source_kind;
                gamepad.source_tag = source_tag;
            }
        }
    }
    true
}

fn ensure_auto_combo_binding(
    state: &mut HidHutState,
    kind: ComboDeviceKind,
    endpoint: ComboEndpoint,
) {
    if state
        .combos
        .iter()
        .any(|combo| endpoint_matches(combo_endpoint(combo, kind), endpoint))
    {
        return;
    }

    // Composite devices sharing one USB slot are the strongest available
    // evidence. For independent devices, discovery order is intentionally only
    // a best-effort boot pairing until a user-facing Combo app stores policy.
    let same_slot = state.combos.iter().position(|combo| {
        combo.flags & INPUT_COMBO_FLAG_AUTO_ASSIGNED != 0
            && endpoint_is_empty(combo_endpoint(combo, kind))
            && endpoint_is_empty(other_cursor_endpoint(combo, kind))
            && combo_contains_usb_slot(combo, endpoint.controller_id, endpoint.slot_id)
    });
    let best_effort = state.combos.iter().position(|combo| {
        combo.flags & INPUT_COMBO_FLAG_AUTO_ASSIGNED != 0
            && endpoint_is_empty(combo_endpoint(combo, kind))
            && endpoint_is_empty(other_cursor_endpoint(combo, kind))
    });
    let target_index = same_slot.or(best_effort).or_else(|| {
        let combo_id = next_combo_id(state)?;
        let color_id = next_combo_color(state);
        state
            .combos
            .push(empty_combo(
                combo_id,
                HidSourceKind::Human,
                "auto",
                color_id,
                INPUT_COMBO_FLAG_AUTO_ASSIGNED,
            ))
            .ok()?;
        Some(state.combos.len() - 1)
    });

    if let Some(index) = target_index {
        let combo_id = state.combos[index].combo_id;
        let _ = bind_device_in_state(state, combo_id, kind, endpoint);
    }
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
    if !virtual_device && source_kind == HidSourceKind::Human {
        ensure_auto_combo_binding(
            &mut guard,
            ComboDeviceKind::Mouse,
            ComboEndpoint::new(controller_id, slot_id, ep_target),
        );
    }
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
    if !virtual_device && source_kind == HidSourceKind::Human {
        ensure_auto_combo_binding(
            &mut guard,
            ComboDeviceKind::Tablet,
            ComboEndpoint::new(controller_id, slot_id, ep_target),
        );
    }
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
    let key_down_bits = keyboard_key_down_bits(modifiers, keys);
    let mut guard = HID_HUT.lock();
    if !virtual_device && source_kind == HidSourceKind::Human {
        ensure_auto_combo_binding(
            &mut guard,
            ComboDeviceKind::Keyboard,
            ComboEndpoint::new(controller_id, slot_id, ep_target),
        );
    }
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
        existing.key_down_bits = key_down_bits;
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
        key_down_bits,
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

fn keyboard_key_down_bits(modifiers: u8, keys: [u8; 6]) -> [u32; 8] {
    let mut bits = [0u32; 8];
    for bit in 0..8u8 {
        if (modifiers & (1u8 << bit)) != 0 {
            let code = 0xE0u8 + bit;
            bits[(code / 32) as usize] |= 1u32 << (code % 32);
        }
    }
    for key in keys {
        if key == 0 {
            continue;
        }
        bits[(key / 32) as usize] |= 1u32 << (key % 32);
    }
    bits
}

/// Enroll an existing mediated gamepad without injecting an input report.
/// Button and axis state remain in the gamepad service; HUT stores pairing
/// metadata only, so Link snapshots remain stable while a program runs.
pub fn register_gamepad_device(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    source_kind: HidSourceKind,
    source_tag: &str,
    virtual_device: bool,
) -> bool {
    if slot_id == 0 {
        return false;
    }
    let id = HidDeviceId {
        kind: HidDeviceKind::Gamepad,
        controller_id,
        slot_id,
        ep_target,
    };
    let mut guard = HID_HUT.lock();
    // Capacity errors must not leave an endpoint reservation behind.
    if !guard.gamepads.iter().any(|device| device.id == id) && guard.gamepads.is_full() {
        return false;
    }
    let endpoint = ComboEndpoint::new(controller_id, slot_id, ep_target);
    if !virtual_device && source_kind == HidSourceKind::Human {
        ensure_auto_combo_binding(&mut guard, ComboDeviceKind::Gamepad, endpoint);
    }
    let combo = guard
        .combos
        .iter()
        .find(|combo| endpoint_matches(combo_endpoint(combo, ComboDeviceKind::Gamepad), endpoint));
    let device = HidDevice {
        id,
        combo_id: combo.map_or(0, |combo| combo.combo_id),
        source_kind: combo.map_or(source_kind, |combo| combo.source_kind),
        source_tag: combo.map_or_else(
            || normalized_tag(source_tag),
            |combo| combo.source_tag.clone(),
        ),
        virtual_device,
    };
    if let Some(existing) = guard.gamepads.iter_mut().find(|device| device.id == id) {
        *existing = device;
        true
    } else {
        guard.gamepads.push(device).is_ok()
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
        existing.flags &= !INPUT_COMBO_FLAG_AUTO_ASSIGNED;
        return true;
    }

    let color_id = next_combo_color(&guard);
    let next = empty_combo(combo_id, source_kind, source_tag, color_id, 0);
    guard.combos.push(next).is_ok()
}

pub fn request_combo(
    source_kind: HidSourceKind,
    source_tag: &str,
    requested_color: Option<u8>,
) -> Option<InputCombo> {
    let mut guard = HID_HUT.lock();
    let combo_id = next_combo_id(&guard)?;
    let color_id = requested_color
        .filter(|color| *color < v::vinput::InputComboColor::COUNT)
        .unwrap_or_else(|| next_combo_color(&guard));
    let combo = empty_combo(combo_id, source_kind, source_tag, color_id, 0);
    guard.combos.push(combo.clone()).ok()?;
    Some(combo)
}

pub fn set_combo_color(combo_id: u32, color_id: u8) -> bool {
    if combo_id == 0 || color_id >= v::vinput::InputComboColor::COUNT {
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
    combo.color_id = color_id;
    true
}

pub fn bind_combo_mouse(combo_id: u32, controller_id: u32, slot_id: u32, ep_target: u32) -> bool {
    if combo_id == 0 || slot_id == 0 {
        return false;
    }
    let mut guard = HID_HUT.lock();
    bind_device_in_state(
        &mut guard,
        combo_id,
        ComboDeviceKind::Mouse,
        ComboEndpoint::new(controller_id, slot_id, ep_target),
    )
}

pub fn bind_combo_keyboard(
    combo_id: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
) -> bool {
    if combo_id == 0 || slot_id == 0 {
        return false;
    }
    let mut guard = HID_HUT.lock();
    bind_device_in_state(
        &mut guard,
        combo_id,
        ComboDeviceKind::Keyboard,
        ComboEndpoint::new(controller_id, slot_id, ep_target),
    )
}

pub fn bind_combo_tablet(combo_id: u32, controller_id: u32, slot_id: u32, ep_target: u32) -> bool {
    if combo_id == 0 || slot_id == 0 {
        return false;
    }
    let mut guard = HID_HUT.lock();
    bind_device_in_state(
        &mut guard,
        combo_id,
        ComboDeviceKind::Tablet,
        ComboEndpoint::new(controller_id, slot_id, ep_target),
    )
}

pub fn bind_combo_gamepad(combo_id: u32, controller_id: u32, slot_id: u32, ep_target: u32) -> bool {
    if combo_id == 0 || slot_id == 0 {
        return false;
    }
    let mut guard = HID_HUT.lock();
    bind_device_in_state(
        &mut guard,
        combo_id,
        ComboDeviceKind::Gamepad,
        ComboEndpoint::new(controller_id, slot_id, ep_target),
    )
}

fn move_device_in_state(
    state: &mut HidHutState,
    device: HidDeviceId,
    combo_id: u32,
) -> Result<(), ComboMoveError> {
    if device.slot_id == 0 {
        return Err(ComboMoveError::DeviceMissing);
    }
    let endpoint = ComboEndpoint::new(device.controller_id, device.slot_id, device.ep_target);
    let kind = match device.kind {
        HidDeviceKind::Mouse => ComboDeviceKind::Mouse,
        HidDeviceKind::Keyboard => ComboDeviceKind::Keyboard,
        HidDeviceKind::Tablet => ComboDeviceKind::Tablet,
        HidDeviceKind::Gamepad => ComboDeviceKind::Gamepad,
    };
    let live = match device.kind {
        HidDeviceKind::Mouse => state.mice.iter().any(|entry| {
            endpoint_matches(
                endpoint,
                ComboEndpoint::new(entry.controller_id, entry.slot_id, entry.ep_target),
            )
        }),
        HidDeviceKind::Keyboard => state.keyboards.iter().any(|entry| {
            endpoint_matches(
                endpoint,
                ComboEndpoint::new(entry.controller_id, entry.slot_id, entry.ep_target),
            )
        }),
        HidDeviceKind::Tablet => state.tablets.iter().any(|entry| {
            endpoint_matches(
                endpoint,
                ComboEndpoint::new(entry.controller_id, entry.slot_id, entry.ep_target),
            )
        }),
        HidDeviceKind::Gamepad => state.gamepads.iter().any(|entry| entry.id == device),
    };
    if !live {
        return Err(ComboMoveError::DeviceMissing);
    }
    let target = state
        .combos
        .iter()
        .find(|combo| combo.combo_id == combo_id)
        .ok_or(ComboMoveError::ComboMissing)?;
    if !endpoint_is_empty(other_cursor_endpoint(target, kind)) {
        return Err(ComboMoveError::CursorOccupied);
    }
    let occupied = combo_endpoint(target, kind);
    if !endpoint_is_empty(occupied) && !endpoint_matches(occupied, endpoint) {
        return Err(match kind {
            ComboDeviceKind::Mouse | ComboDeviceKind::Tablet => ComboMoveError::CursorOccupied,
            _ => ComboMoveError::DeviceKindOccupied,
        });
    }
    let bound = bind_device_in_state(state, combo_id, kind, endpoint);
    debug_assert!(bound);
    // A drop is an explicit pairing choice. Future discovery must leave its
    // remaining slots alone instead of extending this bucket by boot order.
    if let Some(combo) = state
        .combos
        .iter_mut()
        .find(|combo| combo.combo_id == combo_id)
    {
        combo.flags &= !INPUT_COMBO_FLAG_AUTO_ASSIGNED;
    }
    Ok(())
}

/// Move one live device atomically, preserving the old pairing on rejection.
/// A bucket accepts one cursor (mouse or tablet), one keyboard, and one gamepad.
/// Replacing an occupied slot requires first moving its device to another bucket.
pub fn move_device_to_combo(device: HidDeviceId, combo_id: u32) -> Result<(), ComboMoveError> {
    move_device_in_state(&mut HID_HUT.lock(), device, combo_id)
}

/// Delete only an empty bucket, so a stale UI cannot detach live devices.
pub fn remove_empty_combo(combo_id: u32) -> bool {
    let mut guard = HID_HUT.lock();
    let Some(index) = guard
        .combos
        .iter()
        .position(|combo| combo.combo_id == combo_id && combo_is_empty(combo))
    else {
        return false;
    };
    let _ = guard.combos.remove(index);
    true
}

pub fn remove_combo(combo_id: u32) -> bool {
    if combo_id == 0 {
        return false;
    }
    let mut guard = HID_HUT.lock();
    let Some(index) = guard
        .combos
        .iter()
        .position(|combo| combo.combo_id == combo_id)
    else {
        return false;
    };
    let _ = guard.combos.remove(index);
    for mouse in guard.mice.iter_mut() {
        if mouse.combo_id == combo_id {
            mouse.combo_id = 0;
        }
    }
    for keyboard in guard.keyboards.iter_mut() {
        if keyboard.combo_id == combo_id {
            keyboard.combo_id = 0;
        }
    }
    for tablet in guard.tablets.iter_mut() {
        if tablet.combo_id == combo_id {
            tablet.combo_id = 0;
        }
    }
    for gamepad in guard.gamepads.iter_mut() {
        if gamepad.combo_id == combo_id {
            gamepad.combo_id = 0;
        }
    }
    true
}

pub fn combo_color_for_cursor(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    tablet: bool,
) -> Option<u8> {
    let kind = if tablet {
        ComboDeviceKind::Tablet
    } else {
        ComboDeviceKind::Mouse
    };
    let endpoint = ComboEndpoint::new(controller_id, slot_id, ep_target);
    HID_HUT
        .lock()
        .combos
        .iter()
        .find(|combo| endpoint_matches(combo_endpoint(combo, kind), endpoint))
        .map(|combo| combo.color_id)
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

    let mut idx = 0usize;
    while idx < guard.gamepads.len() {
        if guard.gamepads[idx].id.controller_id == controller_id
            && guard.gamepads[idx].id.slot_id == slot_id
        {
            let _ = guard.gamepads.remove(idx);
            removed = true;
        } else {
            idx += 1;
        }
    }

    for combo in guard.combos.iter_mut() {
        if combo.mouse_controller_id == controller_id && combo.mouse_slot_id == slot_id {
            combo.mouse_controller_id = 0;
            combo.mouse_slot_id = 0;
            combo.mouse_ep_target = 0;
            removed = true;
        }
        if combo.keyboard_controller_id == controller_id && combo.keyboard_slot_id == slot_id {
            combo.keyboard_controller_id = 0;
            combo.keyboard_slot_id = 0;
            combo.keyboard_ep_target = 0;
            removed = true;
        }
        if combo.tablet_controller_id == controller_id && combo.tablet_slot_id == slot_id {
            combo.tablet_controller_id = 0;
            combo.tablet_slot_id = 0;
            combo.tablet_ep_target = 0;
            removed = true;
        }
        if combo.gamepad_controller_id == controller_id && combo.gamepad_slot_id == slot_id {
            combo.gamepad_controller_id = 0;
            combo.gamepad_slot_id = 0;
            combo.gamepad_ep_target = 0;
            removed = true;
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

pub fn combos_snapshot() -> Vec<InputCombo, MAX_HID_HUT_COMBOS> {
    HID_HUT.lock().combos.clone()
}

/// Device identities and pairings sampled under one lock. Motion and key
/// state are deliberately omitted so this snapshot is stable while dragging.
pub fn devices_snapshot() -> Vec<HidDevice, HID_HUT_MAX_DEVICES> {
    let guard = HID_HUT.lock();
    let mut devices = Vec::new();
    for mouse in &guard.mice {
        let _ = devices.push(HidDevice {
            id: HidDeviceId {
                kind: HidDeviceKind::Mouse,
                controller_id: mouse.controller_id,
                slot_id: mouse.slot_id,
                ep_target: mouse.ep_target,
            },
            combo_id: mouse.combo_id,
            source_kind: mouse.source_kind,
            source_tag: mouse.source_tag.clone(),
            virtual_device: mouse.virtual_device,
        });
    }
    for tablet in &guard.tablets {
        let _ = devices.push(HidDevice {
            id: HidDeviceId {
                kind: HidDeviceKind::Tablet,
                controller_id: tablet.controller_id,
                slot_id: tablet.slot_id,
                ep_target: tablet.ep_target,
            },
            combo_id: tablet.combo_id,
            source_kind: tablet.source_kind,
            source_tag: tablet.source_tag.clone(),
            virtual_device: tablet.virtual_device,
        });
    }
    for keyboard in &guard.keyboards {
        let _ = devices.push(HidDevice {
            id: HidDeviceId {
                kind: HidDeviceKind::Keyboard,
                controller_id: keyboard.controller_id,
                slot_id: keyboard.slot_id,
                ep_target: keyboard.ep_target,
            },
            combo_id: keyboard.combo_id,
            source_kind: keyboard.source_kind,
            source_tag: keyboard.source_tag.clone(),
            virtual_device: keyboard.virtual_device,
        });
    }
    for gamepad in &guard.gamepads {
        let _ = devices.push(gamepad.clone());
    }
    devices
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
            key_down_bits: keyboard.key_down_bits,
            source_tag: [0; HID_HUT_SOURCE_TAG_MAX],
        };
        next.source_tag_len = copy_source_tag(&mut next.source_tag, &keyboard.source_tag);
        out[wrote] = next;
        wrote += 1;
    }
    wrote
}

#[cfg(test)]
mod link_tests {
    use super::*;

    fn device(kind: HidDeviceKind, slot_id: u32) -> HidDeviceId {
        HidDeviceId {
            kind,
            controller_id: 1,
            slot_id,
            ep_target: 1,
        }
    }

    fn mouse(slot_id: u32) -> HidMouseState {
        HidMouseState {
            controller_id: 1,
            slot_id,
            ep_target: 1,
            x: 12.0,
            y: 34.0,
            buttons_down: 1,
            combo_id: 0,
            source_kind: HidSourceKind::Human,
            source_tag: normalized_tag("physical"),
            virtual_device: false,
        }
    }

    fn keyboard(slot_id: u32) -> HidKeyboardState {
        HidKeyboardState {
            controller_id: 1,
            slot_id,
            ep_target: 1,
            modifiers: 0,
            keys: [0; 6],
            ascii: [0; 6],
            key_down_bits: [0; 8],
            combo_id: 0,
            source_kind: HidSourceKind::Human,
            source_tag: normalized_tag("physical"),
            virtual_device: false,
        }
    }

    fn add_combo(state: &mut HidHutState, id: u32) {
        state
            .combos
            .push(empty_combo(
                id,
                HidSourceKind::Human,
                "desk",
                0,
                INPUT_COMBO_FLAG_AUTO_ASSIGNED,
            ))
            .unwrap();
    }

    #[test]
    fn auto_pairing_separates_mouse_and_tablet_in_the_same_usb_slot() {
        let mut state = HidHutState::new();
        let endpoint = ComboEndpoint::new(1, 2, 1);
        ensure_auto_combo_binding(&mut state, ComboDeviceKind::Mouse, endpoint);
        ensure_auto_combo_binding(&mut state, ComboDeviceKind::Tablet, endpoint);
        ensure_auto_combo_binding(&mut state, ComboDeviceKind::Keyboard, endpoint);
        assert_eq!(state.combos.len(), 2);
        assert_eq!(
            combo_endpoint(&state.combos[0], ComboDeviceKind::Mouse),
            endpoint
        );
        assert_eq!(
            combo_endpoint(&state.combos[0], ComboDeviceKind::Keyboard),
            endpoint
        );
        assert!(endpoint_is_empty(combo_endpoint(
            &state.combos[0],
            ComboDeviceKind::Tablet
        )));
        assert_eq!(
            combo_endpoint(&state.combos[1], ComboDeviceKind::Tablet),
            endpoint
        );
        assert!(endpoint_is_empty(combo_endpoint(
            &state.combos[1],
            ComboDeviceKind::Mouse
        )));
    }

    #[test]
    fn binding_a_second_cursor_preserves_both_previous_pairings() {
        let mut state = HidHutState::new();
        add_combo(&mut state, 1);
        add_combo(&mut state, 2);
        let mouse = ComboEndpoint::new(1, 2, 1);
        let tablet = ComboEndpoint::new(1, 3, 1);
        assert!(bind_device_in_state(
            &mut state,
            1,
            ComboDeviceKind::Mouse,
            mouse
        ));
        assert!(bind_device_in_state(
            &mut state,
            2,
            ComboDeviceKind::Tablet,
            tablet
        ));
        let before = state.combos.clone();
        assert!(!bind_device_in_state(
            &mut state,
            2,
            ComboDeviceKind::Mouse,
            mouse
        ));
        assert!(!bind_device_in_state(
            &mut state,
            1,
            ComboDeviceKind::Tablet,
            tablet
        ));
        assert_eq!(state.combos, before);
    }

    #[test]
    fn rejected_drops_are_atomic_and_successful_moves_keep_the_active_pointer() {
        let mut state = HidHutState::new();
        for id in 1..=3 {
            add_combo(&mut state, id);
        }
        state.mice.push(mouse(2)).unwrap();
        state.mice.push(mouse(3)).unwrap();
        assert!(bind_device_in_state(
            &mut state,
            1,
            ComboDeviceKind::Mouse,
            ComboEndpoint::new(1, 2, 1)
        ));
        assert!(bind_device_in_state(
            &mut state,
            2,
            ComboDeviceKind::Mouse,
            ComboEndpoint::new(1, 3, 1)
        ));
        let before = state.combos.clone();
        assert_eq!(
            move_device_in_state(&mut state, device(HidDeviceKind::Mouse, 2), 2),
            Err(ComboMoveError::CursorOccupied)
        );
        assert_eq!(
            move_device_in_state(&mut state, device(HidDeviceKind::Mouse, 99), 3),
            Err(ComboMoveError::DeviceMissing)
        );
        assert_eq!(
            move_device_in_state(&mut state, device(HidDeviceKind::Mouse, 2), 99),
            Err(ComboMoveError::ComboMissing)
        );
        assert_eq!(state.combos, before);
        let before_pointer = state.mice[0].clone();
        assert_eq!(
            move_device_in_state(&mut state, device(HidDeviceKind::Mouse, 2), 3),
            Ok(())
        );
        assert!(combo_is_empty(&state.combos[0]));
        assert_eq!(state.combos[2].flags & INPUT_COMBO_FLAG_AUTO_ASSIGNED, 0);
        let mut expected_pointer = before_pointer;
        expected_pointer.combo_id = 3;
        assert_eq!(state.mice[0], expected_pointer);
        // Dropping back into the same bucket is harmless.
        assert_eq!(
            move_device_in_state(&mut state, device(HidDeviceKind::Mouse, 2), 3),
            Ok(())
        );
    }

    #[test]
    fn occupied_keyboard_slot_rejects_drop_without_displacing_any_device() {
        let mut state = HidHutState::new();
        add_combo(&mut state, 1);
        add_combo(&mut state, 2);
        state.keyboards.push(keyboard(2)).unwrap();
        state.keyboards.push(keyboard(3)).unwrap();
        assert!(bind_device_in_state(
            &mut state,
            1,
            ComboDeviceKind::Keyboard,
            ComboEndpoint::new(1, 2, 1)
        ));
        assert!(bind_device_in_state(
            &mut state,
            2,
            ComboDeviceKind::Keyboard,
            ComboEndpoint::new(1, 3, 1)
        ));
        let before = state.combos.clone();
        assert_eq!(
            move_device_in_state(&mut state, device(HidDeviceKind::Keyboard, 2), 2),
            Err(ComboMoveError::DeviceKindOccupied)
        );
        assert_eq!(state.combos, before);
        assert_eq!(state.keyboards[0].combo_id, 1);
        assert_eq!(state.keyboards[1].combo_id, 2);
    }

    #[test]
    fn empty_bucket_deletion_and_live_snapshots_do_not_depend_on_input_motion() {
        let combo = request_combo(HidSourceKind::Human, "desk", None).unwrap();
        assert!(combo_is_empty(&combo));
        upsert_mouse_state(1, 42, 1, 0.1, 0.2, 1, HidSourceKind::Human, "mouse", false);
        let mouse = device(HidDeviceKind::Mouse, 42);
        assert_eq!(move_device_to_combo(mouse, combo.combo_id), Ok(()));
        assert!(!remove_empty_combo(combo.combo_id));
        let snapshot = devices_snapshot();
        assert_eq!(
            snapshot
                .iter()
                .find(|entry| entry.id == mouse)
                .unwrap()
                .combo_id,
            combo.combo_id
        );
        upsert_mouse_state(1, 42, 1, 0.9, 0.7, 0, HidSourceKind::Human, "mouse", false);
        assert_eq!(devices_snapshot(), snapshot);
        assert!(remove_slot(1, 42));
        assert_eq!(
            move_device_to_combo(mouse, combo.combo_id),
            Err(ComboMoveError::DeviceMissing)
        );
        assert!(remove_empty_combo(combo.combo_id));
        assert!(!remove_empty_combo(combo.combo_id));
    }

    #[test]
    fn gamepad_slot_is_independent_of_cursor_and_rejects_replacement_atomically() {
        let mut state = HidHutState::new();
        for id in 1..=3 {
            add_combo(&mut state, id);
        }
        for slot in 10..=11 {
            state
                .gamepads
                .push(HidDevice {
                    id: device(HidDeviceKind::Gamepad, slot),
                    combo_id: 0,
                    source_kind: HidSourceKind::Ai,
                    source_tag: normalized_tag("gamepad"),
                    virtual_device: true,
                })
                .unwrap();
        }
        assert!(bind_device_in_state(
            &mut state,
            1,
            ComboDeviceKind::Mouse,
            ComboEndpoint::new(1, 2, 1)
        ));
        assert!(bind_device_in_state(
            &mut state,
            2,
            ComboDeviceKind::Tablet,
            ComboEndpoint::new(1, 3, 1)
        ));
        assert!(bind_device_in_state(
            &mut state,
            1,
            ComboDeviceKind::Gamepad,
            ComboEndpoint::new(1, 10, 1)
        ));
        assert!(bind_device_in_state(
            &mut state,
            2,
            ComboDeviceKind::Gamepad,
            ComboEndpoint::new(1, 11, 1)
        ));
        let before = state.combos.clone();
        let gamepads_before = state.gamepads.clone();
        assert_eq!(
            move_device_in_state(&mut state, device(HidDeviceKind::Gamepad, 10), 2),
            Err(ComboMoveError::DeviceKindOccupied)
        );
        assert_eq!(state.combos, before);
        assert_eq!(state.gamepads, gamepads_before);
        assert_eq!(
            move_device_in_state(&mut state, device(HidDeviceKind::Gamepad, 10), 3),
            Ok(())
        );
        assert_eq!(state.combos[0].gamepad_slot_id, 0);
        assert_eq!(state.combos[0].mouse_slot_id, 2);
        assert_eq!(state.combos[1].tablet_slot_id, 3);
        assert_eq!(state.gamepads[0].combo_id, 3);
        assert_eq!(state.gamepads[0].source_kind, HidSourceKind::Human);
        assert_eq!(state.gamepads[0].source_tag.as_str(), "desk");
        assert!(state.gamepads[0].virtual_device);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(controller_id: u32, slot_id: u32, ep_target: u32) -> ComboEndpoint {
        ComboEndpoint::new(controller_id, slot_id, ep_target)
    }

    #[test]
    fn auto_pairing_uses_discovery_order_for_independent_devices() {
        let mut state = HidHutState::new();
        let mouse_a = endpoint(1, 10, 2);
        let mouse_b = endpoint(1, 11, 2);
        let keyboard_a = endpoint(1, 20, 3);
        let keyboard_b = endpoint(1, 21, 3);

        ensure_auto_combo_binding(&mut state, ComboDeviceKind::Mouse, mouse_a);
        ensure_auto_combo_binding(&mut state, ComboDeviceKind::Mouse, mouse_b);
        ensure_auto_combo_binding(&mut state, ComboDeviceKind::Keyboard, keyboard_a);
        ensure_auto_combo_binding(&mut state, ComboDeviceKind::Keyboard, keyboard_b);

        assert_eq!(state.combos.len(), 2);
        assert!(endpoint_matches(
            combo_endpoint(&state.combos[0], ComboDeviceKind::Mouse),
            mouse_a,
        ));
        assert!(endpoint_matches(
            combo_endpoint(&state.combos[0], ComboDeviceKind::Keyboard),
            keyboard_a,
        ));
        assert!(endpoint_matches(
            combo_endpoint(&state.combos[1], ComboDeviceKind::Mouse),
            mouse_b,
        ));
        assert!(endpoint_matches(
            combo_endpoint(&state.combos[1], ComboDeviceKind::Keyboard),
            keyboard_b,
        ));
        assert_ne!(state.combos[0].color_id, state.combos[1].color_id);
    }

    #[test]
    fn auto_pairing_prefers_members_from_the_same_usb_slot() {
        let mut state = HidHutState::new();
        let mouse_a = endpoint(1, 10, 2);
        let mouse_b = endpoint(1, 11, 2);
        let keyboard_b = endpoint(1, 11, 3);

        ensure_auto_combo_binding(&mut state, ComboDeviceKind::Mouse, mouse_a);
        ensure_auto_combo_binding(&mut state, ComboDeviceKind::Mouse, mouse_b);
        ensure_auto_combo_binding(&mut state, ComboDeviceKind::Keyboard, keyboard_b);

        assert!(endpoint_is_empty(combo_endpoint(&state.combos[0], ComboDeviceKind::Keyboard,)));
        assert!(endpoint_matches(
            combo_endpoint(&state.combos[1], ComboDeviceKind::Keyboard),
            keyboard_b,
        ));
    }

    #[test]
    fn manual_rebinding_keeps_each_device_in_one_combo() {
        let mut state = HidHutState::new();
        let mouse_a = endpoint(1, 10, 2);
        let mouse_b = endpoint(1, 11, 2);
        let first_id = 100;
        let second_id = 200;
        assert!(
            state
                .combos
                .push(empty_combo(first_id, HidSourceKind::Human, "first", 0, 0,))
                .is_ok()
        );
        assert!(
            state
                .combos
                .push(empty_combo(second_id, HidSourceKind::Human, "second", 1, 0,))
                .is_ok()
        );

        assert!(bind_device_in_state(&mut state, first_id, ComboDeviceKind::Mouse, mouse_a,));
        assert!(bind_device_in_state(&mut state, second_id, ComboDeviceKind::Mouse, mouse_a,));
        assert!(endpoint_is_empty(combo_endpoint(&state.combos[0], ComboDeviceKind::Mouse,)));
        assert!(endpoint_matches(
            combo_endpoint(&state.combos[1], ComboDeviceKind::Mouse),
            mouse_a,
        ));

        assert!(bind_device_in_state(&mut state, second_id, ComboDeviceKind::Mouse, mouse_b,));
        assert!(endpoint_matches(
            combo_endpoint(&state.combos[1], ComboDeviceKind::Mouse),
            mouse_b,
        ));
    }
}
