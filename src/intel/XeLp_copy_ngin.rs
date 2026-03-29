// Xe-LP copy-engine helpers for minimal MI command-stream smoke tests.

#[inline]
const fn mi_instr(opcode: u32, flags: u32) -> u32 {
	(opcode << 23) | flags
}

#[inline]
const fn mi_num_dw(total_dwords: u32) -> u32 {
	total_dwords.saturating_sub(2) & 0xFF
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
	pub const STORE_DATA_IMM: u32 = mi_instr(0x20, 0);
	pub const ATOMIC: u32 = mi_instr(0x2F, mi_num_dw(3));
	pub const FLUSH_DW: u32 = mi_instr(0x26, 0);
	pub const MATH_BASE: u32 = mi_instr(0x1A, 0);
	pub const COPY_MEM_MEM: u32 = mi_instr(0x2E, mi_num_dw(5));
	pub const LOAD_REGISTER_REG: u32 = mi_instr(0x2A, mi_num_dw(3));
	pub const LOAD_REGISTER_MEM: u32 = mi_instr(0x29, mi_num_dw(4));
	pub const STORE_REGISTER_MEM: u32 = mi_instr(0x24, mi_num_dw(4));

	pub const STORE_DATA_IMM_GGTT: u32 = 1 << 22;
	pub const COPY_MEM_MEM_SRC_GGTT: u32 = 1 << 22;
	pub const COPY_MEM_MEM_DST_GGTT: u32 = 1 << 21;
	pub const FLUSH_DW_OP_STOREDW: u32 = 1 << 14;
	pub const FLUSH_DW_USE_GTT: u32 = 1 << 2;
	pub const FLUSH_DW_LEN_DW_MASK: u32 = 0x3F;

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
	pub const fn store_data_imm_num_dw(num_dwords: u32) -> u32 {
		(num_dwords.saturating_add(1) & 0x3FF) | STORE_DATA_IMM_GGTT
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

#[derive(Copy, Clone, Debug)]
pub(crate) struct CopySmokePlan {
	pub src_gpu_addr: u64,
	pub dst_gpu_addr: u64,
	pub result_gpu_addr: u64,
	pub start_value: u32,
	pub done_value: u32,
	pub use_force_wakeup: bool,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum CopySmokeBuildError {
	BatchTooSmall { need_dwords: usize, got_dwords: usize },
	UnalignedAddress { field: &'static str, addr: u64 },
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct CopySmokeBuildResult {
	pub used_dwords: usize,
}

#[inline]
fn addr_aligned_4(addr: u64) -> bool {
	(addr & 0x3) == 0
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

	let required = if plan.use_force_wakeup { 17 } else { 15 };
	if batch_dwords.len() < required {
		return Err(CopySmokeBuildError::BatchTooSmall {
			need_dwords: required,
			got_dwords: batch_dwords.len(),
		});
	}

	batch_dwords.fill(0);
	let mut i = 0usize;

	if plan.use_force_wakeup {
		batch_dwords[i] = mi::FORCE_WAKEUP;
		batch_dwords[i + 1] = mi::force_wakeup_masked(mi::FORCE_WAKEUP_RENDER);
		i += 2;
	}

	batch_dwords[i] = mi::STORE_DATA_IMM | mi::store_data_imm_num_dw(1);
	batch_dwords[i + 1] = lo32(plan.result_gpu_addr);
	batch_dwords[i + 2] = hi32(plan.result_gpu_addr);
	batch_dwords[i + 3] = plan.start_value;
	i += 4;

	batch_dwords[i] = mi::COPY_MEM_MEM | mi::COPY_MEM_MEM_SRC_GGTT | mi::COPY_MEM_MEM_DST_GGTT;
	batch_dwords[i + 1] = lo32(plan.dst_gpu_addr);
	batch_dwords[i + 2] = hi32(plan.dst_gpu_addr);
	batch_dwords[i + 3] = lo32(plan.src_gpu_addr);
	batch_dwords[i + 4] = hi32(plan.src_gpu_addr);
	i += 5;

	batch_dwords[i] = mi::FLUSH_DW | mi::FLUSH_DW_OP_STOREDW | mi::flush_dw_len(4);
	batch_dwords[i + 1] = lo32(plan.result_gpu_addr) | mi::FLUSH_DW_USE_GTT;
	batch_dwords[i + 2] = hi32(plan.result_gpu_addr);
	batch_dwords[i + 3] = plan.done_value;
	i += 4;

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
