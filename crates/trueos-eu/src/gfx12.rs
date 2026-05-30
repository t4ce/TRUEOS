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
pub static ILLEGAL_ALL_ONES_WORDS: [u32; 4] = [0xFFFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFF];

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
// mov(8)  g126<1>UD  g0<8,8,1>UD
// send    Thread Spawner EOT from g126
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
    0x7E050220,
    0x00460005,
    0x00000000,
    0x80030131,
    0x00000004,
    0x70007E0C,
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
// mov(8)  g126<1>UD  g0<8,8,1>UD
// send    Thread Spawner EOT from g126
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
    0x7E050220,
    0x00460005,
    0x00000000,
    0x80030131,
    0x00000004,
    0x70007E0C,
    0x00000000,
];

pub static HDC1_STATELESS_STORE_THEN_TS_EOT: EuArtifact = EuArtifact {
    name: "gfx12-hdc1-stateless-store-then-ts-eot",
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::Hdc1BtiStoreThenThreadSpawnerEot,
    words: &HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

pub const C4_STORE_IMM32_STATELESS_WORDS: usize = HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS.len();

pub fn c4_store_imm32_stateless_words(value: u32) -> [u32; C4_STORE_IMM32_STATELESS_WORDS] {
    let mut words = HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS;
    words[HDC1_BTI34_STORE_IMM_DWORD] = value;
    words
}

// Mesa brw_asm source, dependency-shaped tiny ALU rung:
//
// mov(8)   g2<1>D      0xC0DE7729D
// mov(8)   g6<1>D      0x01020304D
// mov(8)   g7<1>D      0x01010101D
// dp4a(8)  g4<1>D      g2<8,8,1>D  g6<8,8,1>D  g7<1,1,1>D
// mov(8)   g127<1>UD   0x00840058UD
// send     HDC1 untyped surface write, stateless/non-coherent BTI 253, SIMD8
// mov(8)   g126<1>UD   g0<8,8,1>UD
// send     Thread Spawner EOT from g126
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
    0x7E050220,
    0x00460005,
    0x00000000,
    0x80030131,
    0x00000004,
    0x70007E0C,
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
    "gfx12-t5-small-live4-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T5_ONE_ROW_MATVEC_LIVE_K: usize = 4;
pub const T5_ONE_ROW_MATVEC_REQUIRES_LIVE_GPU_LOAD: bool = true;
pub const T5_SMALL_LIVE4_TRUEOS_ARENA_EXPECTED_SENTINEL_U32: u32 = 0xC0DE_7505;
pub const T5_SMALL_LIVE4_TRUEOS_ARENA_STORE_SEND_DWORD: usize = 73;
pub const T5_SMALL_LIVE4_TRUEOS_ARENA_SENTINEL_DWORD: usize = 19;
pub const T5_SMALL_LIVE4_WORD_VIEW_PROGRAM_NAME: &str =
    "gfx12-t5-small-live4-word-view-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T5_SMALL_LIVE4_PROVEN_HDC_PROGRAM_NAME: &str =
    "gfx12-t5-small-live4-bf16-dot-proven-hdc-store-then-ts-eot";
pub const T5_SMALL_LIVE4_PROVEN_HDC_STORE_SEND_DWORD: usize = 77;
pub const T5_SMALL_LIVE4_PROVEN_HDC_SENTINEL_DWORD: usize = 19;
pub const T5_TRUEOS_ARENA_OUTPUT_GPU_U32: u32 = 0x0410_2000;
pub const T5_STORE_ONLY_ARENA_PROGRAM_NAME: &str =
    "gfx12-t5-store-only-arena-offset-hdc1-store-then-ts-eot";
pub const T5_STORE_ONLY_ARENA_EXPECTED_RESULT_U32: u32 = 0xC0DE_7506;
pub const T5_STORE_ONLY_ARENA_STORE_SEND_DWORD: usize = 31;
pub const T5_STORE_ONLY_ARENA_SENTINEL_DWORD: usize = 19;
pub const T5_STORE_ONLY_PROVEN_HDC_PROGRAM_NAME: &str =
    "gfx12-t5-store-only-proven-hdc-output-then-ts-eot";
pub const T5_STORE_ONLY_PROVEN_HDC_STORE_SEND_DWORD: usize = 11;
pub const T5_STORE_ONLY_PROVEN_HDC_IMM_DWORD: usize = 3;
pub const T5_LOAD_ECHO_TRUEOS_ARENA_PROGRAM_NAME: &str =
    "gfx12-t5-load-echo-live4-raw-operands-hdc1-store-then-ts-eot";
pub const T5_LOAD_ECHO_TRUEOS_ARENA_FIRST_STORE_SEND_DWORD: usize = 51;
pub const T5_LOAD_ECHO_TRUEOS_ARENA_SECOND_STORE_SEND_DWORD: usize = 79;
pub const T6_ONE_ROW_MATVEC_PROGRAM_NAME: &str =
    "gfx12-t6-small-live8-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T6_ONE_ROW_MATVEC_LIVE_K: usize = 8;
pub const T6_SMALL_LIVE8_TRUEOS_ARENA_EXPECTED_SENTINEL_U32: u32 = 0xC0DE_7606;
pub const T6_SMALL_LIVE8_TRUEOS_ARENA_STORE_SEND_DWORD: usize = 97;
pub const T6_SMALL_LIVE8_TRUEOS_ARENA_SENTINEL_DWORD: usize = 19;
pub const T61_ONE_ROW_MATVEC_PROGRAM_NAME: &str =
    "gfx12-t6-1-live16-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T61_ONE_ROW_MATVEC_LIVE_K: usize = 16;
pub const T61_LIVE16_TRUEOS_ARENA_EXPECTED_SENTINEL_U32: u32 = 0xC0DE_7616;
pub const T61_LIVE16_TRUEOS_ARENA_STORE_SEND_DWORD: usize = 145;
pub const T61_LIVE16_TRUEOS_ARENA_SENTINEL_DWORD: usize = 19;
pub const T62_ROW_INDEXED_LIVE16_PROGRAM_NAME: &str =
    "gfx12-t6-2-lane-indexed-live16-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T62_ROW_INDEXED_LIVE_K: usize = 16;
pub const T62_ROW_INDEXED_PARTIAL_ROWS: usize = 8;
pub const T62_ROW_INDEXED_LIVE16_STORE_SEND_DWORD: usize = 193;
pub const T8_GROUPID_LIVE16_PROGRAM_NAME: &str =
    "gfx12-t8-groupid-live16-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T8_GROUPID_LIVE_K: usize = 16;
pub const T8_GROUPID_LIVE16_STORE_SEND_DWORD: usize = 186;
pub const T63_LANE_INDEXED_LIVE32_PROGRAM_NAME: &str =
    "gfx12-t6-3-lane-indexed-live32-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T63_LANE_INDEXED_LIVE_K: usize = 32;
pub const T63_LANE_INDEXED_PARTIAL_ROWS: usize = 8;
pub const T63_LANE_INDEXED_LIVE32_STORE_SEND_DWORD: usize = 357;
pub const T63_ACCUM16_HI_LIVE32_PROGRAM_NAME: &str =
    "gfx12-t6-3-accum16-hi-live32-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T63_ACCUM16_HI_LIVE32_LIVE_K: usize = 32;
pub const T63_ACCUM16_HI_LIVE32_PARTIAL_ROWS: usize = 8;
pub const T63_ACCUM16_HI_LIVE32_STORE_SEND_DWORD: usize = 201;
pub const T64_WINDOWED_ACCUM16_LIVE48_PROGRAM_NAME: &str =
    "gfx12-t6-4-windowed-accum16-live48-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T64_WINDOWED_ACCUM16_LIVE48_LIVE_K: usize = 48;
pub const T64_WINDOWED_ACCUM16_LIVE48_WINDOW_START: usize = 32;
pub const T65_WINDOWED_ACCUM16_LIVE64_PROGRAM_NAME: &str =
    "gfx12-t6-5-windowed-accum16-live64-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T65_WINDOWED_ACCUM16_LIVE64_LIVE_K: usize = 64;
pub const T65_WINDOWED_ACCUM16_LIVE64_WINDOW_START: usize = 48;
pub const T66_WINDOWED_ACCUM16_LIVE80_PROGRAM_NAME: &str =
    "gfx12-t6-6-windowed-accum16-live80-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T66_WINDOWED_ACCUM16_LIVE80_LIVE_K: usize = 80;
pub const T66_WINDOWED_ACCUM16_LIVE80_WINDOW_START: usize = 64;
pub const T67_WINDOWED_ACCUM16_LIVE96_PROGRAM_NAME: &str =
    "gfx12-t6-7-windowed-accum16-live96-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T67_WINDOWED_ACCUM16_LIVE96_LIVE_K: usize = 96;
pub const T67_WINDOWED_ACCUM16_LIVE96_WINDOW_START: usize = 80;
pub const T68_WINDOWED_ACCUM16_LIVE112_PROGRAM_NAME: &str =
    "gfx12-t6-8-windowed-accum16-live112-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T68_WINDOWED_ACCUM16_LIVE112_LIVE_K: usize = 112;
pub const T68_WINDOWED_ACCUM16_LIVE112_WINDOW_START: usize = 96;
pub const T69_WINDOWED_ACCUM16_LIVE128_PROGRAM_NAME: &str =
    "gfx12-t6-9-windowed-accum16-live128-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T69_WINDOWED_ACCUM16_LIVE128_LIVE_K: usize = 128;
pub const T69_WINDOWED_ACCUM16_LIVE128_WINDOW_START: usize = 112;
pub const T610_WINDOWED_ACCUM16_LIVE144_PROGRAM_NAME: &str =
    "gfx12-t6-10-windowed-accum16-live144-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T610_WINDOWED_ACCUM16_LIVE144_LIVE_K: usize = 144;
pub const T610_WINDOWED_ACCUM16_LIVE144_WINDOW_START: usize = 128;
pub const T611_WINDOWED_ACCUM16_LIVE160_PROGRAM_NAME: &str =
    "gfx12-t6-11-windowed-accum16-live160-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T611_WINDOWED_ACCUM16_LIVE160_LIVE_K: usize = 160;
pub const T611_WINDOWED_ACCUM16_LIVE160_WINDOW_START: usize = 144;
pub const T66_ACCUM32_HI_LIVE96_PROGRAM_NAME: &str =
    "gfx12-t6-6-accum32-hi-live96-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T66_ACCUM32_HI_LIVE96_LIVE_K: usize = 96;
pub const T66_ACCUM32_HI_LIVE96_PARTIAL_ROWS: usize = 8;
pub const T66_ACCUM32_HI_LIVE96_STORE_SEND_DWORD: usize = 374;
pub const PRIMARY_SCANOUT_MANDELBROT8_PROGRAM_NAME: &str =
    "gfx12-primary-scanout-mandelbrot32-scalar-strip-hdc1-stateless-store-then-ts-eot";
pub const PRIMARY_SCANOUT_MANDELBROT8_LANES: usize = 32;
pub const PRIMARY_SCANOUT_MANDELBROT8_COLOR_DWORDS: [usize; 32] = [
    3, 15, 27, 39, 51, 63, 75, 87, 99, 111, 123, 135, 147, 159, 171, 183, 195, 207, 219, 231, 243,
    255, 267, 279, 291, 303, 315, 327, 339, 351, 363, 375,
];
pub const PRIMARY_SCANOUT_MANDELBROT8_ADDRESS_DWORDS: [usize; 32] = [
    7, 19, 31, 43, 55, 67, 79, 91, 103, 115, 127, 139, 151, 163, 175, 187, 199, 211, 223, 235, 247,
    259, 271, 283, 295, 307, 319, 331, 343, 355, 367, 379,
];
pub const PRIMARY_SCANOUT_MANDELBROT8_STORE_SEND_DWORD: usize = 11;
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_COORD_PROGRAM_NAME: &str =
    "gfx12-primary-scanout-mandelbrot8-simd8-coord-color-hdc1-stateless-store-then-ts-eot";
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_COORD_LANES: usize = 8;
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_COORD_X_BASE_DWORD: usize = 11;
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_COORD_COLOR_SEED_DWORD: usize = 19;
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_COORD_ADDRESS_BASE_DWORD: usize = 35;
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_COORD_STORE_SEND_DWORD: usize = 38;
pub const PRIMARY_SCANOUT_LINE8_SCALAR8_BW_PROGRAM_NAME: &str =
    "gfx12-primary-scanout-line8-scalar8-bw-hdc1-stateless-unrolled-store-then-ts-eot";
pub const PRIMARY_SCANOUT_LINE8_SCALAR8_BW_LANES: usize = 8;
pub const PRIMARY_SCANOUT_LINE8_SCALAR8_BW_ADDRESS_BASE_DWORD: usize = 7;
pub const PRIMARY_SCANOUT_LINE8_SCALAR8_BW_COLOR_DWORD: usize = 3;
pub const PRIMARY_SCANOUT_LINE8_SCALAR8_BW_STORE_SEND_DWORD: usize = 11;
pub const PRIMARY_SCANOUT_LINE320_SCALAR_BW_PROGRAM_NAME: &str =
    "gfx12-primary-scanout-line320-scalar-bw-hdc1-stateless-unrolled-store-then-ts-eot";
pub const PRIMARY_SCANOUT_LINE320_SCALAR_BW_LANES: usize = 320;
pub const PRIMARY_SCANOUT_LINE320_SCALAR_BW_WORD_DWORDS: usize =
    PRIMARY_SCANOUT_LINE320_SCALAR_BW_LANES * 8 + 12;
pub const PRIMARY_SCANOUT_LINE320_SCALAR_BW_ADDRESS_BASE_DWORD: usize = 7;
pub const PRIMARY_SCANOUT_LINE320_SCALAR_BW_COLOR_DWORD: usize = 3;
pub const PRIMARY_SCANOUT_LINE320_SCALAR_BW_STORE_SEND_DWORD: usize = 11;
pub const PRIMARY_SCANOUT_LINE1280_SCALAR_BW_PROGRAM_NAME: &str =
    "gfx12-primary-scanout-line1280-scalar-step8-color-hdc1-stateless-unrolled-store-then-ts-eot";
pub const PRIMARY_SCANOUT_LINE1280_SCALAR_BW_LANES: usize = 1280;
pub const PRIMARY_SCANOUT_LINE1280_SCALAR_BW_COLOR_STEP_PIXELS: usize = 8;
pub const PRIMARY_SCANOUT_LINE1280_SCALAR_BW_COLOR_STEP_DWORDS: usize =
    (PRIMARY_SCANOUT_LINE1280_SCALAR_BW_LANES - 1)
        / PRIMARY_SCANOUT_LINE1280_SCALAR_BW_COLOR_STEP_PIXELS;
pub const PRIMARY_SCANOUT_LINE1280_SCALAR_BW_WORD_DWORDS: usize =
    PRIMARY_SCANOUT_LINE1280_SCALAR_BW_LANES * 8
        + PRIMARY_SCANOUT_LINE1280_SCALAR_BW_COLOR_STEP_DWORDS * 4
        + 12;
pub const PRIMARY_SCANOUT_LINE1280_SCALAR_BW_ADDRESS_BASE_DWORD: usize = 7;
pub const PRIMARY_SCANOUT_LINE1280_SCALAR_BW_COLOR_DWORD: usize = 3;
pub const PRIMARY_SCANOUT_LINE1280_SCALAR_BW_STORE_SEND_DWORD: usize = 11;
pub const PRIMARY_SCANOUT_LINE1280_ADDRCOLOR_SCALAR_BW_PROGRAM_NAME: &str = "gfx12-primary-scanout-line1280-scalar-address-color-step8-hdc1-stateless-unrolled-store-then-ts-eot";
pub const PRIMARY_SCANOUT_LINE1280_ADDRCOLOR_SCALAR_BW_LANES: usize =
    PRIMARY_SCANOUT_LINE1280_SCALAR_BW_LANES;
pub const PRIMARY_SCANOUT_LINE1280_ADDRCOLOR_SCALAR_BW_WORD_DWORDS: usize =
    PRIMARY_SCANOUT_LINE1280_SCALAR_BW_WORD_DWORDS + 4;
pub const PRIMARY_SCANOUT_LINE1280_ADDRCOLOR_SCALAR_BW_ADDRESS_BASE_DWORD: usize = 7;
pub const PRIMARY_SCANOUT_LINE1280_ADDRCOLOR_SCALAR_BW_COLOR_SEED_DWORD: usize = 11;
pub const PRIMARY_SCANOUT_LINE1280_ADDRCOLOR_SCALAR_BW_STORE_SEND_DWORD: usize = 15;
pub const PRIMARY_SCANOUT_LINE1280_LANE8ROWS_PROGRAM_NAME: &str = "gfx12-primary-scanout-line1280-lane8rows-address-color-step8-hdc1-t62-send-bti1-simd8-store-then-ts-eot";
pub const PRIMARY_SCANOUT_LINE1280_LANE8ROWS_LANES: usize =
    PRIMARY_SCANOUT_LINE1280_SCALAR_BW_LANES;
pub const PRIMARY_SCANOUT_LINE1280_LANE8ROWS_ROWS: usize = 8;
pub const PRIMARY_SCANOUT_LINE1280_LANE8ROWS_PITCH_BYTES: u32 = 0x2800;
pub const PRIMARY_SCANOUT_LINE1280_LANE8ROWS_COLOR_STEP_PIXELS: usize =
    PRIMARY_SCANOUT_LINE1280_SCALAR_BW_COLOR_STEP_PIXELS;
pub const PRIMARY_SCANOUT_LINE1280_LANE8ROWS_COLOR_STEP_DWORDS: usize =
    PRIMARY_SCANOUT_LINE1280_SCALAR_BW_COLOR_STEP_DWORDS;
pub const PRIMARY_SCANOUT_LINE1280_LANE8ROWS_WORD_DWORDS: usize = 20
    + PRIMARY_SCANOUT_LINE1280_LANE8ROWS_LANES * 4
    + (PRIMARY_SCANOUT_LINE1280_LANE8ROWS_LANES - 1) * 4
    + PRIMARY_SCANOUT_LINE1280_LANE8ROWS_COLOR_STEP_DWORDS * 4
    + 8;
pub const PRIMARY_SCANOUT_LINE1280_LANE8ROWS_ADDRESS_BASE_DWORD: usize = 15;
pub const PRIMARY_SCANOUT_LINE1280_LANE8ROWS_COLOR_SEED_DWORD: usize = 19;
pub const PRIMARY_SCANOUT_LINE1280_LANE8ROWS_STORE_SEND_DWORD: usize = 23;
pub const PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_PROGRAM_NAME: &str =
    "gfx12-primary-scanout-groupid-line320-scalar-bw-hdc1-stateless-unrolled-store-then-ts-eot";
pub const PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_GROUPS: usize = 8;
pub const PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_LANES: usize =
    PRIMARY_SCANOUT_LINE320_SCALAR_BW_LANES;
pub const PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_STRIDE_SHIFT: usize = 12;
pub const PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_WORD_DWORDS: usize =
    PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_LANES * 8 + 34;
pub const PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_ADDRESS_BASE_DWORD: usize = 25;
pub const PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_COLOR_DWORD: usize = 29;
pub const PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_STORE_SEND_DWORD: usize = 30;
pub const PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_PROGRAM_NAME: &str = "gfx12-primary-scanout-groupid-line1280-rows-scalar-step8-color-hdc1-stateless-unrolled-store-then-ts-eot";
pub const PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_LANES: usize =
    PRIMARY_SCANOUT_LINE1280_SCALAR_BW_LANES;
pub const PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_PITCH_BYTES: u32 = 0x2800;
pub const PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_COLOR_STEP_PIXELS: usize =
    PRIMARY_SCANOUT_LINE1280_SCALAR_BW_COLOR_STEP_PIXELS;
pub const PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_COLOR_STEP_DWORDS: usize =
    PRIMARY_SCANOUT_LINE1280_SCALAR_BW_COLOR_STEP_DWORDS;
pub const PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_WORD_DWORDS: usize =
    PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_LANES * 8
        + PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_COLOR_STEP_DWORDS * 4
        + 34;
pub const PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_ADDRESS_BASE_DWORD: usize = 25;
pub const PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_COLOR_DWORD: usize = 29;
pub const PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_STORE_SEND_DWORD: usize = 33;
pub const PRIMARY_SCANOUT_ROW2560_SIMD8_BW_PROGRAM_NAME: &str =
    "gfx12-primary-scanout-row2560-simd8-bw-hdc1-stateless-unrolled-store-then-ts-eot";
pub const PRIMARY_SCANOUT_ROW2560_SIMD8_BW_PIXELS: usize = 2560;
pub const PRIMARY_SCANOUT_ROW2560_SIMD8_BW_SENDS: usize =
    PRIMARY_SCANOUT_ROW2560_SIMD8_BW_PIXELS / PRIMARY_SCANOUT_LINE8_SIMD8_BW_LANES;
pub const PRIMARY_SCANOUT_ROW2560_SIMD8_BW_WORD_DWORDS: usize =
    20 + 4 + ((PRIMARY_SCANOUT_ROW2560_SIMD8_BW_SENDS - 1) * 8) + 8;
pub const PRIMARY_SCANOUT_ROW2560_SIMD8_BW_ADDRESS_BASE_DWORD: usize = 15;
pub const PRIMARY_SCANOUT_ROW2560_SIMD8_BW_COLOR_DWORD: usize = 19;
pub const PRIMARY_SCANOUT_ROW2560_SIMD8_BW_STORE_SEND_DWORD: usize = 23;
pub const PRIMARY_SCANOUT_ROW2560_SIMD16_BW_PROGRAM_NAME: &str =
    "gfx12-primary-scanout-row2560-simd16-bw-hdc1-stateless-unrolled-store-then-ts-eot";
pub const PRIMARY_SCANOUT_ROW2560_SIMD16_BW_PIXELS: usize = 2560;
pub const PRIMARY_SCANOUT_ROW2560_SIMD16_BW_LANES: usize = 16;
pub const PRIMARY_SCANOUT_ROW2560_SIMD16_BW_SENDS: usize =
    PRIMARY_SCANOUT_ROW2560_SIMD16_BW_PIXELS / PRIMARY_SCANOUT_ROW2560_SIMD16_BW_LANES;
pub const PRIMARY_SCANOUT_ROW2560_SIMD16_BW_WORD_DWORDS: usize =
    28 + 4 + ((PRIMARY_SCANOUT_ROW2560_SIMD16_BW_SENDS - 1) * 8) + 8;
pub const PRIMARY_SCANOUT_ROW2560_SIMD16_BW_ADDRESS_BASE_DWORD: usize = 19;
pub const PRIMARY_SCANOUT_ROW2560_SIMD16_BW_COLOR_DWORD: usize = 27;
pub const PRIMARY_SCANOUT_ROW2560_SIMD16_BW_STORE_SEND_DWORD: usize = 28;
pub const PRIMARY_SCANOUT_LINE8_SIMD8_BW_PROGRAM_NAME: &str =
    "gfx12-primary-scanout-line8-simd8-bw-hdc1-bti1-offset-store-then-ts-eot";
pub const PRIMARY_SCANOUT_LINE8_SIMD8_BW_LANES: usize = 8;
pub const PRIMARY_SCANOUT_LINE8_SIMD8_BW_ADDRESS_BASE_DWORD: usize = 15;
pub const PRIMARY_SCANOUT_LINE8_SIMD8_BW_COLOR_DWORD: usize = 19;
pub const PRIMARY_SCANOUT_LINE8_SIMD8_BW_STORE_SEND_DWORD: usize = 23;

// T5 diagnostic control: preserve the T5 arena payload shape and final HDC1
// send, but remove live loads and math.  It should write:
// output[0] = 0xC0DE7506, output[1] = 4, output[2] = 0xC0DE7505,
// output[3] = 0.  If this retires without changing output, the problem is in
// the T5 store/payload/surface contract rather than BF16 load or reduction.
pub static T5_STORE_ONLY_ARENA_OFFSET_HDC1_STORE_THEN_TS_EOT_WORDS: [u32; 42] = [
    0x80030061,
    0x0A050220,
    0x00000024,
    0x00000000,
    0x80030061,
    0x0B054220,
    0x00000000,
    0x00000000,
    0x80030061,
    0x0C054220,
    0x00000000,
    0x00000000,
    0x80030061,
    0x0F054220,
    0x00000000,
    T5_ONE_ROW_MATVEC_LIVE_K as u32,
    0x80030061,
    0x10054220,
    0x00000000,
    T5_SMALL_LIVE4_TRUEOS_ARENA_EXPECTED_SENTINEL_U32,
    0x00030061,
    0x0D054660,
    0x00000000,
    0x00102000,
    0x80030061,
    0x0E054220,
    0x00000000,
    T5_STORE_ONLY_ARENA_EXPECTED_RESULT_U32,
    0x00034231,
    0x00000000,
    0xC0020D0C,
    0x00980E24,
    0x80030061,
    0x7E050220,
    0x00460005,
    0x00000000,
    0x80030131,
    0x00000004,
    0x70007E0C,
    0x00000000,
    0x20000060,
    0x00000000,
];

pub static T5_STORE_ONLY_ARENA_OFFSET_HDC1_STORE_THEN_TS_EOT: EuArtifact = EuArtifact {
    name: T5_STORE_ONLY_ARENA_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::T5StoreOnlyArenaOffsetThenHdc1StoreThenThreadSpawnerEot,
    words: &T5_STORE_ONLY_ARENA_OFFSET_HDC1_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

// T5 store bridge control: same target output slot as the T5 arena rung, but
// using the exact stateless HDC send contract already proven by T47/T48.
// This intentionally leaves the surface-indexed T5 control above preserved as
// the negative-control artifact.
pub static T5_STORE_ONLY_PROVEN_HDC_OUTPUT_THEN_TS_EOT_WORDS: [u32; 20] = [
    0x80030061,
    0x04054660,
    0x00000000,
    T5_STORE_ONLY_ARENA_EXPECTED_RESULT_U32,
    0x80030061,
    0x7F054220,
    0x00000000,
    T5_TRUEOS_ARENA_OUTPUT_GPU_U32,
    0x00030131,
    0x00000000,
    0xCDFA7F0C,
    0x009A040C,
    0x80030061,
    0x7E050220,
    0x00460005,
    0x00000000,
    0x80030131,
    0x00000004,
    0x70007E0C,
    0x00000000,
];

pub static T5_STORE_ONLY_PROVEN_HDC_OUTPUT_THEN_TS_EOT: EuArtifact = EuArtifact {
    name: T5_STORE_ONLY_PROVEN_HDC_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::T5StoreOnlyArenaOffsetThenHdc1StoreThenThreadSpawnerEot,
    words: &T5_STORE_ONLY_PROVEN_HDC_OUTPUT_THEN_TS_EOT_WORDS,
    expects_store: true,
};

// T5 live-load echo: generated from the same TRUEOS-arena SSBO shape as the
// small live4 dot, but with the ALU removed.  It loads raw dwords from:
// - x words at arena + 0x0
// - row words at arena + 0x2000
// and stores:
// - output[0..4] = observed x dwords
// - output[4..8] = observed row dwords
// This proves the EU-side load addresses and raw payload before BF16 unpacking
// or reduction are allowed to decide the T5 result.
pub static T5_LOAD_ECHO_TRUEOS_ARENA_RAW_OPERANDS_HDC1_STORE_THEN_TS_EOT_WORDS: [u32; 88] = [
    0x80030061, 0x03054220, 0x00000000, 0x00000000, 0x80030061, 0x04054220, 0x00000000, 0x00000000,
    0x00030061, 0x05054660, 0x00000000, 0x00102000, 0x80000361, 0x03454620, 0x00000000, 0x00000000,
    0x80000361, 0x04454620, 0x00000000, 0x00002000, 0x8003A031, 0x010C0000, 0xA402030C, 0x02100000,
    0x80039131, 0x020C0000, 0xA402040C, 0x02100000, 0x80002001, 0x00000000, 0x00000000, 0x00000000,
    0x00030061, 0x06050220, 0x00000104, 0x00000000, 0x00030061, 0x07050220, 0x00000124, 0x00000000,
    0x00030061, 0x08050220, 0x00000144, 0x00000000, 0x00030061, 0x09050220, 0x00000164, 0x00000000,
    0x00039231, 0x00000000, 0xC002050C, 0x00980624, 0x00033261, 0x07054660, 0x00000000, 0x00102010,
    0x80002101, 0x00000000, 0x00000000, 0x00000000, 0x00033261, 0x08050220, 0x00000204, 0x00000000,
    0x00033261, 0x09050220, 0x00000224, 0x00000000, 0x00030061, 0x0A050220, 0x00000244, 0x00000000,
    0x00030061, 0x0B050220, 0x00000264, 0x00000000, 0x00039331, 0x00000000, 0xC002070C, 0x00980824,
    0x80030061, 0x7E050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x70007E0C, 0x00000000,
];

pub static T5_LOAD_ECHO_TRUEOS_ARENA_RAW_OPERANDS_HDC1_STORE_THEN_TS_EOT: EuArtifact = EuArtifact {
    name: T5_LOAD_ECHO_TRUEOS_ARENA_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::T5SmallLive4Bf16DotThenHdc1StoreThenThreadSpawnerEot,
    words: &T5_LOAD_ECHO_TRUEOS_ARENA_RAW_OPERANDS_HDC1_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

// Preserved T5 word-view artifact from `.codex_tmp/t5_small_live4_trueos_arena.comp`.
//
// This was the first live-load/math T5 kernel.  It proved the EU can read the
// staged x vector and row words, multiply/reduce, and store to the output slot,
// but its `uint words[]` row view consumed BF16 lanes [0,2,4,6].  Keep it as a
// control artifact; the active T5 matvec rung below unpacks packed BF16 halves.
pub static T5_SMALL_LIVE4_TRUEOS_ARENA_BF16_DOT_WORD_VIEW_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS:
    [u32; 84] = [
    0x80030061, 0x0A050220, 0x00000024, 0x00000000, 0x80030061, 0x0B054220, 0x00000000, 0x00000000,
    0x80030061, 0x0C054220, 0x00000000, 0x00000000, 0x80030061, 0x0F054220, 0x00000000, 0x00000004,
    0x80030061, 0x10054220, 0x00000000, 0xC0DE7505, 0x00030061, 0x0D054660, 0x00000000, 0x00102000,
    0xA4110640, 0x01110A0A, 0x80000661, 0x0B454620, 0x00000000, 0x00000000, 0x80000661, 0x0C454620,
    0x00000000, 0x00002000, 0x8003A031, 0x010C0000, 0xA4020B0C, 0x02100000, 0x80039131, 0x030C0000,
    0xA4020C0C, 0x02100000, 0x80032169, 0x02058660, 0x02000304, 0x00000010, 0x80030069, 0x05058660,
    0x02000324, 0x00000010, 0x80030069, 0x04058660, 0x02000344, 0x00000010, 0x80030069, 0x06058660,
    0x02000364, 0x00000010, 0x2407B041, 0x05110120, 0xA308015B, 0x02010704, 0xA309015B, 0x04010834,
    0xA30E015B, 0x0601092C, 0x80000101, 0x00000000, 0x00000000, 0x00000000, 0x00034231, 0x00000000,
    0xC0020D0C, 0x00980E24, 0x80030061, 0x7E050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004,
    0x70007E0C, 0x00000000, 0x20000060, 0x00000000,
];

pub static T5_SMALL_LIVE4_TRUEOS_ARENA_BF16_DOT_WORD_VIEW_HDC1_STATELESS_STORE_THEN_TS_EOT:
    EuArtifact = EuArtifact {
    name: T5_SMALL_LIVE4_WORD_VIEW_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::T5SmallLive4Bf16DotThenHdc1StoreThenThreadSpawnerEot,
    words: &T5_SMALL_LIVE4_TRUEOS_ARENA_BF16_DOT_WORD_VIEW_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

// Mesa ANV oracle artifact from `.codex_tmp/t5_small_live4_trueos_arena_bf16_unpack.comp`.
//
// Contract:
// - one SSBO bound at the TRUEOS GPGPU tile arena base
// - x f32 words at arena + 0x0
// - packed BF16 row words at arena + 0x2000
// - output words at arena + 0x102000
// - output[0] = dot(x[0..4], bf16(row halves [0,1,2,3]))
// - output[1] = 4
// - output[2] = 0xC0DE7505
// - output[3] = workgroup id x
pub static T5_SMALL_LIVE4_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS: [u32; 84] = [
    0x80030061, 0x0B050220, 0x00000024, 0x00000000, 0x80030061, 0x0C054220, 0x00000000, 0x00000000,
    0x00030061, 0x0D054660, 0x00000000, 0x00002000, 0x00030061, 0x10054220, 0x00000000, 0x00000004,
    0x00030061, 0x11054220, 0x00000000, 0xC0DE7505, 0x00030061, 0x0E054660, 0x00000000, 0x00102000,
    0xA4120640, 0x01110B0A, 0x80000661, 0x0C454620, 0x00000000, 0x00000000, 0x0003E031, 0x02140000,
    0xC8020D0C, 0x001A0000, 0x80039131, 0x010C0000, 0xA4020C0C, 0x02100000, 0x00032069, 0x04058660,
    0x02460205, 0x00000010, 0x00030065, 0x05058220, 0x02460205, 0xFFFF0000, 0x00032069, 0x06058660,
    0x02460305, 0x00000010, 0x00030065, 0x08058220, 0x02460305, 0xFFFF0000, 0x80002101, 0x00000000,
    0x00000000, 0x00000000, 0x21070341, 0x05010120, 0xE109015B, 0x04010700, 0xE10A015B, 0x06010930,
    0xE10F015B, 0x08010A28, 0x80000101, 0x00000000, 0x00000000, 0x00000000, 0x00034231, 0x00000000,
    0xC0020E0C, 0x00980F24, 0x80030061, 0x7E050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004,
    0x70007E0C, 0x00000000, 0x20000060, 0x00000000,
];

pub static T5_SMALL_LIVE4_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT: EuArtifact =
    EuArtifact {
        name: T5_ONE_ROW_MATVEC_PROGRAM_NAME,
        isa: EuIsa::Gfx12,
        kind: EuArtifactKind::T5SmallLive4Bf16DotThenHdc1StoreThenThreadSpawnerEot,
        words: &T5_SMALL_LIVE4_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
        expects_store: true,
    };

// T6 preserved native artifact from `.codex_tmp/t6_small_live8_trueos_arena_bf16_unpack.comp`.
//
// Contract is identical to the proven T5 TRUEOS arena binding, but the partial
// dot expands from live4 to live8.  This is intentionally not the hot Lumen
// artifact yet; keep T5 as the boot-green baseline until the T6 runtime logs are
// wired with their own proof labels.
pub static T6_SMALL_LIVE8_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS: [u32; 108] = [
    0x80030061, 0x12050220, 0x00000024, 0x00000000, 0x80030061, 0x13054220, 0x00000000, 0x00000000,
    0x80030061, 0x14054220, 0x00000000, 0x00000000, 0x80030061, 0x17054220, 0x00000000, 0x00000008,
    0x80030061, 0x18054220, 0x00000000, 0xC0DE7606, 0x00030061, 0x15054660, 0x00000000, 0x00102000,
    0xA4190640, 0x0111120A, 0x80000661, 0x13454620, 0x00000000, 0x00000000, 0x80000661, 0x14454620,
    0x00000000, 0x00002000, 0x8003A031, 0x010C0000, 0xA402130C, 0x02100000, 0x80039131, 0x030C0000,
    0xA402140C, 0x02100000, 0x80032169, 0x02058660, 0x02000304, 0x00000010, 0x80030065, 0x07058220,
    0x02000304, 0xFFFF0000, 0x80030069, 0x04058660, 0x02000324, 0x00000010, 0x80030065, 0x08058220,
    0x02000324, 0xFFFF0000, 0x80030069, 0x06058660, 0x02000344, 0x00000010, 0x80030065, 0x0C058220,
    0x02000344, 0xFFFF0000, 0x80030069, 0x0A058660, 0x02000364, 0x00000010, 0x80030065, 0x10058220,
    0x02000364, 0xFFFF0000, 0x2405F041, 0x07110120, 0xA30B015B, 0x02010504, 0xA309015B, 0x04010B34,
    0xA30F015B, 0x0801092C, 0xA30D015B, 0x06010F54, 0xA30E015B, 0x0C010D1C, 0xA311015B, 0x0A010E4C,
    0xA316015B, 0x1001115C, 0x80000101, 0x00000000, 0x00000000, 0x00000000, 0x00034231, 0x00000000,
    0xC002150C, 0x00981624, 0x80030061, 0x7E050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004,
    0x70007E0C, 0x00000000, 0x20000060, 0x00000000,
];

pub static T6_SMALL_LIVE8_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT: EuArtifact =
    EuArtifact {
        name: T6_ONE_ROW_MATVEC_PROGRAM_NAME,
        isa: EuIsa::Gfx12,
        kind: EuArtifactKind::T6SmallLive8Bf16DotThenHdc1StoreThenThreadSpawnerEot,
        words: &T6_SMALL_LIVE8_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
        expects_store: true,
    };

// Mesa ANV oracle artifact from `.codex_tmp/t6_1_live16_trueos_arena_bf16_unpack.comp`.
//
// Contract is the same TRUEOS tile-record layout as T5/T6, with packed BF16
// lanes widened to 16.  The userland oracle verified the generated program with:
// x = [1..16], packed row BF16 lanes [1..16], expected result 1496.0f
// (`0x44BB0000`), and sentinel `0xC0DE7616`.
pub static T61_LIVE16_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS: [u32; 156] = [
    0x80030061, 0x28050220, 0x00000024, 0x00000000, 0x80030061, 0x29054220, 0x00000000, 0x00000000,
    0x80030061, 0x2A054220, 0x00000000, 0x00000000, 0x80030061, 0x03054220, 0x00000000, 0x00000010,
    0x80030061, 0x04054220, 0x00000000, 0xC0DE7616, 0x00030061, 0x2B054660, 0x00000000, 0x00102000,
    0xA4050640, 0x0111280A, 0x80000661, 0x29454620, 0x00000000, 0x00000000, 0x80000661, 0x2A454620,
    0x00000000, 0x00002000, 0x8004A031, 0x06140000, 0xA602290C, 0x02100000, 0x80039131, 0x190C0000,
    0xA4022A0C, 0x02100000, 0x80032169, 0x09058660, 0x02001904, 0x00000010, 0x80030065, 0x1D058220,
    0x02001904, 0xFFFF0000, 0x80030069, 0x0D058660, 0x02001924, 0x00000010, 0x80030065, 0x0B058220,
    0x02001924, 0xFFFF0000, 0x80030069, 0x11058660, 0x02001944, 0x00000010, 0x80030065, 0x0F058220,
    0x02001944, 0xFFFF0000, 0x80030069, 0x15058660, 0x02001964, 0x00000010, 0x80030065, 0x13058220,
    0x02001964, 0xFFFF0000, 0x80030069, 0x08058660, 0x02001984, 0x00000010, 0x80030065, 0x1A058220,
    0x02001984, 0xFFFF0000, 0x80030069, 0x0A058660, 0x020019A4, 0x00000010, 0x80030065, 0x1E058220,
    0x020019A4, 0xFFFF0000, 0x80030069, 0x0C058660, 0x020019C4, 0x00000010, 0x80030065, 0x22058220,
    0x020019C4, 0xFFFF0000, 0x80030069, 0x10058660, 0x020019E4, 0x00000010, 0x80030065, 0x26058220,
    0x020019E4, 0xFFFF0000, 0x240E2041, 0x1D110620, 0xA321015B, 0x09060E04, 0xA312015B, 0x0D062134,
    0xA325015B, 0x0B06122C, 0xA316015B, 0x11062554, 0xA314015B, 0x0F06161C, 0xA317015B, 0x1506144C,
    0xA318015B, 0x1306175C, 0xA31B905B, 0x08071804, 0xA31C015B, 0x1A071B0C, 0xA31F015B, 0x0A071C34,
    0xA320015B, 0x1E071F2C, 0xA323015B, 0x0C072054, 0xA324015B, 0x2207231C, 0xA327015B, 0x1007244C,
    0xA302015B, 0x2607275C, 0x80000101, 0x00000000, 0x00000000, 0x00000000, 0x00034231, 0x00000000,
    0xC0022B0C, 0x00980224, 0x80030061, 0x7E050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004,
    0x70007E0C, 0x00000000, 0x20000060, 0x00000000,
];

pub static T61_LIVE16_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT: EuArtifact =
    EuArtifact {
        name: T61_ONE_ROW_MATVEC_PROGRAM_NAME,
        isa: EuIsa::Gfx12,
        kind: EuArtifactKind::T61Live16Bf16DotThenHdc1StoreThenThreadSpawnerEot,
        words: &T61_LIVE16_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
        expects_store: true,
    };

// Mesa ANV oracle artifact from
// `.codex_tmp/t6_2_lane_indexed_live16_trueos_arena_bf16_unpack.comp`.
//
// This is the first lane-indexed partial matvec artifact.  The hardware bringup
// path did not provide a usable `gl_WorkGroupID.x` payload, so this rung uses
// `gl_LocalInvocationID.x` across one SIMD8 workgroup as the row/output selector
// and computes eight live16 packed-BF16 partial dots.
// The userland oracle verified the 8-lane output vector.
pub static T62_ROW_INDEXED_LIVE16_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS:
    [u32; 204] = [
    0x80030061, 0x39054220, 0x00000000, 0x00000000, 0x80030061, 0x01054410, 0x00000000, 0x76543210,
    0x80000261, 0x39454620, 0x00000000, 0x00000000, 0x80030261, 0x01050120, 0x00460105, 0x00000000,
    0x8004A031, 0x03140000, 0xA602390C, 0x02100000, 0x80000101, 0x00000000, 0x00000000, 0x00000000,
    0xE10B0065, 0x00700103, 0x00030169, 0x24058660, 0x02460B05, 0x0000000C, 0x00030069, 0x37058660,
    0x02460B05, 0x00000002, 0x00030240, 0x13058660, 0x06462405, 0x00002000, 0x00030040, 0x1F058660,
    0x06462405, 0x00002010, 0x00030340, 0x38058660, 0x06463705, 0x00102000, 0x0003B131, 0x07240000,
    0xC002130C, 0x00180000, 0x0003A231, 0x20240000, 0xC0021F0C, 0x00180000, 0x00032169, 0x02058660,
    0x02460705, 0x00000010, 0x00030061, 0x15050120, 0x00560716, 0x00000000, 0x00032169, 0x10058660,
    0x02460805, 0x00000010, 0x00030061, 0x12050120, 0x00560816, 0x00000000, 0x00032169, 0x0C058660,
    0x02460905, 0x00000010, 0x00030061, 0x17050120, 0x00560916, 0x00000000, 0x00032169, 0x1A058660,
    0x02460A05, 0x00000010, 0x00030061, 0x1C050120, 0x00560A16, 0x00000000, 0x00032269, 0x05058660,
    0x02462005, 0x00000010, 0x00030061, 0x26050120, 0x00562016, 0x00000000, 0x00032269, 0x29058660,
    0x02462105, 0x00000010, 0x00030061, 0x2B050120, 0x00562116, 0x00000000, 0x00032269, 0x0F058660,
    0x02462205, 0x00000010, 0x00030061, 0x30050120, 0x00562216, 0x00000000, 0x00032269, 0x33058660,
    0x02462305, 0x00000010, 0x00030061, 0x35050120, 0x00562316, 0x00000000, 0x00030069, 0x2C058660,
    0x02461505, 0x00000010, 0x00030069, 0x06058660, 0x02461205, 0x00000010, 0x00030069, 0x18058660,
    0x02461705, 0x00000010, 0x00030069, 0x0E058660, 0x02461C05, 0x00000010, 0x00030069, 0x27058660,
    0x02462605, 0x00000010, 0x00030769, 0x0D058660, 0x02462B05, 0x00000010, 0x00030769, 0x31058660,
    0x02463005, 0x00000010, 0x00030769, 0x11058660, 0x02463505, 0x00000010, 0x80002001, 0x00000000,
    0x00000000, 0x00000000, 0x211D0741, 0x2C010320, 0xE12E015B, 0x02031D00, 0xE136015B, 0x10032E30,
    0xE114015B, 0x06033628, 0xE116015B, 0x0C031450, 0xE119015B, 0x18031618, 0xE11B015B, 0x1A031948,
    0xE11E015B, 0x0E031B58, 0x80002001, 0x00000000, 0x00000000, 0x00000000, 0xE125015B, 0x05041E00,
    0xE128015B, 0x27042508, 0xE12A015B, 0x29042830, 0xE12D015B, 0x0D042A28, 0xE12F015B, 0x0F042D50,
    0xE132015B, 0x31042F18, 0xE134015B, 0x33043248, 0xE13A015B, 0x11043458, 0x00039331, 0x00000000,
    0xCC02380C, 0x009A3A0C, 0x80030061, 0x7E050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004,
    0x70007E0C, 0x00000000, 0x20000060, 0x00000000,
];

pub static T62_ROW_INDEXED_LIVE16_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT:
    EuArtifact = EuArtifact {
    name: T62_ROW_INDEXED_LIVE16_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::T62RowIndexedLive16Bf16DotThenHdc1StoreThenThreadSpawnerEot,
    words: &T62_ROW_INDEXED_LIVE16_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

// Mesa ANV oracle artifact from
// `.codex_tmp/t6_2_row_indexed_live16_trueos_arena_bf16_unpack.comp`.
//
// T8 brings the original group-id row selector back under the now-proven
// two-group walker path.  The extracted native oracle used the old g127 EOT
// tail, so the embedded TRUEOS copy applies the same g126 EOT payload fix used
// by the other HDC store-then-EOT artifacts.
pub static T8_GROUPID_LIVE16_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS: [u32;
    200] = [
    0x80030061, 0x31050220, 0x00000024, 0x00000000, 0x80030061, 0x32054220, 0x00000000, 0x00000000,
    0x80030061, 0x33054220, 0x00000000, 0x00000000, 0xA40D0340, 0x0111310A, 0x80000361, 0x32454620,
    0x00000000, 0x00000000, 0x80030269, 0x1C058660, 0x02000D04, 0x0000000C, 0x80030069, 0x2F058660,
    0x02000D04, 0x00000002, 0x8004B031, 0x03140000, 0xA602320C, 0x02100000, 0x80030240, 0x15058660,
    0x06001C04, 0x00002000, 0x80030240, 0x30058660, 0x06002F04, 0x00102000, 0x80000261, 0x33450620,
    0x00001504, 0x00000000, 0x80000201, 0x00000000, 0x00000000, 0x00000000, 0x00030061, 0x34050660,
    0x00003004, 0x00000000, 0x8003A131, 0x240C0000, 0xA402330C, 0x02100000, 0x80032169, 0x08058660,
    0x02002404, 0x00000010, 0x80030061, 0x26050120, 0x00002414, 0x00000000, 0x80030069, 0x02058660,
    0x02002424, 0x00000010, 0x80030061, 0x0F050120, 0x00002434, 0x00000000, 0x80030069, 0x12058660,
    0x02002444, 0x00000010, 0x80030061, 0x14050120, 0x00002454, 0x00000000, 0x80030069, 0x0A058660,
    0x02002464, 0x00000010, 0x80030061, 0x19050120, 0x00002474, 0x00000000, 0x80030069, 0x05058660,
    0x02002484, 0x00000010, 0x80030061, 0x1E050120, 0x00002494, 0x00000000, 0x80030069, 0x21058660,
    0x020024A4, 0x00000010, 0x80030061, 0x23050120, 0x000024B4, 0x00000000, 0x80030069, 0x09058660,
    0x020024C4, 0x00000010, 0x80030061, 0x28050120, 0x000024D4, 0x00000000, 0x80030069, 0x2B058660,
    0x020024E4, 0x00000010, 0x80030061, 0x2D050120, 0x000024F4, 0x00000000, 0x80030069, 0x17058660,
    0x02002604, 0x00000010, 0x80030069, 0x10058660, 0x02000F04, 0x00000010, 0x80030069, 0x06058660,
    0x02001404, 0x00000010, 0x80030069, 0x1A058660, 0x02001904, 0x00000010, 0x80030069, 0x1F058660,
    0x02001E04, 0x00000010, 0x80030769, 0x07058660, 0x02002304, 0x00000010, 0x80030769, 0x29058660,
    0x02002804, 0x00000010, 0x80030769, 0x0B058660, 0x02002D04, 0x00000010, 0x242EF041, 0x17110320,
    0xA30C015B, 0x08032E04, 0xA30E015B, 0x02030C34, 0xA311015B, 0x10030E2C, 0xA313015B, 0x12031154,
    0xA316015B, 0x0603131C, 0xA318015B, 0x0A03164C, 0xA31B015B, 0x1A03185C, 0xA31D905B, 0x05041B04,
    0xA320015B, 0x1F041D0C, 0xA322015B, 0x21042034, 0xA325015B, 0x0704222C, 0xA327015B, 0x09042554,
    0xA32A015B, 0x2904271C, 0xA32C015B, 0x2B042A4C, 0xA335015B, 0x0B042C5C, 0x80000101, 0x00000000,
    0x00000000, 0x00000000, 0x00034231, 0x00000000, 0xCC02340C, 0x009A350C, 0x80030061, 0x7E050220,
    0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x70007E0C, 0x00000000, 0x20000060, 0x00000000,
];

pub static T8_GROUPID_LIVE16_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT: EuArtifact =
    EuArtifact {
        name: T8_GROUPID_LIVE16_PROGRAM_NAME,
        isa: EuIsa::Gfx12,
        kind: EuArtifactKind::T8GroupidLive16Bf16DotThenHdc1StoreThenThreadSpawnerEot,
        words: &T8_GROUPID_LIVE16_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
        expects_store: true,
    };

// Mesa ANV oracle artifact from
// `.codex_tmp/t6_3_lane_indexed_live32_trueos_arena_bf16_unpack.comp`.
//
// This preserves the T6.2 row-block dispatch scheme, but widens each row's
// packed-BF16 prefix from live16 to live32.  One SIMD8 workgroup computes eight
// row partials using `gl_LocalInvocationID.x` as the row/output slot selector.
// The userland oracle verified all eight live32 outputs.
pub static T63_LANE_INDEXED_LIVE32_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS:
    [u32; 368] = [
    0x80030061, 0x6B054220, 0x00000000, 0x00000000, 0x80030061, 0x6C054220, 0x00000000, 0x00000000,
    0x80030061, 0x02054410, 0x00000000, 0x76543210, 0x80000361, 0x6B454620, 0x00000000, 0x00000000,
    0x80000361, 0x6C454620, 0x00000000, 0x00000040, 0x80030361, 0x02050120, 0x00460205, 0x00000000,
    0x8004B031, 0x3A140000, 0xA6026B0C, 0x02100000, 0x8004A131, 0x05140000, 0xA6026C0C, 0x02100000,
    0x80000101, 0x00000000, 0x00000000, 0x00000000, 0xE1030065, 0x00700203, 0x00030169, 0x12058660,
    0x02460305, 0x0000000C, 0x00030069, 0x69058660, 0x02460305, 0x00000002, 0x00030240, 0x29058660,
    0x06461205, 0x00002000, 0x00030040, 0x21058660, 0x06461205, 0x00002010, 0x00030040, 0x3C058660,
    0x06461205, 0x00002020, 0x00030040, 0x55058660, 0x06461205, 0x00002030, 0x00030540, 0x6A058660,
    0x06466905, 0x00102000, 0x0003D231, 0x09240000, 0xC002290C, 0x00180000, 0x0003C331, 0x22240000,
    0xC002210C, 0x00180000, 0x0003B431, 0x3D240000, 0xC0023C0C, 0x00180000, 0x0003A531, 0x01240000,
    0xC002550C, 0x00180000, 0x00032269, 0x0D058660, 0x02460905, 0x00000010, 0x00030061, 0x2B050120,
    0x00560916, 0x00000000, 0x00032269, 0x07058660, 0x02460A05, 0x00000010, 0x00030061, 0x35050120,
    0x00560A16, 0x00000000, 0x00032269, 0x17058660, 0x02460B05, 0x00000010, 0x00030061, 0x19050120,
    0x00560B16, 0x00000000, 0x00032269, 0x11058660, 0x02460C05, 0x00000010, 0x00030061, 0x1E050120,
    0x00560C16, 0x00000000, 0x00032369, 0x26058660, 0x02462205, 0x00000010, 0x00030061, 0x28050120,
    0x00562216, 0x00000000, 0x00032369, 0x0E058660, 0x02462305, 0x00000010, 0x00030061, 0x2D050120,
    0x00562316, 0x00000000, 0x00032369, 0x30058660, 0x02462405, 0x00000010, 0x00030061, 0x32050120,
    0x00562416, 0x00000000, 0x00032369, 0x14058660, 0x02462505, 0x00000010, 0x00030061, 0x37050120,
    0x00562516, 0x00000000, 0x00032469, 0x41058660, 0x02463D05, 0x00000010, 0x00030061, 0x43050120,
    0x00563D16, 0x00000000, 0x00032469, 0x46058660, 0x02463E05, 0x00000010, 0x00030061, 0x48050120,
    0x00563E16, 0x00000000, 0x00032469, 0x4B058660, 0x02463F05, 0x00000010, 0x00030061, 0x4D050120,
    0x00563F16, 0x00000000, 0x00032469, 0x50058660, 0x02464005, 0x00000010, 0x00030061, 0x52050120,
    0x00564016, 0x00000000, 0x00032569, 0x56058660, 0x02460105, 0x00000010, 0x00030061, 0x58050120,
    0x00560116, 0x00000000, 0x00032569, 0x5B058660, 0x02460205, 0x00000010, 0x00030061, 0x5D050120,
    0x00560216, 0x00000000, 0x00032569, 0x60058660, 0x02460305, 0x00000010, 0x00030061, 0x62050120,
    0x00560316, 0x00000000, 0x00032569, 0x65058660, 0x02460405, 0x00000010, 0x00030061, 0x67050120,
    0x00560416, 0x00000000, 0x00030069, 0x1A058660, 0x02462B05, 0x00000010, 0x00030069, 0x15058660,
    0x02463505, 0x00000010, 0x00030069, 0x0F058660, 0x02461905, 0x00000010, 0x00030069, 0x1F058660,
    0x02461E05, 0x00000010, 0x00030069, 0x08058660, 0x02462805, 0x00000010, 0x00030069, 0x2E058660,
    0x02462D05, 0x00000010, 0x00030069, 0x10058660, 0x02463205, 0x00000010, 0x00030069, 0x38058660,
    0x02463705, 0x00000010, 0x00030069, 0x44058660, 0x02464305, 0x00000010, 0x00030069, 0x49058660,
    0x02464805, 0x00000010, 0x00030069, 0x4E058660, 0x02464D05, 0x00000010, 0x00030069, 0x53058660,
    0x02465205, 0x00000010, 0x00030069, 0x59058660, 0x02465805, 0x00000010, 0x00030069, 0x5E058660,
    0x02465D05, 0x00000010, 0x00030069, 0x63058660, 0x02466205, 0x00000010, 0x00030069, 0x68058660,
    0x02466705, 0x00000010, 0x80002001, 0x00000000, 0x00000000, 0x00000000, 0x21330041, 0x1A013A20,
    0xE11C015B, 0x0D3A3300, 0xE113015B, 0x073A1C30, 0xE116015B, 0x153A1328, 0xE118015B, 0x173A1650,
    0xE11B015B, 0x0F3A1818, 0xE11D015B, 0x113A1B48, 0xE120015B, 0x1F3A1D58, 0x80002001, 0x00000000,
    0x00000000, 0x00000000, 0xE127015B, 0x263B2000, 0xE12A015B, 0x083B2708, 0xE12C015B, 0x0E3B2A30,
    0xE12F015B, 0x2E3B2C28, 0xE131015B, 0x303B2F50, 0xE134015B, 0x103B3118, 0xE136015B, 0x143B3448,
    0xE139015B, 0x383B3658, 0x80002101, 0x00000000, 0x00000000, 0x00000000, 0xE142015B, 0x41053900,
    0xE145015B, 0x44054208, 0xE147015B, 0x46054530, 0xE14A015B, 0x49054728, 0xE14C015B, 0x4B054A50,
    0xE14F015B, 0x4E054C18, 0xE151015B, 0x50054F48, 0xE154015B, 0x53055158, 0x80002101, 0x00000000,
    0x00000000, 0x00000000, 0xE157015B, 0x56065400, 0xE15A015B, 0x59065708, 0xE15C015B, 0x5B065A30,
    0xE15F015B, 0x5E065C28, 0xE161015B, 0x60065F50, 0xE164015B, 0x63066118, 0xE166015B, 0x65066448,
    0xE16D015B, 0x68066658, 0x00039631, 0x00000000, 0xCC026A0C, 0x009A6D0C, 0x80030061, 0x7E050220,
    0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x70007E0C, 0x00000000, 0x20000060, 0x00000000,
];

pub static T63_LANE_INDEXED_LIVE32_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT:
    EuArtifact = EuArtifact {
    name: T63_LANE_INDEXED_LIVE32_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::T63LaneIndexedLive32Bf16DotThenHdc1StoreThenThreadSpawnerEot,
    words: &T63_LANE_INDEXED_LIVE32_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

// Mesa ANV oracle artifact from
// `.codex_tmp/t6_3_accum16_hi_live32_trueos_arena_bf16_unpack.comp`.
//
// This is the low-register T6.3 proof path for the current TRUEOS walker: T6.2
// writes the first live16 partial, then this artifact reads that row output,
// accumulates packed BF16 lanes 16..31, and stores the live32 row-block result.
// The userland oracle verified all eight live32 outputs with preloaded live16
// partials.
pub static T63_ACCUM16_HI_LIVE32_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS:
    [u32; 212] = [
    0x80030061, 0x3A054220, 0x00000000, 0x00000000, 0x80030061, 0x01054410, 0x00000000, 0x76543210,
    0x80000261, 0x3A454620, 0x00000000, 0x00000040, 0x80030261, 0x01050120, 0x00460105, 0x00000000,
    0x8004A031, 0x06140000, 0xA6023A0C, 0x02100000, 0x80000101, 0x00000000, 0x00000000, 0x00000000,
    0xE1270065, 0x00700103, 0x00030169, 0x0E058660, 0x02462705, 0x00000002, 0x00030069, 0x31058660,
    0x02462705, 0x0000000C, 0x00030240, 0x2F058660, 0x06460E05, 0x00102000, 0x00030240, 0x18058660,
    0x06463105, 0x00002020, 0x00030040, 0x22058660, 0x06463105, 0x00002030, 0x0003B131, 0x160C0000,
    0xCC022F0C, 0x001A0000, 0x0003A231, 0x0A240000, 0xC002180C, 0x00180000, 0x00039331, 0x23240000,
    0xC002220C, 0x00180000, 0x00032269, 0x03058660, 0x02460A05, 0x00000010, 0x00030061, 0x39050120,
    0x00560A16, 0x00000000, 0x00032269, 0x13058660, 0x02460B05, 0x00000010, 0x00030061, 0x15050120,
    0x00560B16, 0x00000000, 0x00032269, 0x09058660, 0x02460C05, 0x00000010, 0x00030061, 0x1A050120,
    0x00560C16, 0x00000000, 0x00032269, 0x1D058660, 0x02460D05, 0x00000010, 0x00030061, 0x1F050120,
    0x00560D16, 0x00000000, 0x00032369, 0x02058660, 0x02462305, 0x00000010, 0x00030061, 0x29050120,
    0x00562316, 0x00000000, 0x00032369, 0x2C058660, 0x02462405, 0x00000010, 0x00030061, 0x2E050120,
    0x00562416, 0x00000000, 0x00032369, 0x08058660, 0x02462505, 0x00000010, 0x00030061, 0x33050120,
    0x00562516, 0x00000000, 0x00032369, 0x36058660, 0x02462605, 0x00000010, 0x00030061, 0x38050120,
    0x00562616, 0x00000000, 0x80002001, 0x00000000, 0x00000000, 0x00000000, 0xE120215B, 0x03061600,
    0x00030069, 0x11058660, 0x02463905, 0x00000010, 0x00030069, 0x05058660, 0x02461505, 0x00000010,
    0x00030069, 0x1B058660, 0x02461A05, 0x00000010, 0x00030069, 0x0F058660, 0x02461F05, 0x00000010,
    0x00030069, 0x2A058660, 0x02462905, 0x00000010, 0x00030069, 0x04058660, 0x02462E05, 0x00000010,
    0x00030769, 0x34058660, 0x02463305, 0x00000010, 0x00030769, 0x10058660, 0x02463805, 0x00000010,
    0xE112075B, 0x11062008, 0xE114015B, 0x13061230, 0xE117015B, 0x05061428, 0xE119015B, 0x09061750,
    0xE11C015B, 0x1B061918, 0xE11E015B, 0x1D061C48, 0xE121015B, 0x0F061E58, 0x80002001, 0x00000000,
    0x00000000, 0x00000000, 0xE128015B, 0x02072100, 0xE12B015B, 0x2A072808, 0xE12D015B, 0x2C072B30,
    0xE130015B, 0x04072D28, 0xE132015B, 0x08073050, 0xE135015B, 0x34073218, 0xE137015B, 0x36073548,
    0xE13B015B, 0x10073758, 0x80003101, 0x00000000, 0x00000000, 0x00000000, 0x00039431, 0x00000000,
    0xCC022F0C, 0x009A3B0C, 0x80030061, 0x7E050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004,
    0x70007E0C, 0x00000000, 0x20000060, 0x00000000,
];

pub static T63_ACCUM16_HI_LIVE32_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT:
    EuArtifact = EuArtifact {
    name: T63_ACCUM16_HI_LIVE32_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::T63Accum16HiLive32Bf16DotThenHdc1StoreThenThreadSpawnerEot,
    words: &T63_ACCUM16_HI_LIVE32_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

// Mesa ANV oracle artifact from
// `crates/trueos-shader/t6_6_accum32_hi_live96_trueos_arena_bf16_unpack.comp`.
//
// This true post-live64 rung reads the existing live64 row-block output,
// accumulates packed BF16 lanes 64..95 in one native SIMD8 body, and stores the
// live96 row-block result.  It is the pressure/size discriminator before trying
// a second direct high-lane rung.
pub static T66_ACCUM32_HI_LIVE96_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS:
    [u32; 384] = [
    0x80030069, 0x02058660, 0x02000144, 0x00000003, 0x80030061, 0x6D054220, 0x00000000, 0x00000000,
    0x80030061, 0x6E054220, 0x00000000, 0x00000000, 0x80030061, 0x03054410, 0x00000000, 0x76543210,
    0x80000361, 0x6D454620, 0x00000000, 0x00000100, 0x80000361, 0x6E454620, 0x00000000, 0x00000140,
    0x80030361, 0x03050120, 0x00460305, 0x00000000, 0x8004B031, 0x09140000, 0xA6026D0C, 0x02100000,
    0x8004A131, 0x3E140000, 0xA6026E0C, 0x02100000, 0x80000101, 0x00000000, 0x00000000, 0x00000000,
    0xA1040040, 0x02100302, 0xE12A0165, 0x00700403, 0x00030169, 0x11058660, 0x02462A05, 0x00000002,
    0x00030069, 0x34058660, 0x02462A05, 0x0000000C, 0x00030240, 0x32058660, 0x06461105, 0x00102000,
    0x00030240, 0x1B058660, 0x06463405, 0x00002080, 0x00030040, 0x25058660, 0x06463405, 0x00002090,
    0x00030040, 0x40058660, 0x06463405, 0x000020A0, 0x00030040, 0x59058660, 0x06463405, 0x000020B0,
    0x0003D231, 0x190C0000, 0xCC02320C, 0x001A0000, 0x0003C331, 0x0D240000, 0xC0021B0C, 0x00180000,
    0x0003B431, 0x26240000, 0xC002250C, 0x00180000, 0x0003A531, 0x41240000, 0xC002400C, 0x00180000,
    0x80000101, 0x00000000, 0x00000000, 0x00000000, 0x00034631, 0x01240000, 0xC002590C, 0x00180000,
    0x00032369, 0x06058660, 0x02460D05, 0x00000010, 0x00030061, 0x3C050120, 0x00560D16, 0x00000000,
    0x00032369, 0x16058660, 0x02460E05, 0x00000010, 0x00030061, 0x18050120, 0x00560E16, 0x00000000,
    0x00032369, 0x0C058660, 0x02460F05, 0x00000010, 0x00030061, 0x1D050120, 0x00560F16, 0x00000000,
    0x00032369, 0x20058660, 0x02461005, 0x00000010, 0x00030061, 0x22050120, 0x00561016, 0x00000000,
    0x00032469, 0x05058660, 0x02462605, 0x00000010, 0x00030061, 0x2C050120, 0x00562616, 0x00000000,
    0x00032469, 0x2F058660, 0x02462705, 0x00000010, 0x00030061, 0x31050120, 0x00562716, 0x00000000,
    0x00032469, 0x0B058660, 0x02462805, 0x00000010, 0x00030061, 0x36050120, 0x00562816, 0x00000000,
    0x00032469, 0x39058660, 0x02462905, 0x00000010, 0x00030061, 0x3B050120, 0x00562916, 0x00000000,
    0x00032569, 0x45058660, 0x02464105, 0x00000010, 0x00030061, 0x47050120, 0x00564116, 0x00000000,
    0x00032569, 0x4A058660, 0x02464205, 0x00000010, 0x00030061, 0x4C050120, 0x00564216, 0x00000000,
    0x00032569, 0x4F058660, 0x02464305, 0x00000010, 0x00030061, 0x51050120, 0x00564316, 0x00000000,
    0x00032569, 0x54058660, 0x02464405, 0x00000010, 0x00030061, 0x56050120, 0x00564416, 0x00000000,
    0x00032669, 0x5A058660, 0x02460105, 0x00000010, 0x00030061, 0x5C050120, 0x00560116, 0x00000000,
    0x00032669, 0x5F058660, 0x02460205, 0x00000010, 0x00030061, 0x61050120, 0x00560216, 0x00000000,
    0x00032669, 0x64058660, 0x02460305, 0x00000010, 0x00030061, 0x66050120, 0x00560316, 0x00000000,
    0x00032669, 0x69058660, 0x02460405, 0x00000010, 0x00030061, 0x6B050120, 0x00560416, 0x00000000,
    0x80002001, 0x00000000, 0x00000000, 0x00000000, 0xE123225B, 0x06091900, 0x00030069, 0x14058660,
    0x02463C05, 0x00000010, 0x00030069, 0x08058660, 0x02461805, 0x00000010, 0x00030069, 0x1E058660,
    0x02461D05, 0x00000010, 0x00030069, 0x12058660, 0x02462205, 0x00000010, 0x00030069, 0x2D058660,
    0x02462C05, 0x00000010, 0x00030069, 0x07058660, 0x02463105, 0x00000010, 0x00030069, 0x37058660,
    0x02463605, 0x00000010, 0x00030069, 0x13058660, 0x02463B05, 0x00000010, 0x00030069, 0x48058660,
    0x02464705, 0x00000010, 0x00030069, 0x4D058660, 0x02464C05, 0x00000010, 0x00030069, 0x52058660,
    0x02465105, 0x00000010, 0x00030069, 0x57058660, 0x02465605, 0x00000010, 0x00030069, 0x5D058660,
    0x02465C05, 0x00000010, 0x00030069, 0x62058660, 0x02466105, 0x00000010, 0x00030069, 0x67058660,
    0x02466605, 0x00000010, 0x00030069, 0x6C058660, 0x02466B05, 0x00000010, 0xE115005B, 0x14092308,
    0xE117015B, 0x16091530, 0xE11A015B, 0x08091728, 0xE11C015B, 0x0C091A50, 0xE11F015B, 0x1E091C18,
    0xE121015B, 0x20091F48, 0xE124015B, 0x12092158, 0x80002001, 0x00000000, 0x00000000, 0x00000000,
    0xE12B015B, 0x050A2400, 0xE12E015B, 0x2D0A2B08, 0xE130015B, 0x2F0A2E30, 0xE133015B, 0x070A3028,
    0xE135015B, 0x0B0A3350, 0xE138015B, 0x370A3518, 0xE13A015B, 0x390A3848, 0xE13D015B, 0x130A3A58,
    0x80002101, 0x00000000, 0x00000000, 0x00000000, 0xE146015B, 0x453E3D00, 0xE149015B, 0x483E4608,
    0xE14B015B, 0x4A3E4930, 0xE14E015B, 0x4D3E4B28, 0xE150015B, 0x4F3E4E50, 0xE153015B, 0x523E5018,
    0xE155015B, 0x543E5348, 0xE158015B, 0x573E5558, 0x80002101, 0x00000000, 0x00000000, 0x00000000,
    0xE15B015B, 0x5A3F5800, 0xE15E015B, 0x5D3F5B08, 0xE160015B, 0x5F3F5E30, 0xE163015B, 0x623F6028,
    0xE165015B, 0x643F6350, 0xE168015B, 0x673F6518, 0xE16A015B, 0x693F6848, 0xE16F015B, 0x6C3F6A58,
    0x80003201, 0x00000000, 0x00000000, 0x00000000, 0x00039731, 0x00000000, 0xCC02320C, 0x009A6F0C,
    0x80030061, 0x7E050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x70007E0C, 0x00000000,
];

pub static T66_ACCUM32_HI_LIVE96_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT:
    EuArtifact = EuArtifact {
    name: T66_ACCUM32_HI_LIVE96_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::T66Accum32HiLive96Bf16DotThenHdc1StoreThenThreadSpawnerEot,
    words: &T66_ACCUM32_HI_LIVE96_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

// Mandelbrot sidequest visible pilot. This deliberately uses 32 scalar
// stateless HDC stores in one EU program instead of the SIMD8 lane-addressed
// strip: the scalar store is the canary path already proven by the GPGPU ladder,
// while still writing a whole 32-pixel strip from one submitted GPU program.
// Runtime patches each color and absolute scanout GPU address before upload.
pub static PRIMARY_SCANOUT_MANDELBROT8_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS: [u32; 392] = [
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x04054660, 0x00000000, 0x00000000, 0x80030061, 0x7F054220, 0x00000000, 0x00000000,
    0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C, 0x80030061, 0x04054660, 0x00000000, 0x00000000,
    0x80030061, 0x7F054220, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x7E050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x70007E0C, 0x00000000,
];

pub static PRIMARY_SCANOUT_MANDELBROT8_HDC1_STATELESS_STORE_THEN_TS_EOT: EuArtifact = EuArtifact {
    name: PRIMARY_SCANOUT_MANDELBROT8_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::PrimaryScanoutMandelbrot8ThenHdc1StoreThenThreadSpawnerEot,
    words: &PRIMARY_SCANOUT_MANDELBROT8_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

const fn primary_scanout_line8_scalar8_bw_words() -> [u32; 76] {
    let mut words = [0u32; 76];
    words[0] = 0x80030061;
    words[1] = 0x04054660;
    words[2] = 0x00000000;
    words[3] = 0x00FF00FF;
    words[4] = 0x80030061;
    words[5] = 0x7F054220;
    words[6] = 0x00000000;
    words[7] = 0x00840058;

    let mut cursor = 8usize;
    let mut pixel = 0usize;
    while pixel < PRIMARY_SCANOUT_LINE8_SCALAR8_BW_LANES {
        words[cursor] = 0x00030131;
        words[cursor + 1] = 0x00000000;
        words[cursor + 2] = 0xCDFA7F0C;
        words[cursor + 3] = 0x009A040C;
        cursor += 4;
        pixel += 1;

        if pixel < PRIMARY_SCANOUT_LINE8_SCALAR8_BW_LANES {
            words[cursor] = 0x00030140;
            words[cursor + 1] = 0x7F058660;
            words[cursor + 2] = 0x06467F05;
            words[cursor + 3] = 0x00000004;
            cursor += 4;
        }
    }

    words[cursor] = 0x80030061;
    words[cursor + 1] = 0x7E050220;
    words[cursor + 2] = 0x00460005;
    words[cursor + 3] = 0x00000000;
    words[cursor + 4] = 0x80030131;
    words[cursor + 5] = 0x00000004;
    words[cursor + 6] = 0x70007E0C;
    words[cursor + 7] = 0x00000000;
    words
}

// Mandelbrot sidequest line-pilot scalar8 control.
//
// Runtime patches:
// - `g127 = row_gpu` once for the absolute stateless byte address
// - `g4 = color` once for a uniform black/white fill
//
// The EU emits eight known scalar HDC stores, adding four bytes to g127 between
// stores. This keeps the CPU contract to one address and one color while
// reusing the scalar HDC payload shape already proven by the scanout marker
// path.
pub static PRIMARY_SCANOUT_LINE8_SCALAR8_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS: [u32;
    76] = primary_scanout_line8_scalar8_bw_words();

pub static PRIMARY_SCANOUT_LINE8_SCALAR8_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT: EuArtifact =
    EuArtifact {
        name: PRIMARY_SCANOUT_LINE8_SCALAR8_BW_PROGRAM_NAME,
        isa: EuIsa::Gfx12,
        kind: EuArtifactKind::PrimaryScanoutLine8Scalar8BwThenHdc1StoreThenThreadSpawnerEot,
        words: &PRIMARY_SCANOUT_LINE8_SCALAR8_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS,
        expects_store: true,
    };

const fn primary_scanout_line320_scalar_bw_words()
-> [u32; PRIMARY_SCANOUT_LINE320_SCALAR_BW_WORD_DWORDS] {
    let mut words = [0u32; PRIMARY_SCANOUT_LINE320_SCALAR_BW_WORD_DWORDS];
    words[0] = 0x80030061;
    words[1] = 0x04054660;
    words[2] = 0x00000000;
    words[3] = 0x00FF00FF;
    words[4] = 0x80030061;
    words[5] = 0x7F054220;
    words[6] = 0x00000000;
    words[7] = 0x00840058;

    let mut cursor = 8usize;
    let mut pixel = 0usize;
    while pixel < PRIMARY_SCANOUT_LINE320_SCALAR_BW_LANES {
        words[cursor] = 0x00030131;
        words[cursor + 1] = 0x00000000;
        words[cursor + 2] = 0xCDFA7F0C;
        words[cursor + 3] = 0x009A040C;
        cursor += 4;
        pixel += 1;

        if pixel < PRIMARY_SCANOUT_LINE320_SCALAR_BW_LANES {
            words[cursor] = 0x00030140;
            words[cursor + 1] = 0x7F058660;
            words[cursor + 2] = 0x06467F05;
            words[cursor + 3] = 0x00000004;
            cursor += 4;
        }
    }

    words[cursor] = 0x80030061;
    words[cursor + 1] = 0x7E050220;
    words[cursor + 2] = 0x00460005;
    words[cursor + 3] = 0x00000000;
    words[cursor + 4] = 0x80030131;
    words[cursor + 5] = 0x00000004;
    words[cursor + 6] = 0x70007E0C;
    words[cursor + 7] = 0x00000000;
    words
}

// Mandelbrot sidequest visible line pilot.
//
// Runtime patches stay intentionally tiny:
// - `g127 = line_start_gpu` once for the absolute stateless byte address
// - `g4 = color` once for a uniform black/white fill
//
// The EU emits 320 scalar HDC stores, adding four bytes between stores. This
// keeps the same proven scalar send path as the 8-pixel pilot, but makes one
// submit wide enough for the sidequest loop to sweep full scanout rows.
pub static PRIMARY_SCANOUT_LINE320_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS:
    [u32; PRIMARY_SCANOUT_LINE320_SCALAR_BW_WORD_DWORDS] =
    primary_scanout_line320_scalar_bw_words();

pub static PRIMARY_SCANOUT_LINE320_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT: EuArtifact =
    EuArtifact {
        name: PRIMARY_SCANOUT_LINE320_SCALAR_BW_PROGRAM_NAME,
        isa: EuIsa::Gfx12,
        kind: EuArtifactKind::PrimaryScanoutLine320ScalarBwThenHdc1StoreThenThreadSpawnerEot,
        words: &PRIMARY_SCANOUT_LINE320_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS,
        expects_store: true,
    };

const fn primary_scanout_line1280_scalar_bw_words()
-> [u32; PRIMARY_SCANOUT_LINE1280_SCALAR_BW_WORD_DWORDS] {
    let mut words = [0u32; PRIMARY_SCANOUT_LINE1280_SCALAR_BW_WORD_DWORDS];
    words[0] = 0x80030061;
    words[1] = 0x04054660;
    words[2] = 0x00000000;
    words[3] = 0x00FF00FF;
    words[4] = 0x80030061;
    words[5] = 0x7F054220;
    words[6] = 0x00000000;
    words[7] = 0x00840058;

    let mut cursor = 8usize;
    let mut pixel = 0usize;
    while pixel < PRIMARY_SCANOUT_LINE1280_SCALAR_BW_LANES {
        words[cursor] = 0x00030131;
        words[cursor + 1] = 0x00000000;
        words[cursor + 2] = 0xCDFA7F0C;
        words[cursor + 3] = 0x009A040C;
        cursor += 4;
        pixel += 1;

        if pixel < PRIMARY_SCANOUT_LINE1280_SCALAR_BW_LANES {
            if pixel % PRIMARY_SCANOUT_LINE1280_SCALAR_BW_COLOR_STEP_PIXELS == 0 {
                words[cursor] = 0x00030140;
                words[cursor + 1] = 0x04058660;
                words[cursor + 2] = 0x06460405;
                words[cursor + 3] = 0x00010101;
                cursor += 4;
            }
            words[cursor] = 0x00030140;
            words[cursor + 1] = 0x7F058660;
            words[cursor + 2] = 0x06467F05;
            words[cursor + 3] = 0x00000004;
            cursor += 4;
        }
    }

    words[cursor] = 0x80030061;
    words[cursor + 1] = 0x7E050220;
    words[cursor + 2] = 0x00460005;
    words[cursor + 3] = 0x00000000;
    words[cursor + 4] = 0x80030131;
    words[cursor + 5] = 0x00000004;
    words[cursor + 6] = 0x70007E0C;
    words[cursor + 7] = 0x00000000;
    words
}

// Mandelbrot sidequest half-row pilot.
//
// This intentionally stays on the same scalar stateless HDC send as line320.
// The CPU still patches one seed color and one base address per segment; the EU
// bumps the color every 8 stores so the visible proof is not a flat blit.
pub static PRIMARY_SCANOUT_LINE1280_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS:
    [u32; PRIMARY_SCANOUT_LINE1280_SCALAR_BW_WORD_DWORDS] =
    primary_scanout_line1280_scalar_bw_words();

pub static PRIMARY_SCANOUT_LINE1280_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT:
    EuArtifact = EuArtifact {
    name: PRIMARY_SCANOUT_LINE1280_SCALAR_BW_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::PrimaryScanoutLine1280ScalarBwThenHdc1StoreThenThreadSpawnerEot,
    words: &PRIMARY_SCANOUT_LINE1280_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

const fn primary_scanout_line1280_addrcolor_scalar_bw_words()
-> [u32; PRIMARY_SCANOUT_LINE1280_ADDRCOLOR_SCALAR_BW_WORD_DWORDS] {
    let mut words = [0u32; PRIMARY_SCANOUT_LINE1280_ADDRCOLOR_SCALAR_BW_WORD_DWORDS];
    words[0] = 0x80030061;
    words[1] = 0x04054660;
    words[2] = 0x00000000;
    words[3] = 0x00000000;
    words[4] = 0x80030061;
    words[5] = 0x7F054220;
    words[6] = 0x00000000;
    words[7] = 0x00840058;
    words[8] = 0x00030140;
    words[9] = 0x04058660;
    words[10] = 0x06467F05;
    words[11] = 0x00000000;

    let mut cursor = 12usize;
    let mut pixel = 0usize;
    while pixel < PRIMARY_SCANOUT_LINE1280_ADDRCOLOR_SCALAR_BW_LANES {
        words[cursor] = 0x00030131;
        words[cursor + 1] = 0x00000000;
        words[cursor + 2] = 0xCDFA7F0C;
        words[cursor + 3] = 0x009A040C;
        cursor += 4;
        pixel += 1;

        if pixel < PRIMARY_SCANOUT_LINE1280_ADDRCOLOR_SCALAR_BW_LANES {
            if pixel % PRIMARY_SCANOUT_LINE1280_SCALAR_BW_COLOR_STEP_PIXELS == 0 {
                words[cursor] = 0x00030140;
                words[cursor + 1] = 0x04058660;
                words[cursor + 2] = 0x06460405;
                words[cursor + 3] = 0x00010101;
                cursor += 4;
            }
            words[cursor] = 0x00030140;
            words[cursor + 1] = 0x7F058660;
            words[cursor + 2] = 0x06467F05;
            words[cursor + 3] = 0x00000004;
            cursor += 4;
        }
    }

    words[cursor] = 0x80030061;
    words[cursor + 1] = 0x7E050220;
    words[cursor + 2] = 0x00460005;
    words[cursor + 3] = 0x00000000;
    words[cursor + 4] = 0x80030131;
    words[cursor + 5] = 0x00000004;
    words[cursor + 6] = 0x70007E0C;
    words[cursor + 7] = 0x00000000;
    words
}

pub static PRIMARY_SCANOUT_LINE1280_ADDRCOLOR_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS:
    [u32; PRIMARY_SCANOUT_LINE1280_ADDRCOLOR_SCALAR_BW_WORD_DWORDS] =
    primary_scanout_line1280_addrcolor_scalar_bw_words();

pub static PRIMARY_SCANOUT_LINE1280_ADDRCOLOR_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT:
    EuArtifact = EuArtifact {
    name: PRIMARY_SCANOUT_LINE1280_ADDRCOLOR_SCALAR_BW_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::PrimaryScanoutLine1280ScalarBwThenHdc1StoreThenThreadSpawnerEot,
    words: &PRIMARY_SCANOUT_LINE1280_ADDRCOLOR_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

const PRIMARY_SCANOUT_LINE1280_LANE8ROWS_HEX_BYTES: &[u8] =
    include_bytes!("../../trueos-shader/scanout_block/scanout_line1280_lane8rows.hex");

const fn scanout_hex_is_ws(byte: u8) -> bool {
    byte == b' ' || byte == b'\n' || byte == b'\r' || byte == b'\t'
}

const fn scanout_hex_value(byte: u8) -> u32 {
    match byte {
        b'0'..=b'9' => (byte - b'0') as u32,
        b'a'..=b'f' => (byte - b'a' + 10) as u32,
        b'A'..=b'F' => (byte - b'A' + 10) as u32,
        _ => 0,
    }
}

const fn scanout_skip_hex_ws(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && scanout_hex_is_ws(bytes[index]) {
        index += 1;
    }
    index
}

const fn scanout_hex_words<const WORDS: usize>(bytes: &[u8]) -> [u32; WORDS] {
    let mut words = [0u32; WORDS];
    let mut index = 0usize;
    let mut out_byte = 0usize;
    while out_byte < WORDS * 4 {
        index = scanout_skip_hex_ws(bytes, index);
        if index >= bytes.len() {
            break;
        }
        let hi = scanout_hex_value(bytes[index]);
        index += 1;
        index = scanout_skip_hex_ws(bytes, index);
        if index >= bytes.len() {
            break;
        }
        let lo = scanout_hex_value(bytes[index]);
        index += 1;

        let word_index = out_byte / 4;
        let word_shift = (out_byte % 4) * 8;
        words[word_index] |= (hi << 4 | lo) << word_shift;
        out_byte += 1;
    }
    words
}

// One workgroup is still one SIMD8 EU thread, but each lane is now a row pilot:
// lane n stores the same 1280-pixel x run at base + n * scanout pitch.
pub static PRIMARY_SCANOUT_LINE1280_LANE8ROWS_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS:
    [u32; PRIMARY_SCANOUT_LINE1280_LANE8ROWS_WORD_DWORDS] =
    scanout_hex_words(PRIMARY_SCANOUT_LINE1280_LANE8ROWS_HEX_BYTES);

pub static PRIMARY_SCANOUT_LINE1280_LANE8ROWS_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT:
    EuArtifact = EuArtifact {
    name: PRIMARY_SCANOUT_LINE1280_LANE8ROWS_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::PrimaryScanoutLine1280ScalarBwThenHdc1StoreThenThreadSpawnerEot,
    words: &PRIMARY_SCANOUT_LINE1280_LANE8ROWS_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

const fn primary_scanout_groupid_line320_scalar_bw_words()
-> [u32; PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_WORD_DWORDS] {
    let mut words = [0u32; PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_WORD_DWORDS];

    // Mesa row-indexed oracle reads `gl_WorkGroupID.x` from `g0.1` and adds a
    // push/base value from `g1.4`.  TRUEOS still uses a dummy CURBE here, so
    // this first probe deliberately shifts `g0.1` directly.
    words[0] = 0x80030061;
    words[1] = 0x31050220;
    words[2] = 0x00000024;
    words[3] = 0x00000000;
    words[4] = 0x80030061;
    words[5] = 0x32054220;
    words[6] = 0x00000000;
    words[7] = 0x00000000;
    words[8] = 0x80030061;
    words[9] = 0x33054220;
    words[10] = 0x00000000;
    words[11] = 0x00000000;
    words[12] = 0xA40D0340;
    words[13] = 0x0111310A;
    words[14] = 0x80000361;
    words[15] = 0x32454620;
    words[16] = 0x00000000;
    words[17] = 0x00000000;
    words[18] = 0x80030269;
    words[19] = 0x1C058660;
    words[20] = 0x02003104;
    words[21] = PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_STRIDE_SHIFT as u32;
    words[22] = 0x80030240;
    words[23] = 0x7F058660;
    words[24] = 0x06001C04;
    words[25] = 0x00840058;
    words[26] = 0x80030061;
    words[27] = 0x04054660;
    words[28] = 0x00000000;
    words[29] = 0x00000000;

    let mut cursor = PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_STORE_SEND_DWORD;
    let mut pixel = 0usize;
    while pixel < PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_LANES {
        words[cursor] = 0x00030131;
        words[cursor + 1] = 0x00000000;
        words[cursor + 2] = 0xCDFA7F0C;
        words[cursor + 3] = 0x009A040C;
        cursor += 4;
        pixel += 1;

        if pixel < PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_LANES {
            words[cursor] = 0x00030140;
            words[cursor + 1] = 0x7F058660;
            words[cursor + 2] = 0x06467F05;
            words[cursor + 3] = 0x00000004;
            cursor += 4;
        }
    }

    words[cursor] = 0x80030061;
    words[cursor + 1] = 0x7E050220;
    words[cursor + 2] = 0x00460005;
    words[cursor + 3] = 0x00000000;
    words[cursor + 4] = 0x80030131;
    words[cursor + 5] = 0x00000004;
    words[cursor + 6] = 0x70007E0C;
    words[cursor + 7] = 0x00000000;
    words
}

// Mandelbrot sidequest group-id visible probe.
//
// Runtime patches stay to one base address and one color.  If the walker
// exposes a real group id, one submit with x_dim=8 paints eight separate
// 320-pixel blocks.  If all groups see zero, the same artifact collapses to the
// first block and the readback mask shows that directly.
pub static PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS:
    [u32; PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_WORD_DWORDS] =
    primary_scanout_groupid_line320_scalar_bw_words();

pub static PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT:
    EuArtifact = EuArtifact {
    name: PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::PrimaryScanoutGroupidLine320ScalarBwThenHdc1StoreThenThreadSpawnerEot,
    words:
        &PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

const fn primary_scanout_groupid_line1280_rows_scalar_bw_words()
-> [u32; PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_WORD_DWORDS] {
    let mut words = [0u32; PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_WORD_DWORDS];

    words[0] = 0x80030061;
    words[1] = 0x31050220;
    words[2] = 0x00000024;
    words[3] = 0x00000000;
    words[4] = 0x80030061;
    words[5] = 0x32054220;
    words[6] = 0x00000000;
    words[7] = 0x00000000;
    words[8] = 0x80030061;
    words[9] = 0x33054220;
    words[10] = 0x00000000;
    words[11] = 0x00000000;
    words[12] = 0xA40D0340;
    words[13] = 0x0111310A;
    words[14] = 0x80000361;
    words[15] = 0x32454620;
    words[16] = 0x00000000;
    words[17] = 0x00000000;
    words[18] = 0x80030241;
    words[19] = 0x1C058660;
    words[20] = 0x02003104;
    words[21] = PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_PITCH_BYTES;
    words[22] = 0x80030240;
    words[23] = 0x7F058660;
    words[24] = 0x06001C04;
    words[25] = 0x00840058;
    words[26] = 0x80030061;
    words[27] = 0x04054660;
    words[28] = 0x00000000;
    words[29] = 0x00000000;

    let mut cursor = 30usize;
    let mut pixel = 0usize;
    while pixel < PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_LANES {
        words[cursor] = 0x00030131;
        words[cursor + 1] = 0x00000000;
        words[cursor + 2] = 0xCDFA7F0C;
        words[cursor + 3] = 0x009A040C;
        cursor += 4;
        pixel += 1;

        if pixel < PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_LANES {
            if pixel % PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_COLOR_STEP_PIXELS == 0 {
                words[cursor] = 0x00030140;
                words[cursor + 1] = 0x04058660;
                words[cursor + 2] = 0x06460405;
                words[cursor + 3] = 0x00010101;
                cursor += 4;
            }
            words[cursor] = 0x00030140;
            words[cursor + 1] = 0x7F058660;
            words[cursor + 2] = 0x06467F05;
            words[cursor + 3] = 0x00000004;
            cursor += 4;
        }
    }

    words[cursor] = 0x80030061;
    words[cursor + 1] = 0x7E050220;
    words[cursor + 2] = 0x00460005;
    words[cursor + 3] = 0x00000000;
    words[cursor + 4] = 0x80030131;
    words[cursor + 5] = 0x00000004;
    words[cursor + 6] = 0x70007E0C;
    words[cursor + 7] = 0x00000000;
    words
}

// Mandelbrot sidequest row-pilot artifact.
//
// Runtime patches one base address and one color per row burst.  The walker
// x-dimension supplies `gl_WorkGroupID.x`; EU code multiplies it by the live
// scanout pitch (0x2800 bytes) so one walker covers many rows without a CPU
// address patch for each row.
pub static PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS:
    [u32; PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_WORD_DWORDS] =
    primary_scanout_groupid_line1280_rows_scalar_bw_words();

pub static PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT:
    EuArtifact = EuArtifact {
    name: PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::PrimaryScanoutGroupidLine1280RowsScalarBwThenHdc1StoreThenThreadSpawnerEot,
    words: &PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

const fn primary_scanout_row2560_simd8_bw_words()
-> [u32; PRIMARY_SCANOUT_ROW2560_SIMD8_BW_WORD_DWORDS] {
    let mut words = [0u32; PRIMARY_SCANOUT_ROW2560_SIMD8_BW_WORD_DWORDS];

    words[0] = 0x80030061;
    words[1] = 0x01054410;
    words[2] = 0x00000000;
    words[3] = 0x76543210;
    words[4] = 0x80030261;
    words[5] = 0x01050120;
    words[6] = 0x00460105;
    words[7] = 0x00000000;
    words[8] = 0x00030069;
    words[9] = 0x7F058660;
    words[10] = 0x02460105;
    words[11] = 0x00000002;
    words[12] = 0x00030140;
    words[13] = 0x7F058660;
    words[14] = 0x06467F05;
    words[15] = 0x00000000;
    words[16] = 0x80030061;
    words[17] = 0x04054660;
    words[18] = 0x00000000;
    words[19] = 0x00FF00FF;

    let mut cursor = 20usize;
    let mut send = 0usize;
    while send < PRIMARY_SCANOUT_ROW2560_SIMD8_BW_SENDS {
        words[cursor] = 0x00030131;
        words[cursor + 1] = 0x00000000;
        words[cursor + 2] = 0xCC027F0C;
        words[cursor + 3] = 0x009A040C;
        cursor += 4;
        send += 1;

        if send < PRIMARY_SCANOUT_ROW2560_SIMD8_BW_SENDS {
            words[cursor] = 0x00030140;
            words[cursor + 1] = 0x7F058660;
            words[cursor + 2] = 0x06467F05;
            words[cursor + 3] = 0x00000020;
            cursor += 4;
        }
    }

    words[cursor] = 0x80030061;
    words[cursor + 1] = 0x7E050220;
    words[cursor + 2] = 0x00460005;
    words[cursor + 3] = 0x00000000;
    words[cursor + 4] = 0x80030131;
    words[cursor + 5] = 0x00000004;
    words[cursor + 6] = 0x70007E0C;
    words[cursor + 7] = 0x00000000;
    words
}

pub static PRIMARY_SCANOUT_ROW2560_SIMD8_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS: [u32;
    PRIMARY_SCANOUT_ROW2560_SIMD8_BW_WORD_DWORDS] = primary_scanout_row2560_simd8_bw_words();

pub static PRIMARY_SCANOUT_ROW2560_SIMD8_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT: EuArtifact =
    EuArtifact {
        name: PRIMARY_SCANOUT_ROW2560_SIMD8_BW_PROGRAM_NAME,
        isa: EuIsa::Gfx12,
        kind: EuArtifactKind::PrimaryScanoutRow2560Simd8BwThenHdc1StoreThenThreadSpawnerEot,
        words: &PRIMARY_SCANOUT_ROW2560_SIMD8_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS,
        expects_store: true,
    };

const fn primary_scanout_row2560_simd16_bw_words()
-> [u32; PRIMARY_SCANOUT_ROW2560_SIMD16_BW_WORD_DWORDS] {
    let mut words = [0u32; PRIMARY_SCANOUT_ROW2560_SIMD16_BW_WORD_DWORDS];

    // Build the SIMD16 byte-address vector and a SIMD16 color vector:
    //   g20 = row_base + [0, 4, 8, ... 60]
    //   g22 = repeated color dwords
    words[0] = 0x80030061;
    words[1] = 0x01054410;
    words[2] = 0x00000000;
    words[3] = 0x76543210;
    words[4] = 0x80030061;
    words[5] = 0x02054410;
    words[6] = 0x00000000;
    words[7] = 0xFEDCBA98;
    words[8] = 0x80040261;
    words[9] = 0x01050120;
    words[10] = 0x00460105;
    words[11] = 0x00000000;
    words[12] = 0x00040069;
    words[13] = 0x14058660;
    words[14] = 0x02460105;
    words[15] = 0x00000002;
    words[16] = 0x00040140;
    words[17] = 0x14058660;
    words[18] = 0x06461405;
    words[19] = 0x00000000;
    words[20] = 0x00040140;
    words[21] = 0x16050660;
    words[22] = 0x06461405;
    words[23] = 0x02461405;
    words[24] = 0x00040166;
    words[25] = 0x16058220;
    words[26] = 0x02461605;
    words[27] = 0x00FF00FF;

    let mut cursor = PRIMARY_SCANOUT_ROW2560_SIMD16_BW_STORE_SEND_DWORD;
    let mut send = 0usize;
    while send < PRIMARY_SCANOUT_ROW2560_SIMD16_BW_SENDS {
        words[cursor] = 0x00040131;
        words[cursor + 1] = 0x00000000;
        words[cursor + 2] = 0xCC021414;
        words[cursor + 3] = 0x00961614;
        cursor += 4;
        send += 1;

        if send < PRIMARY_SCANOUT_ROW2560_SIMD16_BW_SENDS {
            words[cursor] = 0x00040140;
            words[cursor + 1] = 0x14058660;
            words[cursor + 2] = 0x06461405;
            words[cursor + 3] = 0x00000040;
            cursor += 4;
        }
    }

    words[cursor] = 0x80030061;
    words[cursor + 1] = 0x7E050220;
    words[cursor + 2] = 0x00460005;
    words[cursor + 3] = 0x00000000;
    words[cursor + 4] = 0x80030131;
    words[cursor + 5] = 0x00000004;
    words[cursor + 6] = 0x70007E0C;
    words[cursor + 7] = 0x00000000;
    words
}

pub static PRIMARY_SCANOUT_ROW2560_SIMD16_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS: [u32;
    PRIMARY_SCANOUT_ROW2560_SIMD16_BW_WORD_DWORDS] = primary_scanout_row2560_simd16_bw_words();

pub static PRIMARY_SCANOUT_ROW2560_SIMD16_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT: EuArtifact =
    EuArtifact {
        name: PRIMARY_SCANOUT_ROW2560_SIMD16_BW_PROGRAM_NAME,
        isa: EuIsa::Gfx12,
        kind: EuArtifactKind::PrimaryScanoutRow2560Simd16BwThenHdc1StoreThenThreadSpawnerEot,
        words: &PRIMARY_SCANOUT_ROW2560_SIMD16_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS,
        expects_store: true,
    };

// Mandelbrot sidequest line-pilot probe from
// `crates/trueos-shader/scanout_block/scanout_8px.asm`.
//
// Runtime patches:
// - `g127 += row_offset` for the byte offset into BTI 1
// - `g4 = color` for a uniform black/white fill
//
// The EU derives `row_offset + lane * 4`, and BTI 1 is bound to the primary
// scanout. One SIMD8 HDC send should write eight adjacent pixels. This is
// intentionally simpler than the coord-color pilot so the next boot
// distinguishes "SIMD8 store payload broken" from color math.
pub static PRIMARY_SCANOUT_LINE8_SIMD8_BW_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS: [u32; 32] = [
    0x80030061, 0x01054410, 0x00000000, 0x76543210, 0x80030261, 0x01050120, 0x00460105, 0x00000000,
    0x00030069, 0x7F058660, 0x02460105, 0x00000002, 0x00030140, 0x7F058660, 0x06467F05, 0x00000000,
    0x80030061, 0x04054660, 0x00000000, 0x00FF00FF, 0x00030131, 0x00000000, 0xCC027F0C, 0x009A040C,
    0x80030061, 0x7E050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x70007E0C, 0x00000000,
];

pub static PRIMARY_SCANOUT_LINE8_SIMD8_BW_HDC1_STATELESS_STORE_THEN_TS_EOT: EuArtifact =
    EuArtifact {
        name: PRIMARY_SCANOUT_LINE8_SIMD8_BW_PROGRAM_NAME,
        isa: EuIsa::Gfx12,
        kind: EuArtifactKind::PrimaryScanoutLine8Simd8BwThenHdc1StoreThenThreadSpawnerEot,
        words: &PRIMARY_SCANOUT_LINE8_SIMD8_BW_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
        expects_store: true,
    };

// Mandelbrot sidequest coord-color pilot. This is the first artifact after the
// scalar store proof that moves useful strip work into the EU program:
// - runtime patches x_base, row/phase color seed, and row_gpu
// - EU derives SIMD8 lane coordinates, per-lane store addresses, and colors
// - one HDC send writes the 8-pixel strip
//
// It is deliberately not the final Mandelbrot iteration kernel yet; it proves
// the uniform-patched, lane-derived shape that the real tilewalker artifact
// should use next.
pub static PRIMARY_SCANOUT_MANDELBROT8_SIMD8_COORD_COLOR_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS:
    [u32; 48] = [
    0x80030061, 0x01054410, 0x00000000, 0x76543210, 0x80030261, 0x01050160, 0x00460105, 0x00000000,
    0x00030140, 0x02058660, 0x06460105, 0x00000000, 0x00030141, 0x03058660, 0x06460205, 0x0000000B,
    0x00030140, 0x04058660, 0x06460305, 0x00003080, 0x00030069, 0x05058660, 0x02460305, 0x00000008,
    0x00030166, 0x04050660, 0x06460405, 0x00460505, 0x00030069, 0x7F058660, 0x02460105, 0x00000002,
    0x00030140, 0x7F058660, 0x06467F05, 0x00840058, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x7E050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x70007E0C, 0x00000000,
];

pub static PRIMARY_SCANOUT_MANDELBROT8_SIMD8_COORD_COLOR_HDC1_STATELESS_STORE_THEN_TS_EOT:
    EuArtifact = EuArtifact {
    name: PRIMARY_SCANOUT_MANDELBROT8_SIMD8_COORD_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::PrimaryScanoutMandelbrot8Simd8CoordColorThenHdc1StoreThenThreadSpawnerEot,
    words: &PRIMARY_SCANOUT_MANDELBROT8_SIMD8_COORD_COLOR_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

// Mandelbrot sidequest q12 escape-count artifact. This keeps the SIMD8 math
// body and 32-pixel window, but uses the same stateless HDC scanout write
// suffix as the visible 8-pixel strip proof: the CPU patches setup scalars plus
// absolute row GPU bases, while the EU computes escape counts, colors, and lane
// byte offsets for four contiguous SIMD8 strips.
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_PROGRAM_NAME: &str = "gfx12-primary-scanout-mandelbrot32-simd8x4-q12i8-stateless-absolute-g127-sync-store-then-ts-eot";
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_LANES: usize = 8;
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIPS_PER_PROGRAM: usize = 4;
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_PIXELS_PER_PROGRAM: usize =
    PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_LANES
        * PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIPS_PER_PROGRAM;
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_FRAC_BITS: u32 = 12;
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_MAX_ITER: u32 = 8;
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STORE_SURFACE: u32 = 0xFD;
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_BINDING_TABLE_SURFACE: u32 = 0x01;
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_X_STEP_DWORDS: [usize;
    PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIPS_PER_PROGRAM] = [11, 535, 1055, 1575];
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_C_RE_BASE_DWORDS: [usize;
    PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIPS_PER_PROGRAM] = [15, 539, 1059, 1579];
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_C_IM_DWORD: usize = 19;
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ADDRESS_BASE_DWORDS: [usize;
    PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIPS_PER_PROGRAM] = [519, 1039, 1559, 2079];
pub const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STORE_EXDESC_DWORD: usize = 527;

// Real SIMD16 Mandelbrot artifact slot.
//
// Keep the active baseline at the 16-lane store/EOT shape. SIMD8 remains a
// diagnostic subset, but this sidequest must prove the 16-flow artifact surface
// before adding Mandelbrot math back on top.
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PROGRAM_NAME: &str =
    "gfx12-primary-scanout-mandelbrot16-t10-groupid-row-simd16-bw-store-then-ts-eot";
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES: usize = 16;
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_FRAC_BITS: u32 = 12;
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_MAX_ITER: u32 = 10;
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM: usize =
    PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES;
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_STORE_SENDS: usize = 2;
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_WORD_DWORDS: usize = 768;
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD: usize = 32;
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_X_STEP_DWORD: usize = usize::MAX;
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_C_RE_BASE_DWORD: usize = usize::MAX;
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_C_IM_DWORD: usize = usize::MAX;
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_ADDRESS_BASE_DWORD: usize = 27;
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_ADDRESS_BASE_DWORDS: [usize; 1] =
    [PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_ADDRESS_BASE_DWORD];
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_COLOR_DWORD: usize = usize::MAX;
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_ONE_ITER_DWORD: usize =
    PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD + 4;
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_COLOR_FROM_DEPTH_DWORD: usize =
    PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD + 8;
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_STORE_SEND_DWORD: usize =
    PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD + 12;
pub const PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_FIXED10_STORE_SEND_DWORD: usize =
    PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD + 488;

#[allow(dead_code)]
const fn emit_mandelbrot16_fixed10_iter(
    words: &mut [u32; PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_WORD_DWORDS],
    cursor: usize,
) -> usize {
    words[cursor] = 0x00030141;
    words[cursor + 1] = 0x0C050660;
    words[cursor + 2] = 0x06460605;
    words[cursor + 3] = 0x00460605;
    words[cursor + 4] = 0x0003016C;
    words[cursor + 5] = 0x0C058660;
    words[cursor + 6] = 0x06460C05;
    words[cursor + 7] = 0x0000000C;
    words[cursor + 8] = 0x00030141;
    words[cursor + 9] = 0x0E050660;
    words[cursor + 10] = 0x06460805;
    words[cursor + 11] = 0x00460805;
    words[cursor + 12] = 0x0003016C;
    words[cursor + 13] = 0x0E058660;
    words[cursor + 14] = 0x06460E05;
    words[cursor + 15] = 0x0000000C;
    words[cursor + 16] = 0x00030140;
    words[cursor + 17] = 0x10050660;
    words[cursor + 18] = 0x06460C05;
    words[cursor + 19] = 0x00460E05;
    words[cursor + 20] = 0x00030170;
    words[cursor + 21] = 0x00018660;
    words[cursor + 22] = 0x56461005;
    words[cursor + 23] = 0x00004000;
    words[cursor + 24] = 0x01030140;
    words[cursor + 25] = 0x0A058660;
    words[cursor + 26] = 0x06460A05;
    words[cursor + 27] = 0x00000001;
    words[cursor + 28] = 0x00030140;
    words[cursor + 29] = 0x12050660;
    words[cursor + 30] = 0x06460C05;
    words[cursor + 31] = 0x02460E05;
    words[cursor + 32] = 0x00030140;
    words[cursor + 33] = 0x12050660;
    words[cursor + 34] = 0x06461205;
    words[cursor + 35] = 0x00460205;
    words[cursor + 36] = 0x00030141;
    words[cursor + 37] = 0x14050660;
    words[cursor + 38] = 0x06460605;
    words[cursor + 39] = 0x00460805;
    words[cursor + 40] = 0x0003016C;
    words[cursor + 41] = 0x14058660;
    words[cursor + 42] = 0x06461405;
    words[cursor + 43] = 0x0000000B;
    words[cursor + 44] = 0x00030140;
    words[cursor + 45] = 0x14050660;
    words[cursor + 46] = 0x06461405;
    words[cursor + 47] = 0x00460405;
    words[cursor + 48] = 0x01030161;
    words[cursor + 49] = 0x06050660;
    words[cursor + 50] = 0x00461205;
    words[cursor + 51] = 0x00000000;
    words[cursor + 52] = 0x01030161;
    words[cursor + 53] = 0x08050660;
    words[cursor + 54] = 0x00461405;
    words[cursor + 55] = 0x00000000;
    cursor + 56
}

#[allow(dead_code)]
const fn emit_mandelbrot16_fixed10_color_and_store(
    words: &mut [u32; PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_WORD_DWORDS],
    cursor: usize,
) -> usize {
    words[cursor] = 0x00030169;
    words[cursor + 1] = 0x14058660;
    words[cursor + 2] = 0x02460805;
    words[cursor + 3] = 0x00000012;
    words[cursor + 4] = 0x00030169;
    words[cursor + 5] = 0x15058660;
    words[cursor + 6] = 0x02460805;
    words[cursor + 7] = 0x0000000A;
    words[cursor + 8] = 0x00030169;
    words[cursor + 9] = 0x16058660;
    words[cursor + 10] = 0x02460805;
    words[cursor + 11] = 0x00000002;
    words[cursor + 12] = 0x00030140;
    words[cursor + 13] = 0x04050660;
    words[cursor + 14] = 0x06461405;
    words[cursor + 15] = 0x00461505;
    words[cursor + 16] = 0x00030140;
    words[cursor + 17] = 0x04050660;
    words[cursor + 18] = 0x06460405;
    words[cursor + 19] = 0x00461605;
    words[cursor + 20] = 0x00030140;
    words[cursor + 21] = 0x04058660;
    words[cursor + 22] = 0x06460405;
    words[cursor + 23] = 0x00002040;
    words[cursor + 24] = 0x00030170;
    words[cursor + 25] = 0x00018660;
    words[cursor + 26] = 0x46460805;
    words[cursor + 27] = PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_MAX_ITER;
    words[cursor + 28] = 0x01030161;
    words[cursor + 29] = 0x04054660;
    words[cursor + 30] = 0x00000000;
    words[cursor + 31] = 0x00000000;
    words[cursor + 32] = 0x00030069;
    words[cursor + 33] = 0x7F058660;
    words[cursor + 34] = 0x02460105;
    words[cursor + 35] = 0x00000002;
    words[cursor + 36] = 0x00030140;
    words[cursor + 37] = 0x7F058660;
    words[cursor + 38] = 0x06467F05;
    words[cursor + 39] = 0x00000000;
    words[cursor + 40] = 0x80000101;
    words[cursor + 41] = 0x00000000;
    words[cursor + 42] = 0x00000000;
    words[cursor + 43] = 0x00000000;
    words[cursor + 44] = 0x00030131;
    words[cursor + 45] = 0x00000000;
    words[cursor + 46] = 0xCC027F0C;
    words[cursor + 47] = 0x009A040C;
    cursor + 48
}

const fn primary_scanout_mandelbrot16_simd16_bw_words()
-> [u32; PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_WORD_DWORDS] {
    let mut words = [0u32; PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_WORD_DWORDS];

    words[0] = 0x80030061;
    words[1] = 0x01054410;
    words[2] = 0x00000000;
    words[3] = 0x76543210;
    words[4] = 0x80030061;
    words[5] = 0x02054410;
    words[6] = 0x00000000;
    words[7] = 0xFEDCBA98;
    words[8] = 0x80040261;
    words[9] = 0x01050120;
    words[10] = 0x00460105;
    words[11] = 0x00000000;
    words[12] = 0x00040069;
    words[13] = 0x14058660;
    words[14] = 0x02460105;
    words[15] = 0x00000002;
    words[16] = 0x80030061;
    words[17] = 0x31050220;
    words[18] = 0x00000024;
    words[19] = 0x00000000;
    words[20] = 0x80030241;
    words[21] = 0x1C058660;
    words[22] = 0x02003104;
    words[23] = PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_PITCH_BYTES;
    words[24] = 0x80030240;
    words[25] = 0x1C058660;
    words[26] = 0x06001C04;
    words[27] = 0x00000000;
    words[28] = 0x00040140;
    words[29] = 0x14058660;
    words[30] = 0x06461405;
    words[31] = 0x06001C04;
    words[32] = 0x80040061;
    words[33] = 0x06054660;
    words[34] = 0x00000000;
    words[35] = 0x00000003;
    words[36] = 0x00040141;
    words[37] = 0x16050660;
    words[38] = 0x06460605;
    words[39] = 0x00460605;
    words[40] = 0x80040061;
    words[41] = 0x16054660;
    words[42] = 0x00000000;
    words[43] = 0x00000009;
    words[44] = 0x00040131;
    words[45] = 0x00000000;
    words[46] = 0xCC021414;
    words[47] = 0x00961614;
    words[48] = 0x80030061;
    words[49] = 0x7E050220;
    words[50] = 0x00460005;
    words[51] = 0x00000000;
    words[52] = 0x80030131;
    words[53] = 0x00000004;
    words[54] = 0x70007E0C;
    words[55] = 0x00000000;
    words
}

pub static PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS: [u32;
    PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_WORD_DWORDS] =
    primary_scanout_mandelbrot16_simd16_bw_words();

pub static PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_HDC1_STATELESS_STORE_THEN_TS_EOT: EuArtifact =
    EuArtifact {
        name: PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PROGRAM_NAME,
        isa: EuIsa::Gfx12,
        kind: EuArtifactKind::PrimaryScanoutMandelbrot16Simd16BwThenHdc1StoreThenThreadSpawnerEot,
        words: &PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
        expects_store: true,
    };

// Preserve the generated SIMD8x2 Q12 program body and widen only by reusing
// its already-proven second-strip block. The Q12 iteration math, color math,
// and TS EOT suffix remain byte-for-byte from the generated base artifact; only
// the store descriptors are switched to the proven stateless scanout suffix.
const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_BASE_WORDS_DWORDS: usize = 1056;
const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIP0_DWORDS: usize = 132 * 4;
const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIPN_DWORDS: usize = 130 * 4;
const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_EOT_DWORDS: usize = 2 * 4;
const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_WORDS_DWORDS: usize =
    PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIP0_DWORDS
        + (PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIPS_PER_PROGRAM - 1)
            * PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIPN_DWORDS
        + PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_EOT_DWORDS;

const PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ESCAPE_HDC1_STATELESS_STORE_THEN_TS_EOT_BASE_WORDS:
    [u32; 1056] = [
    0x80030061, 0x01054410, 0x00000000, 0x76543210, 0x80030261, 0x01050160, 0x00460105, 0x00000000,
    0x00030141, 0x02058660, 0x06460105, 0x00000005, 0x00030140, 0x02058660, 0x06460205, 0xFFFFE000,
    0x80030061, 0x03054660, 0x00000000, 0xFFFFF000, 0x80030061, 0x06054660, 0x00000000, 0x00000000,
    0x80030061, 0x07054660, 0x00000000, 0x00000000, 0x80030061, 0x08054660, 0x00000000, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030169, 0x14058660, 0x02460805, 0x00000012, 0x00030169, 0x15058660, 0x02460805, 0x0000000A,
    0x00030169, 0x16058660, 0x02460805, 0x00000002, 0x00030140, 0x04050660, 0x06461405, 0x00461505,
    0x00030140, 0x04050660, 0x06460405, 0x00461605, 0x00030140, 0x04058660, 0x06460405, 0x00002040,
    0x00030170, 0x00018660, 0x46460805, 0x00000008, 0x01030161, 0x04054660, 0x00000000, 0x00000000,
    0x00030069, 0x7F058660, 0x02460105, 0x00000002, 0x00030140, 0x7F058660, 0x06467F05, 0x00000000,
    0x80000101, 0x00000000, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x00030140, 0x01058660, 0x06460105, 0x00000008, 0x00030141, 0x02058660, 0x06460105, 0x00000005,
    0x00030140, 0x02058660, 0x06460205, 0xFFFFE000, 0x80030061, 0x06054660, 0x00000000, 0x00000000,
    0x80030061, 0x07054660, 0x00000000, 0x00000000, 0x80030061, 0x08054660, 0x00000000, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030141, 0x0A050660, 0x06460605, 0x00460605, 0x0003016C, 0x0A058660, 0x06460A05, 0x0000000C,
    0x00030141, 0x0B050660, 0x06460705, 0x00460705, 0x0003016C, 0x0B058660, 0x06460B05, 0x0000000C,
    0x00030140, 0x0C050660, 0x06460A05, 0x00460B05, 0x00030170, 0x00018660, 0x56460C05, 0x00004000,
    0x01030140, 0x08058660, 0x06460805, 0x00000001, 0x00030140, 0x0D050660, 0x06460A05, 0x02460B05,
    0x00030140, 0x0D050660, 0x06460D05, 0x00460205, 0x00030141, 0x0E050660, 0x06460605, 0x00460705,
    0x0003016C, 0x0E058660, 0x06460E05, 0x0000000B, 0x00030140, 0x0E050660, 0x06460E05, 0x00460305,
    0x01030161, 0x06050660, 0x00460D05, 0x00000000, 0x01030161, 0x07050660, 0x00460E05, 0x00000000,
    0x00030169, 0x14058660, 0x02460805, 0x00000012, 0x00030169, 0x15058660, 0x02460805, 0x0000000A,
    0x00030169, 0x16058660, 0x02460805, 0x00000002, 0x00030140, 0x04050660, 0x06461405, 0x00461505,
    0x00030140, 0x04050660, 0x06460405, 0x00461605, 0x00030140, 0x04058660, 0x06460405, 0x00002040,
    0x00030170, 0x00018660, 0x46460805, 0x00000008, 0x01030161, 0x04054660, 0x00000000, 0x00000000,
    0x00030069, 0x7F058660, 0x02460105, 0x00000002, 0x00030140, 0x7F058660, 0x06467F05, 0x00000000,
    0x80000101, 0x00000000, 0x00000000, 0x00000000, 0x00030131, 0x00000000, 0xCDFA7F0C, 0x009A040C,
    0x80030061, 0x7E050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x70007E0C, 0x00000000,
];

const fn primary_scanout_mandelbrot8_simd8_q12_escape_words()
-> [u32; PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_WORDS_DWORDS] {
    let mut words = [0u32; PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_WORDS_DWORDS];
    let mut dst = 0usize;
    let mut src = 0usize;

    while src < PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIP0_DWORDS {
        words[dst] =
            PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ESCAPE_HDC1_STATELESS_STORE_THEN_TS_EOT_BASE_WORDS
                [src];
        src += 1;
        dst += 1;
    }

    let mut strip = 1usize;
    while strip < PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIPS_PER_PROGRAM {
        src = PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIP0_DWORDS;
        let strip_end = PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIP0_DWORDS
            + PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIPN_DWORDS;
        while src < strip_end {
            words[dst] =
                PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ESCAPE_HDC1_STATELESS_STORE_THEN_TS_EOT_BASE_WORDS
                    [src];
            src += 1;
            dst += 1;
        }
        strip += 1;
    }

    src = PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_BASE_WORDS_DWORDS
        - PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_EOT_DWORDS;
    while src < PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_BASE_WORDS_DWORDS {
        words[dst] =
            PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ESCAPE_HDC1_STATELESS_STORE_THEN_TS_EOT_BASE_WORDS
                [src];
        src += 1;
        dst += 1;
    }

    words
}

pub static PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ESCAPE_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS:
    [u32; PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_WORDS_DWORDS] =
    primary_scanout_mandelbrot8_simd8_q12_escape_words();

pub static PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ESCAPE_HDC1_STATELESS_STORE_THEN_TS_EOT:
    EuArtifact = EuArtifact {
    name: PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::PrimaryScanoutMandelbrot8Simd8Q12EscapeThenHdc1StoreThenThreadSpawnerEot,
    words: &PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ESCAPE_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
    expects_store: true,
};

// T5 live4 bridge: preserve the Mesa live-load/math prefix, but replace only
// the final surface-indexed store with the proven stateless HDC store suffix.
// If this writes, the next blocker is live load/math; if it still does not,
// the prefix is preventing the value from reaching the proven store.
pub static T5_SMALL_LIVE4_TRUEOS_ARENA_BF16_DOT_PROVEN_HDC_STORE_THEN_TS_EOT_WORDS: [u32; 86] = [
    0x80030061,
    0x0A050220,
    0x00000024,
    0x00000000,
    0x80030061,
    0x0B054220,
    0x00000000,
    0x00000000,
    0x80030061,
    0x0C054220,
    0x00000000,
    0x00000000,
    0x80030061,
    0x0F054220,
    0x00000000,
    0x00000004,
    0x80030061,
    0x10054220,
    0x00000000,
    0xC0DE7505,
    0x00030061,
    0x0D054660,
    0x00000000,
    0x00102000,
    0xA4110640,
    0x01110A0A,
    0x80000661,
    0x0B454620,
    0x00000000,
    0x00000000,
    0x80000661,
    0x0C454620,
    0x00000000,
    0x00002000,
    0x8003A031,
    0x010C0000,
    0xA4020B0C,
    0x02100000,
    0x80039131,
    0x030C0000,
    0xA4020C0C,
    0x02100000,
    0x80032169,
    0x02058660,
    0x02000304,
    0x00000010,
    0x80030069,
    0x05058660,
    0x02000324,
    0x00000010,
    0x80030069,
    0x04058660,
    0x02000344,
    0x00000010,
    0x80030069,
    0x06058660,
    0x02000364,
    0x00000010,
    0x2407B041,
    0x05110120,
    0xA308015B,
    0x02010704,
    0xA309015B,
    0x04010834,
    0xA30E015B,
    0x0601092C,
    0x80030061,
    0x04050660,
    0x00460E05,
    0x00000000,
    0x80030061,
    0x7F054220,
    0x00000000,
    T5_TRUEOS_ARENA_OUTPUT_GPU_U32,
    0x00030131,
    0x00000000,
    0xCDFA7F0C,
    0x009A040C,
    0x80030061,
    0x7E050220,
    0x00460005,
    0x00000000,
    0x80030131,
    0x00000004,
    0x70007E0C,
    0x00000000,
];

pub static T5_SMALL_LIVE4_TRUEOS_ARENA_BF16_DOT_PROVEN_HDC_STORE_THEN_TS_EOT: EuArtifact =
    EuArtifact {
        name: T5_SMALL_LIVE4_PROVEN_HDC_PROGRAM_NAME,
        isa: EuIsa::Gfx12,
        kind: EuArtifactKind::T5SmallLive4Bf16DotThenHdc1StoreThenThreadSpawnerEot,
        words: &T5_SMALL_LIVE4_TRUEOS_ARENA_BF16_DOT_PROVEN_HDC_STORE_THEN_TS_EOT_WORDS,
        expects_store: true,
    };

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
