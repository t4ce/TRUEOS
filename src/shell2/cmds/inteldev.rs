use core::{ptr, str::SplitWhitespace};

use alloc::{format, string::String, vec::Vec};
use embassy_time::Instant;
use spin::Mutex;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

const RCS_RING_BASE: usize = 0x0000_2000;
const BCS_RING_BASE: usize = 0x0002_2000;
const GEN11_VCS0_RING_BASE: usize = 0x001C_0000;
const GEN11_VCS1_RING_BASE: usize = 0x001C_4000;
const GEN11_VECS0_RING_BASE: usize = 0x001C_8000;
const GEN11_VECS1_RING_BASE: usize = 0x001D_8000;

const RING_TAIL: usize = 0x30;
const RING_HEAD: usize = 0x34;
const RING_START: usize = 0x38;
const RING_CTL: usize = 0x3C;
const RING_ACTHD: usize = 0x74;
const RING_MI_MODE: usize = 0x9C;
const RING_IPEIR: usize = 0x64;
const RING_IPEHR: usize = 0x68;
const RING_EIR: usize = 0xB0;
const RING_EMR: usize = 0xB4;
const RING_INSTDONE: usize = 0x6C;
const RING_INSTPS: usize = 0x70;
const RING_BBADDR: usize = 0x140;
const RING_BBADDR_UDW: usize = 0x168;
const RING_CONTEXT_CONTROL: usize = 0x244;
const RING_MODE_GEN7: usize = 0x29C;
const RING_EXECLIST_STATUS_LO: usize = 0x234;
const RING_EXECLIST_STATUS_HI: usize = 0x238;
const RING_EXECLIST_CONTROL: usize = 0x550;

const FORCEWAKE_MEDIA_GEN11: usize = 0x0A184;
const FORCEWAKE_MEDIA_VDBOX0: usize = 0x0A540;
const FORCEWAKE_MEDIA_VEBOX3: usize = 0x0A56C;
const GUC_STATUS: usize = 0x0000_C000;
const GUC_WOPCM_SIZE: usize = 0x0000_C050;
const GUC_SHIM_CONTROL: usize = 0x0000_C064;
const GUC_SHIM_CONTROL2: usize = 0x0000_C068;
const DMA_GUC_WOPCM_OFFSET: usize = 0x0000_C340;
const GEN12_FAULT_TLB_DATA0: usize = 0x0000_CEB8;
const GEN12_RING_FAULT_REG: usize = 0x0000_CEC4;
const GEN8_RING_FAULT_REG_RCS: usize = 0x0000_4094;
const GEN8_RING_FAULT_REG_VCS: usize = 0x0000_4194;
const GEN8_RING_FAULT_REG_BCS: usize = 0x0000_4294;
const GEN8_RING_FAULT_REG_VECS: usize = 0x0000_4394;

const CTX_CONTEXT_CONTROL_DW: usize = 0x02 + 1;
const CTX_RING_HEAD_DW: usize = 0x04 + 1;
const CTX_RING_TAIL_DW: usize = 0x06 + 1;
const CTX_RING_START_DW: usize = 0x08 + 1;
const CTX_RING_CTL_DW: usize = 0x0A + 1;
const CTX_RING_MI_MODE_DW: usize = 0x54 + 1;
const LRC_STATE_OFFSET_DWORDS: usize = 4096 / core::mem::size_of::<u32>();

const CTX_CTRL_RS_CTX_ENABLE: u32 = 1 << 1;
const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;
const CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT: u32 = 1 << 2;
const CTX_CTRL_INHIBIT_SYN_CTX_SWITCH: u32 = 1 << 3;
const RING_MI_MODE_STOP_RING: u32 = 1 << 8;
const MODE_IDLE: u32 = 1 << 9;

const MI_BATCH_BUFFER_START_GEN8: u32 = (0x31 << 23) | 1;
const MI_STORE_DWORD_IMM_GEN4: u32 = 0x1000_0000;
const MI_LOAD_REGISTER_IMM: u32 = 0x1100_0000;
const MI_BATCH_BUFFER_END: u32 = 0x0500_0000;
const MI_NOOP: u32 = 0;

const GS_BOOTROM_SHIFT: u32 = 0;
const GS_BOOTROM_MASK: u32 = 0xFF << GS_BOOTROM_SHIFT;
const GS_UKERNEL_SHIFT: u32 = 8;
const GS_UKERNEL_MASK: u32 = 0xFF << GS_UKERNEL_SHIFT;
const GS_AUTH_STATUS_SHIFT: u32 = 16;
const GS_AUTH_STATUS_MASK: u32 = 0x3 << GS_AUTH_STATUS_SHIFT;

const GUC_WOPCM_OFFSET_VALID: u32 = 1 << 0;
const GUC_WOPCM_OFFSET_SHIFT: u32 = 14;
const GUC_WOPCM_SIZE_LOCKED: u32 = 1 << 0;
const GUC_WOPCM_SIZE_MASK: u32 = 0xFFFFF << 12;
const GEN11_WOPCM_SIZE: u32 = 0x0020_0000;
const WOPCM_RESERVED_SIZE: u32 = 0x0000_4000;
const GUC_WOPCM_RESERVED_SIZE: u32 = 0x0000_4000;
const GUC_WOPCM_STACK_RESERVED_SIZE: u32 = 0x0000_2000;
const WOPCM_HW_CTX_RESERVED_SIZE: u32 = 0x0000_9000;
const GUC_WOPCM_OFFSET_ALIGNMENT: u32 = 1 << GUC_WOPCM_OFFSET_SHIFT;

const RING_FAULT_ENGINE_ID_MASK: u32 = 0x1F << 12;
const RING_FAULT_SRCID_MASK: u32 = 0xFF << 3;
const RING_FAULT_FAULT_TYPE_MASK: u32 = 0x3 << 1;
const RING_FAULT_VALID: u32 = 1 << 0;

const COPY_RESULT_SLOT_BYTES: usize = 8;

#[derive(Clone)]
struct SnapshotValue {
    name: String,
    value: u64,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum SnapshotKind {
    Engine,
    MmioBlock,
    Buffer,
}

#[derive(Clone)]
struct SnapshotRecord {
    kind: SnapshotKind,
    key: String,
    ts_ms: u64,
    values: Vec<SnapshotValue>,
}

struct SessionState {
    last_snapshot: Option<SnapshotRecord>,
    last_write_json: Option<String>,
    last_buffer_patch_json: Option<String>,
    last_submit_json: Option<String>,
}

impl SessionState {
    const fn new() -> Self {
        Self {
            last_snapshot: None,
            last_write_json: None,
            last_buffer_patch_json: None,
            last_submit_json: None,
        }
    }
}

static SESSION_STATE: Mutex<SessionState> = Mutex::new(SessionState::new());

#[derive(Clone, Copy)]
enum EngineTarget {
    Render,
    Blitter,
    Media,
    MediaVcs0,
    MediaVcs1,
    MediaVecs0,
    MediaVecs1,
    Guc,
}

struct ParsedArgs {
    action: String,
    scope: Option<String>,
    engine: Option<String>,
    addr: Option<String>,
    value: Option<String>,
    mask: Option<String>,
    expected: Option<String>,
    count: Option<String>,
    len: Option<String>,
    offset: Option<String>,
    timeout_iters: Option<String>,
    data_hex: Option<String>,
    guard: Option<String>,
}

impl ParsedArgs {
    fn new() -> Self {
        Self {
            action: String::new(),
            scope: None,
            engine: None,
            addr: None,
            value: None,
            mask: None,
            expected: None,
            count: None,
            len: None,
            offset: None,
            timeout_iters: None,
            data_hex: None,
            guard: None,
        }
    }
}

fn escape_json(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                out.push_str(format!("\\u{:04X}", c as u32).as_str());
            }
            _ => out.push(ch),
        }
    }
    out
}

fn now_ms() -> u64 {
    Instant::now().as_millis() as u64
}

fn hex_u32(value: u32) -> String {
    format!("0x{:08X}", value)
}

fn hex_u64(value: u64) -> String {
    format!("0x{:X}", value)
}

fn parse_u64(raw: Option<&str>, field: &str) -> Result<u64, String> {
    let Some(raw) = raw else {
        return Err(format!("missing {}", field));
    };
    let trimmed = raw.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).map_err(|_| format!("invalid {}", field))
    } else {
        trimmed
            .parse::<u64>()
            .map_err(|_| format!("invalid {}", field))
    }
}

fn parse_usize(raw: Option<&str>, field: &str) -> Result<usize, String> {
    let value = parse_u64(raw, field)?;
    usize::try_from(value).map_err(|_| format!("{} out of range", field))
}

fn parse_hex_bytes(raw: Option<&str>) -> Result<Vec<u8>, String> {
    let Some(raw) = raw else {
        return Err(String::from("missing data_hex"));
    };
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && *c != '_')
        .collect();
    if cleaned.len() % 2 != 0 {
        return Err(String::from("data_hex must have even length"));
    }
    let mut out = Vec::new();
    let mut idx = 0usize;
    while idx < cleaned.len() {
        let byte = u8::from_str_radix(&cleaned[idx..idx + 2], 16)
            .map_err(|_| String::from("invalid data_hex"))?;
        out.push(byte);
        idx += 2;
    }
    Ok(out)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::new();
    for byte in bytes {
        out.push_str(format!("{:02X}", byte).as_str());
    }
    out
}

fn parse_tokens(args: &mut SplitWhitespace<'_>) -> Result<ParsedArgs, String> {
    let mut parsed = ParsedArgs::new();
    while let Some(token) = args.next() {
        if let Some((key, value)) = token.split_once('=') {
            match key {
                "action" => parsed.action = String::from(value),
                "scope" => parsed.scope = Some(String::from(value)),
                "engine" => parsed.engine = Some(String::from(value)),
                "addr" => parsed.addr = Some(String::from(value)),
                "value" => parsed.value = Some(String::from(value)),
                "mask" => parsed.mask = Some(String::from(value)),
                "expected" => parsed.expected = Some(String::from(value)),
                "count" => parsed.count = Some(String::from(value)),
                "len" => parsed.len = Some(String::from(value)),
                "offset" => parsed.offset = Some(String::from(value)),
                "timeout_iters" => parsed.timeout_iters = Some(String::from(value)),
                "data_hex" => parsed.data_hex = Some(String::from(value)),
                "guard" => parsed.guard = Some(String::from(value)),
                _ => return Err(format!("unknown token {}", key)),
            }
        } else if parsed.action.is_empty() {
            parsed.action = String::from(token);
        } else {
            return Err(format!("unexpected token {}", token));
        }
    }
    if parsed.action.is_empty() {
        return Err(String::from("missing action"));
    }
    Ok(parsed)
}

