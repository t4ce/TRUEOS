#[derive(Copy, Clone)]
pub struct Intel770Flag {
    pub mask: u32,
    pub value: u32,
    pub name: &'static str,
    pub description: &'static str,
}

#[derive(Copy, Clone)]
pub struct Intel770Register {
    pub block: &'static str,
    pub name: &'static str,
    pub offset: usize,
    pub description: &'static str,
    pub flags: &'static [Intel770Flag],
}

const FORCEWAKE_RENDER_FLAGS: &[Intel770Flag] = &[
    Intel770Flag {
        mask: 1 << 0,
        value: 1 << 0,
        name: "FORCEWAKE_KERNEL",
        description: "keep render/GT power-gated slices awake for kernel MMIO access",
    },
    Intel770Flag {
        mask: 1 << 15,
        value: 1 << 15,
        name: "FORCEWAKE_KERNEL_FALLBACK",
        description: "fallback forcewake lane used when the primary request does not latch cleanly",
    },
];

const FORCEWAKE_GT_FLAGS: &[Intel770Flag] = &[
    Intel770Flag {
        mask: 1 << 0,
        value: 1 << 0,
        name: "GT_AWAKE_REQ",
        description: "request the GT/common power well stay awake for MMIO access",
    },
];

const FORCEWAKE_MEDIA_FLAGS: &[Intel770Flag] = &[
    Intel770Flag {
        mask: 1 << 0,
        value: 1 << 0,
        name: "MEDIA_AWAKE_REQ",
        description: "request the media engine power well stay awake",
    },
];

const FORCEWAKE_ACK_FLAGS: &[Intel770Flag] = &[
    Intel770Flag {
        mask: 1 << 0,
        value: 1 << 0,
        name: "ACK_KERNEL",
        description: "render forcewake request acknowledged",
    },
    Intel770Flag {
        mask: 1 << 15,
        value: 1 << 15,
        name: "ACK_KERNEL_FALLBACK",
        description: "fallback render forcewake request acknowledged",
    },
];

const GDRST_FLAGS: &[Intel770Flag] = &[
    Intel770Flag {
        mask: 1 << 0,
        value: 1 << 0,
        name: "RCS_RESET",
        description: "render engine reset control bit; if held, ring MMIO may not latch normally",
    },
];

const RCS_RING_CTL_FLAGS: &[Intel770Flag] = &[
    Intel770Flag {
        mask: 1 << 0,
        value: 1 << 0,
        name: "RING_VALID",
        description: "ring context is enabled and the programmed start/size should be used",
    },
];

const RCS_MI_MODE_FLAGS: &[Intel770Flag] = &[
    Intel770Flag {
        mask: 1 << 8,
        value: 1 << 8,
        name: "STOP_RING",
        description: "MI parser is held stopped; tail writes will not execute work",
    },
    Intel770Flag {
        mask: 1 << 9,
        value: 1 << 9,
        name: "MODE_IDLE",
        description: "ring is reporting idle state",
    },
];

const RCS_MODE_FLAGS: &[Intel770Flag] = &[
    Intel770Flag {
        mask: 1 << 13,
        value: 1 << 13,
        name: "TLB_INVALIDATE_EXPLICIT",
        description: "software must explicitly invalidate TLBs after GGTT/PPGTT changes",
    },
    Intel770Flag {
        mask: 1 << 9,
        value: 1 << 9,
        name: "PPGTT_ENABLE",
        description: "per-process graphics translation enabled for this engine",
    },
    Intel770Flag {
        mask: 1 << 3,
        value: 1 << 3,
        name: "DISABLE_LEGACY_MODE",
        description: "Gen11+ legacy submission path disabled; execlists/context mode expected",
    },
];

const EMPTY_FLAGS: &[Intel770Flag] = &[];

