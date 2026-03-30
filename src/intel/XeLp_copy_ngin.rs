// Xe-LP copy-engine helpers for minimal MI command-stream smoke tests.

#[inline]
const fn mi_instr(opcode: u32, flags: u32) -> u32 {
	(opcode << 23) | flags
}

#[inline]
const fn blt_instr(opcode: u32, flags: u32) -> u32 {
	(0x2 << 29) | (opcode << 22) | flags
}

#[inline]
const fn mi_num_dw(total_dwords: u32) -> u32 {
	total_dwords.saturating_sub(2) & 0xFF
}

#[inline]
const fn blt_num_dw(total_dwords: u32) -> u32 {
	total_dwords.saturating_sub(2) & 0x1FF
}

#[inline]
const fn lo32(addr: u64) -> u32 {
	addr as u32
}

#[inline]
const fn hi32(addr: u64) -> u32 {
	(addr >> 32) as u32
}

pub(crate) mod mi {
	use super::{mi_instr, mi_num_dw};

	pub const NOOP: u32 = mi_instr(0x00, 0);
	pub const USER_INTERRUPT: u32 = mi_instr(0x02, 0);
	pub const WAIT_FOR_EVENT: u32 = mi_instr(0x03, 0);
	pub const REPORT_HEAD: u32 = mi_instr(0x07, 0);
	pub const BATCH_BUFFER_END: u32 = mi_instr(0x0A, 0);
	pub const SUSPEND_FLUSH: u32 = mi_instr(0x0B, 0);
	pub const LOAD_SCAN_LINES_INCL: u32 = mi_instr(0x12, 0);
	pub const LOAD_SCAN_LINES_EXCL: u32 = mi_instr(0x13, 0);
	pub const SEMAPHORE_SIGNAL: u32 = mi_instr(0x1B, 0);
	pub const SEMAPHORE_WAIT: u32 = mi_instr(0x1C, mi_num_dw(4));
	pub const FORCE_WAKEUP: u32 = mi_instr(0x1D, mi_num_dw(2));
	pub const ATOMIC: u32 = mi_instr(0x2F, mi_num_dw(3));
	pub const FLUSH_DW: u32 = mi_instr(0x26, 0);
	pub const MATH_BASE: u32 = mi_instr(0x1A, 0);
	pub const COPY_MEM_MEM: u32 = mi_instr(0x2E, mi_num_dw(5));
	pub const LOAD_REGISTER_REG: u32 = mi_instr(0x2A, mi_num_dw(3));
	pub const LOAD_REGISTER_MEM: u32 = mi_instr(0x29, mi_num_dw(4));
	pub const STORE_REGISTER_MEM: u32 = mi_instr(0x24, mi_num_dw(4));

	pub const COPY_MEM_MEM_SRC_GGTT: u32 = 1 << 22;
	pub const COPY_MEM_MEM_DST_GGTT: u32 = 1 << 21;
	pub const FLUSH_DW_OP_STOREDW: u32 = 1 << 14;
	pub const FLUSH_DW_USE_GTT: u32 = 1 << 2;
	pub const FLUSH_DW_LEN_DW_MASK: u32 = 0x3F;
	pub const FLUSH_DW_LEN_FIVE_DWORDS: u32 = 3;

	pub const FORCE_WAKEUP_MEDIA_SLICE0: u32 = 1 << 0;
	pub const FORCE_WAKEUP_RENDER: u32 = 1 << 1;
	pub const FORCE_WAKEUP_MEDIA_SLICE1: u32 = 1 << 2;
	pub const FORCE_WAKEUP_MEDIA_SLICE2: u32 = 1 << 3;
	pub const FORCE_WAKEUP_MEDIA_SLICE3: u32 = 1 << 4;
	pub const FORCE_WAKEUP_HEVC_PWR_WELL: u32 = 1 << 8;
	pub const FORCE_WAKEUP_MFX_PWR_WELL: u32 = 1 << 9;

