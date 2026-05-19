#[path = "../../../crates/trueos-gboi/mod.rs"]
pub(crate) mod gb;

#[path = "../../../crates/trueos-gboi/nes/mod.rs"]
pub(crate) mod nes;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HostControl {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    Select,
    Start,
    Menu,
    Power,
    AuxG,
    AuxH,
    AuxJ,
    AuxK,
    Volume(u8),
}

impl HostControl {
    pub(crate) fn from_keyboard_event(
        event: crate::r::keyboard::TrueosKeyboardOutputEvent,
    ) -> Option<Self> {
        match event.kind {
            crate::r::keyboard::KEYBOARD_OUTPUT_KIND_TEXT => {
                char::from_u32(event.codepoint).and_then(Self::from_char)
            }
            crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY => match event.key_code {
                crate::r::keyboard::KEYBOARD_KEY_ARROW_UP => Some(Self::Up),
                crate::r::keyboard::KEYBOARD_KEY_ARROW_DOWN => Some(Self::Down),
                crate::r::keyboard::KEYBOARD_KEY_ARROW_LEFT => Some(Self::Left),
                crate::r::keyboard::KEYBOARD_KEY_ARROW_RIGHT => Some(Self::Right),
                crate::r::keyboard::KEYBOARD_KEY_SPACE => Some(Self::A),
                crate::r::keyboard::KEYBOARD_KEY_ENTER => Some(Self::Start),
                _ => char::from_u32(event.codepoint).and_then(Self::from_char),
            },
            _ => None,
        }
    }

    pub(crate) fn nes_button(self) -> Option<nes::NesControllerButton> {
        match self {
            Self::Up => Some(nes::NesControllerButton::Up),
            Self::Down => Some(nes::NesControllerButton::Down),
            Self::Left => Some(nes::NesControllerButton::Left),
            Self::Right => Some(nes::NesControllerButton::Right),
            Self::A => Some(nes::NesControllerButton::A),
            Self::B => Some(nes::NesControllerButton::B),
            Self::Select => Some(nes::NesControllerButton::Select),
            Self::Start => Some(nes::NesControllerButton::Start),
            Self::Menu
            | Self::Power
            | Self::AuxG
            | Self::AuxH
            | Self::AuxJ
            | Self::AuxK
            | Self::Volume(_) => None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn gb_button(self) -> Option<gb::GameBoyButton> {
        match self {
            Self::Up => Some(gb::GameBoyButton::Up),
            Self::Down => Some(gb::GameBoyButton::Down),
            Self::Left => Some(gb::GameBoyButton::Left),
            Self::Right => Some(gb::GameBoyButton::Right),
            Self::A => Some(gb::GameBoyButton::A),
            Self::B => Some(gb::GameBoyButton::B),
            Self::Select => Some(gb::GameBoyButton::Select),
            Self::Start => Some(gb::GameBoyButton::Start),
            Self::Menu
            | Self::Power
            | Self::AuxG
            | Self::AuxH
            | Self::AuxJ
            | Self::AuxK
            | Self::Volume(_) => None,
        }
    }

    fn from_char(ch: char) -> Option<Self> {
        match ch {
            'w' | 'W' => Some(Self::Up),
            's' | 'S' => Some(Self::Down),
            'a' | 'A' => Some(Self::Left),
            'd' | 'D' => Some(Self::Right),
            'x' | 'X' | ' ' => Some(Self::A),
            'z' | 'Z' => Some(Self::B),
            'c' | 'C' => Some(Self::Select),
            '\r' | '\n' => Some(Self::Start),
            'm' | 'M' => Some(Self::Menu),
            'p' | 'P' => Some(Self::Power),
            'g' | 'G' => Some(Self::AuxG),
            'h' | 'H' => Some(Self::AuxH),
            'j' | 'J' => Some(Self::AuxJ),
            'k' | 'K' => Some(Self::AuxK),
            '1' => Some(Self::Volume(1)),
            '2' => Some(Self::Volume(2)),
            '3' => Some(Self::Volume(3)),
            '4' => Some(Self::Volume(4)),
            '5' => Some(Self::Volume(5)),
            '6' => Some(Self::Volume(6)),
            _ => None,
        }
    }
}