pub const INTEL_770_ENGINE_WAKE_REGISTERS: &[Intel770Register] = &[
    Intel770Register {
        block: "FORCEWAKE",
        name: "FORCEWAKE_RENDER_GEN11",
        offset: 0x0A278,
        description: "render forcewake request register for uncached MMIO access into GT/RCS power wells",
        flags: FORCEWAKE_RENDER_FLAGS,
    },
    Intel770Register {
        block: "FORCEWAKE",
        name: "FORCEWAKE_RENDER_REF",
        offset: 0x0A180,
        description: "reference-file primary render forcewake definition; kept alongside the Gen11 path for comparison",
        flags: FORCEWAKE_RENDER_FLAGS,
    },
    Intel770Register {
        block: "FORCEWAKE",
        name: "FORCEWAKE_MEDIA",
        offset: 0x0A184,
        description: "media engine forcewake request register",
        flags: FORCEWAKE_MEDIA_FLAGS,
    },
    Intel770Register {
        block: "FORCEWAKE",
        name: "FORCEWAKE_GT",
        offset: 0x0A188,
        description: "GT/common forcewake request register for shared GT infrastructure",
        flags: FORCEWAKE_GT_FLAGS,
    },
    Intel770Register {
        block: "FORCEWAKE",
        name: "FORCEWAKE_ACK_RENDER",
        offset: 0x00D84,
        description: "render forcewake acknowledge state mirrored by hardware",
        flags: FORCEWAKE_ACK_FLAGS,
    },
    Intel770Register {
        block: "FORCEWAKE",
        name: "FORCEWAKE_ACK_REF",
        offset: 0x130040,
        description: "reference-file forcewake acknowledge register alias used on some platform definitions",
        flags: FORCEWAKE_ACK_FLAGS,
    },
    Intel770Register {
        block: "RESET",
        name: "GDRST",
        offset: 0x0941C,
        description: "graphics device reset control; one of the first places to inspect if engine MMIO writes do not stick",
        flags: GDRST_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RING_CTL_REF",
        offset: 0x02000,
        description: "reference-file ring control location for comparison against the currently used RCS offsets",
        flags: RCS_RING_CTL_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RING_HEAD_REF",
        offset: 0x02010,
        description: "reference-file ring head location",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RING_START_REF",
        offset: 0x02018,
        description: "reference-file ring start location",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_TAIL",
        offset: 0x02030,
        description: "submission producer pointer; writing this kicks newly queued ring commands",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_HEAD",
        offset: 0x02034,
        description: "consumer pointer advanced by the engine as commands retire",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_START",
        offset: 0x02038,
        description: "GGTT base address of the ring buffer backing this engine",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_CTL",
        offset: 0x0203C,
        description: "ring enable/size register; low bits indicate validity, upper bits encode ring length",
        flags: RCS_RING_CTL_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_IPEIR",
        offset: 0x02064,
        description: "exotic but useful: internal parser error syndrome for MI/parser faults",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_IPEHR",
        offset: 0x02068,
        description: "exotic but useful: last parser instruction dword seen near an error or stall",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_INSTDONE",
        offset: 0x0206C,
        description: "engine unit completion bitmap; helps show which blocks are still busy",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_INSTPS",
        offset: 0x02070,
        description: "instruction parser state snapshot, handy when head/tail are not moving",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_ACTHD",
        offset: 0x02074,
        description: "current active head address inside the command stream",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_HWS_PGA",
        offset: 0x02080,
        description: "hardware status page address used by the engine for status writes",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_HWSTAM",
        offset: 0x02098,
        description: "hardware status mask; can hide interesting interrupt/status causes if over-masked",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_MI_MODE",
        offset: 0x0209C,
        description: "basic engine wake register: MI parser control with stop/idle state",
        flags: RCS_MI_MODE_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_IMR",
        offset: 0x020A8,
        description: "interrupt mask register for the render engine",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_EIR",
        offset: 0x020B0,
        description: "engine interrupt/error identity register for sticky fault causes",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_EMR",
        offset: 0x020B4,
        description: "error mask register deciding which engine faults are suppressed",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RING_RESET_CTL",
        offset: 0x020C0,
        description: "per-engine reset control relative to the RCS base, included because reset may block normal wake/submit",
        flags: GDRST_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_EXECLIST_STATUS_LO",
        offset: 0x02234,
        description: "execlist status low dword; exotic but important for modern submission debugging",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_EXECLIST_STATUS_HI",
        offset: 0x02238,
        description: "execlist status high dword paired with the low status register",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_CONTEXT_CONTROL",
        offset: 0x02244,
        description: "context save/restore control; useful when ring mode and execlist mode disagree",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_MODE_GEN7",
        offset: 0x0229C,
        description: "engine mode bits covering explicit TLB invalidate, PPGTT, and legacy-mode gating",
        flags: RCS_MODE_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RING_EXECLIST_SQ_LO",
        offset: 0x02510,
        description: "execlist submission queue low dword from the reference path",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RING_EXECLIST_SQ_HI",
        offset: 0x02514,
        description: "execlist submission queue high dword from the reference path",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RCS_RING_EXECLIST_CONTROL",
        offset: 0x02550,
        description: "execlist control; another exotic register that explains whether legacy ring submit is even expected to work",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "RCS",
        name: "RING_CONTEXT_CONTROL_REF",
        offset: 0x025A0,
        description: "reference-file context control location for modern submission path comparison",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "DISPLAY_PWR",
        name: "PWR_WELL_CTL",
        offset: 0x45400,
        description: "display/GT power well control, useful on integrated graphics where display and GT power are coupled",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "DISPLAY_PWR",
        name: "PWR_WELL_CTL2",
        offset: 0x45404,
        description: "secondary display/GT power well control register",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "DISPLAY_PWR",
        name: "DC_STATE_EN",
        offset: 0x45504,
        description: "deep C-state enable register that may influence whether GT power domains are reachable",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "GGTT",
        name: "GFX_FLSH_CNTL_GEN6",
        offset: 0x101008,
        description: "graphics flush/TLB invalidation control after GGTT PTE programming",
        flags: EMPTY_FLAGS,
    },
    Intel770Register {
        block: "DISPLAY_PWR",
        name: "GT_DISP_PWRON",
        offset: 0x138090,
        description: "GT display power-on request/visibility register used by the current code path",
        flags: EMPTY_FLAGS,
    },
];

