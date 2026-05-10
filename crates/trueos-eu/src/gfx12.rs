use crate::{EuArtifact, EuArtifactKind, EuIsa};

pub const STORE_SENTINEL_U32: u32 = 0xC0DE_7733;
pub const HDC1_BTI34_STORE_SEND_DWORD: usize = 11;
pub const HDC1_BTI34_STORE_IMM_DWORD: usize = 3;
pub const HDC1_STATELESS_STATIC_DP4A_STORE_SEND_DWORD: usize = 23;
pub const HDC1_STATELESS_STATIC_DP4A_BASE_DWORD: usize = 3;
pub const STATIC_DP4A_DOT_ADDEND_U32: u32 = 10;
pub const STATIC_DP4A_BASE_U32: u32 = STORE_SENTINEL_U32 - STATIC_DP4A_DOT_ADDEND_U32;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Gfx12EotVariant {
    TsR0ToG112,
    TsR0ToG120,
    TsR0ToG126,
    TsR0R1ToG126G127Mlen2,
    TsNopThenR0ToG126,
    TsR0ToG127,
    TsR0ToG127Send1,
    GatewayR0ToG127,
    GatewayR0ToG127Dg2,
    TsR0ToG127AccClear,
    IllegalAllOnes,
}

pub static TS_EOT_R0_TO_G112_WORDS: [u32; 8] = [
    0x80030061, 0x70050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x7000700C, 0x00000000,
];

pub static TS_EOT_R0_TO_G112: EuArtifact = EuArtifact {
    name: "gfx12-ts-eot-r0-to-g112-assembled",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::ThreadSpawnerEot,
    words: &TS_EOT_R0_TO_G112_WORDS,
    expects_store: false,
};

pub static TS_EOT_R0_TO_G120_WORDS: [u32; 8] = [
    0x80030061, 0x78050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x7000780C, 0x00000000,
];

pub static TS_EOT_R0_TO_G120: EuArtifact = EuArtifact {
    name: "gfx12-ts-eot-r0-to-g120-assembled",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::ThreadSpawnerEot,
    words: &TS_EOT_R0_TO_G120_WORDS,
    expects_store: false,
};

pub static TS_EOT_R0_TO_G126_WORDS: [u32; 8] = [
    0x80030061, 0x7E050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x70007E0C, 0x00000000,
];

pub static TS_EOT_R0_TO_G126: EuArtifact = EuArtifact {
    name: "gfx12-ts-eot-r0-to-g126-assembled",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::ThreadSpawnerEot,
    words: &TS_EOT_R0_TO_G126_WORDS,
    expects_store: false,
};

// Deliberate Gfx12.0/Xe-LP EOT payload-width probe. The PRM names a single
// GRF R0-copy payload for TS_EOT, but this variant tests whether this legacy
// GPGPU_WALKER path expects a wider header/token image.
//
// Assembled with Mesa brw_asm for `tgl`:
//
// mov(8)  g126<1>UD  g0<8,8,1>UD
// mov(8)  g127<1>UD  g1<8,8,1>UD
// send(8) nullUD     g126UD nullUD 0x04000000 0x00000000
//         ts/btd MsgDesc: mlen 2 ex_mlen 0 rlen 0 EOT
pub static TS_EOT_R0_R1_TO_G126_G127_MLEN2_WORDS: [u32; 12] = [
    0x80030061, 0x7E050220, 0x00460005, 0x00000000, 0x80030061, 0x7F050220, 0x00460105, 0x00000000,
    0x80030131, 0x00000004, 0x70007E14, 0x00000000,
];

pub static TS_EOT_R0_R1_TO_G126_G127_MLEN2: EuArtifact = EuArtifact {
    name: "gfx12-ts-eot-r0-r1-to-g126-g127-mlen2-assembled",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::ThreadSpawnerEot,
    words: &TS_EOT_R0_R1_TO_G126_G127_MLEN2_WORDS,
    expects_store: false,
};

