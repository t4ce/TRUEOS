//! Safe little-endian codecs for fixed-width C ABI records.
//!
//! Each schema names every field and its Rust type. Implicit C padding is
//! always written as zero and ignored on decode; explicitly declared reserved
//! fields are encoded like any other field.
#![forbid(unsafe_code)]

use core::mem::{offset_of, size_of};

use crate::r::keyboard::TrueosKeyboardOutputEvent as KernelKeyboardOutputEvent;
use crate::ui4::blueprint_ui4::{
    BlueprintImageSourceInfo, TrueosUi4ContextMenuEvent as KernelUi4ContextMenuEvent,
    TrueosUi4ContextMenuEventV2 as KernelUi4ContextMenuEventV2,
    TrueosUi4CursorSource as KernelUi4CursorSource, TrueosUi4CursorStep as KernelUi4CursorStep,
    TrueosUi4InputRouteState as KernelUi4InputRouteState,
    TrueosUi4KeyboardState as KernelUi4KeyboardState, TrueosUi4PanEvent as KernelUi4PanEvent,
    TrueosUi4PointerEvent as KernelUi4PointerEvent, TrueosUi4ResizeEvent as KernelUi4ResizeEvent,
    TrueosUi4WindowStateV1 as KernelUi4WindowStateV1,
};
use v::bp_abi::{
    GamepadControlCommand, GamepadControlDeviceInfo, GamepadControlSnapshot,
    KeyboardControlCommand, KeyboardControlDeviceInfo, MouseMotionCommand, MouseMotionCursorInfo,
    TrueosHidCursorEvent, TrueosKeyboardOutputEvent, TrueosUi4ContextMenuEvent,
    TrueosUi4ContextMenuEventV2, TrueosUi4CursorSource, TrueosUi4CursorStep,
    TrueosUi4InputRouteState, TrueosUi4KeyboardState, TrueosUi4PanEvent, TrueosUi4PointerEvent,
    TrueosUi4ResizeEvent, TrueosUi4WindowStateV1,
};

/// A fixed-layout value with an explicit little-endian wire representation.
pub(crate) trait WireRecord: Sized {
    const WIRE_SIZE: usize;
    #[cfg(test)]
    const WIRE_FIELDS: &'static [(usize, usize)];

    fn decode(bytes: &[u8]) -> Option<Self>;

    fn encode(&self, bytes: &mut [u8]) -> bool;
}

/// Private allowlist for the scalar and array field types accepted by schemas.
trait WireField: Copy {
    const WIRE_SIZE: usize;

    fn decode(bytes: &[u8], offset: usize) -> Option<Self>;

    fn encode(self, bytes: &mut [u8], offset: usize) -> bool;
}

macro_rules! integer_wire_field {
    ($ty:ty, $n:expr) => {
        impl WireField for $ty {
            const WIRE_SIZE: usize = $n;

            fn decode(bytes: &[u8], offset: usize) -> Option<Self> {
                let end = offset.checked_add(Self::WIRE_SIZE)?;
                let raw: [u8; $n] = bytes.get(offset..end)?.try_into().ok()?;
                Some(<$ty>::from_le_bytes(raw))
            }

            fn encode(self, bytes: &mut [u8], offset: usize) -> bool {
                let Some(end) = offset.checked_add(Self::WIRE_SIZE) else {
                    return false;
                };
                let Some(target) = bytes.get_mut(offset..end) else {
                    return false;
                };
                target.copy_from_slice(&self.to_le_bytes());
                true
            }
        }
    };
}

integer_wire_field!(u8, 1);
integer_wire_field!(i8, 1);
integer_wire_field!(u16, 2);
integer_wire_field!(i16, 2);
integer_wire_field!(u32, 4);
integer_wire_field!(i32, 4);
integer_wire_field!(u64, 8);
integer_wire_field!(i64, 8);
integer_wire_field!(u128, 16);
integer_wire_field!(i128, 16);

impl WireField for f32 {
    const WIRE_SIZE: usize = 4;

    fn decode(bytes: &[u8], offset: usize) -> Option<Self> {
        Some(Self::from_bits(u32::decode(bytes, offset)?))
    }

    fn encode(self, bytes: &mut [u8], offset: usize) -> bool {
        self.to_bits().encode(bytes, offset)
    }
}

impl WireField for f64 {
    const WIRE_SIZE: usize = 8;

    fn decode(bytes: &[u8], offset: usize) -> Option<Self> {
        Some(Self::from_bits(u64::decode(bytes, offset)?))
    }

    fn encode(self, bytes: &mut [u8], offset: usize) -> bool {
        self.to_bits().encode(bytes, offset)
    }
}

impl<T: WireField + Default, const N: usize> WireField for [T; N] {
    const WIRE_SIZE: usize = T::WIRE_SIZE * N;

    fn decode(bytes: &[u8], offset: usize) -> Option<Self> {
        let mut values = [T::default(); N];
        for (index, value) in values.iter_mut().enumerate() {
            let relative = index.checked_mul(T::WIRE_SIZE)?;
            *value = T::decode(bytes, offset.checked_add(relative)?)?;
        }
        Some(values)
    }