fn error_json(action: &str, code: &str, detail: &str) -> String {
    format!(
        "{{\"ok\":false,\"action\":\"{}\",\"code\":\"{}\",\"detail\":\"{}\",\"ts_ms\":{}}}",
        escape_json(action),
        escape_json(code),
        escape_json(detail),
        now_ms()
    )
}

fn success_prefix(action: &str) -> String {
    format!("{{\"ok\":true,\"action\":\"{}\",\"ts_ms\":{}", escape_json(action), now_ms())
}

fn warm_state() -> Result<crate::intel::Igpu770WarmState, String> {
    crate::intel::warm_state().ok_or_else(|| String::from("intel warm state unavailable"))
}

fn mmio_read32(warm: crate::intel::Igpu770WarmState, addr: usize) -> Result<u32, String> {
    if addr.checked_add(4).is_none_or(|end| end > warm.mmio_len) {
        return Err(String::from("mmio addr out of range"));
    }
    let ptr = (warm.mmio_base + addr) as *const u32;
    Ok(unsafe { ptr::read_volatile(ptr) })
}

fn mmio_read64(warm: crate::intel::Igpu770WarmState, addr: usize) -> Result<u64, String> {
    let lo = mmio_read32(warm, addr)? as u64;
    let hi = mmio_read32(warm, addr.saturating_add(4))? as u64;
    Ok(lo | (hi << 32))
}

fn mmio_write32(
    warm: crate::intel::Igpu770WarmState,
    addr: usize,
    value: u32,
) -> Result<(), String> {
    if addr.checked_add(4).is_none_or(|end| end > warm.mmio_len) {
        return Err(String::from("mmio addr out of range"));
    }
    let ptr = (warm.mmio_base + addr) as *mut u32;
    unsafe { ptr::write_volatile(ptr, value) };
    Ok(())
}

fn mmio_write64(
    warm: crate::intel::Igpu770WarmState,
    addr: usize,
    value: u64,
) -> Result<(), String> {
    mmio_write32(warm, addr, value as u32)?;
    mmio_write32(warm, addr.saturating_add(4), (value >> 32) as u32)?;
    Ok(())
}

fn guard_write(parsed: &ParsedArgs) -> Result<(), String> {
    if parsed.guard.as_deref() != Some("write-ok") {
        return Err(String::from("write action requires guard=write-ok"));
    }
    if parsed.scope.is_none() {
        return Err(String::from("write action requires explicit scope"));
    }
    Ok(())
}

fn scope_windows(scope: &str) -> &'static [(usize, usize)] {
    const RENDER: &[(usize, usize)] = &[(RCS_RING_BASE, 0x800)];
    const BLITTER: &[(usize, usize)] = &[(BCS_RING_BASE, 0x800)];
    const MEDIA: &[(usize, usize)] = &[
        (GEN11_VCS0_RING_BASE, 0x10000),
        (GEN11_VECS0_RING_BASE, 0x12000),
        (FORCEWAKE_MEDIA_GEN11, FORCEWAKE_MEDIA_VEBOX3 - FORCEWAKE_MEDIA_GEN11 + 4),
        (0x0D50, 0x40),
        (GEN12_FAULT_TLB_DATA0, GEN12_RING_FAULT_REG - GEN12_FAULT_TLB_DATA0 + 4),
    ];
    const GUC: &[(usize, usize)] = &[(GUC_STATUS, DMA_GUC_WOPCM_OFFSET - GUC_STATUS + 4)];
    const EMPTY: &[(usize, usize)] = &[];

    match scope {
        "render" => RENDER,
        "blitter" => BLITTER,
        "media" => MEDIA,
        "guc" => GUC,
        _ => EMPTY,
    }
}

fn validate_write_scope(scope: &str, addr: usize, bytes: usize) -> Result<(), String> {
    let end = addr.saturating_add(bytes);
    if scope_windows(scope)
        .iter()
        .any(|(base, len)| addr >= *base && end <= base.saturating_add(*len))
    {
        Ok(())
    } else {
        Err(String::from("addr outside guarded scope window"))
    }
}

fn engine_from_str(raw: Option<&str>) -> Result<EngineTarget, String> {
    match raw.unwrap_or("render") {
        "render" => Ok(EngineTarget::Render),
        "blitter" => Ok(EngineTarget::Blitter),
        "media" => Ok(EngineTarget::Media),
        "media.vcs0" => Ok(EngineTarget::MediaVcs0),
        "media.vcs1" => Ok(EngineTarget::MediaVcs1),
        "media.vecs0" => Ok(EngineTarget::MediaVecs0),
        "media.vecs1" => Ok(EngineTarget::MediaVecs1),
        "guc" => Ok(EngineTarget::Guc),
        _ => Err(String::from("unknown engine")),
    }
}

fn engine_key(engine: EngineTarget) -> &'static str {
    match engine {
        EngineTarget::Render => "render",
        EngineTarget::Blitter => "blitter",
        EngineTarget::Media => "media",
        EngineTarget::MediaVcs0 => "media.vcs0",
        EngineTarget::MediaVcs1 => "media.vcs1",
        EngineTarget::MediaVecs0 => "media.vecs0",
        EngineTarget::MediaVecs1 => "media.vecs1",
        EngineTarget::Guc => "guc",
    }
}

fn media_engine_class_str(class: crate::intel::xelp_media_ngin::MediaEngineClass) -> &'static str {
    match class {
        crate::intel::xelp_media_ngin::MediaEngineClass::VideoDecode => "video-decode",
        crate::intel::xelp_media_ngin::MediaEngineClass::VideoEnhancement => "video-enhancement",
    }
}

fn media_provisioning_str(
    provisioning: crate::intel::xelp_media_ngin::MediaProvisioning,
) -> &'static str {
    match provisioning {
        crate::intel::xelp_media_ngin::MediaProvisioning::Kickoff => "kickoff",
        crate::intel::xelp_media_ngin::MediaProvisioning::ScaleOutReserve => "scaleout-reserve",
        crate::intel::xelp_media_ngin::MediaProvisioning::Disabled => "disabled",
    }
}

fn media_workload_str(workload: crate::intel::xelp_media_ngin::MediaWorkloadKind) -> &'static str {
    match workload {
        crate::intel::xelp_media_ngin::MediaWorkloadKind::DecodeBitstream => "decode-bitstream",
        crate::intel::xelp_media_ngin::MediaWorkloadKind::DecodeFrame => "decode-frame",
        crate::intel::xelp_media_ngin::MediaWorkloadKind::EnhanceFrame => "enhance-frame",
        crate::intel::xelp_media_ngin::MediaWorkloadKind::SessionSnapshot => "session-snapshot",
        crate::intel::xelp_media_ngin::MediaWorkloadKind::EngineReset => "engine-reset",
        crate::intel::xelp_media_ngin::MediaWorkloadKind::Smoke => "smoke",
    }
}

fn media_transport_str(
    transport: crate::intel::xelp_media_ngin::MediaSubmissionTransport,
) -> &'static str {
    match transport {
        crate::intel::xelp_media_ngin::MediaSubmissionTransport::GuC => "guc",
        crate::intel::xelp_media_ngin::MediaSubmissionTransport::Execlists => "execlists",
        crate::intel::xelp_media_ngin::MediaSubmissionTransport::Disabled => "disabled",
    }
}

fn media_completion_str(
    completion: crate::intel::xelp_media_ngin::MediaCompletionMode,
) -> &'static str {
    match completion {
        crate::intel::xelp_media_ngin::MediaCompletionMode::ResultMemoryPoll => {
            "result-memory-poll"
        }
        crate::intel::xelp_media_ngin::MediaCompletionMode::ExeclistStatusPoll => {
            "execlist-status-poll"
        }
        crate::intel::xelp_media_ngin::MediaCompletionMode::None => "none",
    }
}

fn media_stage_str(stage: crate::intel::xelp_media_ngin::MediaKickoffStage) -> &'static str {
    match stage {
        crate::intel::xelp_media_ngin::MediaKickoffStage::Discovery => "discovery",
        crate::intel::xelp_media_ngin::MediaKickoffStage::ResourcePlanning => "resource-planning",
        crate::intel::xelp_media_ngin::MediaKickoffStage::SubmissionWiring => "submission-wiring",
        crate::intel::xelp_media_ngin::MediaKickoffStage::CommandEncoding => "command-encoding",
        crate::intel::xelp_media_ngin::MediaKickoffStage::Smoke => "smoke",
    }
}

fn engine_for_ring_decode(parsed: &ParsedArgs) -> Result<EngineTarget, String> {
    match engine_from_str(parsed.engine.as_deref())? {
        EngineTarget::Media => Ok(EngineTarget::MediaVcs0),
        EngineTarget::Guc => Err(String::from("guc does not expose a ring decode view")),
        other => Ok(other),
    }
}

fn ring_base_for_engine(engine: EngineTarget) -> Option<usize> {
    match engine {
        EngineTarget::Render => Some(RCS_RING_BASE),
        EngineTarget::Blitter => Some(BCS_RING_BASE),
        EngineTarget::Media | EngineTarget::MediaVcs0 => Some(GEN11_VCS0_RING_BASE),
        EngineTarget::MediaVcs1 => Some(GEN11_VCS1_RING_BASE),
        EngineTarget::MediaVecs0 => Some(GEN11_VECS0_RING_BASE),
        EngineTarget::MediaVecs1 => Some(GEN11_VECS1_RING_BASE),
        EngineTarget::Guc => None,
    }
}