// Gfx12.0/Xe-LP single harmless EU instruction before the canonical TS EOT.
// Assembled with Mesa brw_asm for `tgl`:
//
// sync nop(8) null<0,1,0>UB
// mov(8)      g126<1>UD       g0<8,8,1>UD
// send(8)     nullUD          g126UD nullUD 0x02000000 0x00000000 EOT
pub static TS_NOP_THEN_EOT_R0_TO_G126_WORDS: [u32; 12] = [
    0x80030101, 0x00000000, 0x00000000, 0x00000000, 0x80030061, 0x7E050220, 0x00460005, 0x00000000,
    0x80030131, 0x00000004, 0x70007E0C, 0x00000000,
];

pub static TS_NOP_THEN_EOT_R0_TO_G126: EuArtifact = EuArtifact {
    name: "gfx12-ts-nop-then-eot-r0-to-g126-assembled",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::ThreadSpawnerEot,
    words: &TS_NOP_THEN_EOT_R0_TO_G126_WORDS,
    expects_store: false,
};

// Gfx12.0/Xe-LP Thread Spawner EOT, assembled with Mesa brw_asm for `tgl`.
// This is the PRM path for media/GPGPU root threads launched by
// MEDIA_INTERFACE_DESCRIPTOR_LOAD + GPGPU_WALKER on ADL-S 8086:4680.
//
// PRM TS_EOT descriptor contract:
// - SFID_TS = 7 on Gfx12.0 and earlier.
// - desc = 0x02000000: mlen 1, rlen 0, message type End Thread.
// - send EOT control is set in the extended descriptor sideband.
// - payload is one GRF copied from the R0 thread payload.
//
// mov(8)  g127<1>UD  g0<8,8,1>UD
// send(8) nullUD     g127UD nullUD 0x02000000 0x00000000
//         ts/btd MsgDesc: mlen 1 ex_mlen 0 rlen 0 EOT
pub static TS_EOT_R0_TO_G127_WORDS: [u32; 8] = [
    0x80030061, 0x7F050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x70007F0C, 0x00000000,
];

pub static TS_EOT_R0_TO_G127: EuArtifact = EuArtifact {
    name: "gfx12-ts-eot-r0-to-g127-assembled",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::ThreadSpawnerEot,
    words: &TS_EOT_R0_TO_G127_WORDS,
    expects_store: false,
};

pub static TS_EOT_R0_TO_G127_SEND1_WORDS: [u32; 8] = [
    0x80030061, 0x7F050220, 0x00460005, 0x00000000, 0x80000131, 0x00000004, 0x70007F0C, 0x00000000,
];

pub static TS_EOT_R0_TO_G127_SEND1: EuArtifact = EuArtifact {
    name: "gfx12-ts-eot-r0-to-g127-send1-assembled",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::ThreadSpawnerEotSend1,
    words: &TS_EOT_R0_TO_G127_SEND1_WORDS,
    expects_store: false,
};

pub static GATEWAY_EOT_R0_TO_G127_WORDS: [u32; 8] = [
    0x80030061, 0x7F050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x30007F0C, 0x00000000,
];

pub static GATEWAY_EOT_R0_TO_G127: EuArtifact = EuArtifact {
    name: "gfx12-gateway-eot-r0-to-g127-assembled",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::GatewayEot,
    words: &GATEWAY_EOT_R0_TO_G127_WORDS,
    expects_store: false,
};

// Kept as an explicitly non-mainline probe: on Gfx12.5+ SFID 7 no longer means
// Thread Spawner in the same way, so this is not the ADL-S 8086:4680 EOT path.
pub static GATEWAY_EOT_R0_TO_G127_DG2_WORDS: [u32; 8] = [
    0x80030061, 0x7F050220, 0x00460005, 0x00000000, 0x80030931, 0x00000004, 0x30007F0C, 0x00000000,
];

