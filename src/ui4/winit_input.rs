//! A small, `no_std` winit-shaped input surface over UI4's owner broker.
//!
//! Helio examples use only winit's portable event vocabulary, not its hosted
//! OS event-loop implementation.  This module keeps that vocabulary at the
//! application boundary while UI4 remains responsible for focus, hit testing,
//! pointer capture, and HUT/vLayer identity.

use alloc::vec::Vec;

use super::{
    Ui4ButtonPhase, Ui4InputEvent, WindowId, WindowOwner, focused_keyboard_state,
    take_owner_input_events,
};

const MAX_TRACKED_WINDOWS: usize = 16;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ElementState {
    Pressed,
    Released,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Other(u16),
}

/// The physical-key subset used by Helio's examples today.
///
/// Values which winit knows but Helio does not currently consume remain
/// available through [`PhysicalKey::Unidentified`] with their HID usage.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum KeyCode {
    KeyA,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyW,
    KeyX,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Enter,
    Escape,
    Backspace,
    Tab,
    Space,
    Minus,
    Equal,
    Delete,
    ArrowRight,
    ArrowLeft,
    ArrowDown,
    ArrowUp,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    NumpadSubtract,
    NumpadAdd,
    NumpadEnter,
    ControlLeft,
    ShiftLeft,
    AltLeft,
    ControlRight,
    ShiftRight,
    AltRight,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalKey {
    Code(KeyCode),
    Unidentified(u8),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct KeyEvent {
    pub(crate) physical_key: PhysicalKey,
    pub(crate) state: ElementState,
    pub(crate) repeat: bool,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum MouseScrollDelta {
    LineDelta(f32, f32),
    PixelDelta(f64, f64),
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum WindowEvent {
    Focused(bool),
    Resized {
        width: u32,
        height: u32,
    },
    KeyboardInput {
        event: KeyEvent,
    },
    CursorMoved {
        position: (f64, f64),
    },
    MouseInput {
        state: ElementState,
        button: MouseButton,
    },
    MouseWheel {
        delta: MouseScrollDelta,
    },
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum DeviceEvent {
    MouseMotion { delta: (f64, f64) },
}

/// Stable UI4 device identity carried alongside winit-shaped events.
///
/// `combo_id` keeps a physical or virtual pointer/keyboard persona intact for
/// local multiplayer and remote/AI input without exposing global HUT queues.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DeviceId {
    pub(crate) controller_id: u32,
    pub(crate) slot_id: u32,
    pub(crate) ep_target: u32,
    pub(crate) combo_id: u32,
    pub(crate) virtual_device: bool,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum Event {
    WindowEvent {
        window_id: WindowId,
        device_id: DeviceId,
        event: WindowEvent,
    },
    DeviceEvent {
        device_id: DeviceId,
        event: DeviceEvent,
    },
}

#[derive(Copy, Clone)]
struct TrackedWindow {
    id: WindowId,
    key_down_bits: [u32; 8],
    keyboard_device: DeviceId,
}

/// Owner-level input pump analogous to one winit `EventLoop`.
pub(crate) struct EventLoopInput {
    owner: WindowOwner,
    windows: heapless::Vec<TrackedWindow, MAX_TRACKED_WINDOWS>,
}

impl EventLoopInput {
    pub(crate) const fn new(owner: WindowOwner) -> Self {
        Self {
            owner,
            windows: heapless::Vec::new(),
        }
    }

    pub(crate) fn register_window(&mut self, id: WindowId) -> bool {
        if self.windows.iter().any(|window| window.id == id) {
            return true;
        }
        self.windows
            .push(TrackedWindow {
                id,
                key_down_bits: [0; 8],
                keyboard_device: DeviceId::default(),
            })
            .is_ok()
    }

    pub(crate) fn unregister_window(&mut self, id: WindowId) {
        if let Some(index) = self.windows.iter().position(|window| window.id == id) {
            self.windows.swap_remove(index);
        }
    }

    pub(crate) fn key_is_down(&self, id: WindowId, key: KeyCode) -> bool {
        let Some(usage) = key_code_to_hid_usage(key) else {
            return false;
        };
        self.windows
            .iter()
            .find(|window| window.id == id)
            .is_some_and(|window| usage_is_down(&window.key_down_bits, usage))
    }

    /// Drain UI4's exact owner queue once and append held-key transitions.
    pub(crate) fn poll(&mut self) -> Vec<Event> {
        let broker_events = take_owner_input_events(self.owner);
        let mut out = Vec::with_capacity(broker_events.len().saturating_mul(2));
        for event in broker_events {
            translate_broker_event(event, &mut out);
        }
        self.poll_keyboard_transitions(&mut out);
        out
    }

    fn poll_keyboard_transitions(&mut self, out: &mut Vec<Event>) {
        for window in &mut self.windows {
            let snapshot = focused_keyboard_state(self.owner, window.id);
            let (next_bits, next_device) =
                snapshot
                    .as_ref()
                    .map_or(([0; 8], window.keyboard_device), |keyboard| {
                        (
                            keyboard.key_down_bits,
                            DeviceId {
                                controller_id: keyboard.controller_id,
                                slot_id: keyboard.slot_id,
                                ep_target: keyboard.ep_target,
                                combo_id: keyboard.combo_id,
                                virtual_device: keyboard.virtual_device,
                            },
                        )
                    });
            let changed = xor_bits(window.key_down_bits, next_bits);
            for usage in 0u16..=u8::MAX as u16 {
                let usage = usage as u8;
                if !usage_is_down(&changed, usage) {
                    continue;
                }
                out.push(Event::WindowEvent {
                    window_id: window.id,
                    device_id: next_device,
                    event: WindowEvent::KeyboardInput {
                        event: KeyEvent {
                            physical_key: physical_key_from_hid_usage(usage),
                            state: if usage_is_down(&next_bits, usage) {
                                ElementState::Pressed
                            } else {
                                ElementState::Released
                            },
                            repeat: false,
                        },
                    },
                });
            }
            window.key_down_bits = next_bits;
            if snapshot.is_some() {
                window.keyboard_device = next_device;
            }
        }
    }
}

fn translate_broker_event(event: Ui4InputEvent, out: &mut Vec<Event>) {
    match event {
        Ui4InputEvent::Pointer(pointer) => {
            let device_id = pointer_device_id(pointer.source, pointer.combo_id, pointer.vcursor);
            out.push(Event::WindowEvent {
                window_id: pointer.window,
                device_id,
                event: WindowEvent::CursorMoved {
                    position: (f64::from(pointer.local_x), f64::from(pointer.local_y)),
                },
            });
            if pointer.wheel != 0 {
                out.push(Event::WindowEvent {
                    window_id: pointer.window,
                    device_id,
                    event: WindowEvent::MouseWheel {
                        delta: MouseScrollDelta::LineDelta(0.0, f32::from(pointer.wheel)),
                    },
                });
            }
            if pointer.dx != 0 || pointer.dy != 0 {
                out.push(Event::DeviceEvent {
                    device_id,
                    event: DeviceEvent::MouseMotion {
                        delta: (f64::from(pointer.dx), f64::from(pointer.dy)),
                    },
                });
            }
        }
        Ui4InputEvent::Button(button) => {
            let device_id = pointer_device_id(button.source, button.combo_id, button.vcursor);
            for bit in 0..u32::BITS {
                if button.changed_buttons & (1u32 << bit) == 0 {
                    continue;
                }
                out.push(Event::WindowEvent {
                    window_id: button.window,
                    device_id,
                    event: WindowEvent::MouseInput {
                        state: match button.phase {
                            Ui4ButtonPhase::Down => ElementState::Pressed,
                            Ui4ButtonPhase::Up => ElementState::Released,
                        },
                        button: mouse_button_from_bit(bit),
                    },
                });
            }
        }
        Ui4InputEvent::Resize(resize) => out.push(Event::WindowEvent {
            window_id: resize.window,
            device_id: DeviceId::default(),
            event: WindowEvent::Resized {
                width: resize.width,
                height: resize.height,
            },
        }),
        Ui4InputEvent::Focus(focus) => out.push(Event::WindowEvent {
            window_id: focus.window,
            device_id: pointer_device_id(focus.source, focus.combo_id, focus.vcursor),
            event: WindowEvent::Focused(focus.focused),
        }),
        // Cooked keyboard events remain useful for text widgets. Helio's
        // physical-key contract comes from the focused HUT bitset above,
        // which includes releases, modifiers, and virtual keyboards equally.
        Ui4InputEvent::Keyboard(_) | Ui4InputEvent::Pan(_) => {}
    }
}

fn pointer_device_id(
    source: super::Ui4CursorSource,
    combo_id: u32,
    virtual_device: bool,
) -> DeviceId {
    DeviceId {
        controller_id: source.controller_id,
        slot_id: source.slot_id,
        ep_target: source.ep_target,
        combo_id,
        virtual_device,
    }
}

fn mouse_button_from_bit(bit: u32) -> MouseButton {
    match bit {
        0 => MouseButton::Left,
        1 => MouseButton::Right,
        2 => MouseButton::Middle,
        3 => MouseButton::Back,
        4 => MouseButton::Forward,
        other => MouseButton::Other(other as u16),
    }
}

fn xor_bits(left: [u32; 8], right: [u32; 8]) -> [u32; 8] {
    let mut changed = [0; 8];
    for (index, word) in changed.iter_mut().enumerate() {
        *word = left[index] ^ right[index];
    }
    changed
}

fn usage_is_down(bits: &[u32; 8], usage: u8) -> bool {
    bits[usize::from(usage / 32)] & (1u32 << (usage % 32)) != 0
}

fn physical_key_from_hid_usage(usage: u8) -> PhysicalKey {
    hid_usage_to_key_code(usage)
        .map(PhysicalKey::Code)
        .unwrap_or(PhysicalKey::Unidentified(usage))
}

fn hid_usage_to_key_code(usage: u8) -> Option<KeyCode> {
    Some(match usage {
        0x04 => KeyCode::KeyA,
        0x06 => KeyCode::KeyC,
        0x07 => KeyCode::KeyD,
        0x08 => KeyCode::KeyE,
        0x09 => KeyCode::KeyF,
        0x0A => KeyCode::KeyG,
        0x0B => KeyCode::KeyH,
        0x0C => KeyCode::KeyI,
        0x0D => KeyCode::KeyJ,
        0x0E => KeyCode::KeyK,
        0x0F => KeyCode::KeyL,
        0x10 => KeyCode::KeyM,
        0x14 => KeyCode::KeyQ,
        0x15 => KeyCode::KeyR,
        0x16 => KeyCode::KeyS,
        0x17 => KeyCode::KeyT,
        0x1A => KeyCode::KeyW,
        0x1B => KeyCode::KeyX,
        0x1E => KeyCode::Digit1,
        0x1F => KeyCode::Digit2,
        0x20 => KeyCode::Digit3,
        0x21 => KeyCode::Digit4,
        0x22 => KeyCode::Digit5,
        0x23 => KeyCode::Digit6,
        0x24 => KeyCode::Digit7,
        0x25 => KeyCode::Digit8,
        0x26 => KeyCode::Digit9,
        0x27 => KeyCode::Digit0,
        0x28 => KeyCode::Enter,
        0x29 => KeyCode::Escape,
        0x2A => KeyCode::Backspace,
        0x2B => KeyCode::Tab,
        0x2C => KeyCode::Space,
        0x2D => KeyCode::Minus,
        0x2E => KeyCode::Equal,
        0x3A => KeyCode::F1,
        0x3B => KeyCode::F2,
        0x3C => KeyCode::F3,
        0x3D => KeyCode::F4,
        0x3E => KeyCode::F5,
        0x3F => KeyCode::F6,
        0x40 => KeyCode::F7,
        0x41 => KeyCode::F8,
        0x42 => KeyCode::F9,
        0x43 => KeyCode::F10,
        0x44 => KeyCode::F11,
        0x45 => KeyCode::F12,
        0x4C => KeyCode::Delete,
        0x4F => KeyCode::ArrowRight,
        0x50 => KeyCode::ArrowLeft,
        0x51 => KeyCode::ArrowDown,
        0x52 => KeyCode::ArrowUp,
        0x56 => KeyCode::NumpadSubtract,
        0x57 => KeyCode::NumpadAdd,
        0x58 => KeyCode::NumpadEnter,
        0xE0 => KeyCode::ControlLeft,
        0xE1 => KeyCode::ShiftLeft,
        0xE2 => KeyCode::AltLeft,
        0xE4 => KeyCode::ControlRight,
        0xE5 => KeyCode::ShiftRight,
        0xE6 => KeyCode::AltRight,
        _ => return None,
    })
}

fn key_code_to_hid_usage(key: KeyCode) -> Option<u8> {
    // Keep the reverse lookup mechanically tied to the forward table. This is
    // cold-path state inspection for examples, not event translation.
    (0u16..=u8::MAX as u16)
        .map(|usage| usage as u8)
        .find(|usage| hid_usage_to_key_code(*usage) == Some(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helio_example_keys_have_winit_names() {
        assert_eq!(physical_key_from_hid_usage(0x1A), PhysicalKey::Code(KeyCode::KeyW));
        assert_eq!(physical_key_from_hid_usage(0x2E), PhysicalKey::Code(KeyCode::Equal));
        assert_eq!(physical_key_from_hid_usage(0xE1), PhysicalKey::Code(KeyCode::ShiftLeft));
        assert_eq!(physical_key_from_hid_usage(0x58), PhysicalKey::Code(KeyCode::NumpadEnter));
    }

    #[test]
    fn unknown_hid_usages_are_not_dropped() {
        assert_eq!(physical_key_from_hid_usage(0x75), PhysicalKey::Unidentified(0x75));
    }

    #[test]
    fn held_state_uses_usb_hid_usage_indices() {
        let mut bits = [0; 8];
        bits[0] = 1 << 0x1A;
        assert!(usage_is_down(&bits, 0x1A));
        assert!(!usage_is_down(&bits, 0x16));
    }
}