fn legacy_fault_reg_for_engine(engine: EngineTarget) -> Option<usize> {
    match engine {
        EngineTarget::Render => Some(GEN8_RING_FAULT_REG_RCS),
        EngineTarget::Blitter => Some(GEN8_RING_FAULT_REG_BCS),
        EngineTarget::Media | EngineTarget::MediaVcs0 | EngineTarget::MediaVcs1 => {
            Some(GEN8_RING_FAULT_REG_VCS)
        }
        EngineTarget::MediaVecs0 | EngineTarget::MediaVecs1 => Some(GEN8_RING_FAULT_REG_VECS),
        EngineTarget::Guc => None,
    }
}

fn align_up_u32(value: u32, align: u32) -> Option<u32> {
    if align == 0 {
        return None;
    }
    let mask = align.checked_sub(1)?;
    value.checked_add(mask).map(|sum| sum & !mask)
}

fn compute_gen11_guc_wopcm_layout_local(guc_fw_size: u32, huc_fw_size: u32) -> Option<(u32, u32)> {
    if guc_fw_size == 0 || guc_fw_size >= GEN11_WOPCM_SIZE {
        return None;
    }
    let usable_limit = GEN11_WOPCM_SIZE.checked_sub(WOPCM_HW_CTX_RESERVED_SIZE)?;
    let min_guc_space = guc_fw_size
        .checked_add(GUC_WOPCM_RESERVED_SIZE)?
        .checked_add(GUC_WOPCM_STACK_RESERVED_SIZE)?;
    let huc_floor = huc_fw_size.checked_add(WOPCM_RESERVED_SIZE)?;
    let guc_base = align_up_u32(huc_floor, GUC_WOPCM_OFFSET_ALIGNMENT)?;
    if guc_base >= usable_limit {
        return None;
    }
    let guc_size = (usable_limit - guc_base) & GUC_WOPCM_SIZE_MASK;
    if guc_size < min_guc_space {
        return None;
    }
    Some((guc_base, guc_size))
}

fn guc_bootrom(status: u32) -> u32 {
    (status & GS_BOOTROM_MASK) >> GS_BOOTROM_SHIFT
}

fn guc_ukernel(status: u32) -> u32 {
    (status & GS_UKERNEL_MASK) >> GS_UKERNEL_SHIFT
}

fn guc_auth(status: u32) -> u32 {
    (status & GS_AUTH_STATUS_MASK) >> GS_AUTH_STATUS_SHIFT
}

fn guc_bootrom_name(code: u32) -> &'static str {
    match code {
        0x00 => "NO_KEY",
        0x03 => "AES_PROD_KEY_FOUND",
        0x04 => "RSA_FAILED",
        0x05 => "PAVPC_FAILED",
        0x06 => "WOPCM_FAILED",
        0x07 => "LOADLOC_FAILED",
        0x76 => "JUMP_PASSED",
        0x77 => "JUMP_FAILED",
        0x79 => "RC6CTXCONFIG_FAILED",
        0x7A => "MPUMAP_INCORRECT",
        0x7B => "EXCEPTION",
        0x7C => "PROD_KEY_CHECK",
        _ => "OK_OR_UNKNOWN",
    }
}

fn guc_ukernel_name(code: u32) -> &'static str {
    match code {
        0x00 => "START",
        0x01 => "HWCONFIG_START",
        0x05 => "HWCONFIG_DONE",
        0x06 => "GDT_DONE",
        0x07 => "IDT_DONE",
        0x08 => "LAPIC_DONE",
        0x09 => "GUCINT_DONE",
        0x0A => "DPC_READY",
        0xF0 => "READY",
        0xF1 => "DEVID_MISMATCH",
        0xF2 => "PREPROD_MISMATCH",
        0xF3 => "INVALID_GUCTYPE",
        0xF4 => "HWCONFIG_ERROR",
        0xF5 => "BOOTROM_VERSION_MISMATCH",
        0xF6 => "DPC_ERROR",
        0xF7 => "EXCEPTION",
        0xF8 => "INIT_DATA_INVALID",
        0xF9 => "PXP_TEARDOWN_CTRL_ENABLED",
        0xFA => "MPU_DATA_INVALID",
        0xFB => "MMIO_SR_INVALID",
        0xFC => "KLV_INIT_ERROR",
        _ => "UNKNOWN",
    }
}

fn guc_auth_name(code: u32) -> &'static str {
    match code {
        0 => "none",
        1 => "in-progress",
        2 => "done",
        3 => "failed",
        _ => "unknown",
    }
}

fn guc_status_terminal(status: u32) -> Option<bool> {
    match guc_ukernel(status) {
        0xF0 => return Some(true),
        0xF1..=0xFC => return Some(false),
        _ => {}
    }
    match guc_bootrom(status) {
        0x00 | 0x04 | 0x05 | 0x06 | 0x07 | 0x77 | 0x79 | 0x7A | 0x7B | 0x7C => Some(false),
        _ => None,
    }
}

fn pagefault_type_name(code: u32) -> &'static str {
    match code {
        0 => "not-present",
        1 => "write-access-violation",
        2 => "atomic-access-violation",
        _ => "reserved",
    }
}

fn mi_instruction_words(first: u32) -> usize {
    let low = (first & 0xFF) as usize;
    if first == MI_NOOP || first == MI_BATCH_BUFFER_END {
        1
    } else if (first & 0xFF80_0000) == MI_LOAD_REGISTER_IMM
        || (first & 0xFF80_0000) == MI_BATCH_BUFFER_START_GEN8
        || (first & 0xFF80_0000) == MI_STORE_DWORD_IMM_GEN4
    {
        low.saturating_add(2)
    } else {
        1
    }
}

fn append_json_quoted_array(out: &mut String, items: &[String]) {
    out.push('[');
    let mut first = true;
    for item in items {
        if !first {
            out.push(',');
        }
        first = false;
        out.push('"');
        out.push_str(escape_json(item.as_str()).as_str());
        out.push('"');
    }
    out.push(']');
}

fn execute_ring_decode(parsed: &ParsedArgs) -> Result<String, String> {
    let warm = warm_state()?;
    let engine = engine_for_ring_decode(parsed)?;
    let base = ring_base_for_engine(engine).ok_or_else(|| String::from("ring base unavailable"))?;
    let tail = mmio_read32(warm, base + RING_TAIL)?;
    let head = mmio_read32(warm, base + RING_HEAD)?;
    let start = mmio_read32(warm, base + RING_START)?;
    let ctl = mmio_read32(warm, base + RING_CTL)?;
    let acthd = mmio_read32(warm, base + RING_ACTHD)?;
    let mi_mode = mmio_read32(warm, base + RING_MI_MODE)?;
    let mode = mmio_read32(warm, base + RING_MODE_GEN7)?;
    let ctx_ctl = mmio_read32(warm, base + RING_CONTEXT_CONTROL)?;
    let execlist_lo = mmio_read32(warm, base + RING_EXECLIST_STATUS_LO)?;
    let execlist_hi = mmio_read32(warm, base + RING_EXECLIST_STATUS_HI)?;
    let execlist_ctl = mmio_read32(warm, base + RING_EXECLIST_CONTROL)?;
    let bbaddr = mmio_read64(warm, base + RING_BBADDR)?;

    Ok(format!(
        "{{\"ok\":true,\"action\":\"ring_decode\",\"ts_ms\":{},\"engine\":\"{}\",\"ring_base_hex\":\"{}\",\"raw\":{{\"tail_hex\":\"{}\",\"head_hex\":\"{}\",\"start_hex\":\"{}\",\"ctl_hex\":\"{}\",\"acthd_hex\":\"{}\",\"mi_mode_hex\":\"{}\",\"mode_hex\":\"{}\",\"ctx_ctl_hex\":\"{}\",\"execlist_lo_hex\":\"{}\",\"execlist_hi_hex\":\"{}\",\"execlist_ctl_hex\":\"{}\",\"bbaddr_hex\":\"{}\"}},\"decoded\":{{\"head_offset_bytes\":{},\"tail_offset_bytes\":{},\"ring_running\":{},\"stop_ring\":{},\"idle\":{},\"context_restore_inhibit\":{},\"context_save_inhibit\":{},\"inhibit_sync_switch\":{},\"rs_ctx_enable\":{}}}}}",
        now_ms(),
        engine_key(engine),
        hex_u64(base as u64),
        hex_u32(tail),
        hex_u32(head),
        hex_u32(start),
        hex_u32(ctl),
        hex_u32(acthd),
        hex_u32(mi_mode),
        hex_u32(mode),
        hex_u32(ctx_ctl),
        hex_u32(execlist_lo),
        hex_u32(execlist_hi),
        hex_u32(execlist_ctl),
        hex_u64(bbaddr),
        (head & !0x7) as usize,
        (tail & !0x7) as usize,
        (ctl & 1) != 0 && (mi_mode & RING_MI_MODE_STOP_RING) == 0,
        (mi_mode & RING_MI_MODE_STOP_RING) != 0,
        (mode & MODE_IDLE) != 0,
        (ctx_ctl & CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT) != 0,
        (ctx_ctl & CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT) != 0,
        (ctx_ctl & CTX_CTRL_INHIBIT_SYN_CTX_SWITCH) != 0,
        (ctx_ctl & CTX_CTRL_RS_CTX_ENABLE) != 0,
    ))
}