#[inline]
pub fn describe_register(offset: usize) -> Option<&'static Intel770Register> {
    let mut idx = 0usize;
    while idx < INTEL_770_ENGINE_WAKE_REGISTERS.len() {
        let reg = &INTEL_770_ENGINE_WAKE_REGISTERS[idx];
        if reg.offset == offset {
            return Some(reg);
        }
        idx += 1;
    }
    None
}

pub fn log_engine_wakeup_table<F>(label: &str, mut read32: F)
where
    F: FnMut(usize) -> u32,
{
    let mut idx = 0usize;
    while idx < INTEL_770_ENGINE_WAKE_REGISTERS.len() {
        let reg = &INTEL_770_ENGINE_WAKE_REGISTERS[idx];
        let value = read32(reg.offset);
        crate::log!(
            "intel/igpu770: reg-table label={} block={} reg={} off=0x{:05X} value=0x{:08X} desc={}\n",
            label,
            reg.block,
            reg.name,
            reg.offset,
            value,
            reg.description
        );
        log_active_flags(label, reg, value);
        log_bit_state(label, reg.name, value);
        idx += 1;
    }
}

fn log_active_flags(label: &str, reg: &Intel770Register, value: u32) {
    if reg.flags.is_empty() {
        return;
    }

    let mut matched = false;
    let mut idx = 0usize;
    while idx < reg.flags.len() {
        let flag = &reg.flags[idx];
        if (value & flag.mask) == flag.value {
            matched = true;
            crate::log!(
                "intel/igpu770: reg-flag label={} reg={} flag={} mask=0x{:08X} value=0x{:08X} desc={}\n",
                label,
                reg.name,
                flag.name,
                flag.mask,
                flag.value,
                flag.description
            );
        }
        idx += 1;
    }

    if !matched {
        crate::log!(
            "intel/igpu770: reg-flag label={} reg={} flag=none desc=no known tracked flags active\n",
            label,
            reg.name
        );
    }
}

fn log_bit_state(label: &str, reg_name: &str, value: u32) {
    crate::log!(
        "intel/igpu770: reg-bits label={} reg={} raw=0x{:08X} set_bits={}\n",
        label,
        reg_name,
        value,
        BitSetList(value)
    );
}

struct BitSetList(u32);

impl core::fmt::Display for BitSetList {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let value = self.0;
        if value == 0 {
            return f.write_str("none");
        }

        let mut first = true;
        let mut bit = 0u32;
        while bit < 32 {
            if (value & (1u32 << bit)) != 0 {
                if !first {
                    f.write_str(",")?;
                }
                first = false;
                core::fmt::Write::write_fmt(f, format_args!("{}", bit))?;
            }
            bit += 1;
        }
        Ok(())
    }
}