    fn encode(self, bytes: &mut [u8], offset: usize) -> bool {
        for (index, value) in self.into_iter().enumerate() {
            let Some(relative) = index.checked_mul(T::WIRE_SIZE) else {
                return false;
            };
            let Some(at) = offset.checked_add(relative) else {
                return false;
            };
            if !value.encode(bytes, at) {
                return false;
            }
        }
        true
    }
}

macro_rules! wire_record {
    ($record:ty { $($field:ident : $field_ty:ty),+ $(,)? }) => {
        impl WireRecord for $record {
            const WIRE_SIZE: usize = size_of::<Self>();
            #[cfg(test)]
            const WIRE_FIELDS: &'static [(usize, usize)] = &[
                $((offset_of!($record, $field), size_of::<$field_ty>()),)+
            ];

            fn decode(bytes: &[u8]) -> Option<Self> {
                if bytes.len() < Self::WIRE_SIZE {
                    return None;
                }
                Some(Self {
                    $($field: <$field_ty as WireField>::decode(bytes, offset_of!($record, $field))?,)+
                })
            }

            fn encode(&self, bytes: &mut [u8]) -> bool {
                if bytes.len() < Self::WIRE_SIZE {
                    return false;
                }
                bytes[..Self::WIRE_SIZE].fill(0);
                $(if !<$field_ty as WireField>::encode(self.$field, bytes, offset_of!($record, $field)) {
                    return false;
                })+
                true
            }
        }

        const _: () = {
            $(
                assert!(offset_of!($record, $field) + size_of::<$field_ty>() <= size_of::<$record>());
                assert!(<$field_ty as WireField>::WIRE_SIZE == size_of::<$field_ty>());
                let _: fn(&$record) -> &$field_ty = |value| &value.$field;
            )+
        };
    };
}

wire_record!(TrueosUi4ContextMenuEvent {
    context: u64,
    action_id: u32,
    selected: u32,
    reason: u32,
});

wire_record!(TrueosUi4ContextMenuEventV2 {
    serial: u64,
    local_x: i32,
    local_y: i32,
    action_id: u32,
    selected: u32,
    reason: u32,
    reserved: u32,
});

wire_record!(TrueosUi4CursorSource {
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    hid_kind: u32,
});

wire_record!(TrueosUi4CursorStep {
    origin_x: u32,
    origin_y: u32,
    cell_width_subpx: u32,
    cell_height_subpx: u32,
});

wire_record!(TrueosUi4WindowStateV1 {
    version: u32,
    visible: u32,
    hit_testable: u32,
    opacity: u32,
    focused: u32,
    reserved: [u32; 3],
});

wire_record!(TrueosUi4PanEvent {
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    hid_kind: u32,
    phase: u32,
    x: u32,
    y: u32,
    local_x: i32,
    local_y: i32,
    dx: i32,
    dy: i32,
    combo_id: u32,
    vcursor: u32,
});

wire_record!(TrueosUi4ResizeEvent {
    old_width: u32,
    old_height: u32,
    width: u32,
    height: u32,
});

wire_record!(TrueosUi4PointerEvent {
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    hid_kind: u32,
    x: u32,
    y: u32,
    local_x: i32,
    local_y: i32,
    dx: i32,
    dy: i32,
    wheel: i32,
    buttons_down: u32,
    buttons_pressed: u32,
    buttons_released: u32,
    combo_id: u32,
    vcursor: u32,
});

wire_record!(TrueosUi4KeyboardState {
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    combo_id: u32,
    modifiers: u8,
    source_kind: u8,
    virtual_keyboard: u8,
    reserved0: u8,
    keys: [u8; 6],
    ascii: [u8; 6],
    key_down_bits: [u32; 8],
});

wire_record!(TrueosUi4InputRouteState {
    cursor_controller_id: u32,
    cursor_slot_id: u32,
    cursor_ep_target: u32,
    cursor_hid_kind: u32,
    combo_id: u32,
    color_rgba: u32,
    flags: u32,
    keyboard_controller_id: u32,
    keyboard_slot_id: u32,
    keyboard_ep_target: u32,
    keyboard_modifiers: u8,
    keyboard_source_kind: u8,
    virtual_keyboard: u8,
    reserved0: u8,
    keys: [u8; 6],
    ascii: [u8; 6],
    key_down_bits: [u32; 8],
});

wire_record!(MouseMotionCursorInfo {
    handle: u64,
    slot_id: u32,
    reserved: u32,
});

wire_record!(MouseMotionCommand {
    opcode: u8,
    path: u8,
    easing: u8,
    flags: u8,
    duration_ms: u32,
    x: i32,
    y: i32,
    control1_x: i32,
    control1_y: i32,
    control2_x: i32,
    control2_y: i32,
    buttons_set: u32,
    buttons_clear: u32,
    wheel: i16,
    reserved: u16,
});

