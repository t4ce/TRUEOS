use super::RgbaColor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct NameEntry {
    pub(super) name: &'static str,
    pub(super) color: RgbaColor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct NameGroup {
    pub(super) name: &'static str,
    pub(super) names: &'static [NameEntry],
}
pub const GROUP_OPEN: char = '[';
pub const GROUP_CLOSE: char = ']';

const HV_GROUP_1: [NameEntry; 1] = [NameEntry { name: "peer", color: RgbaColor::White }];

const HV_GROUP_2: [NameEntry; 2] = [
    NameEntry {
        name: "status",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "pause",
        color: RgbaColor::White,
    },
];

const HV_GROUP_3: [NameEntry; 8] = [
    NameEntry {
        name: "snap",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "preserve",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "eject",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "delete",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "kick",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "load",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "store",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "probe",
        color: RgbaColor::White,
    },
];

pub(super) const HV_GROUPS: [NameGroup; 3] = [
    NameGroup {
        name: "",
        names: &HV_GROUP_1,
    },
    NameGroup {
        name: "",
        names: &HV_GROUP_2,
    },
    NameGroup {
        name: "",
        names: &HV_GROUP_3,
    },
];

const CMD_AKA_NAMES: [NameEntry; 0] = [];

const CMD_MEDIA_NAMES: [NameEntry; 4] = [
    NameEntry {
        name: "pic",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "vid",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "aud",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "vaud",
        color: RgbaColor::White,
    }
];

const CMD_APPDB_NAMES: [NameEntry; 0] = [];

pub(super) const CMD_GROUPS: [NameGroup; 3] = [
    NameGroup {
        name: "Aka",
        names: &CMD_AKA_NAMES,
    },
    NameGroup {
        name: "Capture",
        names: &CMD_MEDIA_NAMES,
    },
    NameGroup {
        name: "AppDB",
        names: &CMD_APPDB_NAMES,
    },
];

pub(super) const ADM_NAMES: &[NameEntry] = &[
    NameEntry {
        name: "cry",
        color: RgbaColor::Pink,
    },
    NameEntry {
        name: "sh3",
        color: RgbaColor::Pink,
    },
    NameEntry {
        name: "disc",
        color: RgbaColor::Pink,
    },
    NameEntry {
        name: "tlb",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "ram",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "smp",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "net",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "vgpu",
        color: RgbaColor::White,
    },
    NameEntry {
        name: "vcpy",
        color: RgbaColor::White,
    },
];

/// Slot-specific VM environment controls; not a Tab mode.
pub(super) const VME_GROUP: NameGroup = NameGroup {
    name: "VME",
    names: &[
        NameEntry {
            name: "tui",
            color: RgbaColor::White,
        },
        NameEntry {
            name: "env",
            color: RgbaColor::White,
        },
        NameEntry {
            name: "smp",
            color: RgbaColor::White,
        },
        NameEntry {
            name: "esc",
            color: RgbaColor::White,
        },
        NameEntry {
            name: "stop",
            color: RgbaColor::White,
        },
    ],
};

/// Global online fastspawn is submitted by Enter, never exact-name recognition.
pub(super) fn fastspawn_selector(input: &str) -> Option<&str> {
    let name = input.strip_prefix("§§")?;
    if name.is_empty() || name.chars().count() > 16
        || name.chars().any(|ch| ch == '§' || ch.is_whitespace() || ch.is_control()) {
        return None;
    }
    Some(name)
}
#[cfg(test)]
mod fastspawn_tests {
    use super::fastspawn_selector;
    #[test] fn accepts_one_to_sixteen_characters_and_preserves_case() {
        assert_eq!(fastspawn_selector("§§Appname"),Some("Appname"));
        assert_eq!(fastspawn_selector("§§abcdefghijklmnop"),Some("abcdefghijklmnop"));
        assert_eq!(fastspawn_selector("§§é"),Some("é"));
    }
    #[test] fn leaves_bare_operator_and_slot_forms_outside_fastspawn() {
        for input in ["§", "§§", "§sh1", "§sh1§", "§§§", "§§a§", "§§a b", "§§a\n", "§§abcdefghijklmnopq"] {
            assert_eq!(fastspawn_selector(input),None,"{input:?}");
        }
    }
}
