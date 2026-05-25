use super::descriptors::{
    GPGPU_INTERFACE_DESCRIPTOR_DWORDS, build_gpgpu_interface_descriptor_words,
};
use super::kernel_startpointer::{
    compute_gpgpu_kernel_start_pointer, gpgpu_kernel_start_pointer_negative_control_enabled,
};
use super::payload::{
    gpgpu_curbe_read_length_8dw, gpgpu_curbe_total_bytes, gpgpu_vfe_curbe_allocation_32b,
    write_gpgpu_dummy_curbe,
};
use super::*;

pub(super) fn encode_gfx12_gpgpu_walker_probe_batch(
    warm: RenderWarmState,
    batch_dwords: &mut [u32],
    store_surface: GpgpuStoreSurfaceState,
    program: GpgpuEuProgram,
    walker_group_x_dim: u32,
) -> Result<usize, &'static str> {
    const MEDIA_VFE_STATE_CMD: u32 = (3 << 29) | (2 << 27) | 7;
    const MEDIA_CURBE_LOAD_CMD: u32 = (3 << 29) | (2 << 27) | (1 << 16) | 2;
    const MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD: u32 = (3 << 29) | (2 << 27) | (2 << 16) | 2;
    const GPGPU_WALKER_CMD: u32 = (3 << 29) | (2 << 27) | (1 << 24) | (5 << 16) | 13;
    const MEDIA_STATE_FLUSH_CMD: u32 = (3 << 29) | (2 << 27) | (4 << 16);
    const PIPELINE_SELECT_BASE: u32 = (3 << 29) | (1 << 27) | (1 << 24) | (4 << 16);
    const PIPELINE_SELECT_GFX12_MASK: u32 = 0x13 << 8;
    const PIPELINE_SELECT_MEDIA_SAMPLER_DOP_CG_ENABLE: u32 = 1 << 4;
    const PIPELINE_SELECT_3D: u32 = PIPELINE_SELECT_BASE
        | PIPELINE_SELECT_GFX12_MASK
        | PIPELINE_SELECT_MEDIA_SAMPLER_DOP_CG_ENABLE;
    const PIPELINE_SELECT_GPGPU: u32 = PIPELINE_SELECT_3D | 2;
    const COMPUTE_SBA_SPAN_BYTES: usize = 0xFFFF_F000;
    const CS_GPR_STAMP_HI: u32 = 0x0000_0001;
    const CS_GPR0_STAMP_LO: u32 = 0xC5A0_2600;
    const CS_GPR1_STAMP_LO: u32 = 0xC5A0_2608;
    const IDD_STATE_OFFSET_BYTES: usize = GPGPU_WALKER_SCRATCH_OFFSET_BYTES;
    const CURBE_STATE_OFFSET_BYTES: usize = GPGPU_WALKER_SCRATCH_OFFSET_BYTES + 0x100;
    const GPGPU_WALKER_GROUP_Y_DIM: u32 = 1;
    const GPGPU_WALKER_GROUP_Z_DIM: u32 = 1;
    const GPGPU_VFE_MAX_THREADS: u32 = 223;
    const GPGPU_VFE_URB_ENTRIES: u32 = 2;
    const GPGPU_VFE_FUSED_EU_DISPATCH_LEGACY_MODE: u32 = 0;
    const GPGPU_VFE_URB_ENTRY_ALLOCATION_32B: u32 = 2;
    const GPGPU_RELATIVE_STATE_BASES: bool = true;
    const GPGPU_TEMPORARY_3D_FOR_SBA: bool = true;
    const GPGPU_DYNAMIC_STATE_BASE: u64 = if GPGPU_RELATIVE_STATE_BASES {
        GPU_VA_DRAW_STATE_BASE
    } else {
        0
    };
    const IDD_DYNAMIC_OFFSET_BYTES: usize = if GPGPU_RELATIVE_STATE_BASES {
        IDD_STATE_OFFSET_BYTES
    } else {
        GPU_VA_DRAW_STATE_BASE as usize + IDD_STATE_OFFSET_BYTES
    };
    const CURBE_DYNAMIC_OFFSET_BYTES: usize = if GPGPU_RELATIVE_STATE_BASES {
        CURBE_STATE_OFFSET_BYTES
    } else {
        GPU_VA_DRAW_STATE_BASE as usize + CURBE_STATE_OFFSET_BYTES
    };
    const GPGPU_KERNEL_GPU: u64 = GPU_VA_DRAW_STATE_BASE + GPGPU_EU_KERNEL_OFFSET_BYTES as u64;
    const GPGPU_INSTRUCTION_BASE: u64 = GPGPU_KERNEL_GPU;
    const GPGPU_MIDL_NEGATIVE_CONTROL: bool = false;
    const GPGPU_WALKER_SIMD8_RIGHT_MASK: u32 = 0x0000_00FF;
    const GPGPU_WALKER_BOTTOM_MASK: u32 = 0xFFFF_FFFF;
    const STATE_SIP_CMD: u32 = 0x6102_0001;
    const GPGPU_SIP_GPU: u64 = GPU_VA_DRAW_STATE_BASE + GPGPU_SIP_HANDLER_OFFSET_BYTES as u64;

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("compute-walker-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_pipe_control_full(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        header_flags: u32,
        dw1_flags: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD | header_flags)?;
        push(batch_dwords, cursor, dw1_flags)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_pipe_control(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags: u32,
    ) -> Result<(), &'static str> {
        push_pipe_control_full(batch_dwords, cursor, 0, flags)
    }

    fn push_store_marker(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        slot: usize,
        value: u32,
    ) -> Result<(), &'static str> {
        let dst = GPU_VA_RESULT_BASE + (slot as u64) * core::mem::size_of::<u32>() as u64;
        push(batch_dwords, cursor, MI_STORE_DATA_IMM_GGTT_DW1)?;
        push(batch_dwords, cursor, dst as u32)?;
        push(batch_dwords, cursor, (dst >> 32) as u32)?;
        push(batch_dwords, cursor, value)
    }

    fn push_cs_gpr_stamp(batch_dwords: &mut [u32], cursor: &mut usize) -> Result<(), &'static str> {
        push(batch_dwords, cursor, mi_lri_cmd(4, MI_LRI_FORCE_POSTED))?;
        push(batch_dwords, cursor, RCS_CS_GPR_REL_BASE as u32)?;
        push(batch_dwords, cursor, CS_GPR0_STAMP_LO)?;
        push(batch_dwords, cursor, (RCS_CS_GPR_REL_BASE + 4) as u32)?;
        push(batch_dwords, cursor, CS_GPR_STAMP_HI)?;
        push(batch_dwords, cursor, (RCS_CS_GPR_REL_BASE + 8) as u32)?;
        push(batch_dwords, cursor, CS_GPR1_STAMP_LO)?;
        push(batch_dwords, cursor, (RCS_CS_GPR_REL_BASE + 12) as u32)?;
        push(batch_dwords, cursor, CS_GPR_STAMP_HI)
    }

    fn push_sba_address(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        mocs: u32,
        address: u64,
    ) -> Result<(), &'static str> {
        let low = ((address as u32) & 0xFFFF_F000) | (mocs << 4) | u32::from(enable);
        push(batch_dwords, cursor, low)?;
        push(batch_dwords, cursor, (address >> 32) as u32)
    }

    fn push_sba_size(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        size_bytes: usize,
    ) -> Result<(), &'static str> {
        let size_bytes =
            crate::intel::align_up(size_bytes, 4096).ok_or("compute-sba-size-align")?;
        let size_bytes = u32::try_from(size_bytes).map_err(|_| "compute-sba-size-convert")?;
        push(batch_dwords, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
    }

    batch_dwords.fill(0);
    write_gpgpu_dummy_curbe(warm, CURBE_STATE_OFFSET_BYTES)?;
    let mut cursor = 0usize;

    let curbe_total_bytes = gpgpu_curbe_total_bytes();
    let curbe_read_length_8dw = gpgpu_curbe_read_length_8dw();
    let vfe_curbe_allocation_32b = gpgpu_vfe_curbe_allocation_32b();

    const PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER: u32 = 1 << 9;
    const PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER: u32 = 1 << 11;
    const PIPE_CONTROL_GPGPU_SELECT_DW1: u32 =
        (1 << 0) | PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL;

    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER,
        PIPE_CONTROL_GPGPU_SELECT_DW1,
    )?;
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_GPGPU)?;
    push_store_marker(batch_dwords, &mut cursor, 23, 0xC0DE_7801)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER,
        PIPE_CONTROL_CS_STALL,
    )?;

    if GPGPU_TEMPORARY_3D_FOR_SBA {
        push(batch_dwords, &mut cursor, PIPELINE_SELECT_3D)?;
        push_pipe_control_full(
            batch_dwords,
            &mut cursor,
            PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER,
            PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
        )?;
    }
    let idd_index = IDD_STATE_OFFSET_BYTES / core::mem::size_of::<u32>();
    let program_bytes = program.words.len() * core::mem::size_of::<u32>();
    let program_end_offset = GPGPU_EU_KERNEL_OFFSET_BYTES
        .checked_add(program_bytes)
        .ok_or("gpgpu-program-offset-overflow")?;
    if program_end_offset > IDD_STATE_OFFSET_BYTES {
        return Err("gpgpu-program-overlaps-idd-state");
    }
    if idd_index
        .checked_add(GPGPU_INTERFACE_DESCRIPTOR_DWORDS)
        .is_none_or(|end| end * core::mem::size_of::<u32>() > warm.draw_state_len)
    {
        return Err("gpgpu-idd-state-exhausted");
    }
    let kernel_start_pointer =
        compute_gpgpu_kernel_start_pointer(GPGPU_KERNEL_GPU, GPGPU_INSTRUCTION_BASE);
    let idd_words =
        build_gpgpu_interface_descriptor_words(program, store_surface, kernel_start_pointer);
    unsafe {
        let idd_dst = warm.draw_state_virt.add(IDD_STATE_OFFSET_BYTES) as *mut u32;
        for (index, word) in idd_words.iter().enumerate() {
            core::ptr::write_volatile(idd_dst.add(index), *word);
        }
    }
    crate::intel::dma_flush(
        unsafe { warm.draw_state_virt.add(IDD_STATE_OFFSET_BYTES) },
        GPGPU_INTERFACE_DESCRIPTOR_DWORDS * core::mem::size_of::<u32>(),
    );
    let log_program_shape = should_log_gpgpu_program_shape(program.name);
    if log_program_shape {
        crate::log!(
            "intel/gpgpu: compute-kernel-shape program_source={} expects_store={} code_bytes=0x{:X} code_words={} dyn_state_off=0x{:X} idd_words={} bt_off=0x{:X} surf_off=0x{:X} bt_entry=0x{:08X} surf_gpu=0x{:X} target_gpu=0x{:X} send_word_off={} visible_seed_off={} artifact_kind={:?} note=eu-program-uploaded-separately-walker-consumes-idd-surface\n",
            program.name,
            program.expects_store as u8,
            program_bytes,
            program.words.len(),
            IDD_STATE_OFFSET_BYTES,
            GPGPU_INTERFACE_DESCRIPTOR_DWORDS,
            store_surface.binding_table_offset,
            store_surface.surface_state_offset,
            store_surface.binding_entry,
            store_surface.surface_gpu,
            store_surface.target_gpu,
            program.store_send_dword.unwrap_or(0),
            program.visible_seed_dword.unwrap_or(0),
            program.kind,
        );
        crate::log!(
            "intel/gpgpu: idd-shape program_source={} dw0_ksp_lo=0x{:08X} dw1_ksp_hi=0x{:08X} dw2=0x{:08X} dw3=0x{:08X} dw4=0x{:08X} dw5=0x{:08X} dw6=0x{:08X} dw7=0x{:08X} binding_table_present={} curbe_read_len_8dw={} threads_in_group={} barrier_enable={} slm_size=0x{:X} note=legacy-8dw-interface-descriptor\n",
            program.name,
            idd_words[0],
            idd_words[1],
            idd_words[2],
            idd_words[3],
            idd_words[4],
            idd_words[5],
            idd_words[6],
            idd_words[7],
            (idd_words[4] != 0) as u8,
            curbe_read_length_8dw,
            GPGPU_WALKER_GROUP_THREADS,
            (idd_words[6] >> 21) & 1,
            (idd_words[6] >> 16) & 0x1F,
        );
        crate::log!(
            "intel/gpgpu: idd-debug-policy program_source={} idd_dw2=0x{:08X} software_exception_enable={} illegal_opcode_exception_enable={} mask_stack_exception_enable={} sip_programmed={} sip_offset=0x00000000 ksp_negative_control={} note=prm-idd-dw2-loads-eu-cr0-exception-enable-bits\n",
            program.name,
            idd_words[2],
            (idd_words[2] >> 7) & 1,
            (idd_words[2] >> 13) & 1,
            (idd_words[2] >> 11) & 1,
            GPGPU_ENABLE_SIP_EXCEPTIONS as u8,
            gpgpu_kernel_start_pointer_negative_control_enabled() as u8,
        );
        crate::log!(
            "intel/gpgpu: eu-ksp-placement-proof program_source={} instruction_base=0x{:X} ksp=0x{:X} ksp_resolves_to=0x{:X} uploaded_gpu=0x{:X} ksp_unit=byte-offset-low6-mbz ksp_64b_aligned={} instruction_base_4k_aligned={} artifact_bytes=0x{:X} dynamic_state_off=0x{:X} artifact_end_off=0x{:X} overlaps_dynamic_state={} crosses_64b_boundary={} placement_shape=mesa-base0-ksp-absolute-offset expected_delta=\"if fetch base was the bug, illegal/eot signature changes without EU byte changes\"\n",
            program.name,
            GPGPU_INSTRUCTION_BASE,
            kernel_start_pointer,
            GPGPU_INSTRUCTION_BASE + kernel_start_pointer,
            GPGPU_KERNEL_GPU,
            (kernel_start_pointer & 0x3F == 0) as u8,
            (GPGPU_INSTRUCTION_BASE & 0xFFF == 0) as u8,
            program_bytes,
            IDD_STATE_OFFSET_BYTES,
            program_end_offset,
            (program_end_offset > IDD_STATE_OFFSET_BYTES) as u8,
            (((kernel_start_pointer & 0x3F) + program_bytes as u64) > 0x40) as u8,
        );
    }

    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push(batch_dwords, &mut cursor, RENDER_MOCS << 16)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPGPU_DYNAMIC_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPGPU_INSTRUCTION_BASE)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_size(batch_dwords, &mut cursor, true, COMPUTE_SBA_SPAN_BYTES)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    if GPGPU_ENABLE_SIP_EXCEPTIONS {
        let sip_offset = GPGPU_SIP_GPU - GPGPU_INSTRUCTION_BASE;
        push(batch_dwords, &mut cursor, STATE_SIP_CMD)?;
        push(batch_dwords, &mut cursor, sip_offset as u32)?;
        push(batch_dwords, &mut cursor, (sip_offset >> 32) as u32)?;
        if log_program_shape {
            crate::log!(
                "intel/gpgpu: state-sip-policy program_source={} cmd=0x{:08X} instruction_base=0x{:X} sip_offset=0x{:X} sip_resolves_to=0x{:X} exception_target={} note=illegal-opcode-diagnostic\n",
                program.name,
                STATE_SIP_CMD,
                GPGPU_INSTRUCTION_BASE,
                sip_offset,
                GPGPU_INSTRUCTION_BASE + sip_offset,
                trueos_eu::gfx12::eot_artifact(GPGPU_SIP_HANDLER_VARIANT).name,
            );
        }
    } else if log_program_shape {
        crate::log!(
            "intel/gpgpu: state-sip-policy program_source={} cmd=0x{:08X} instruction_base=0x{:X} sip_offset=0x00000000 sip_resolves_to=0x00000000 exception_target=disabled note=minimal-eot-probe\n",
            program.name,
            STATE_SIP_CMD,
            GPGPU_INSTRUCTION_BASE,
        );
    }
    push_store_marker(batch_dwords, &mut cursor, 24, 0xC0DE_7802)?;
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;
    push_store_marker(batch_dwords, &mut cursor, 25, 0xC0DE_7803)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER | PIPE_CONTROL_UNTYPED_DATAPORT_FLUSH_HEADER,
        PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH | PIPE_CONTROL_CS_STALL,
    )?;
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_GPGPU)?;
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER,
        PIPE_CONTROL_CS_STALL,
    )?;
    push_cs_gpr_stamp(batch_dwords, &mut cursor)?;
    let vfe_start = cursor;
    push(batch_dwords, &mut cursor, MEDIA_VFE_STATE_CMD)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(
        batch_dwords,
        &mut cursor,
        (GPGPU_VFE_MAX_THREADS << 16)
            | (GPGPU_VFE_URB_ENTRIES << 8)
            | GPGPU_VFE_FUSED_EU_DISPATCH_LEGACY_MODE,
    )?;
    push(batch_dwords, &mut cursor, 0)?;
    push(
        batch_dwords,
        &mut cursor,
        (GPGPU_VFE_URB_ENTRY_ALLOCATION_32B << 16) | vfe_curbe_allocation_32b,
    )?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_store_marker(batch_dwords, &mut cursor, 26, 0xC0DE_7804)?;
    if !GPGPU_CONTIGUOUS_VFE_IDD_WALKER {
        push_pipe_control_full(
            batch_dwords,
            &mut cursor,
            PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER,
            PIPE_CONTROL_CS_STALL,
        )?;
    }
    if GPGPU_MESA_POST_VFE_PIPE_CONTROL {
        push_pipe_control_full(
            batch_dwords,
            &mut cursor,
            PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER,
            PIPE_CONTROL_FLUSH_ENABLE | PIPE_CONTROL_CS_STALL,
        )?;
    }
    let id_load_start = cursor;
    let midl_total_bytes = if GPGPU_MIDL_NEGATIVE_CONTROL {
        0
    } else {
        GPGPU_INTERFACE_DESCRIPTOR_DWORDS * core::mem::size_of::<u32>()
    };
    let midl_start_address = if GPGPU_MIDL_NEGATIVE_CONTROL {
        0
    } else {
        IDD_DYNAMIC_OFFSET_BYTES as u32
    };
    push(batch_dwords, &mut cursor, MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, midl_total_bytes as u32)?;
    push(batch_dwords, &mut cursor, midl_start_address)?;
    push_store_marker(batch_dwords, &mut cursor, 27, 0xC0DE_7805)?;
    if GPGPU_LOAD_DUMMY_CURBE {
        push(batch_dwords, &mut cursor, MEDIA_CURBE_LOAD_CMD)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, curbe_total_bytes as u32)?;
        push(batch_dwords, &mut cursor, CURBE_DYNAMIC_OFFSET_BYTES as u32)?;
        push_store_marker(batch_dwords, &mut cursor, 28, 0xC0DE_7806)?;
    }
    let walker_start = cursor;
    push(batch_dwords, &mut cursor, GPGPU_WALKER_CMD)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, GPGPU_WALKER_GROUP_THREADS - 1)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, walker_group_x_dim)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, GPGPU_WALKER_GROUP_Y_DIM)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, GPGPU_WALKER_GROUP_Z_DIM)?;
    push(batch_dwords, &mut cursor, GPGPU_WALKER_SIMD8_RIGHT_MASK)?;
    push(batch_dwords, &mut cursor, GPGPU_WALKER_BOTTOM_MASK)?;
    push(batch_dwords, &mut cursor, MEDIA_STATE_FLUSH_CMD)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_FLUSH_BITS)?;
    push_store_marker(
        batch_dwords,
        &mut cursor,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
    )?;
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;
    let command_bytes = cursor * core::mem::size_of::<u32>();
    let batch_bytes = command_bytes;
    let walker_dw4 = batch_dwords[walker_start + 4];
    let walker_x_dim = batch_dwords[walker_start + 7];
    let walker_y_dim = batch_dwords[walker_start + 10];
    let walker_z_dim = batch_dwords[walker_start + 12];
    let thread_width = (walker_dw4 & 0x3F) + 1;
    let thread_height = ((walker_dw4 >> 8) & 0x3F) + 1;
    let thread_depth = ((walker_dw4 >> 16) & 0x3F) + 1;
    let walker_group_threads = thread_width * thread_height * thread_depth;
    let walker_group_count = walker_x_dim * walker_y_dim * walker_z_dim;
    let expected_hw_threads = walker_group_count * walker_group_threads;
    let idd_dw6 = idd_words[6];
    let idd_barrier_enable = (idd_dw6 >> 21) & 1;
    let idd_slm_size = (idd_dw6 >> 16) & 0x1F;
    let idd_threads_in_group = idd_dw6 & 0x3FF;
    let simd_mask_bits = match (walker_dw4 >> 30) & 0x3 {
        0 => 8,
        1 => 16,
        2 => 32,
        _ => 0,
    };
    let simd_mask = if simd_mask_bits == 32 {
        u32::MAX
    } else {
        (1u32 << simd_mask_bits) - 1
    };
    let right_lanes_consumed = (batch_dwords[walker_start + 13] & simd_mask).count_ones();
    let bottom_lanes_consumed = (batch_dwords[walker_start + 14] & simd_mask).count_ones();

    if log_program_shape {
        crate::log!(
            "intel/gpgpu: compute-walker-layout program_source={} expects_store={} launch_profile=split-vfe-msf-curbe-pc-midl vfe_off=0x{:X} vfe_dw3=0x{:08X} vfe_dw5=0x{:08X} fused_eu_dispatch_legacy={} urb_entry_alloc_32b={} curbe_present={} curbe_bytes=0x{:X} curbe_read_len_8dw={} id_load_off=0x{:X} id_load_bytes=0x{:X} idd_payload_bytes=0x{:X} midl_negative_control={} state_bases_relative={} temporary_3d_for_sba={} midl_start=0x{:X} walker_off=0x{:X} walker_cmd=0x{:08X} exec_mask=0x{:08X} idd_gpu=0x{:X} idd_dynamic_offset=0x{:X} idd_ksp=0x{:08X} instruction_base=0x{:X} ksp_resolves_to=0x{:X} idd_dw2=0x{:08X} idd_dw4=0x{:08X} idd_dw6=0x{:08X} surface_base=0x{:X} dynamic_state_base=0x{:X} contiguous_vfe_idd_walker={} mesa_post_vfe_pipe_control={} tail_off=0x{:X} cs_marker=0x{:08X} note=legacy-vfe-dispatch-with-prm-len13-walker\n",
            program.name,
            program.expects_store as u8,
            vfe_start * core::mem::size_of::<u32>(),
            batch_dwords[vfe_start + 3],
            batch_dwords[vfe_start + 5],
            ((batch_dwords[vfe_start + 3] & GPGPU_VFE_FUSED_EU_DISPATCH_LEGACY_MODE) != 0) as u8,
            GPGPU_VFE_URB_ENTRY_ALLOCATION_32B,
            GPGPU_LOAD_DUMMY_CURBE as u8,
            curbe_total_bytes,
            curbe_read_length_8dw,
            id_load_start * core::mem::size_of::<u32>(),
            midl_total_bytes,
            GPGPU_INTERFACE_DESCRIPTOR_DWORDS * core::mem::size_of::<u32>(),
            GPGPU_MIDL_NEGATIVE_CONTROL as u8,
            GPGPU_RELATIVE_STATE_BASES as u8,
            GPGPU_TEMPORARY_3D_FOR_SBA as u8,
            midl_start_address,
            walker_start * core::mem::size_of::<u32>(),
            batch_dwords[walker_start],
            batch_dwords[walker_start + 13],
            GPGPU_DYNAMIC_STATE_BASE + IDD_DYNAMIC_OFFSET_BYTES as u64,
            IDD_DYNAMIC_OFFSET_BYTES,
            idd_words[0],
            GPGPU_INSTRUCTION_BASE,
            GPGPU_INSTRUCTION_BASE + kernel_start_pointer,
            idd_words[2],
            idd_words[4],
            idd_words[6],
            GPU_VA_DRAW_STATE_BASE,
            GPGPU_DYNAMIC_STATE_BASE,
            GPGPU_CONTIGUOUS_VFE_IDD_WALKER as u8,
            GPGPU_MESA_POST_VFE_PIPE_CONTROL as u8,
            batch_bytes,
            RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        );
        crate::log!(
            "intel/gpgpu: compute-walker-contract program_source={} groups={}x{}x{} threads_per_group={} expected_hw_threads={} expected_lane_dispatch={} idd_threads_in_group={} barrier_enable={} slm_size=0x{:X} simd_mask_bits={} right_mask=0x{:08X} bottom_mask=0x{:08X} right_lanes={} bottom_lanes={} use_gfx125_compute_walker={} note=legacy-walker-lane-shape\n",
            program.name,
            walker_x_dim,
            walker_y_dim,
            walker_z_dim,
            walker_group_threads,
            expected_hw_threads,
            expected_hw_threads.saturating_mul(GPGPU_WALKER_SIMD8_LANES),
            idd_threads_in_group,
            idd_barrier_enable,
            idd_slm_size,
            simd_mask_bits,
            batch_dwords[walker_start + 13],
            batch_dwords[walker_start + 14],
            right_lanes_consumed,
            bottom_lanes_consumed,
            GPGPU_USE_GFX125_COMPUTE_WALKER as u8,
        );
    }

    Ok(batch_bytes)
}