fn execute_context_image_decode(parsed: &ParsedArgs) -> Result<String, String> {
    let scope = parsed.scope.as_deref().unwrap_or("context");
    let offset = parse_usize(parsed.offset.as_deref().or(Some("0")), "offset")?;
    let len = parse_usize(parsed.len.as_deref().or(Some("64")), "len")?;
    if (offset & 3) != 0 || (len & 3) != 0 {
        return Err(String::from("context decode requires offset and len aligned to 4 bytes"));
    }
    let (_, gpu_addr, phys, virt, bytes) = buffer_window(scope)?;
    if virt.is_null() {
        return Err(String::from("buffer virtual address unavailable"));
    }
    if offset.checked_add(len).is_none_or(|end| end > bytes) {
        return Err(String::from("context decode window out of range"));
    }
    let dwords = unsafe { core::slice::from_raw_parts(virt as *const u32, bytes / 4) };
    let read_dw = |index: usize| -> Result<u32, String> {
        dwords
            .get(index)
            .copied()
            .ok_or_else(|| String::from("context image too small"))
    };
    let raw_window =
        unsafe { core::slice::from_raw_parts(unsafe { virt.add(offset) } as *const u32, len / 4) };
    let mut window_hex = Vec::new();
    for value in raw_window {
        window_hex.push(hex_u32(*value));
    }
    let ctx_ctl = read_dw(CTX_CONTEXT_CONTROL_DW)?;
    let ring_head = read_dw(CTX_RING_HEAD_DW)?;
    let ring_tail = read_dw(CTX_RING_TAIL_DW)?;
    let ring_start = read_dw(CTX_RING_START_DW)?;
    let ring_ctl = read_dw(CTX_RING_CTL_DW)?;
    let ring_mi_mode = read_dw(CTX_RING_MI_MODE_DW)?;
    let lrc_dword0 = read_dw(LRC_STATE_OFFSET_DWORDS)?;

    let mut out = success_prefix("context_image_decode");
    out.push_str(
        format!(
            ",\"scope\":\"{}\",\"gpu_addr_hex\":\"{}\",\"phys_hex\":\"{}\",\"context_bytes\":{},\"window_offset\":{},\"window_len\":{},\"decoded\":{{\"context_control_hex\":\"{}\",\"ring_head_hex\":\"{}\",\"ring_tail_hex\":\"{}\",\"ring_start_hex\":\"{}\",\"ring_ctl_hex\":\"{}\",\"ring_mi_mode_hex\":\"{}\",\"lrc_state_offset_dwords\":{},\"lrc_state_dword0_hex\":\"{}\",\"rs_ctx_enable\":{},\"restore_inhibit\":{},\"save_inhibit\":{},\"inhibit_sync_switch\":{},\"stop_ring\":{}}},\"window_dwords_hex\":",
            escape_json(scope),
            hex_u64(gpu_addr),
            hex_u64(phys),
            bytes,
            offset,
            len,
            hex_u32(ctx_ctl),
            hex_u32(ring_head),
            hex_u32(ring_tail),
            hex_u32(ring_start),
            hex_u32(ring_ctl),
            hex_u32(ring_mi_mode),
            LRC_STATE_OFFSET_DWORDS,
            hex_u32(lrc_dword0),
            (ctx_ctl & CTX_CTRL_RS_CTX_ENABLE) != 0,
            (ctx_ctl & CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT) != 0,
            (ctx_ctl & CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT) != 0,
            (ctx_ctl & CTX_CTRL_INHIBIT_SYN_CTX_SWITCH) != 0,
            (ring_mi_mode & RING_MI_MODE_STOP_RING) != 0,
        )
        .as_str(),
    );
    append_json_quoted_array(&mut out, window_hex.as_slice());
    out.push('}');
    Ok(out)
}

fn decode_batch_instruction(words: &[u32], idx: usize, gpu_addr: u64) -> (usize, String) {
    let first = words[idx];
    let count = mi_instruction_words(first)
        .min(words.len().saturating_sub(idx))
        .max(1);
    let mut raw_hex = Vec::new();
    for word in &words[idx..idx + count] {
        raw_hex.push(hex_u32(*word));
    }
    let mut name = "unknown-mi";
    let mut detail = String::new();
    if first == MI_NOOP {
        name = "MI_NOOP";
        detail.push_str("noop");
    } else if first == MI_BATCH_BUFFER_END {
        name = "MI_BATCH_BUFFER_END";
        detail.push_str("end");
    } else if (first & 0xFF80_0000) == MI_LOAD_REGISTER_IMM {
        name = "MI_LOAD_REGISTER_IMM";
        let reg_pairs = count.saturating_sub(1) / 2;
        detail.push_str(format!("register_pairs={}", reg_pairs).as_str());
    } else if (first & 0xFF80_0000) == MI_STORE_DWORD_IMM_GEN4 {
        name = "MI_STORE_DWORD_IMM";
        if count >= 4 {
            let addr = words[idx + 1] as u64 | ((words[idx + 2] as u64) << 32);
            detail.push_str(
                format!(
                    "store_addr_hex={} immediate_hex={}",
                    hex_u64(addr),
                    hex_u32(words[idx + 3])
                )
                .as_str(),
            );
        }
    } else if (first & 0xFF80_0000) == MI_BATCH_BUFFER_START_GEN8 {
        name = "MI_BATCH_BUFFER_START";
        if count >= 3 {
            let addr = words[idx + 1] as u64 | ((words[idx + 2] as u64) << 32);
            detail.push_str(
                format!(
                    "target_hex={} non_secure={} ggtt={}",
                    hex_u64(addr),
                    (first & (1 << 8)) != 0,
                    (first & (1 << 22)) != 0,
                )
                .as_str(),
            );
        }
    } else {
        detail.push_str(format!("opcode_class=0x{:02X}", first >> 23).as_str());
    }

    let mut out = String::from("{");
    out.push_str(
        format!(
            "\"dword_index\":{},\"offset_bytes\":{},\"gpu_addr_hex\":\"{}\",\"opcode_hex\":\"{}\",\"name\":\"{}\",\"dword_count\":{},\"annotation\":\"{}\",\"raw_dwords_hex\":",
            idx,
            idx.saturating_mul(4),
            hex_u64(gpu_addr + (idx as u64 * 4)),
            hex_u32(first),
            name,
            count,
            escape_json(detail.as_str())
        )
        .as_str(),
    );
    append_json_quoted_array(&mut out, raw_hex.as_slice());
    out.push('}');
    (count, out)
}

fn execute_batch_disasm(parsed: &ParsedArgs, action: &str) -> Result<String, String> {
    let scope = parsed.scope.as_deref().unwrap_or("batch");
    let offset = parse_usize(parsed.offset.as_deref().or(Some("0")), "offset")?;
    let (name, gpu_addr, phys, virt, bytes) = buffer_window(scope)?;
    if virt.is_null() {
        return Err(String::from("buffer virtual address unavailable"));
    }
    if (offset & 3) != 0 {
        return Err(String::from("batch disasm requires a 4-byte aligned offset"));
    }
    let default_len = bytes.saturating_sub(offset).min(256);
    let len = if let Some(raw) = parsed.len.as_deref() {
        parse_usize(Some(raw), "len")?
    } else {
        default_len
    };
    if (len & 3) != 0 {
        return Err(String::from("batch disasm requires len aligned to 4 bytes"));
    }
    if offset.checked_add(len).is_none_or(|end| end > bytes) {
        return Err(String::from("batch disasm window out of range"));
    }
    let words =
        unsafe { core::slice::from_raw_parts(unsafe { virt.add(offset) } as *const u32, len / 4) };
    let mut out = success_prefix(action);
    out.push_str(
        format!(
            ",\"scope\":\"{}\",\"gpu_addr_hex\":\"{}\",\"phys_hex\":\"{}\",\"offset\":{},\"len\":{},\"instruction_count\":",
            escape_json(name.as_str()),
            hex_u64(gpu_addr),
            hex_u64(phys),
            offset,
            len,
        )
        .as_str(),
    );
    let count_pos = out.len();
    out.push('0');
    out.push_str(",\"instructions\":[");
    let mut idx = 0usize;
    let mut first = true;
    let mut inst_count = 0usize;
    while idx < words.len() && inst_count < 128 {
        let (step, json) = decode_batch_instruction(words, idx, gpu_addr + offset as u64);
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(json.as_str());
        idx = idx.saturating_add(step.max(1));
        inst_count += 1;
        if words[idx.saturating_sub(1)] == MI_BATCH_BUFFER_END {
            break;
        }
    }
    out.push_str("]}");
    let count_str = format!("{}", inst_count);
    out.replace_range(count_pos..count_pos + 1, count_str.as_str());
    Ok(out)
}

fn execute_fault_status_decode(parsed: &ParsedArgs) -> Result<String, String> {
    let warm = warm_state()?;
    let engine = parsed
        .engine
        .as_deref()
        .map(|_| engine_for_ring_decode(parsed))
        .transpose()?
        .unwrap_or(EngineTarget::Render);
    let legacy_addr = legacy_fault_reg_for_engine(engine)
        .ok_or_else(|| String::from("fault decode unavailable for engine"))?;
    let ring_fault = mmio_read32(warm, GEN12_RING_FAULT_REG)?;
    let legacy_fault = mmio_read32(warm, legacy_addr)?;
    let tlb0 = mmio_read32(warm, GEN12_FAULT_TLB_DATA0)?;
    let active = if (ring_fault & RING_FAULT_VALID) != 0 {
        ring_fault
    } else {
        legacy_fault
    };
    let fault_type = (active & RING_FAULT_FAULT_TYPE_MASK) >> 1;
    let srcid = (active & RING_FAULT_SRCID_MASK) >> 3;
    let engine_id = (active & RING_FAULT_ENGINE_ID_MASK) >> 12;

    Ok(format!(
        "{{\"ok\":true,\"action\":\"fault_status_decode\",\"ts_ms\":{},\"engine\":\"{}\",\"raw\":{{\"gen12_ring_fault_hex\":\"{}\",\"legacy_ring_fault_hex\":\"{}\",\"fault_tlb_data0_hex\":\"{}\",\"legacy_addr_hex\":\"{}\"}},\"decoded\":{{\"valid\":{},\"fault_type\":{},\"fault_type_name\":\"{}\",\"srcid\":{},\"engine_id\":{},\"tlb_page_hex\":\"{}\"}}}}",
        now_ms(),
        engine_key(engine),
        hex_u32(ring_fault),
        hex_u32(legacy_fault),
        hex_u32(tlb0),
        hex_u64(legacy_addr as u64),
        (active & RING_FAULT_VALID) != 0,
        fault_type,
        pagefault_type_name(fault_type),
        srcid,
        engine_id,
        hex_u64((tlb0 as u64) & !0xFFF)
    ))
}

