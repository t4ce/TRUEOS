use super::*;
use super::descriptors::{encode_gpgpu_walker_candidate, GpgpuWalkerCandidate};

#[derive(Copy, Clone)]
pub(super) struct GpgpuEuProgram {
	pub(super) name: &'static str,
	pub(super) kind: trueos_eu::EuArtifactKind,
	pub(super) words: &'static [u32],
	pub(super) expects_store: bool,
	pub(super) expected_store_value: u32,
	pub(super) store_send_dword: Option<usize>,
	pub(super) visible_seed_dword: Option<usize>,
}

#[derive(Copy, Clone)]
pub(super) struct GpgpuProgramArtifactProof {
	pub(super) program_name: &'static str,
	pub(super) expects_store: bool,
	pub(super) program_uploaded: bool,
	pub(super) walker_encoded: bool,
	pub(super) result_changed_by_current_backend: bool,
	pub(super) program_gpu: u64,
	pub(super) program_bytes: usize,
	pub(super) program_sig: u64,
	pub(super) walker_gpu: u64,
	pub(super) walker_bytes: usize,
}

const GPGPU_KSP_NEGATIVE_CONTROL: bool = false;
const GPGPU_BAD_KERNEL_START_POINTER: u64 = 0x00F0_0000;

pub(super) fn prepare_gpgpu_program_artifact(
	warm: RenderWarmState,
	result_changed_by_current_backend: bool,
) -> GpgpuProgramArtifactProof {
	let program = selected_gpgpu_eu_program();
	let sip_handler = trueos_eu::gfx12::eot_artifact(GPGPU_SIP_HANDLER_VARIANT);
	let program_bytes = program.words.len() * core::mem::size_of::<u32>();
	let program_gpu = GPU_VA_DRAW_STATE_BASE + GPGPU_EU_KERNEL_OFFSET_BYTES as u64;
	let walker_gpu = GPU_VA_BATCH_BASE + GPGPU_WALKER_SCRATCH_OFFSET_BYTES as u64;

	let primary_uploaded = program_bytes != 0
		&& GPGPU_EU_KERNEL_OFFSET_BYTES
			.checked_add(program_bytes)
			.is_some_and(|end| end <= warm.draw_state_len)
		&& upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, program.words);
	let sip_bytes = sip_handler.words.len() * core::mem::size_of::<u32>();
	let sip_uploaded = !GPGPU_ENABLE_SIP_EXCEPTIONS
		|| (sip_bytes != 0
			&& GPGPU_SIP_HANDLER_OFFSET_BYTES
				.checked_add(sip_bytes)
				.is_some_and(|end| end <= warm.draw_state_len)
			&& upload_and_verify_gpu_program_at(
				warm,
				GPGPU_SIP_HANDLER_OFFSET_BYTES,
				sip_handler.words,
			));
	let program_uploaded = primary_uploaded && sip_uploaded;
	GPGPU_EU_KERNEL_UPLOADED.store(program_uploaded, Ordering::Release);

	let walker_bytes = core::mem::size_of::<GpgpuWalkerCandidate>();
	let walker_encoded = program_uploaded
		&& GPGPU_WALKER_SCRATCH_OFFSET_BYTES
			.checked_add(walker_bytes)
			.is_some_and(|end| end <= warm.batch_len)
		&& encode_gpgpu_walker_candidate(warm, program_gpu, program_bytes as u32);
	GPGPU_EU_WALKER_ENCODED.store(walker_encoded, Ordering::Release);

	GpgpuProgramArtifactProof {
		program_name: program.name,
		expects_store: program.expects_store,
		program_uploaded,
		walker_encoded,
		result_changed_by_current_backend,
		program_gpu,
		program_bytes,
		program_sig: shader_word_signature(program.words),
		walker_gpu,
		walker_bytes,
	}
}

pub(super) fn compute_gpgpu_kernel_start_pointer(program_gpu: u64, instruction_base: u64) -> u64 {
	if GPGPU_KSP_NEGATIVE_CONTROL {
		GPGPU_BAD_KERNEL_START_POINTER
	} else {
		program_gpu - instruction_base
	}
}

pub(super) fn gpgpu_kernel_start_pointer_negative_control_enabled() -> bool {
	GPGPU_KSP_NEGATIVE_CONTROL
}

pub(super) fn upload_and_verify_gpu_program_at(
	warm: RenderWarmState,
	offset_bytes: usize,
	program: &[u32],
) -> bool {
	unsafe {
		core::ptr::copy_nonoverlapping(
			program.as_ptr() as *const u8,
			warm.draw_state_virt.add(offset_bytes),
			core::mem::size_of_val(program),
		);
	}
	crate::intel::dma_flush(
		unsafe { warm.draw_state_virt.add(offset_bytes) },
		core::mem::size_of_val(program),
	);
	let uploaded = unsafe {
		core::slice::from_raw_parts(
			warm.draw_state_virt.add(offset_bytes) as *const u32,
			program.len(),
		)
	};
	uploaded == program
}