	#[inline]
	pub const fn math(num_alu_dwords: u32) -> u32 {
		MATH_BASE | mi_num_dw(num_alu_dwords.saturating_add(1))
	}

	#[inline]
	pub const fn flush_dw_len(total_dwords: u32) -> u32 {
		total_dwords.saturating_sub(2) & FLUSH_DW_LEN_DW_MASK
	}

	#[inline]
	pub const fn force_wakeup_masked(set_bits: u32) -> u32 {
		set_bits | (set_bits << 16)
	}
}

pub(crate) mod blt {
	use super::{blt_instr, blt_num_dw};

	pub const XY_COLOR_BLT: u32 = blt_instr(0x50, 0);
	pub const WRITE_A: u32 = 2 << 20;
	pub const WRITE_RGB: u32 = 1 << 20;
	pub const WRITE_RGBA: u32 = WRITE_RGB | WRITE_A;
	pub const DEPTH_32: u32 = 3 << 24;
	pub const ROP_COLOR_COPY: u32 = 0xF0 << 16;

	#[inline]
	pub const fn xy_color_blt_len(total_dwords: u32) -> u32 {
		blt_num_dw(total_dwords)
	}
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct CopySmokePlan {
	pub src_gpu_addr: u64,
	pub dst_gpu_addr: u64,
	pub result_gpu_addr: u64,
	pub start_value: u32,
	pub pre_copy_value: u32,
	pub post_copy_value: u32,
	pub done_value: u32,
	pub use_force_wakeup: bool,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ColorFillSmokePlan {
	pub dst_gpu_addr: u64,
	pub pitch_bytes: u32,
	pub rect_w: u32,
	pub rect_h: u32,
	pub color: u32,
	pub result_gpu_addr: u64,
	pub start_value: u32,
	pub pre_color_value: u32,
	pub post_color_value: u32,
	pub done_value: u32,
	pub use_force_wakeup: bool,
}

pub(crate) const COPY_SMOKE_RESULT_SLOT_BYTES: u64 = 8;
pub(crate) const COPY_SMOKE_START_SLOT: u64 = 0;
pub(crate) const COPY_SMOKE_PRE_COPY_SLOT: u64 = COPY_SMOKE_START_SLOT + COPY_SMOKE_RESULT_SLOT_BYTES;
pub(crate) const COPY_SMOKE_POST_COPY_SLOT: u64 = COPY_SMOKE_PRE_COPY_SLOT + COPY_SMOKE_RESULT_SLOT_BYTES;
pub(crate) const COPY_SMOKE_DONE_SLOT: u64 = COPY_SMOKE_POST_COPY_SLOT + COPY_SMOKE_RESULT_SLOT_BYTES;

#[derive(Copy, Clone, Debug)]
pub(crate) enum CopySmokeBuildError {
	BatchTooSmall { need_dwords: usize, got_dwords: usize },
	UnalignedAddress { field: &'static str, addr: u64 },
	InvalidPitch { pitch_bytes: u32 },
	InvalidRect {
		field: &'static str,
		value: u32,
	},
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct CopySmokeBuildResult {
	pub used_dwords: usize,
}

#[inline]
fn addr_aligned_4(addr: u64) -> bool {
	(addr & 0x3) == 0
}

#[inline]
pub(crate) const fn copy_smoke_result_addr(base: u64, slot_off: u64) -> u64 {
	base + slot_off
}

pub(crate) fn build_mi_copy_smoke_batch(
	batch_dwords: &mut [u32],
	plan: CopySmokePlan,
) -> Result<CopySmokeBuildResult, CopySmokeBuildError> {
	if !addr_aligned_4(plan.src_gpu_addr) {
		return Err(CopySmokeBuildError::UnalignedAddress {
			field: "src_gpu_addr",
			addr: plan.src_gpu_addr,
		});
	}
	if !addr_aligned_4(plan.dst_gpu_addr) {
		return Err(CopySmokeBuildError::UnalignedAddress {
			field: "dst_gpu_addr",
			addr: plan.dst_gpu_addr,
		});
	}
	if !addr_aligned_4(plan.result_gpu_addr) {
		return Err(CopySmokeBuildError::UnalignedAddress {
			field: "result_gpu_addr",
			addr: plan.result_gpu_addr,
		});
	}
	if !addr_aligned_4(copy_smoke_result_addr(
		plan.result_gpu_addr,
		COPY_SMOKE_PRE_COPY_SLOT,
	)) {
		return Err(CopySmokeBuildError::UnalignedAddress {
			field: "result_gpu_addr+pre_copy",
			addr: copy_smoke_result_addr(plan.result_gpu_addr, COPY_SMOKE_PRE_COPY_SLOT),
		});
	}
	if !addr_aligned_4(copy_smoke_result_addr(
		plan.result_gpu_addr,
		COPY_SMOKE_POST_COPY_SLOT,
	)) {
		return Err(CopySmokeBuildError::UnalignedAddress {
			field: "result_gpu_addr+post_copy",
			addr: copy_smoke_result_addr(plan.result_gpu_addr, COPY_SMOKE_POST_COPY_SLOT),
		});
	}
	if !addr_aligned_4(copy_smoke_result_addr(
		plan.result_gpu_addr,
		COPY_SMOKE_DONE_SLOT,
	)) {
		return Err(CopySmokeBuildError::UnalignedAddress {
			field: "result_gpu_addr+done",
			addr: copy_smoke_result_addr(plan.result_gpu_addr, COPY_SMOKE_DONE_SLOT),
		});
	}

	let required = if plan.use_force_wakeup { 30 } else { 28 };
	if batch_dwords.len() < required {
		return Err(CopySmokeBuildError::BatchTooSmall {
			need_dwords: required,
			got_dwords: batch_dwords.len(),
		});
	}

	batch_dwords.fill(0);
	let mut i = 0usize;

	let emit_flush_dw_store = |batch_dwords: &mut [u32], i: &mut usize, dst: u64, value: u32| {
		batch_dwords[*i] =
			mi::FLUSH_DW | mi::FLUSH_DW_LEN_FIVE_DWORDS | mi::FLUSH_DW_OP_STOREDW;
		batch_dwords[*i + 1] = lo32(dst) | mi::FLUSH_DW_USE_GTT;
		batch_dwords[*i + 2] = hi32(dst);
		batch_dwords[*i + 3] = value;
		batch_dwords[*i + 4] = 0;
		*i += 5;
	};

	if plan.use_force_wakeup {
		batch_dwords[i] = mi::FORCE_WAKEUP;
		batch_dwords[i + 1] = mi::force_wakeup_masked(mi::FORCE_WAKEUP_RENDER);
		i += 2;
	}

	batch_dwords[i] = mi::NOOP;
	i += 1;

	emit_flush_dw_store(batch_dwords, &mut i, plan.result_gpu_addr, plan.start_value);
	emit_flush_dw_store(
		batch_dwords,
		&mut i,
		copy_smoke_result_addr(plan.result_gpu_addr, COPY_SMOKE_PRE_COPY_SLOT),
		plan.pre_copy_value,
	);

	batch_dwords[i] = mi::COPY_MEM_MEM | mi::COPY_MEM_MEM_SRC_GGTT | mi::COPY_MEM_MEM_DST_GGTT;
	batch_dwords[i + 1] = lo32(plan.dst_gpu_addr);
	batch_dwords[i + 2] = hi32(plan.dst_gpu_addr);
	batch_dwords[i + 3] = lo32(plan.src_gpu_addr);
	batch_dwords[i + 4] = hi32(plan.src_gpu_addr);
	i += 5;

	emit_flush_dw_store(
		batch_dwords,
		&mut i,
		copy_smoke_result_addr(plan.result_gpu_addr, COPY_SMOKE_POST_COPY_SLOT),
		plan.post_copy_value,
	);
	emit_flush_dw_store(
		batch_dwords,
		&mut i,
		copy_smoke_result_addr(plan.result_gpu_addr, COPY_SMOKE_DONE_SLOT),
		plan.done_value,
	);

	batch_dwords[i] = mi::BATCH_BUFFER_END;
	batch_dwords[i + 1] = mi::NOOP;
	i += 2;

	Ok(CopySmokeBuildResult { used_dwords: i })
}

pub(crate) fn build_copy_smoke_batch_bytes(
	batch_dwords: &mut [u32],
	plan: CopySmokePlan,
) -> Result<usize, CopySmokeBuildError> {
	let build = build_mi_copy_smoke_batch(batch_dwords, plan)?;
	Ok(build.used_dwords.saturating_mul(core::mem::size_of::<u32>()))
}

pub(crate) fn build_color_fill_smoke_batch_bytes(
	batch_dwords: &mut [u32],
	plan: ColorFillSmokePlan,
) -> Result<usize, CopySmokeBuildError> {
	if !addr_aligned_4(plan.dst_gpu_addr) {
		return Err(CopySmokeBuildError::UnalignedAddress {
			field: "dst_gpu_addr",
			addr: plan.dst_gpu_addr,
		});
	}
	if !addr_aligned_4(plan.result_gpu_addr) {
		return Err(CopySmokeBuildError::UnalignedAddress {
			field: "result_gpu_addr",
			addr: plan.result_gpu_addr,
		});
	}
	if !addr_aligned_4(copy_smoke_result_addr(
		plan.result_gpu_addr,
		COPY_SMOKE_PRE_COPY_SLOT,
	)) {
		return Err(CopySmokeBuildError::UnalignedAddress {
			field: "result_gpu_addr+pre_color",
			addr: copy_smoke_result_addr(plan.result_gpu_addr, COPY_SMOKE_PRE_COPY_SLOT),
		});
	}
	if !addr_aligned_4(copy_smoke_result_addr(
		plan.result_gpu_addr,
		COPY_SMOKE_POST_COPY_SLOT,
	)) {
		return Err(CopySmokeBuildError::UnalignedAddress {
			field: "result_gpu_addr+post_color",
			addr: copy_smoke_result_addr(plan.result_gpu_addr, COPY_SMOKE_POST_COPY_SLOT),
		});
	}
	if !addr_aligned_4(copy_smoke_result_addr(
		plan.result_gpu_addr,
		COPY_SMOKE_DONE_SLOT,
	)) {
		return Err(CopySmokeBuildError::UnalignedAddress {
			field: "result_gpu_addr+done",
			addr: copy_smoke_result_addr(plan.result_gpu_addr, COPY_SMOKE_DONE_SLOT),
		});
	}
	if plan.pitch_bytes == 0 || plan.pitch_bytes > 0xFFFF {
		return Err(CopySmokeBuildError::InvalidPitch {
			pitch_bytes: plan.pitch_bytes,
		});
	}
	if plan.rect_w == 0 || plan.rect_w > 0xFFFF {
		return Err(CopySmokeBuildError::InvalidRect {
			field: "rect_w",
			value: plan.rect_w,
		});
	}
	if plan.rect_h == 0 || plan.rect_h > 0xFFFF {
		return Err(CopySmokeBuildError::InvalidRect {
			field: "rect_h",
			value: plan.rect_h,
		});
		}

	let required = if plan.use_force_wakeup { 28 } else { 26 };
	if batch_dwords.len() < required {
		return Err(CopySmokeBuildError::BatchTooSmall {
			need_dwords: required,
			got_dwords: batch_dwords.len(),
		});
	}

	batch_dwords.fill(0);
	let mut i = 0usize;

	let emit_flush_dw_store = |batch_dwords: &mut [u32], i: &mut usize, dst: u64, value: u32| {
		batch_dwords[*i] =
			mi::FLUSH_DW | mi::FLUSH_DW_LEN_FIVE_DWORDS | mi::FLUSH_DW_OP_STOREDW;
		batch_dwords[*i + 1] = lo32(dst) | mi::FLUSH_DW_USE_GTT;
		batch_dwords[*i + 2] = hi32(dst);
		batch_dwords[*i + 3] = value;
		batch_dwords[*i + 4] = 0;
		*i += 5;
	};

	if plan.use_force_wakeup {
		batch_dwords[i] = mi::FORCE_WAKEUP;
		batch_dwords[i + 1] = mi::force_wakeup_masked(mi::FORCE_WAKEUP_RENDER);
		i += 2;
	}

	batch_dwords[i] = mi::NOOP;
	i += 1;

	emit_flush_dw_store(batch_dwords, &mut i, plan.result_gpu_addr, plan.start_value);
	emit_flush_dw_store(
		batch_dwords,
		&mut i,
		copy_smoke_result_addr(plan.result_gpu_addr, COPY_SMOKE_PRE_COPY_SLOT),
		plan.pre_color_value,
	);

	batch_dwords[i] = blt::XY_COLOR_BLT | blt::WRITE_RGBA | blt::xy_color_blt_len(7);
	batch_dwords[i + 1] = blt::DEPTH_32 | blt::ROP_COLOR_COPY | plan.pitch_bytes;
	batch_dwords[i + 2] = 0;
	batch_dwords[i + 3] = (plan.rect_h << 16) | plan.rect_w;
	batch_dwords[i + 4] = lo32(plan.dst_gpu_addr);
	batch_dwords[i + 5] = hi32(plan.dst_gpu_addr);
	batch_dwords[i + 6] = plan.color;
	batch_dwords[i + 7] = mi::NOOP;
	i += 8;

	emit_flush_dw_store(
		batch_dwords,
		&mut i,
		copy_smoke_result_addr(plan.result_gpu_addr, COPY_SMOKE_POST_COPY_SLOT),
		plan.post_color_value,
	);

	batch_dwords[i] = mi::BATCH_BUFFER_END;
	batch_dwords[i + 1] = mi::NOOP;
	i += 2;

	Ok(i.saturating_mul(core::mem::size_of::<u32>()))
}


/*

Context Management 
Copy Engine Register State Context  
Register State Context  
EXECLIST CONTEXT 
EXECLIST CONTEXT(PPGTT Base) 
ENGINE CONTEXT 
 
Description 
MMIO 
Offset/Command Unit 
Dw 
Count 
DW 
Offset 
CSFE Execlist Context  BCSFE 192 0 
MI_BATCH_BUFFER_END   CSEND  1 00C0 
NOOP  CSEND  127 00C1 
   DW 320 
   K Bytes 1.28 
Copy Engine Power Context 
This section lists the power context image of Copy Engine across generations. 
Blitter Engine Power Context  
The table below captures the data from BCS power context save/restored by PM. Address offset in the 
below table is relative to the starting location of BCS in the overall power context image managed by PM. 
    
 
Doc Ref # IHD-OS-TGL-Vol 10-12.21   3 
BCS Power Context Image 
Description   
# of 
DW 
Address 
Offset(PWR) CSFE/CSBE 
CSFE Power Context with 
Display 
  192 0 CSFE 
NOOP  BCS  1 00D0 CSBE 
Load_Register_Immediate 
header 
0x1100_1003  BCS  1 00D1 CSBE 
GAB MODE REGISTER 0x220a0 BCS  2 00D4 CSBE 
NOOP  BCS  8 00D6 CSBE 
NOOP  BCS  1 00DE CSBE 
MI_BATCH_BUFFER_END  BCS  1 00DF CSBE 
Blitter Command Formats 
2D Commands  
The 2D commands include various flavors of BLT operations, along with commands to set up BLT engine 
state without actually performing a BLT. Most commands are of fixed length, though there are a few 
commands that include a variable amount of "inline" data at the end of the command (in case of legacy 
Blitter command). 
All the following commands are defined in Blitter Instructions. 
2D Command Map 
Opcode 
(28:22) Command 
00h Reserved 
01h XY_SETUP_BLT 
02h Reserved 
03h XY_SETUP_CLIP_BLT 
04h-10h  Reserved 
11h XY_SETUP_MONO_PATTERN_SL_BLT 
12h-23h  Reserved 
24h XY_PIXEL_BLT 
25h XY_SCANLINES_BLT 
26h XY_TEXT_BLT 
27h-30h  ReservedReserved 
31h XY_TEXT_IMMEDIATE_BLT 
32h-3Fh Reserved 
40h COLOR_BLT 
 
    
4   Doc Ref # IHD-OS-TGL-Vol 10-12.21 
Opcode 
(28:22) Command 
41h XY_BLOCK_COPY_BLT 
42h XY_FAST_COPY_BLT 
43h SRC_COPY_BLT 
44h XY_FAST_COLOR_BLT 
45h-47h  Reserved 
49h-4Fh Reserved 
50h XY_COLOR_BLT 
51h XY_PAT_BLT 
52h XY_MONO_PAT_BLT 
53h XY_SRC_COPY_BLT 
54h XY_MONO_SRC_COPY_BLT 
55h XY_FULL_BLT 
56h XY_FULL_MONO_SRC_BLT 
57h XY_FULL_MONO_PATTERN_BLT 
58h XY_FULL_MONO_PATTERN_MONO_SRC_BLT 
59h XY_MONO_PAT_FIXED_BLT 
71h XY_MONO_SRC_COPY_IMMEDIATE_BLT 
72h XY_PAT_BLT_IMMEDIATE 
73h XY_SRC_COPY_CHROMA_BLT 
74h XY_FULL_IMMEDIATE_PATTERN_BLT 
75h XY_FULL_MONO_SRC_IMMEDIATE_PATTERN_BL 
76h XY_PAT_CHROMA_BLT 
77h XY_PAT_CHROMA_BLT_IMMEDIATE 
78h-7Fh Reserved 
Blitter Command Header Format 
Type Bits 
 31:29 28:24 23 22 21:0 
Memory 
 Interface 
 (MI) 
000 Opcode 
 00h - NOP 
 0Xh - Single DWord Commands 
 1Xh - Two+ DWord Commands 
 2Xh - Store Data Commands 
 3Xh - Ring/Batch Buffer Cmds 
 Identification No./DWord Count 
 Command Dependent Data 
 5:0 - DWord Count 
 5:0 - DWord Count 
 5:0 - DWord Count 
Reserved 001     
Reserved 011     
 
    
 
Doc Ref # IHD-OS-TGL-Vol 10-12.21   5 
Type Bits 
 31:29 28:22 21:9 8:0 
Blitter (2D)  010 Command Opcode  Command Dependent Data  Dword Count 
Logical Context Support 
The following are the Logical Context Support Registers: 
Register 
BB_ADDR - Batch Buffer Head Pointer Register 
BB_ADDR_UDW - Batch Buffer Upper Head Pointer Register 
SBB_ADDR - Second Level Batch Buffer Head Pointer Register 
SBB_ADDR_UDW - Second Level Batch Buffer Upper Head Pointer Register 
SYNC_FLIP_STATUS - Wait For Event and Display Flip Flags Register 
SYNC_FLIP_STATUS_1 - Wait For Event and Display Flip Flags Register 1 
SYNC_FLIP_STATUS_2 - Wait For Event and Display Flip Flags Register 2 
CXT_EL_OFFSET - Exec-List Context Offset 
BB_START_ADDR_UDW - Batch Buffer Start Upper Head Pointer Register 
BB_ADDR_DIFF - Batch Address Difference Register 
WAIT_FOR_RC6_EXIT - Control Register for Power Management 
SBB_STATE - Second Level Batch Buffer State Register 
BB_OFFSET - Batch Offset Register 
RING_BUFFER_HEAD_PREEMPT_REG - RING_BUFFER_HEAD_PREEMPT_REG 
BB_PREEMPT_ADDR - Batch Buffer Head Pointer Preemption Register 
BB_PREEMPT_ADDR_UDW - Batch Buffer Upper Head Pointer Preemption Register 
SBB_PREEMPT_ADDR - Second Level Batch Buffer Head Pointer Preemption Register 
SBB_PREEMPT_ADDR_UDW - Second Level Batch Buffer Upper Head Pointer Preemption Register 
MI_PREDICATE_RESULT_1 - Predicate Rendering Data Result 1 
MI_PREDICATE_RESULT_2 - Predicate Rendering Data Result 2 
INDIRECT_CTX - Indirect Context Pointer 
INDIRECT_CTX_OFFSET - Indirect Context Offset Pointer 
BB_PER_CTX_PTR - Batch Buffer Per Context Pointer 
Mode Registers 
The following table describes the Mode Registers. 
Registers 
BCS_CXT_SIZE - BCS Context Sizes 
MI_MODE - Mode Register for Software Interface 
 
    
6   Doc Ref # IHD-OS-TGL-Vol 10-12.21 
Mode Registers (continued) 
Reisters 
INSTPM - Instruction Parser Mode Register 
EXCC - Execute Condition Code Register 
IDLEDLY - Idle Switch Delay 
SEMA_WAIT_POLL - Semaphore Polling Interval on Wait 
RESET_CTRL - Reset Control Register 
PREEMPTION_HINT - Preemption Hint 
PREEMPTION_HINT_UDW - Preemption Hint Upper DWord 
 
Register 
HWS_PGA - Hardware Status Page Address Register 
MI Commands for Blitter Engine 
This chapter describes the formats of the "Memory Interface" commands, including brief descriptions of 
their use. The functions performed by these commands are discussed fully in the Memory Interface 
Functions Device Programming Environment chapter. 
This chapter describes MI Commands for the blitter graphics processing engine. The term "for Blitter 
Engine" in the title has been added to differentiate this chapter from a similar one describing the MI 
commands for the Media Decode Engine and the Rendering Engine. 
The commands detailed in this chapter are used across products. However, slight changes may be 
present in some commands (i.e., for features added or removed), or some commands may be removed 
entirely. Refer to the Preface chapter for product specific summary. 
Commands 
MI_NOOP 
MI_ARB_ON_OFF 
MI_BATCH_BUFFER_START 
The following table lists the non-privileged registers that can be written to from a non-secure batch 
buffer executed from Render Command Streamer. 
User Mode Non-Privileged Registers 
MMIO Name MMIO Offset Size in DWords 
BCS_GPR 22600h 32 
BCS_SWCTRL 22200h 32 
 
Commands 
MI_BATCH_BUFFER_END 
MI_CONDITIONAL_BATCH_BUFFER_END 
MI_DISPLAY_FLIP 
    
 
Doc Ref # IHD-OS-TGL-Vol 10-12.21   7 
Commands 
MI_LOAD_SCAN_LINES_EXCL 
MI_LOAD_SCAN_LINES_INCL 
MI_FLUSH_DW 
MI_MATH 
MI_REPORT_HEAD 
MI_STORE_DATA_IMM 
MI_ATOMIC 
MI_COPY_MEM_MEM 
MI_LOAD_REGISTER_REG 
MI_LOAD_REGISTER_MEM 
MI_STORE_REGISTER_MEM 
MI_SUSPEND_FLUSH 
MI_USER_INTERRUPT 
MI_WAIT_FOR_EVENT 
MI_SEMAPHORE_SIGNAL 
MI_SEMAPHORE_WAIT 
MI_FORCE_WAKEUP 
Software Control Bit Definitions 
Registers in the range 22XX are not protected from the load register immediate instruction if the 
command is executed in the non-secure batch buffer. 
BCS_SWCTRL - BCS SW Control 
Registers for Blitter Engine Command Streamer 
These are the Registers for the Blitter Engine Command Streamer. 
Also see the Observability volume for related information 
GAB_MODE - Mode Register for GAB

*/