wire_record!(KeyboardControlDeviceInfo {
    handle: u64,
    slot_id: u32,
    reserved: u32,
});

wire_record!(KeyboardControlCommand {
    opcode: u8,
    flags: u8,
    modifiers: u8,
    reserved0: u8,
    duration_ms: u32,
    codepoint: u32,
    key_code: u16,
    reserved1: u16,
});

wire_record!(GamepadControlDeviceInfo {
    handle: u64,
    slot_id: u32,
    reserved: u32,
});

wire_record!(GamepadControlCommand {
    opcode: u8,
    easing: u8,
    flags: u8,
    reserved0: u8,
    duration_ms: u32,
    buttons_set: u32,
    buttons_clear: u32,
    left_x: i16,
    left_y: i16,
    right_x: i16,
    right_y: i16,
    left_trigger: u16,
    right_trigger: u16,
});

wire_record!(GamepadControlSnapshot {
    slot_id: u32,
    sequence: u32,
    buttons_down: u32,
    reserved0: u32,
    left_x: i16,
    left_y: i16,
    right_x: i16,
    right_y: i16,
    left_trigger: u16,
    right_trigger: u16,
});

wire_record!(TrueosHidCursorEvent {
    t_ms: u32,
    seq: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    hid_kind: u8,
    reserved0: u8,
    reserved1: u16,
    buttons_down: u32,
    wheel: i16,
    reserved2: u16,
    x: f64,
    y: f64,
    flags: u32,
});

wire_record!(TrueosKeyboardOutputEvent {
    t_ms: u32,
    seq: u32,
    device_seq: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    modifiers: u8,
    kind: u8,
    utf8_len: u8,
    reserved0: u8,
    key_code: u16,
    reserved1: u16,
    codepoint: u32,
    utf8: [u8; 4],
    flags: u32,
});

wire_record!(KernelKeyboardOutputEvent {
    t_ms: u32,
    seq: u32,
    device_seq: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    modifiers: u8,
    kind: u8,
    utf8_len: u8,
    reserved0: u8,
    key_code: u16,
    reserved1: u16,
    codepoint: u32,
    utf8: [u8; 4],
    flags: u32,
});

// The kernel owns parallel UI4 records rather than re-exporting the guest ABI
// types. Keep both layouts independently checked against their declarations.
wire_record!(KernelUi4ContextMenuEvent {
    context: u64,
    action_id: u32,
    selected: u32,
    reason: u32,
});

wire_record!(KernelUi4ContextMenuEventV2 {
    serial: u64,
    local_x: i32,
    local_y: i32,
    action_id: u32,
    selected: u32,
    reason: u32,
    reserved: u32,
});

wire_record!(KernelUi4CursorSource {
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    hid_kind: u32,
});

wire_record!(KernelUi4CursorStep {
    origin_x: u32,
    origin_y: u32,
    cell_width_subpx: u32,
    cell_height_subpx: u32,
});

wire_record!(KernelUi4WindowStateV1 {
    version: u32,
    visible: u32,
    hit_testable: u32,
    opacity: u32,
    focused: u32,
    reserved: [u32; 3],
});

wire_record!(KernelUi4PanEvent {
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    hid_kind: u32,
    phase: u32,
    x: u32,
    y: u32,
    local_x: i32,
    local_y: i32,
    dx: i32,
    dy: i32,
    combo_id: u32,
    vcursor: u32,
});

wire_record!(KernelUi4ResizeEvent {
    old_width: u32,
    old_height: u32,
    width: u32,
    height: u32,
});

wire_record!(KernelUi4PointerEvent {
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    hid_kind: u32,
    x: u32,
    y: u32,
    local_x: i32,
    local_y: i32,
    dx: i32,
    dy: i32,
    wheel: i32,
    buttons_down: u32,
    buttons_pressed: u32,
    buttons_released: u32,
    combo_id: u32,
    vcursor: u32,
});

wire_record!(KernelUi4KeyboardState {
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    combo_id: u32,
    modifiers: u8,
    source_kind: u8,
    virtual_keyboard: u8,
    reserved0: u8,
    keys: [u8; 6],
    ascii: [u8; 6],
    key_down_bits: [u32; 8],
});

wire_record!(KernelUi4InputRouteState {
    cursor_controller_id: u32,
    cursor_slot_id: u32,
    cursor_ep_target: u32,
    cursor_hid_kind: u32,
    combo_id: u32,
    color_rgba: u32,
    flags: u32,
    keyboard_controller_id: u32,
    keyboard_slot_id: u32,
    keyboard_ep_target: u32,
    keyboard_modifiers: u8,
    keyboard_source_kind: u8,
    virtual_keyboard: u8,
    reserved0: u8,
    keys: [u8; 6],
    ascii: [u8; 6],
    key_down_bits: [u32; 8],
});

wire_record!(BlueprintImageSourceInfo {
    format: u32,
    width: u32,
    height: u32,
    byte_len: u32,
});