fn execute_guc_state_view() -> Result<String, String> {
    let warm = warm_state()?;
    let status = crate::intel::guc_status(warm);
    let wopcm_size = mmio_read32(warm, GUC_WOPCM_SIZE)?;
    let wopcm_offset = mmio_read32(warm, DMA_GUC_WOPCM_OFFSET)?;
    let computed = u32::try_from(warm.guc_fw_len)
        .ok()
        .and_then(|fw| compute_gen11_guc_wopcm_layout_local(fw, 0));
    let bootrom = guc_bootrom(status);
    let ukernel = guc_ukernel(status);
    let auth = guc_auth(status);
    let computed_base = computed.map(|(base, _)| base);
    let computed_size = computed.map(|(_, size)| size);

    Ok(format!(
        "{{\"ok\":true,\"action\":\"guc_state_view\",\"ts_ms\":{},\"ready\":{},\"status_hex\":\"{}\",\"decode\":{{\"bootrom\":{{\"value\":{},\"name\":\"{}\"}},\"ukernel\":{{\"value\":{},\"name\":\"{}\"}},\"auth\":{{\"value\":{},\"name\":\"{}\"}},\"terminal\":{}}},\"buffers\":{{\"fw_gpu_hex\":\"{}\",\"fw_phys_hex\":\"{}\",\"fw_len\":{},\"ads_gpu_hex\":\"{}\",\"ads_phys_hex\":\"{}\",\"ads_len\":{}}},\"wopcm\":{{\"size_reg_hex\":\"{}\",\"offset_reg_hex\":\"{}\",\"size_locked\":{},\"offset_valid\":{},\"programmed_size_bytes\":{},\"programmed_base_bytes\":{},\"computed_base_bytes\":{},\"computed_size_bytes\":{}}}}}",
        now_ms(),
        guc_status_terminal(status) == Some(true),
        hex_u32(status),
        bootrom,
        guc_bootrom_name(bootrom),
        ukernel,
        guc_ukernel_name(ukernel),
        auth,
        guc_auth_name(auth),
        match guc_status_terminal(status) {
            Some(true) => "true",
            Some(false) => "false",
            None => "null",
        },
        hex_u64(warm.guc_fw_gpu_addr),
        hex_u64(warm.guc_fw_phys),
        warm.guc_fw_len,
        hex_u64(warm.guc_ads_gpu_addr),
        hex_u64(warm.guc_ads_phys),
        warm.guc_ads_len,
        hex_u32(wopcm_size),
        hex_u32(wopcm_offset),
        (wopcm_size & GUC_WOPCM_SIZE_LOCKED) != 0,
        (wopcm_offset & GUC_WOPCM_OFFSET_VALID) != 0,
        wopcm_size & GUC_WOPCM_SIZE_MASK,
        wopcm_offset & !((1 << GUC_WOPCM_OFFSET_SHIFT) - 1),
        computed_base
            .map(|value| format!("{}", value))
            .unwrap_or_else(|| String::from("null")),
        computed_size
            .map(|value| format!("{}", value))
            .unwrap_or_else(|| String::from("null")),
    ))
}

fn execute_huc_state_view() -> Result<String, String> {
    let Some(state) = crate::intel::media_kickoff_state() else {
        return Err(String::from("media kickoff state unavailable"));
    };
    let mut capable_engines = Vec::new();
    for engine in state
        .topology
        .engines
        .iter()
        .take(state.topology.planned_engine_count)
    {
        if engine.capabilities.huc_assist {
            capable_engines.push(String::from(engine.name));
        }
    }
    let warm = warm_state()?;
    let computed = u32::try_from(warm.guc_fw_len)
        .ok()
        .and_then(|fw| compute_gen11_guc_wopcm_layout_local(fw, 0));
    let mut out = success_prefix("huc_state_view");
    out.push_str(
        format!(
            ",\"image_present\":false,\"status\":\"not-wired\",\"capable_engine_count\":{},\"capable_engines\":",
            capable_engines.len()
        )
        .as_str(),
    );
    append_json_quoted_array(&mut out, capable_engines.as_slice());
    out.push_str(",\"derived\":{");
    out.push_str(format!("\"wopcm_reserved_bytes\":{},", WOPCM_RESERVED_SIZE).as_str());
    if let Some((base, size)) = computed {
        out.push_str(
            format!(
                "\"computed_guc_base_without_huc_bytes\":{},\"computed_guc_size_without_huc_bytes\":{}",
                base, size
            )
            .as_str(),
        );
    } else {
        out.push_str("\"computed_guc_base_without_huc_bytes\":null,\"computed_guc_size_without_huc_bytes\":null");
    }
    out.push_str("}}");
    Ok(out)
}

fn execute_media_state_view() -> Result<String, String> {
    let Some(state) = crate::intel::media_kickoff_state() else {
        return Err(String::from("media kickoff state unavailable"));
    };
    let mut out = success_prefix("media_state_view");
    out.push_str(
        format!(
            ",\"stage\":\"{}\",\"preferred_transport\":\"{}\",\"guc_ready\":{},\"guc_status_hex\":\"{}\",\"topology\":{{\"sku_name\":\"{}\",\"active_engine_count\":{},\"planned_engine_count\":{},\"default_decode\":{},\"default_enhance\":{},\"engines\":[",
            media_stage_str(state.stage),
            media_transport_str(state.preferred_transport),
            state.guc_ready,
            hex_u32(state.guc_status),
            escape_json(state.topology.sku_name),
            state.topology.active_engine_count,
            state.topology.planned_engine_count,
            state.topology.default_decode.map(|id| format!("\"{}.{}\"", media_engine_class_str(id.class), id.instance)).unwrap_or_else(|| String::from("null")),
            state.topology.default_enhance.map(|id| format!("\"{}.{}\"", media_engine_class_str(id.class), id.instance)).unwrap_or_else(|| String::from("null")),
        )
        .as_str(),
    );
    let mut first = true;
    for engine in state
        .topology
        .engines
        .iter()
        .take(state.topology.planned_engine_count)
    {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(
            format!(
                "{{\"name\":\"{}\",\"class\":\"{}\",\"instance\":{},\"ring_base_hex\":\"{}\",\"provisioning\":\"{}\",\"default_workload\":\"{}\",\"capabilities\":{{\"decode\":{},\"enhance\":{},\"huc_assist\":{},\"sfc\":{},\"relative_mmio_lrc\":{}}}}}",
                escape_json(engine.name),
                media_engine_class_str(engine.id.class),
                engine.id.instance,
                hex_u64(engine.ring_base as u64),
                media_provisioning_str(engine.provisioning),
                media_workload_str(engine.default_workload),
                engine.capabilities.decode,
                engine.capabilities.enhance,
                engine.capabilities.huc_assist,
                engine.capabilities.sfc,
                engine.capabilities.relative_mmio_lrc,
            )
            .as_str(),
        );
    }
    out.push_str("]},\"plans\":[");
    first = true;
    for plan in state.plans.iter().take(state.plan_count) {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(
            format!(
                "{{\"engine\":\"{}\",\"next_stage\":\"{}\",\"resources\":{{\"ring_bytes\":{},\"context_bytes\":{},\"batch_bytes\":{},\"scratch_bytes\":{},\"bitstream_bytes\":{},\"output_surface_bytes\":{},\"result_bytes\":{},\"ring_gpu_hex\":\"{}\",\"context_gpu_hex\":\"{}\",\"batch_gpu_hex\":\"{}\",\"bitstream_gpu_hex\":\"{}\",\"output_surface_gpu_hex\":\"{}\",\"result_gpu_hex\":\"{}\"}},\"context\":{{\"bytes\":{},\"lrc_state_offset_dwords\":{},\"indirect_state_dwords_budget\":{},\"uses_relative_mmio\":{},\"engine_mmio_base_hex\":\"{}\"}},\"batch\":{{\"preamble_dwords\":{},\"payload_budget_dwords\":{},\"epilogue_dwords\":{},\"completion_slot_gpu_hex\":\"{}\",\"completion_marker_hex\":\"{}\"}},\"submission\":{{\"workload\":\"{}\",\"transport\":\"{}\",\"completion\":\"{}\",\"queue_depth\":{},\"watchdog_iters\":{},\"prefers_parallel_submission\":{}}}}}",
                escape_json(plan.descriptor.name),
                media_stage_str(plan.next_stage),
                plan.resources.ring_bytes,
                plan.resources.context_bytes,
                plan.resources.batch_bytes,
                plan.resources.scratch_bytes,
                plan.resources.bitstream_bytes,
                plan.resources.output_surface_bytes,
                plan.resources.result_bytes,
                hex_u64(plan.resources.windows.ring_gpu_addr),
                hex_u64(plan.resources.windows.context_gpu_addr),
                hex_u64(plan.resources.windows.batch_gpu_addr),
                hex_u64(plan.resources.windows.bitstream_gpu_addr),
                hex_u64(plan.resources.windows.output_surface_gpu_addr),
                hex_u64(plan.resources.windows.result_gpu_addr),
                plan.context.bytes,
                plan.context.lrc_state_offset_dwords,
                plan.context.indirect_state_dwords_budget,
                plan.context.uses_relative_mmio,
                hex_u64(plan.context.engine_mmio_base as u64),
                plan.batch.preamble_dwords,
                plan.batch.payload_budget_dwords,
                plan.batch.epilogue_dwords,
                hex_u64(plan.batch.completion_slot_gpu_addr),
                hex_u32(plan.batch.completion_marker_value),
                media_workload_str(plan.submission.workload),
                media_transport_str(plan.submission.transport),
                media_completion_str(plan.submission.completion),
                plan.submission.queue_depth,
                plan.submission.watchdog_iters,
                plan.submission.prefers_parallel_submission,
            )
            .as_str(),
        );
    }
    out.push_str("],\"runtimes\":[");
    first = true;
    for runtime in state.runtimes.iter().take(state.runtime_count) {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(
            format!(
                "{{\"name\":\"{}\",\"ring_base_hex\":\"{}\",\"observed\":{},\"tail_hex\":\"{}\",\"head_hex\":\"{}\",\"start_hex\":\"{}\",\"ctl_hex\":\"{}\",\"acthd_hex\":\"{}\",\"mi_mode_hex\":\"{}\",\"mode_hex\":\"{}\",\"ctx_ctl_hex\":\"{}\",\"execlist_ctl_hex\":\"{}\",\"execlist_lo_hex\":\"{}\",\"execlist_hi_hex\":\"{}\",\"ipeir_hex\":\"{}\",\"ipehr_hex\":\"{}\",\"instdone_hex\":\"{}\",\"instps_hex\":\"{}\"}}",
                escape_json(runtime.name),
                hex_u64(runtime.ring_base as u64),
                runtime.observed,
                hex_u32(runtime.tail),
                hex_u32(runtime.head),
                hex_u32(runtime.start),
                hex_u32(runtime.ctl),
                hex_u32(runtime.acthd),
                hex_u32(runtime.mi_mode),
                hex_u32(runtime.mode),
                hex_u32(runtime.ctx_ctl),
                hex_u32(runtime.execlist_ctl),
                hex_u32(runtime.execlist_status_lo),
                hex_u32(runtime.execlist_status_hi),
                hex_u32(runtime.ipeir),
                hex_u32(runtime.ipehr),
                hex_u32(runtime.instdone),
                hex_u32(runtime.instps),
            )
            .as_str(),
        );
    }
    out.push_str("],\"api\":[");
    first = true;
    for route in state.api.routes.iter().take(state.api.route_count) {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(
            format!(
                "{{\"name\":\"{}\",\"workload\":\"{}\",\"preferred_engine_class\":{},\"transport\":\"{}\",\"summary\":\"{}\"}}",
                escape_json(route.name),
                media_workload_str(route.workload),
                route.preferred_engine_class.map(|class| format!("\"{}\"", media_engine_class_str(class))).unwrap_or_else(|| String::from("null")),
                media_transport_str(route.transport),
                escape_json(route.summary),
            )
            .as_str(),
        );
    }
    out.push_str("]}");
    Ok(out)
}

