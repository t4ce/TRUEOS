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
pub const T63_LANE_INDEXED_LIVE32_PROGRAM_NAME: &str =
    "gfx12-t6-3-lane-indexed-live32-packed-bf16-dot-hdc1-stateless-store-then-ts-eot";
pub const T63_LANE_INDEXED_LIVE_K: usize = 32;
pub const T63_LANE_INDEXED_PARTIAL_ROWS: usize = 8;
pub const T63_LANE_INDEXED_LIVE32_STORE_SEND_DWORD: usize = 357;

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
    0x7F050220,
    0x00460005,
    0x00000000,
    0x80030131,
    0x00000004,
    0x70007F0C,
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
    0x7F050220,
    0x00460005,
    0x00000000,
    0x80030131,
    0x00000004,
    0x70007F0C,
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
    0x80030061, 0x7F050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x70007F0C, 0x00000000,
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
    0xC0020D0C, 0x00980E24, 0x80030061, 0x7F050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004,
    0x70007F0C, 0x00000000, 0x20000060, 0x00000000,
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
    0xC0020E0C, 0x00980F24, 0x80030061, 0x7F050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004,
    0x70007F0C, 0x00000000, 0x20000060, 0x00000000,
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
    0xC002150C, 0x00981624, 0x80030061, 0x7F050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004,
    0x70007F0C, 0x00000000, 0x20000060, 0x00000000,
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
    0xC0022B0C, 0x00980224, 0x80030061, 0x7F050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004,
    0x70007F0C, 0x00000000, 0x20000060, 0x00000000,
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
    0xCC02380C, 0x009A3A0C, 0x80030061, 0x7F050220, 0x00460005, 0x00000000, 0x80030131, 0x00000004,
    0x70007F0C, 0x00000000, 0x20000060, 0x00000000,
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
    0xE16D015B, 0x68066658, 0x00039631, 0x00000000, 0xCC026A0C, 0x009A6D0C, 0x80030061, 0x7F050220,
    0x00460005, 0x00000000, 0x80030131, 0x00000004, 0x70007F0C, 0x00000000, 0x20000060, 0x00000000,
];

pub static T63_LANE_INDEXED_LIVE32_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT:
    EuArtifact = EuArtifact {
    name: T63_LANE_INDEXED_LIVE32_PROGRAM_NAME,
    isa: EuIsa::Gfx12,
    kind: EuArtifactKind::T63LaneIndexedLive32Bf16DotThenHdc1StoreThenThreadSpawnerEot,
    words: &T63_LANE_INDEXED_LIVE32_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS,
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
    0x7F050220,
    0x00460005,
    0x00000000,
    0x80030131,
    0x00000004,
    0x70007F0C,
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