pub static GATEWAY_EOT_R0_TO_G127_DG2: EuArtifact = EuArtifact {
    name: "gfx125-gateway-eot-r0-to-g127-dg2-assembled",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::GatewayEot,
    words: &GATEWAY_EOT_R0_TO_G127_DG2_WORDS,
    expects_store: false,
};

pub static TS_EOT_R0_TO_G127_ACC_CLEAR_WORDS: [u32; 12] = [
    0x80040061, 0x20014AA0, 0x00000000, 0x00000000, 0x80030061, 0x7F050220, 0x00460005, 0x00000000,
    0x80030131, 0x00000004, 0x70007F0C, 0x00000000,
];

pub static TS_EOT_R0_TO_G127_ACC_CLEAR: EuArtifact = EuArtifact {
    name: "gfx12-ts-eot-r0-to-g127-acc-clear-assembled",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::ThreadSpawnerEot,
    words: &TS_EOT_R0_TO_G127_ACC_CLEAR_WORDS,
    expects_store: false,
};

// Deliberately invalid EU instruction payload. This is not an EOT probe; it is
// an exception/SIP visibility probe. If IDD illegal-opcode exception routing is
// wired correctly, selecting this artifact should fail loudly before any EOT.
pub static ILLEGAL_ALL_ONES_WORDS: [u32; 4] = [
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
];

pub static ILLEGAL_ALL_ONES: EuArtifact = EuArtifact {
    name: "gfx12-illegal-all-ones-exception-probe",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::IllegalInstructionTrap,
    words: &ILLEGAL_ALL_ONES_WORDS,
    expects_store: false,
};

pub static EOT_CATALOG: [EuArtifact; 11] = [
    TS_EOT_R0_TO_G112,
    TS_EOT_R0_TO_G120,
    TS_EOT_R0_TO_G126,
    TS_EOT_R0_R1_TO_G126_G127_MLEN2,
    TS_NOP_THEN_EOT_R0_TO_G126,
    TS_EOT_R0_TO_G127,
    TS_EOT_R0_TO_G127_SEND1,
    GATEWAY_EOT_R0_TO_G127,
    GATEWAY_EOT_R0_TO_G127_DG2,
    TS_EOT_R0_TO_G127_ACC_CLEAR,
    ILLEGAL_ALL_ONES,
];

pub const fn eot_artifact(variant: Gfx12EotVariant) -> EuArtifact {
    match variant {
        Gfx12EotVariant::TsR0ToG112 => TS_EOT_R0_TO_G112,
        Gfx12EotVariant::TsR0ToG120 => TS_EOT_R0_TO_G120,
        Gfx12EotVariant::TsR0ToG126 => TS_EOT_R0_TO_G126,
        Gfx12EotVariant::TsR0R1ToG126G127Mlen2 => TS_EOT_R0_R1_TO_G126_G127_MLEN2,
        Gfx12EotVariant::TsNopThenR0ToG126 => TS_NOP_THEN_EOT_R0_TO_G126,
        Gfx12EotVariant::TsR0ToG127 => TS_EOT_R0_TO_G127,
        Gfx12EotVariant::TsR0ToG127Send1 => TS_EOT_R0_TO_G127_SEND1,
        Gfx12EotVariant::GatewayR0ToG127 => GATEWAY_EOT_R0_TO_G127,
        Gfx12EotVariant::GatewayR0ToG127Dg2 => GATEWAY_EOT_R0_TO_G127_DG2,
        Gfx12EotVariant::TsR0ToG127AccClear => TS_EOT_R0_TO_G127_ACC_CLEAR,
        Gfx12EotVariant::IllegalAllOnes => ILLEGAL_ALL_ONES,
    }
}