fn buffer_window(scope: &str) -> Result<(String, u64, u64, *mut u8, usize), String> {
    let warm = warm_state()?;
    match scope {
        "ring" => {
            Ok((String::from("ring"), 0x0080_0000, warm.ring_phys, warm.ring_virt, warm.ring_len))
        }
        "context" => Ok((
            String::from("context"),
            0x0081_0000,
            warm.context_phys,
            warm.context_virt,
            warm.context_len,
        )),
        "batch" => Ok((
            String::from("batch"),
            0x0083_0000,
            warm.batch_phys,
            warm.batch_virt,
            warm.batch_len,
        )),
        "result" => Ok((
            String::from("result"),
            0x0084_0000,
            warm.result_phys,
            warm.result_virt,
            warm.result_len,
        )),
        "guc_fw" => Ok((
            String::from("guc_fw"),
            warm.guc_fw_gpu_addr,
            warm.guc_fw_phys,
            warm.guc_fw_virt,
            warm.guc_fw_len,
        )),
        "guc_ads" => Ok((
            String::from("guc_ads"),
            warm.guc_ads_gpu_addr,
            warm.guc_ads_phys,
            warm.guc_ads_virt,
            warm.guc_ads_len,
        )),
        "framebuffer" => Ok((
            String::from("framebuffer"),
            0,
            warm.limine_fb_phys,
            warm.limine_fb_virt as *mut u8,
            warm.limine_fb_size,
        )),
        other if other.starts_with("media.") => {
            let Some(surface) = crate::intel::media_demo_surface_window(other) else {
                return Err(String::from("media surface unavailable"));
            };
            Ok((
                String::from(surface.name),
                surface.gpu_addr,
                surface.phys,
                surface.virt,
                surface.bytes,
            ))
        }
        _ => Err(String::from("unknown buffer scope")),
    }
}

fn push_snapshot_value(values: &mut Vec<SnapshotValue>, name: &str, value: u64) {
    values.push(SnapshotValue {
        name: String::from(name),
        value,
    });
}

fn capture_ring_snapshot(engine: EngineTarget) -> Result<SnapshotRecord, String> {
    let warm = warm_state()?;
    let base = match engine {
        EngineTarget::Render => RCS_RING_BASE,
        EngineTarget::Blitter => BCS_RING_BASE,
        _ => return Err(String::from("engine is not a ring engine")),
    };

    let mut values = Vec::new();
    for (name, off) in [
        ("tail", RING_TAIL),
        ("head", RING_HEAD),
        ("start", RING_START),
        ("ctl", RING_CTL),
        ("acthd", RING_ACTHD),
        ("mi_mode", RING_MI_MODE),
        ("mode", RING_MODE_GEN7),
        ("ctx_ctl", RING_CONTEXT_CONTROL),
        ("execlist_ctl", RING_EXECLIST_CONTROL),
        ("execlist_lo", RING_EXECLIST_STATUS_LO),
        ("execlist_hi", RING_EXECLIST_STATUS_HI),
        ("ipeir", RING_IPEIR),
        ("ipehr", RING_IPEHR),
        ("eir", RING_EIR),
        ("emr", RING_EMR),
        ("instdone", RING_INSTDONE),
        ("instps", RING_INSTPS),
        ("bbaddr", RING_BBADDR),
        ("bbaddr_udw", RING_BBADDR_UDW),
    ] {
        push_snapshot_value(&mut values, name, mmio_read32(warm, base + off)? as u64);
    }

    let result_ptr = warm.result_virt as *const u32;
    if !result_ptr.is_null() {
        unsafe {
            if engine_key(engine) == "render" {
                push_snapshot_value(&mut values, "result0", ptr::read_volatile(result_ptr) as u64);
            } else {
                push_snapshot_value(
                    &mut values,
                    "result_start",
                    ptr::read_volatile(result_ptr) as u64,
                );
                push_snapshot_value(
                    &mut values,
                    "result_pre_copy",
                    ptr::read_volatile(
                        (warm.result_virt as usize + COPY_RESULT_SLOT_BYTES) as *const u32,
                    ) as u64,
                );
                push_snapshot_value(
                    &mut values,
                    "result_post_copy",
                    ptr::read_volatile(
                        (warm.result_virt as usize + (2 * COPY_RESULT_SLOT_BYTES)) as *const u32,
                    ) as u64,
                );
                push_snapshot_value(
                    &mut values,
                    "result_done",
                    ptr::read_volatile(
                        (warm.result_virt as usize + (3 * COPY_RESULT_SLOT_BYTES)) as *const u32,
                    ) as u64,
                );
            }
        }
    }

    Ok(SnapshotRecord {
        kind: SnapshotKind::Engine,
        key: String::from(engine_key(engine)),
        ts_ms: now_ms(),
        values,
    })
}

fn capture_media_snapshot(engine: EngineTarget) -> Result<SnapshotRecord, String> {
    let Some(state) = crate::intel::media_kickoff_state() else {
        return Err(String::from("media kickoff state unavailable"));
    };

    let mut values = Vec::new();
    push_snapshot_value(&mut values, "guc_ready", state.guc_ready as u64);
    push_snapshot_value(&mut values, "guc_status", state.guc_status as u64);
    push_snapshot_value(&mut values, "wake.global_req", state.wake.global_req as u64);
    push_snapshot_value(&mut values, "wake.global_ack", state.wake.global_ack as u64);
    push_snapshot_value(&mut values, "wake.awake_count", state.wake.awake_count as u64);

    let target_name = match engine {
        EngineTarget::Media => None,
        EngineTarget::MediaVcs0 => Some("vcs0"),
        EngineTarget::MediaVcs1 => Some("vcs1"),
        EngineTarget::MediaVecs0 => Some("vecs0"),
        EngineTarget::MediaVecs1 => Some("vecs1"),
        _ => None,
    };

    for runtime in state.runtimes.iter().take(state.runtime_count) {
        if let Some(name) = target_name
            && runtime.name != name
        {
            continue;
        }
        push_snapshot_value(
            &mut values,
            format!("{}.observed", runtime.name).as_str(),
            runtime.observed as u64,
        );
        push_snapshot_value(
            &mut values,
            format!("{}.tail", runtime.name).as_str(),
            runtime.tail as u64,
        );
        push_snapshot_value(
            &mut values,
            format!("{}.head", runtime.name).as_str(),
            runtime.head as u64,
        );
        push_snapshot_value(
            &mut values,
            format!("{}.start", runtime.name).as_str(),
            runtime.start as u64,
        );
        push_snapshot_value(
            &mut values,
            format!("{}.ctl", runtime.name).as_str(),
            runtime.ctl as u64,
        );
        push_snapshot_value(
            &mut values,
            format!("{}.acthd", runtime.name).as_str(),
            runtime.acthd as u64,
        );
        push_snapshot_value(
            &mut values,
            format!("{}.mi_mode", runtime.name).as_str(),
            runtime.mi_mode as u64,
        );
        push_snapshot_value(
            &mut values,
            format!("{}.mode", runtime.name).as_str(),
            runtime.mode as u64,
        );
        push_snapshot_value(
            &mut values,
            format!("{}.ctx_ctl", runtime.name).as_str(),
            runtime.ctx_ctl as u64,
        );
        push_snapshot_value(
            &mut values,
            format!("{}.execlist_ctl", runtime.name).as_str(),
            runtime.execlist_ctl as u64,
        );
        push_snapshot_value(
            &mut values,
            format!("{}.execlist_lo", runtime.name).as_str(),
            runtime.execlist_status_lo as u64,
        );
        push_snapshot_value(
            &mut values,
            format!("{}.execlist_hi", runtime.name).as_str(),
            runtime.execlist_status_hi as u64,
        );
        push_snapshot_value(
            &mut values,
            format!("{}.ipeir", runtime.name).as_str(),
            runtime.ipeir as u64,
        );
        push_snapshot_value(
            &mut values,
            format!("{}.ipehr", runtime.name).as_str(),
            runtime.ipehr as u64,
        );
        push_snapshot_value(
            &mut values,
            format!("{}.instdone", runtime.name).as_str(),
            runtime.instdone as u64,
        );
        push_snapshot_value(
            &mut values,
            format!("{}.instps", runtime.name).as_str(),
            runtime.instps as u64,
        );
    }

    Ok(SnapshotRecord {
        kind: SnapshotKind::Engine,
        key: String::from(engine_key(engine)),
        ts_ms: now_ms(),
        values,
    })
}

