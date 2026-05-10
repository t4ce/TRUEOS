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
pub static T6_SMALL_LIVE8_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS:
    [u32; 108] = [
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