// Mesa brw_asm source:
//
// mov(8)  g4<1>UD    0xC0DE7733UD
// mov(8)  g127<1>UD  0UD
// send    HDC1 untyped surface write, BTI 0x34, SIMD8
// mov(8)  g127<1>UD  g0<8,8,1>UD
// send    Thread Spawner EOT from g127
pub static HDC1_BTI34_STORE_THEN_TS_EOT_WORDS: [u32; 20] = [
    0x80030061,
    0x04054660,
    0x00000000,
    STORE_SENTINEL_U32,
    0x80030061,
    0x7F054220,
    0x00000000,
    0x00000000,
    0x00030131,
    0x00000000,
    0xCC687F0C,
    0x009A040C,
    0x80030061,
    0x7F050220,
    0x00460005,
    0x00000000,
    0x80030131,
    0x00000004,
    0x70007F0C,
    0x00000000,
];

pub static HDC1_BTI34_STORE_THEN_TS_EOT: EuArtifact = EuArtifact {
    name: "gfx12-hdc1-bti34-store-then-ts-eot",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::Hdc1BtiStoreThenThreadSpawnerEot,
    words: &HDC1_BTI34_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

// Mesa brw_asm source:
//
// mov(8)  g4<1>UD    0xC0DE7733UD
// mov(8)  g127<1>UD  0x00840058UD
// send    HDC1 untyped surface write, stateless/non-coherent BTI 253, SIMD8
// mov(8)  g127<1>UD  g0<8,8,1>UD
// send    Thread Spawner EOT from g127
//
// This is a diagnostic "EU executed a visible send" canary.  The address is
// TRUEOS' current result slot 22 GPU VA in the minimal GPGPU probe.
pub static HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS: [u32; 20] = [
    0x80030061,
    0x04054660,
    0x00000000,
    STORE_SENTINEL_U32,
    0x80030061,
    0x7F054220,
    0x00000000,
    0x00840058,
    0x00030131,
    0x00000000,
    0xCDFA7F0C,
    0x009A040C,
    0x80030061,
    0x7F050220,
    0x00460005,
    0x00000000,
    0x80030131,
    0x00000004,
    0x70007F0C,
    0x00000000,
];

pub static HDC1_STATELESS_STORE_THEN_TS_EOT: EuArtifact = EuArtifact {
    name: "gfx12-hdc1-stateless-store-then-ts-eot",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::Hdc1BtiStoreThenThreadSpawnerEot,
    words: &HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

// Mesa brw_asm source, dependency-shaped tiny ALU rung:
//
// mov(8)   g2<1>D      0xC0DE7729D
// mov(8)   g6<1>D      0x01020304D
// mov(8)   g7<1>D      0x01010101D
// dp4a(8)  g4<1>D      g2<8,8,1>D  g6<8,8,1>D  g7<1,1,1>D
// mov(8)   g127<1>UD   0x00840058UD
// send     HDC1 untyped surface write, stateless/non-coherent BTI 253, SIMD8
// mov(8)   g127<1>UD   g0<8,8,1>UD
// send     Thread Spawner EOT from g127
//
// The dot is 1+2+3+4 = 10, so the value stored by the HDC send is not a
// directly moved canary. It is STATIC_DP4A_BASE_U32 + 10 = STORE_SENTINEL_U32.
pub static STATIC_DP4A_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS: [u32; 32] = [
    0x80030061,
    0x02054660,
    0x00000000,
    STATIC_DP4A_BASE_U32,
    0x80030061,
    0x06054660,
    0x00000000,
    0x01020304,
    0x80030061,
    0x07054660,
    0x00000000,
    0x01010101,
    0x00030158,
    0x04040E68,
    0x0E0E0205,
    0x07050605,
    0x80030061,
    0x7F054220,
    0x00000000,
    0x00840058,
    0x00030131,
    0x00000000,
    0xCDFA7F0C,
    0x009A040C,
    0x80030061,
    0x7F050220,
    0x00460005,
    0x00000000,
    0x80030131,
    0x00000004,
    0x70007F0C,
    0x00000000,
];

pub static STATIC_DP4A_HDC1_STATELESS_STORE_THEN_TS_EOT: EuArtifact = EuArtifact {
    name: "gfx12-static-dp4a-hdc1-stateless-store-then-ts-eot",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::StaticDp4aThenHdc1StoreThenThreadSpawnerEot,
    words: &STATIC_DP4A_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

// T4 catalog rung: this intentionally reuses the proven T3 shell until the
// live activation load/send encoding is filled in.  The artifact kind and name
// let the boot harness and Lumen sidepath describe the next exact contract
// without risking the known-good static DP4A proof.
pub const T4_LIVE_X_STATIC_DP4A_LANES: usize = 4;
pub const T4_LIVE_X_STATIC_DP4A_WEIGHTS_U8: [u8; T4_LIVE_X_STATIC_DP4A_LANES] = [1, 2, 3, 4];
pub const T4_LIVE_X_STATIC_DP4A_EXPECTS_GPU_LOAD: bool = true;

pub static T4_LIVE_X_STATIC_DP4A_REQUIREMENT_HDC1_STORE_THEN_TS_EOT: EuArtifact = EuArtifact {
    name: "gfx12-t4-live-x-static-dp4a-requirement-hdc1-store-then-ts-eot",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::LiveXStaticDp4aRequirementThenHdc1StoreThenThreadSpawnerEot,
    words: &STATIC_DP4A_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

// T5 is the first rung whose success condition is a real one-row matvec:
// load the staged live f32 activation vector, load one staged BF16 model row,
// multiply/reduce, and store the computed row output.  Older T47/T48 artifacts
// remain preserved as sentinel/echo controls and must not satisfy this rung.
pub const T5_ONE_ROW_MATVEC_PROGRAM_NAME: &str =
    "gfx12-t5-one-row-live-bf16-matvec-hdc1-stateless-store-then-ts-eot";
pub const T5_ONE_ROW_MATVEC_LIVE_K: usize = 2048;
pub const T5_ONE_ROW_MATVEC_REQUIRES_LIVE_GPU_LOAD: bool = true;

#[cfg(feature = "alloc")]
pub mod builder {
    use alloc::vec::Vec;

    use super::{
        HDC1_BTI34_STORE_THEN_TS_EOT_WORDS, TS_EOT_R0_TO_G112_WORDS, TS_EOT_R0_TO_G120_WORDS,
        TS_EOT_R0_TO_G126_WORDS, TS_EOT_R0_TO_G127_SEND1_WORDS, TS_EOT_R0_TO_G127_WORDS,
    };

    #[derive(Clone, Debug, Default)]
    pub struct Program {
        words: Vec<u32>,
    }

    impl Program {
        pub fn new() -> Self {
            Self { words: Vec::new() }
        }

        pub fn push_template(&mut self, words: &[u32]) {
            self.words.extend_from_slice(words);
        }

        pub fn push_ts_eot_r0_to_g127(&mut self) {
            self.push_template(&TS_EOT_R0_TO_G127_WORDS);
        }

        pub fn push_ts_eot_r0_to_g126(&mut self) {
            self.push_template(&TS_EOT_R0_TO_G126_WORDS);
        }

        pub fn push_ts_eot_r0_to_g120(&mut self) {
            self.push_template(&TS_EOT_R0_TO_G120_WORDS);
        }

        pub fn push_ts_eot_r0_to_g112(&mut self) {
            self.push_template(&TS_EOT_R0_TO_G112_WORDS);
        }

        pub fn push_ts_eot_r0_to_g127_send1(&mut self) {
            self.push_template(&TS_EOT_R0_TO_G127_SEND1_WORDS);
        }

        pub fn push_hdc1_bti34_store_then_ts_eot(&mut self) {
            self.push_template(&HDC1_BTI34_STORE_THEN_TS_EOT_WORDS);
        }

        pub fn words(&self) -> &[u32] {
            &self.words
        }

        pub fn into_words(self) -> Vec<u32> {
            self.words
        }
    }
}