fn capture_guc_snapshot() -> Result<SnapshotRecord, String> {
    let warm = warm_state()?;
    let mut values = Vec::new();
    push_snapshot_value(&mut values, "guc_status", crate::intel::guc_status(warm) as u64);
    push_snapshot_value(&mut values, "mmio.guc_status", mmio_read32(warm, GUC_STATUS)? as u64);
    push_snapshot_value(
        &mut values,
        "mmio.guc_shim_control",
        mmio_read32(warm, GUC_SHIM_CONTROL)? as u64,
    );
    push_snapshot_value(
        &mut values,
        "mmio.guc_shim_control2",
        mmio_read32(warm, GUC_SHIM_CONTROL2)? as u64,
    );
    push_snapshot_value(
        &mut values,
        "mmio.dma_guc_wopcm_offset",
        mmio_read32(warm, DMA_GUC_WOPCM_OFFSET)? as u64,
    );
    push_snapshot_value(&mut values, "buffer.guc_fw_phys", warm.guc_fw_phys);
    push_snapshot_value(&mut values, "buffer.guc_fw_gpu", warm.guc_fw_gpu_addr);
    push_snapshot_value(&mut values, "buffer.guc_fw_len", warm.guc_fw_len as u64);
    push_snapshot_value(&mut values, "buffer.guc_ads_phys", warm.guc_ads_phys);
    push_snapshot_value(&mut values, "buffer.guc_ads_gpu", warm.guc_ads_gpu_addr);
    push_snapshot_value(&mut values, "buffer.guc_ads_len", warm.guc_ads_len as u64);
    Ok(SnapshotRecord {
        kind: SnapshotKind::Engine,
        key: String::from("guc"),
        ts_ms: now_ms(),
        values,
    })
}

fn capture_engine_snapshot(engine: EngineTarget) -> Result<SnapshotRecord, String> {
    match engine {
        EngineTarget::Render | EngineTarget::Blitter => capture_ring_snapshot(engine),
        EngineTarget::Media
        | EngineTarget::MediaVcs0
        | EngineTarget::MediaVcs1
        | EngineTarget::MediaVecs0
        | EngineTarget::MediaVecs1 => capture_media_snapshot(engine),
        EngineTarget::Guc => capture_guc_snapshot(),
    }
}

fn capture_mmio_block_snapshot(
    addr: usize,
    count: usize,
    width_bits: usize,
) -> Result<SnapshotRecord, String> {
    let warm = warm_state()?;
    let mut values = Vec::new();
    let stride = width_bits / 8;
    for idx in 0..count {
        let cur = addr.saturating_add(idx.saturating_mul(stride));
        let value = if width_bits == 32 {
            mmio_read32(warm, cur)? as u64
        } else {
            mmio_read64(warm, cur)?
        };
        push_snapshot_value(
            &mut values,
            format!("+0x{:X}", idx.saturating_mul(stride)).as_str(),
            value,
        );
    }
    Ok(SnapshotRecord {
        kind: SnapshotKind::MmioBlock,
        key: format!("mmio:{}:{}:{}", addr, count, width_bits),
        ts_ms: now_ms(),
        values,
    })
}

fn capture_buffer_snapshot(
    scope: &str,
    offset: usize,
    len: usize,
) -> Result<SnapshotRecord, String> {
    let (name, _, _, virt, bytes) = buffer_window(scope)?;
    if virt.is_null() {
        return Err(String::from("buffer virtual address unavailable"));
    }
    if offset.checked_add(len).is_none_or(|end| end > bytes) {
        return Err(String::from("buffer read out of range"));
    }
    let mut values = Vec::new();
    for idx in 0..len {
        let byte = unsafe { ptr::read_volatile(virt.add(offset + idx)) };
        push_snapshot_value(&mut values, format!("+0x{:X}", idx).as_str(), byte as u64);
    }
    Ok(SnapshotRecord {
        kind: SnapshotKind::Buffer,
        key: format!("buffer:{}:{}:{}", name, offset, len),
        ts_ms: now_ms(),
        values,
    })
}

fn snapshot_record_json(action: &str, snapshot: &SnapshotRecord) -> String {
    let mut out = success_prefix(action);
    out.push_str(
        format!(
            ",\"snapshot_kind\":\"{}\",\"key\":\"{}\",\"entry_count\":{}",
            match snapshot.kind {
                SnapshotKind::Engine => "engine",
                SnapshotKind::MmioBlock => "mmio_block",
                SnapshotKind::Buffer => "buffer",
            },
            escape_json(snapshot.key.as_str()),
            snapshot.values.len()
        )
        .as_str(),
    );
    out.push_str(",\"entries\":[");
    let mut first = true;
    for value in &snapshot.values {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(
            format!(
                "{{\"name\":\"{}\",\"value_hex\":\"{}\"}}",
                escape_json(value.name.as_str()),
                hex_u64(value.value)
            )
            .as_str(),
        );
    }
    out.push_str("]}");
    out
}

fn diff_snapshots(current: &SnapshotRecord, previous: Option<&SnapshotRecord>) -> String {
    let mut out = success_prefix("snapshot_diff");
    out.push_str(
        format!(
            ",\"snapshot_kind\":\"{}\",\"key\":\"{}\",\"current_ts_ms\":{}",
            match current.kind {
                SnapshotKind::Engine => "engine",
                SnapshotKind::MmioBlock => "mmio_block",
                SnapshotKind::Buffer => "buffer",
            },
            escape_json(current.key.as_str()),
            current.ts_ms
        )
        .as_str(),
    );
    if let Some(prev) = previous.filter(|prev| prev.kind == current.kind && prev.key == current.key)
    {
        out.push_str(format!(",\"previous_ts_ms\":{}", prev.ts_ms).as_str());
        out.push_str(",\"changed\":[");
        let mut first = true;
        let mut changed_count = 0usize;
        for (idx, cur) in current.values.iter().enumerate() {
            let changed = match prev.values.get(idx) {
                Some(old) => old.name != cur.name || old.value != cur.value,
                None => true,
            };
            if changed {
                if !first {
                    out.push(',');
                }
                first = false;
                changed_count += 1;
                let before = prev.values.get(idx).map(|entry| entry.value).unwrap_or(0);
                out.push_str(
                    format!(
                        "{{\"name\":\"{}\",\"before_hex\":\"{}\",\"after_hex\":\"{}\"}}",
                        escape_json(cur.name.as_str()),
                        hex_u64(before),
                        hex_u64(cur.value)
                    )
                    .as_str(),
                );
            }
        }
        out.push_str(format!("],\"changed_count\":{}", changed_count).as_str());
    } else {
        out.push_str(",\"previous_ts_ms\":null,\"changed\":[],\"changed_count\":0");
    }
    out.push('}');
    out
}

fn execute_session_state() -> String {
    let session = SESSION_STATE.lock();
    let mut out = success_prefix("session_state");
    if let Some(snapshot) = &session.last_snapshot {
        out.push_str(
            format!(
                ",\"last_snapshot\":{{\"kind\":\"{}\",\"key\":\"{}\",\"ts_ms\":{},\"entry_count\":{}}}",
                match snapshot.kind {
                    SnapshotKind::Engine => "engine",
                    SnapshotKind::MmioBlock => "mmio_block",
                    SnapshotKind::Buffer => "buffer",
                },
                escape_json(snapshot.key.as_str()),
                snapshot.ts_ms,
                snapshot.values.len()
            )
            .as_str(),
        );
    } else {
        out.push_str(",\"last_snapshot\":null");
    }
    out.push_str(",\"last_write\":");
    if let Some(json) = &session.last_write_json {
        out.push_str(json.as_str());
    } else {
        out.push_str("null");
    }
    out.push_str(",\"last_buffer_patch\":");
    if let Some(json) = &session.last_buffer_patch_json {
        out.push_str(json.as_str());
    } else {
        out.push_str("null");
    }
    out.push_str(",\"last_submit_result\":");
    if let Some(json) = &session.last_submit_json {
        out.push_str(json.as_str());
    } else {
        out.push_str("null");
    }
    out.push('}');
    out
}

fn execute_mmio_read(parsed: &ParsedArgs, width_bits: usize) -> Result<String, String> {
    let warm = warm_state()?;
    let addr = parse_usize(parsed.addr.as_deref(), "addr")?;
    let value = if width_bits == 32 {
        mmio_read32(warm, addr)? as u64
    } else {
        mmio_read64(warm, addr)?
    };
    Ok(format!(
        "{{\"ok\":true,\"action\":\"mmio_read{}\",\"ts_ms\":{},\"addr_hex\":\"{}\",\"width_bits\":{},\"value_hex\":\"{}\"}}",
        width_bits,
        now_ms(),
        hex_u64(addr as u64),
        width_bits,
        hex_u64(value)
    ))
}

fn execute_mmio_write(parsed: &ParsedArgs, width_bits: usize) -> Result<String, String> {
    guard_write(parsed)?;
    let warm = warm_state()?;
    let scope = parsed
        .scope
        .as_deref()
        .ok_or_else(|| String::from("missing scope"))?;
    let addr = parse_usize(parsed.addr.as_deref(), "addr")?;
    validate_write_scope(scope, addr, width_bits / 8)?;
    let value = parse_u64(parsed.value.as_deref(), "value")?;
    let before = if width_bits == 32 {
        mmio_read32(warm, addr)? as u64
    } else {
        mmio_read64(warm, addr)?
    };
    if width_bits == 32 {
        mmio_write32(warm, addr, value as u32)?;
    } else {
        mmio_write64(warm, addr, value)?;
    }
    let after = if width_bits == 32 {
        mmio_read32(warm, addr)? as u64
    } else {
        mmio_read64(warm, addr)?
    };
    let json = format!(
        "{{\"scope\":\"{}\",\"addr_hex\":\"{}\",\"width_bits\":{},\"before_hex\":\"{}\",\"requested_hex\":\"{}\",\"after_hex\":\"{}\"}}",
        escape_json(scope),
        hex_u64(addr as u64),
        width_bits,
        hex_u64(before),
        hex_u64(value),
        hex_u64(after)
    );
    SESSION_STATE.lock().last_write_json = Some(json.clone());
    Ok(format!(
        "{{\"ok\":true,\"action\":\"mmio_write{}\",\"ts_ms\":{},\"result\":{}}}",
        width_bits,
        now_ms(),
        json
    ))
}

fn execute_mmio_read_block(parsed: &ParsedArgs) -> Result<String, String> {
    let addr = parse_usize(parsed.addr.as_deref(), "addr")?;
    let count = parse_usize(parsed.count.as_deref(), "count")?;
    let width_bits = parse_u64(parsed.value.as_deref().or(Some("32")), "value")? as usize;
    if width_bits != 32 && width_bits != 64 {
        return Err(String::from("value must be 32 or 64 for mmio_read_block width"));
    }
    let snapshot = capture_mmio_block_snapshot(addr, count, width_bits)?;
    Ok(snapshot_record_json("mmio_read_block", &snapshot))
}

fn execute_wait_mmio_bits(parsed: &ParsedArgs) -> Result<String, String> {
    let warm = warm_state()?;
    let addr = parse_usize(parsed.addr.as_deref(), "addr")?;
    let mask = parse_u64(parsed.mask.as_deref(), "mask")? as u32;
    let expected = parse_u64(parsed.expected.as_deref(), "expected")? as u32;
    let timeout_iters =
        parse_usize(parsed.timeout_iters.as_deref().or(Some("100000")), "timeout_iters")?;
    let mut last = 0u32;
    let mut iter = 0usize;
    while iter < timeout_iters {
        last = mmio_read32(warm, addr)?;
        if (last & mask) == expected {
            return Ok(format!(
                "{{\"ok\":true,\"action\":\"wait_mmio_bits\",\"ts_ms\":{},\"addr_hex\":\"{}\",\"mask_hex\":\"{}\",\"expected_hex\":\"{}\",\"matched\":true,\"iterations\":{},\"last_hex\":\"{}\"}}",
                now_ms(),
                hex_u64(addr as u64),
                hex_u32(mask),
                hex_u32(expected),
                iter,
                hex_u32(last)
            ));
        }
        iter += 1;
    }
    Ok(format!(
        "{{\"ok\":true,\"action\":\"wait_mmio_bits\",\"ts_ms\":{},\"addr_hex\":\"{}\",\"mask_hex\":\"{}\",\"expected_hex\":\"{}\",\"matched\":false,\"iterations\":{},\"last_hex\":\"{}\"}}",
        now_ms(),
        hex_u64(addr as u64),
        hex_u32(mask),
        hex_u32(expected),
        timeout_iters,
        hex_u32(last)
    ))
}

fn execute_engine_dump(parsed: &ParsedArgs, action: &str) -> Result<String, String> {
    let engine = engine_from_str(parsed.engine.as_deref())?;
    let snapshot = capture_engine_snapshot(engine)?;
    Ok(snapshot_record_json(action, &snapshot))
}

fn execute_buffer_read(parsed: &ParsedArgs) -> Result<String, String> {
    let scope = parsed
        .scope
        .as_deref()
        .ok_or_else(|| String::from("missing scope"))?;
    let offset = parse_usize(parsed.offset.as_deref().or(Some("0")), "offset")?;
    let len = parse_usize(parsed.len.as_deref(), "len")?;
    let (name, gpu_addr, phys, virt, bytes) = buffer_window(scope)?;
    if virt.is_null() {
        return Err(String::from("buffer virtual address unavailable"));
    }
    if offset.checked_add(len).is_none_or(|end| end > bytes) {
        return Err(String::from("buffer read out of range"));
    }
    let slice = unsafe { core::slice::from_raw_parts(virt.add(offset), len) };
    Ok(format!(
        "{{\"ok\":true,\"action\":\"gpu_buffer_read\",\"ts_ms\":{},\"scope\":\"{}\",\"gpu_addr_hex\":\"{}\",\"phys_hex\":\"{}\",\"offset\":{},\"len\":{},\"buffer_bytes\":{},\"data_hex\":\"{}\"}}",
        now_ms(),
        escape_json(name.as_str()),
        hex_u64(gpu_addr),
        hex_u64(phys),
        offset,
        len,
        bytes,
        bytes_to_hex(slice)
    ))
}

fn execute_buffer_write(parsed: &ParsedArgs) -> Result<String, String> {
    guard_write(parsed)?;
    let scope = parsed
        .scope
        .as_deref()
        .ok_or_else(|| String::from("missing scope"))?;
    let offset = parse_usize(parsed.offset.as_deref().or(Some("0")), "offset")?;
    let data = parse_hex_bytes(parsed.data_hex.as_deref())?;
    let (name, gpu_addr, phys, virt, bytes) = buffer_window(scope)?;
    if virt.is_null() {
        return Err(String::from("buffer virtual address unavailable"));
    }
    if offset.checked_add(data.len()).is_none_or(|end| end > bytes) {
        return Err(String::from("buffer write out of range"));
    }
    let before = unsafe { core::slice::from_raw_parts(virt.add(offset), data.len()) }.to_vec();
    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), virt.add(offset), data.len());
    }
    crate::intel::dma_cache_flush_range(unsafe { virt.add(offset) }, data.len());
    let after = unsafe { core::slice::from_raw_parts(virt.add(offset), data.len()) };
    let patch_json = format!(
        "{{\"scope\":\"{}\",\"gpu_addr_hex\":\"{}\",\"phys_hex\":\"{}\",\"offset\":{},\"len\":{},\"before_hex\":\"{}\",\"after_hex\":\"{}\"}}",
        escape_json(name.as_str()),
        hex_u64(gpu_addr),
        hex_u64(phys),
        offset,
        data.len(),
        bytes_to_hex(before.as_slice()),
        bytes_to_hex(after)
    );
    SESSION_STATE.lock().last_buffer_patch_json = Some(patch_json.clone());
    Ok(format!(
        "{{\"ok\":true,\"action\":\"gpu_buffer_write\",\"ts_ms\":{},\"result\":{}}}",
        now_ms(),
        patch_json
    ))
}

fn execute_submit_test_batch(parsed: &ParsedArgs) -> Result<String, String> {
    let engine = engine_from_str(parsed.engine.as_deref())?;
    match engine {
        EngineTarget::Render => crate::intel::ggtt_blt_smoke_test_once(),
        EngineTarget::Blitter => crate::intel::ggtt_bcs_smoke_test_once(),
        EngineTarget::Media
        | EngineTarget::MediaVcs0
        | EngineTarget::MediaVcs1
        | EngineTarget::MediaVecs0
        | EngineTarget::MediaVecs1 => crate::intel::media_kickoff_once(),
        EngineTarget::Guc => {
            return Err(String::from("submit_test_batch is not defined for guc"));
        }
    }
    let followup = capture_engine_snapshot(engine)?;
    let json = snapshot_record_json("submit_test_batch", &followup);
    SESSION_STATE.lock().last_submit_json = Some(json.clone());
    Ok(json)
}

fn execute_snapshot_diff(parsed: &ParsedArgs) -> Result<String, String> {
    let snapshot = if parsed.engine.is_some() {
        capture_engine_snapshot(engine_from_str(parsed.engine.as_deref())?)?
    } else if parsed.addr.is_some() {
        let addr = parse_usize(parsed.addr.as_deref(), "addr")?;
        let count = parse_usize(parsed.count.as_deref(), "count")?;
        let width_bits = parse_u64(parsed.value.as_deref().or(Some("32")), "value")? as usize;
        capture_mmio_block_snapshot(addr, count, width_bits)?
    } else {
        let scope = parsed
            .scope
            .as_deref()
            .ok_or_else(|| String::from("missing scope"))?;
        let offset = parse_usize(parsed.offset.as_deref().or(Some("0")), "offset")?;
        let len = parse_usize(parsed.len.as_deref(), "len")?;
        capture_buffer_snapshot(scope, offset, len)?
    };
    let mut session = SESSION_STATE.lock();
    let json = diff_snapshots(&snapshot, session.last_snapshot.as_ref());
    session.last_snapshot = Some(snapshot);
    Ok(json)
}

fn execute(parsed: &ParsedArgs) -> Result<String, String> {
    match parsed.action.as_str() {
        "session_state" => Ok(execute_session_state()),
        "mmio_read32" => execute_mmio_read(parsed, 32),
        "mmio_read64" => execute_mmio_read(parsed, 64),
        "mmio_write32" => execute_mmio_write(parsed, 32),
        "mmio_write64" => execute_mmio_write(parsed, 64),
        "mmio_read_block" => execute_mmio_read_block(parsed),
        "wait_mmio_bits" => execute_wait_mmio_bits(parsed),
        "engine_dump_state" => execute_engine_dump(parsed, "engine_dump_state"),
        "gpu_buffer_read" => execute_buffer_read(parsed),
        "gpu_buffer_write" => execute_buffer_write(parsed),
        "submit_test_batch" => execute_submit_test_batch(parsed),
        "poll_engine_once" => execute_engine_dump(parsed, "poll_engine_once"),
        "snapshot_diff" => execute_snapshot_diff(parsed),
        "ring_decode" => execute_ring_decode(parsed),
        "context_image_decode" => execute_context_image_decode(parsed),
        "batch_disasm" | "batch_annotate" => execute_batch_disasm(parsed, parsed.action.as_str()),
        "fault_status_decode" => execute_fault_status_decode(parsed),
        "guc_state_view" => execute_guc_state_view(),
        "huc_state_view" => execute_huc_state_view(),
        "media_state_view" => execute_media_state_view(),
        _ => Err(String::from("unknown action")),
    }
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let response = match parse_tokens(args).and_then(|parsed| {
        execute(&parsed).map_err(|err| error_json(parsed.action.as_str(), "inteldev", err.as_str()))
    }) {
        Ok(json) => json,
        Err(json) => json,
    };
    print_shell_line(io, response.as_str());
    ParseOutcome::Handled
}
