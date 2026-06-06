use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use super::super as intel;
use super::*;

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuShellCube20ProjectResult {
    pub(crate) ok: bool,
    pub(crate) frames: u32,
    pub(crate) submitted: u32,
    pub(crate) presented: u32,
    pub(crate) visible_points: usize,
    pub(crate) stamped_pixels: usize,
    pub(crate) duration_ms: u64,
    pub(crate) elapsed_ms: u64,
    pub(crate) cadence_us: u64,
    pub(crate) total_submit_ms: u64,
    pub(crate) max_submit_ms: u64,
    pub(crate) primary_width: u32,
    pub(crate) primary_height: u32,
    pub(crate) canvas_xy: GpgpuPoint,
    pub(crate) vertex_count: usize,
    pub(crate) radius_px: u32,
    pub(crate) last_angle_deg: u32,
}

pub(crate) struct GpgpuCanvas3dUi2TextureFrame {
    pub(crate) result: GpgpuShellCube20ProjectResult,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}
pub(crate) fn submit_canvas3d_project_once() -> bool {
    if !DIRECT_RCS_ENABLED || CANVAS3D_PROJECT_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = intel::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-project skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(upload) = upload_canvas3d_project_rgba8_kernel() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-project skipped reason=no-kernel-upload\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-project failed rung=alloc\n"
        );
        return false;
    };
    if CANVAS3D_PROJECT_TEST_BYTES > CLEAR_RECT_TEST_BYTES {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-project failed rung=test-buffer bytes={} cap={}\n",
            CANVAS3D_PROJECT_TEST_BYTES,
            CLEAR_RECT_TEST_BYTES,
        );
        return false;
    }
    if CANVAS3D_PROJECT_OUT_BYTES > CANVAS3D_PROJECT_OUT_ALLOC_BYTES {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-project failed rung=out-buffer bytes={} cap={}\n",
            CANVAS3D_PROJECT_OUT_BYTES,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
        return false;
    }

    let expected = direct_rcs_seed_canvas3d_project(state);
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let vertices_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
            state.clear_test_phys,
            CLEAR_RECT_TEST_BYTES,
        );
    let out_ppgtt_ok = vertices_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            CANVAS3D_PROJECT_OUT_GPU,
            state.canvas3d_out_phys,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
    let params = Canvas3dProjectRgba8Params {
        vertices_gpu: DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        out_gpu: CANVAS3D_PROJECT_OUT_GPU,
        src_first_vertex: CANVAS3D_PROJECT_SMOKE_SRC_FIRST,
        out_first_point: CANVAS3D_PROJECT_SMOKE_OUT_FIRST,
        vertex_count: CANVAS3D_PROJECT_SAMPLE_COUNT as u32,
        canvas_width: CANVAS3D_PROJECT_SMOKE_CANVAS_WIDTH,
        canvas_height: CANVAS3D_PROJECT_SMOKE_CANVAS_HEIGHT,
    };
    let batch_ok = out_ppgtt_ok
        && direct_rcs_encode_canvas3d_project_batch(
            state,
            upload,
            params,
            CANVAS3D_PROJECT_VERTEX_BYTES,
            CANVAS3D_PROJECT_OUT_BYTES,
        );
    let submit_start_tick = direct_rcs_now_tick();
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let (observed, retire_ms) = if submitted {
        direct_rcs_poll_result_slot_elapsed(
            state,
            CANVAS3D_PROJECT_POST_MARKER_SLOT,
            CANVAS3D_PROJECT_POST_MARKER,
            submit_start_tick,
        )
    } else {
        (0, 0)
    };
    let pre_marker = direct_rcs_read_result_slot(state, CANVAS3D_PROJECT_PRE_MARKER_SLOT);
    let after =
        direct_rcs_read_canvas3d_project_samples(state, CANVAS3D_PROJECT_SMOKE_OUT_FIRST as usize);
    let retired = observed == CANVAS3D_PROJECT_POST_MARKER;
    let samples_ok = direct_rcs_canvas3d_project_count_matching(after, expected);
    let visible = direct_rcs_canvas3d_project_count_visible(after);
    let ok = retired && samples_ok == CANVAS3D_PROJECT_SAMPLE_COUNT;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: canvas3d-project forcewake={} ggtt={} ppgtt={} kernel_ppgtt={} vertices_ppgtt={} out_ppgtt={} batch={} submitted={} retired={} retire_ms={} ok={} samples={}/{} visible={}/{} src_first={} out_first={} vertex_count={} canvas={}x{} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} vertices_gpu=0x{:X} out_gpu=0x{:X} vertex_bytes=0x{:X} out_bytes=0x{:X} idd_off=0x{:X} payload_off=0x{:X} out0=[xy=0x{:08X},rgba=0x{:08X}] out1=[xy=0x{:08X},rgba=0x{:08X}] out2=[xy=0x{:08X},rgba=0x{:08X}] out6=[xy=0x{:08X},rgba=0x{:08X}] ring_gpu=0x{:X} batch_gpu=0x{:X} result_gpu=0x{:X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} path=direct-execlist no_guc_submit=1 next=canvas-lines-or-cpu-copy\n",
        forcewake_ok as u8,
        mapped_ok as u8,
        ppgtt_ok as u8,
        kernel_ppgtt_ok as u8,
        vertices_ppgtt_ok as u8,
        out_ppgtt_ok as u8,
        batch_ok as u8,
        submitted as u8,
        retired as u8,
        retire_ms,
        ok as u8,
        samples_ok,
        CANVAS3D_PROJECT_SAMPLE_COUNT,
        visible,
        CANVAS3D_PROJECT_SAMPLE_COUNT,
        CANVAS3D_PROJECT_SMOKE_SRC_FIRST,
        CANVAS3D_PROJECT_SMOKE_OUT_FIRST,
        params.vertex_count,
        CANVAS3D_PROJECT_SMOKE_CANVAS_WIDTH,
        CANVAS3D_PROJECT_SMOKE_CANVAS_HEIGHT,
        pre_marker,
        observed,
        CANVAS3D_PROJECT_POST_MARKER,
        upload.gpu,
        upload.gpu + CANVAS3D_PROJECT_RGBA8_TEXT_OFFSET_BYTES,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        CANVAS3D_PROJECT_OUT_GPU,
        CANVAS3D_PROJECT_VERTEX_BYTES,
        CANVAS3D_PROJECT_OUT_BYTES,
        CANVAS3D_PROJECT_IDD_OFFSET_BYTES,
        CANVAS3D_PROJECT_PAYLOAD_OFFSET_BYTES,
        after[0].packed_xy,
        after[0].rgba,
        after[1].packed_xy,
        after[1].rgba,
        after[2].packed_xy,
        after[2].rgba,
        after[6].packed_xy,
        after[6].rgba,
        DIRECT_RCS_GPU_VA_RING_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_RESULT_BASE,
        intel::mmio_read(dev, RCS_RING_HEAD),
        intel::mmio_read(dev, RCS_RING_TAIL),
        intel::mmio_read(dev, RCS_RING_ACTHD),
        intel::mmio_read(dev, RCS_RING_IPEIR),
        intel::mmio_read(dev, RCS_RING_IPEHR),
        intel::mmio_read(dev, RCS_RING_EIR),
    );

    ok
}

pub(crate) fn submit_canvas3d_transform_smoke_once() -> bool {
    if !DIRECT_RCS_ENABLED || CANVAS3D_TRANSFORM_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = intel::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-transform skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-transform failed rung=alloc\n"
        );
        return false;
    };
    if CANVAS3D_PROJECT_VERTEX_BYTES > CLEAR_RECT_TEST_BYTES
        || CANVAS3D_PROJECT_VERTEX_BYTES > CANVAS3D_PROJECT_OUT_ALLOC_BYTES
    {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-transform failed rung=buffer-cap vertex_bytes=0x{:X} src_cap=0x{:X} dst_cap=0x{:X}\n",
            CANVAS3D_PROJECT_VERTEX_BYTES,
            CLEAR_RECT_TEST_BYTES,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
        return false;
    }

    let q = CANVAS3D_PROJECT_Q16_ONE;
    let translate = Canvas3dVec3Q16 {
        x: q / 4,
        y: -(q / 8),
        z: q / 2,
        pad: 0,
    };
    let scale = Canvas3dVec3Q16 {
        x: q * 2,
        y: -q,
        z: q / 2,
        pad: 0,
    };
    let rotate_z_180 = Canvas3dVec3Q16 {
        x: 0,
        y: 0,
        z: q,
        pad: 0,
    };

    let fused_ok = submit_canvas3d_transform_fused_smoke(
        dev,
        state,
        upload_canvas3d_transform_q16_kernel(),
        scale,
        rotate_z_180,
        translate,
    );

    fused_ok
}

pub(crate) fn submit_canvas3d_clip_box_q16_once() -> bool {
    if !DIRECT_RCS_ENABLED || CANVAS3D_CLIP_BOX_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = intel::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-clip-box-q16 skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-clip-box-q16 failed rung=alloc\n"
        );
        return false;
    };
    let Some(upload) = upload_canvas3d_clip_box_q16_kernel() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-clip-box-q16 skipped reason=no-kernel-upload\n"
        );
        return false;
    };
    if CANVAS3D_PROJECT_VERTEX_BYTES > CLEAR_RECT_TEST_BYTES
        || CANVAS3D_PROJECT_VERTEX_BYTES > CANVAS3D_PROJECT_OUT_ALLOC_BYTES
    {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-clip-box-q16 failed rung=buffer-cap vertex_bytes=0x{:X} src_cap=0x{:X} dst_cap=0x{:X}\n",
            CANVAS3D_PROJECT_VERTEX_BYTES,
            CLEAR_RECT_TEST_BYTES,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
        return false;
    }

    let q = CANVAS3D_PROJECT_Q16_ONE;
    let min_q16 = Canvas3dVec3Q16 {
        x: -(q / 2),
        y: -(q / 4),
        z: q / 4,
        pad: 0,
    };
    let max_q16 = Canvas3dVec3Q16 {
        x: q / 2,
        y: q / 4,
        z: q + q / 2,
        pad: 0,
    };

    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let expected = direct_rcs_seed_canvas3d_clip_box(state, min_q16, max_q16);
    let params = Canvas3dClipBoxQ16Params {
        src_gpu: DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        dst_gpu: CANVAS3D_PROJECT_OUT_GPU,
        src_first_vertex: CANVAS3D_TRANSFORM_SRC_FIRST,
        dst_first_vertex: CANVAS3D_TRANSFORM_DST_FIRST,
        vertex_count: CANVAS3D_TRANSFORM_TEST_COUNT,
        min_q16,
        max_q16,
    };
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let src_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
            state.clear_test_phys,
            CLEAR_RECT_TEST_BYTES,
        );
    let dst_ppgtt_ok = src_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            CANVAS3D_PROJECT_OUT_GPU,
            state.canvas3d_out_phys,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_canvas3d_clip_box_batch(
            state,
            upload,
            params,
            CANVAS3D_PROJECT_VERTEX_BYTES,
            CANVAS3D_PROJECT_VERTEX_BYTES,
        );
    let submit_start_tick = direct_rcs_now_tick();
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let (observed, retire_ms) = if submitted {
        direct_rcs_poll_result_slot_elapsed(
            state,
            CANVAS3D_CLIP_BOX_POST_MARKER_SLOT,
            CANVAS3D_CLIP_BOX_POST_MARKER,
            submit_start_tick,
        )
    } else {
        (0, 0)
    };
    let pre_marker = direct_rcs_read_result_slot(state, CANVAS3D_CLIP_BOX_PRE_MARKER_SLOT);
    let (matches, src_preserved, guards_ok, first, last) =
        direct_rcs_read_canvas3d_clip_box_result(state, expected);
    let retired = observed == CANVAS3D_CLIP_BOX_POST_MARKER;
    let ok = retired
        && pre_marker == CANVAS3D_CLIP_BOX_PRE_MARKER
        && matches == CANVAS3D_TRANSFORM_TEST_COUNT as usize
        && src_preserved == CANVAS3D_TRANSFORM_TEST_COUNT as usize
        && guards_ok;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: canvas3d-clip-box-q16 forcewake={} ggtt={} ppgtt={} kernel_ppgtt={} src_ppgtt={} dst_ppgtt={} batch={} submitted={} retired={} retire_ms={} ok={} matched={}/{} src_preserved={}/{} guards_ok={} src_first={} dst_first={} count={} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} src_gpu=0x{:X} dst_gpu=0x{:X} min=[{}, {}, {}, {}] max=[{}, {}, {}, {}] first=[{}, {}, {}, {}] last=[{}, {}, {}, {}] idd_off=0x{:X} payload_off=0x{:X} ring_gpu=0x{:X} batch_gpu=0x{:X} result_gpu=0x{:X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} path=direct-execlist no_guc_submit=1 next=project-or-visual\n",
        forcewake_ok as u8,
        mapped_ok as u8,
        ppgtt_ok as u8,
        kernel_ppgtt_ok as u8,
        src_ppgtt_ok as u8,
        dst_ppgtt_ok as u8,
        batch_ok as u8,
        submitted as u8,
        retired as u8,
        retire_ms,
        ok as u8,
        matches,
        CANVAS3D_TRANSFORM_TEST_COUNT,
        src_preserved,
        CANVAS3D_TRANSFORM_TEST_COUNT,
        guards_ok as u8,
        CANVAS3D_TRANSFORM_SRC_FIRST,
        CANVAS3D_TRANSFORM_DST_FIRST,
        CANVAS3D_TRANSFORM_TEST_COUNT,
        pre_marker,
        observed,
        CANVAS3D_CLIP_BOX_POST_MARKER,
        upload.gpu,
        upload.gpu + CANVAS3D_CLIP_BOX_Q16_TEXT_OFFSET_BYTES,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        CANVAS3D_PROJECT_OUT_GPU,
        min_q16.x,
        min_q16.y,
        min_q16.z,
        min_q16.pad,
        max_q16.x,
        max_q16.y,
        max_q16.z,
        max_q16.pad,
        first.x,
        first.y,
        first.z,
        first.pad,
        last.x,
        last.y,
        last.z,
        last.pad,
        CANVAS3D_CLIP_BOX_IDD_OFFSET_BYTES,
        CANVAS3D_CLIP_BOX_PAYLOAD_OFFSET_BYTES,
        DIRECT_RCS_GPU_VA_RING_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_RESULT_BASE,
        intel::mmio_read(dev, RCS_RING_HEAD),
        intel::mmio_read(dev, RCS_RING_TAIL),
        intel::mmio_read(dev, RCS_RING_ACTHD),
        intel::mmio_read(dev, RCS_RING_IPEIR),
        intel::mmio_read(dev, RCS_RING_IPEHR),
        intel::mmio_read(dev, RCS_RING_EIR),
    );

    ok
}

pub(crate) fn submit_canvas3d_plane_sample_rgba8_once() -> bool {
    if !DIRECT_RCS_ENABLED || CANVAS3D_PLANE_SAMPLE_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = intel::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-sample skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-sample failed rung=alloc\n"
        );
        return false;
    };
    let Some(upload) = upload_canvas3d_plane_sample_rgba8_kernel() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-sample skipped reason=no-kernel-upload\n"
        );
        return false;
    };
    if CANVAS3D_PROJECT_OUT_BYTES > CANVAS3D_PROJECT_OUT_ALLOC_BYTES {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-sample failed rung=buffer-cap out_bytes=0x{:X} out_cap=0x{:X}\n",
            CANVAS3D_PROJECT_OUT_BYTES,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
        return false;
    }

    let q = CANVAS3D_PROJECT_Q16_ONE;
    let params = Canvas3dPlaneSampleRgba8Params {
        out_gpu: CANVAS3D_PROJECT_OUT_GPU,
        out_first_point: CANVAS3D_PLANE_SAMPLE_OUT_FIRST,
        sample_count: CANVAS3D_PLANE_SAMPLE_COUNT_U32,
        canvas_width: CANVAS3D_PROJECT_SMOKE_CANVAS_WIDTH,
        canvas_height: CANVAS3D_PROJECT_SMOKE_CANVAS_HEIGHT,
        origin_q16: Canvas3dVec3Q16 {
            x: 0,
            y: 0,
            z: q * 2,
            pad: 0,
        },
        axis_u_q16: Canvas3dVec3Q16 {
            x: q / 2,
            y: 0,
            z: 0,
            pad: 0,
        },
        axis_v_q16: Canvas3dVec3Q16 {
            x: 0,
            y: q / 2,
            z: 0,
            pad: 0,
        },
        constraint0_q16: Canvas3dVec3Q16 {
            x: q,
            y: 0,
            z: q / 2,
            pad: 0,
        },
        constraint1_q16: Canvas3dVec3Q16 {
            x: 0,
            y: -q,
            z: q / 2,
            pad: 0,
        },
        constraint2_q16: Canvas3dVec3Q16 {
            x: -q,
            y: -q,
            z: q,
            pad: 0,
        },
        constraint3_q16: Canvas3dVec3Q16::default(),
        constraint_count: 3,
        u_steps: CANVAS3D_PLANE_SAMPLE_GRID_U,
        v_steps: CANVAS3D_PLANE_SAMPLE_GRID_V,
        color_rgba: CANVAS3D_PLANE_SAMPLE_COLOR,
    };

    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let expected = direct_rcs_seed_canvas3d_plane_sample(state, params);
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let out_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            CANVAS3D_PROJECT_OUT_GPU,
            state.canvas3d_out_phys,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
    let batch_ok = out_ppgtt_ok
        && direct_rcs_encode_canvas3d_plane_sample_batch(
            state,
            upload,
            params,
            CANVAS3D_PROJECT_OUT_BYTES,
        );
    let submit_start_tick = direct_rcs_now_tick();
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let (observed, retire_ms) = if submitted {
        direct_rcs_poll_result_slot_elapsed(
            state,
            CANVAS3D_PLANE_SAMPLE_POST_MARKER_SLOT,
            CANVAS3D_PLANE_SAMPLE_POST_MARKER,
            submit_start_tick,
        )
    } else {
        (0, 0)
    };
    let pre_marker = direct_rcs_read_result_slot(state, CANVAS3D_PLANE_SAMPLE_PRE_MARKER_SLOT);
    let after = direct_rcs_read_canvas3d_plane_sample_result(state);
    let retired = observed == CANVAS3D_PLANE_SAMPLE_POST_MARKER;
    let matched = direct_rcs_canvas3d_plane_sample_count_matching(after, expected);
    let visible = direct_rcs_canvas3d_plane_sample_count_visible(after);
    let ok = retired
        && pre_marker == CANVAS3D_PLANE_SAMPLE_PRE_MARKER
        && matched == CANVAS3D_PLANE_SAMPLE_COUNT;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: canvas3d-plane-sample forcewake={} ggtt={} ppgtt={} kernel_ppgtt={} out_ppgtt={} batch={} submitted={} retired={} retire_ms={} ok={} matched={}/{} visible={}/{} out_first={} samples={} grid={}x{} constraints={} canvas={}x{} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} out_gpu=0x{:X} out_bytes=0x{:X} idd_off=0x{:X} payload_off=0x{:X} out0=[xy=0x{:08X},rgba=0x{:08X}] out5=[xy=0x{:08X},rgba=0x{:08X}] out15=[xy=0x{:08X},rgba=0x{:08X}] ring_gpu=0x{:X} batch_gpu=0x{:X} result_gpu=0x{:X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} path=direct-execlist no_guc_submit=1 next=plane-worklist-or-fill\n",
        forcewake_ok as u8,
        mapped_ok as u8,
        ppgtt_ok as u8,
        kernel_ppgtt_ok as u8,
        out_ppgtt_ok as u8,
        batch_ok as u8,
        submitted as u8,
        retired as u8,
        retire_ms,
        ok as u8,
        matched,
        CANVAS3D_PLANE_SAMPLE_COUNT,
        visible,
        CANVAS3D_PLANE_SAMPLE_COUNT,
        params.out_first_point,
        params.sample_count,
        params.u_steps,
        params.v_steps,
        params.constraint_count,
        params.canvas_width,
        params.canvas_height,
        pre_marker,
        observed,
        CANVAS3D_PLANE_SAMPLE_POST_MARKER,
        upload.gpu,
        upload.gpu + CANVAS3D_PLANE_SAMPLE_RGBA8_TEXT_OFFSET_BYTES,
        CANVAS3D_PROJECT_OUT_GPU,
        CANVAS3D_PROJECT_OUT_BYTES,
        CANVAS3D_PLANE_SAMPLE_IDD_OFFSET_BYTES,
        CANVAS3D_PLANE_SAMPLE_PAYLOAD_OFFSET_BYTES,
        after[0].packed_xy,
        after[0].rgba,
        after[5].packed_xy,
        after[5].rgba,
        after[15].packed_xy,
        after[15].rgba,
        DIRECT_RCS_GPU_VA_RING_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_RESULT_BASE,
        intel::mmio_read(dev, RCS_RING_HEAD),
        intel::mmio_read(dev, RCS_RING_TAIL),
        intel::mmio_read(dev, RCS_RING_ACTHD),
        intel::mmio_read(dev, RCS_RING_IPEIR),
        intel::mmio_read(dev, RCS_RING_IPEHR),
        intel::mmio_read(dev, RCS_RING_EIR),
    );

    ok
}

pub(crate) fn submit_canvas3d_plane_fill_rgba8_once() -> bool {
    if !DIRECT_RCS_ENABLED || CANVAS3D_PLANE_FILL_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = intel::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-fill skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-fill failed rung=alloc\n"
        );
        return false;
    };
    let Some(upload) = upload_canvas3d_plane_fill_rgba8_kernel() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-fill skipped reason=no-kernel-upload\n"
        );
        return false;
    };
    if CANVAS3D_PLANE_FILL_TEST_BYTES > CANVAS3D_PROJECT_OUT_ALLOC_BYTES {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-fill failed rung=buffer-cap dst_bytes=0x{:X} dst_cap=0x{:X}\n",
            CANVAS3D_PLANE_FILL_TEST_BYTES,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
        return false;
    }

    let q = CANVAS3D_PROJECT_Q16_ONE;
    let params = Canvas3dPlaneFillRgba8Params {
        dst_gpu: CANVAS3D_PROJECT_OUT_GPU,
        dst_pitch_bytes: CANVAS3D_PLANE_FILL_TEST_PITCH_BYTES,
        dst_width: CANVAS3D_PLANE_FILL_TEST_WIDTH,
        dst_height: CANVAS3D_PLANE_FILL_TEST_HEIGHT,
        rect_x: 0,
        rect_y: 0,
        rect_width: CANVAS3D_PLANE_FILL_TEST_WIDTH,
        rect_height: CANVAS3D_PLANE_FILL_TEST_HEIGHT,
        canvas_width: CANVAS3D_PLANE_FILL_TEST_WIDTH,
        canvas_height: CANVAS3D_PLANE_FILL_TEST_HEIGHT,
        origin_q16: Canvas3dVec3Q16 {
            x: 0,
            y: 0,
            z: q * 2,
            pad: 0,
        },
        axis_u_q16: Canvas3dVec3Q16 {
            x: q / 2,
            y: 0,
            z: 0,
            pad: 0,
        },
        axis_v_q16: Canvas3dVec3Q16 {
            x: 0,
            y: q / 2,
            z: 0,
            pad: 0,
        },
        constraint0_q16: Canvas3dVec3Q16 {
            x: q,
            y: 0,
            z: q,
            pad: 0,
        },
        constraint1_q16: Canvas3dVec3Q16 {
            x: -q,
            y: 0,
            z: q,
            pad: 0,
        },
        constraint2_q16: Canvas3dVec3Q16 {
            x: 0,
            y: q,
            z: q,
            pad: 0,
        },
        constraint3_q16: Canvas3dVec3Q16 {
            x: 0,
            y: -q,
            z: q,
            pad: 0,
        },
        constraint_count: 4,
        color_rgba: CANVAS3D_PLANE_FILL_TEST_COLOR,
    };

    let poison = 0x1020_3040u32;
    unsafe {
        let dst = state.canvas3d_out_virt as *mut u32;
        for index in 0..(CANVAS3D_PLANE_FILL_TEST_BYTES / core::mem::size_of::<u32>()) {
            core::ptr::write_volatile(dst.add(index), poison);
        }
    }
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);

    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let dst_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            CANVAS3D_PROJECT_OUT_GPU,
            state.canvas3d_out_phys,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_canvas3d_plane_fill_batch(
            state,
            upload,
            params,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
    let submit_start_tick = direct_rcs_now_tick();
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let (observed, retire_ms) = if submitted {
        direct_rcs_poll_result_slot_elapsed(
            state,
            CANVAS3D_PLANE_FILL_POST_MARKER_SLOT,
            CANVAS3D_PLANE_FILL_POST_MARKER,
            submit_start_tick,
        )
    } else {
        (0, 0)
    };
    let pre_marker = direct_rcs_read_result_slot(state, CANVAS3D_PLANE_FILL_PRE_MARKER_SLOT);
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);

    let pitch_pixels =
        (CANVAS3D_PLANE_FILL_TEST_PITCH_BYTES / core::mem::size_of::<u32>() as u32) as usize;
    let center_index = (CANVAS3D_PLANE_FILL_TEST_HEIGHT as usize / 2) * pitch_pixels
        + CANVAS3D_PLANE_FILL_TEST_WIDTH as usize / 2;
    let corner_index = 0usize;
    let mut changed = 0usize;
    let mut colored = 0usize;
    let mut first_changed_index = usize::MAX;
    let mut first_changed = poison;
    let (center, corner) = unsafe {
        let dst = state.canvas3d_out_virt as *const u32;
        for index in 0..(CANVAS3D_PLANE_FILL_TEST_BYTES / core::mem::size_of::<u32>()) {
            let value = core::ptr::read_volatile(dst.add(index));
            if value != poison {
                changed += 1;
                if first_changed_index == usize::MAX {
                    first_changed_index = index;
                    first_changed = value;
                }
            }
            if value == CANVAS3D_PLANE_FILL_TEST_COLOR {
                colored += 1;
            }
        }
        (
            core::ptr::read_volatile(dst.add(center_index)),
            core::ptr::read_volatile(dst.add(corner_index)),
        )
    };

    let retired = observed == CANVAS3D_PLANE_FILL_POST_MARKER;
    let ok = retired
        && pre_marker == CANVAS3D_PLANE_FILL_PRE_MARKER
        && changed > 0
        && colored > 0
        && center == CANVAS3D_PLANE_FILL_TEST_COLOR;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: canvas3d-plane-fill forcewake={} ggtt={} ppgtt={} kernel_ppgtt={} dst_ppgtt={} batch={} submitted={} retired={} retire_ms={} ok={} changed={} colored={} center=0x{:08X} corner=0x{:08X} first_changed={} first_value=0x{:08X} poison=0x{:08X} dst={}x{} pitch={} rect={}x{} constraints={} canvas={}x{} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} dst_gpu=0x{:X} dst_bytes=0x{:X} idd_off=0x{:X} payload_off=0x{:X} color=0x{:08X} ring_gpu=0x{:X} batch_gpu=0x{:X} result_gpu=0x{:X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} path=direct-execlist no_guc_submit=1 next=plane-worklist-or-z\n",
        forcewake_ok as u8,
        mapped_ok as u8,
        ppgtt_ok as u8,
        kernel_ppgtt_ok as u8,
        dst_ppgtt_ok as u8,
        batch_ok as u8,
        submitted as u8,
        retired as u8,
        retire_ms,
        ok as u8,
        changed,
        colored,
        center,
        corner,
        first_changed_index,
        first_changed,
        poison,
        params.dst_width,
        params.dst_height,
        params.dst_pitch_bytes,
        params.rect_width,
        params.rect_height,
        params.constraint_count,
        params.canvas_width,
        params.canvas_height,
        pre_marker,
        observed,
        CANVAS3D_PLANE_FILL_POST_MARKER,
        upload.gpu,
        upload.gpu + CANVAS3D_PLANE_FILL_RGBA8_TEXT_OFFSET_BYTES,
        CANVAS3D_PROJECT_OUT_GPU,
        CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        CANVAS3D_PLANE_FILL_IDD_OFFSET_BYTES,
        CANVAS3D_PLANE_FILL_PAYLOAD_OFFSET_BYTES,
        params.color_rgba,
        DIRECT_RCS_GPU_VA_RING_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_RESULT_BASE,
        intel::mmio_read(dev, RCS_RING_HEAD),
        intel::mmio_read(dev, RCS_RING_TAIL),
        intel::mmio_read(dev, RCS_RING_ACTHD),
        intel::mmio_read(dev, RCS_RING_IPEIR),
        intel::mmio_read(dev, RCS_RING_IPEHR),
        intel::mmio_read(dev, RCS_RING_EIR),
    );

    ok
}

pub(crate) fn submit_canvas3d_plane_patch_fill_cut_rgba8_once() -> bool {
    if !DIRECT_RCS_ENABLED || CANVAS3D_PLANE_PATCH_FILL_CUT_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = intel::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-patch-fill-cut skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-patch-fill-cut failed rung=alloc\n"
        );
        return false;
    };
    let Some(upload) = upload_canvas3d_plane_patch_fill_cut_rgba8_kernel() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-patch-fill-cut skipped reason=no-kernel-upload\n"
        );
        return false;
    };
    if CANVAS3D_PLANE_FILL_TEST_BYTES > CANVAS3D_PROJECT_OUT_ALLOC_BYTES {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-patch-fill-cut failed rung=buffer-cap dst_bytes=0x{:X} dst_cap=0x{:X}\n",
            CANVAS3D_PLANE_FILL_TEST_BYTES,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
        return false;
    }

    let q = CANVAS3D_PROJECT_Q16_ONE;
    let params = Canvas3dPlaneFillRgba8Params {
        dst_gpu: CANVAS3D_PROJECT_OUT_GPU,
        dst_pitch_bytes: CANVAS3D_PLANE_FILL_TEST_PITCH_BYTES,
        dst_width: CANVAS3D_PLANE_FILL_TEST_WIDTH,
        dst_height: CANVAS3D_PLANE_FILL_TEST_HEIGHT,
        rect_x: 0,
        rect_y: 0,
        rect_width: CANVAS3D_PLANE_FILL_TEST_WIDTH,
        rect_height: CANVAS3D_PLANE_FILL_TEST_HEIGHT,
        canvas_width: CANVAS3D_PLANE_FILL_TEST_WIDTH,
        canvas_height: CANVAS3D_PLANE_FILL_TEST_HEIGHT,
        origin_q16: Canvas3dVec3Q16 {
            x: 0,
            y: 0,
            z: q * 2,
            pad: 0,
        },
        axis_u_q16: Canvas3dVec3Q16 {
            x: (q * 3) / 4,
            y: 0,
            z: q / 4,
            pad: 0,
        },
        axis_v_q16: Canvas3dVec3Q16 {
            x: 0,
            y: (q * 5) / 8,
            z: 0,
            pad: 0,
        },
        constraint0_q16: Canvas3dVec3Q16 {
            x: q,
            y: 0,
            z: q,
            pad: 0,
        },
        constraint1_q16: Canvas3dVec3Q16 {
            x: -q,
            y: 0,
            z: q,
            pad: 0,
        },
        constraint2_q16: Canvas3dVec3Q16 {
            x: 0,
            y: q,
            z: q,
            pad: 0,
        },
        constraint3_q16: Canvas3dVec3Q16 {
            x: 0,
            y: -q,
            z: q,
            pad: 0,
        },
        constraint_count: 4,
        color_rgba: 0xFFFF_8844,
    };

    let poison = 0x1020_3040u32;
    unsafe {
        let dst = state.canvas3d_out_virt as *mut u32;
        for index in 0..(CANVAS3D_PLANE_FILL_TEST_BYTES / core::mem::size_of::<u32>()) {
            core::ptr::write_volatile(dst.add(index), poison);
        }
    }
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);

    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let dst_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            CANVAS3D_PROJECT_OUT_GPU,
            state.canvas3d_out_phys,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_canvas3d_plane_patch_fill_cut_batch(
            state,
            upload,
            params,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
    let submit_start_tick = direct_rcs_now_tick();
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let (observed, retire_ms) = if submitted {
        direct_rcs_poll_result_slot_elapsed(
            state,
            CANVAS3D_PLANE_PATCH_FILL_CUT_POST_MARKER_SLOT,
            CANVAS3D_PLANE_PATCH_FILL_CUT_POST_MARKER,
            submit_start_tick,
        )
    } else {
        (0, 0)
    };
    let pre_marker =
        direct_rcs_read_result_slot(state, CANVAS3D_PLANE_PATCH_FILL_CUT_PRE_MARKER_SLOT);
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);

    let pitch_pixels =
        (CANVAS3D_PLANE_FILL_TEST_PITCH_BYTES / core::mem::size_of::<u32>() as u32) as usize;
    let center_index = (CANVAS3D_PLANE_FILL_TEST_HEIGHT as usize / 2) * pitch_pixels
        + CANVAS3D_PLANE_FILL_TEST_WIDTH as usize / 2;
    let corner_index = 0usize;
    let mut changed = 0usize;
    let mut colored = 0usize;
    let mut first_changed_index = usize::MAX;
    let mut first_changed = poison;
    let (center, corner) = unsafe {
        let dst = state.canvas3d_out_virt as *const u32;
        for index in 0..(CANVAS3D_PLANE_FILL_TEST_BYTES / core::mem::size_of::<u32>()) {
            let value = core::ptr::read_volatile(dst.add(index));
            if value != poison {
                changed += 1;
                if first_changed_index == usize::MAX {
                    first_changed_index = index;
                    first_changed = value;
                }
            }
            if value == params.color_rgba {
                colored += 1;
            }
        }
        (
            core::ptr::read_volatile(dst.add(center_index)),
            core::ptr::read_volatile(dst.add(corner_index)),
        )
    };

    let retired = observed == CANVAS3D_PLANE_PATCH_FILL_CUT_POST_MARKER;
    let ok = retired
        && pre_marker == CANVAS3D_PLANE_PATCH_FILL_CUT_PRE_MARKER
        && changed > 0
        && colored > 0
        && center == params.color_rgba
        && corner == poison;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: canvas3d-plane-patch-fill-cut forcewake={} ggtt={} ppgtt={} kernel_ppgtt={} dst_ppgtt={} batch={} submitted={} retired={} retire_ms={} ok={} changed={} colored={} center=0x{:08X} corner=0x{:08X} first_changed={} first_value=0x{:08X} poison=0x{:08X} dst={}x{} pitch={} rect={}x{} constraints={} canvas={}x{} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} dst_gpu=0x{:X} dst_bytes=0x{:X} idd_off=0x{:X} payload_off=0x{:X} origin=[{}, {}, {}, {}] axis_u=[{}, {}, {}, {}] axis_v=[{}, {}, {}, {}] color=0x{:08X} ring_gpu=0x{:X} batch_gpu=0x{:X} result_gpu=0x{:X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} path=direct-execlist no_guc_submit=1 next=patch-worklist-or-shade\n",
        forcewake_ok as u8,
        mapped_ok as u8,
        ppgtt_ok as u8,
        kernel_ppgtt_ok as u8,
        dst_ppgtt_ok as u8,
        batch_ok as u8,
        submitted as u8,
        retired as u8,
        retire_ms,
        ok as u8,
        changed,
        colored,
        center,
        corner,
        first_changed_index,
        first_changed,
        poison,
        params.dst_width,
        params.dst_height,
        params.dst_pitch_bytes,
        params.rect_width,
        params.rect_height,
        params.constraint_count,
        params.canvas_width,
        params.canvas_height,
        pre_marker,
        observed,
        CANVAS3D_PLANE_PATCH_FILL_CUT_POST_MARKER,
        upload.gpu,
        upload.gpu + CANVAS3D_PLANE_PATCH_FILL_CUT_RGBA8_TEXT_OFFSET_BYTES,
        CANVAS3D_PROJECT_OUT_GPU,
        CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        CANVAS3D_PLANE_FILL_IDD_OFFSET_BYTES,
        CANVAS3D_PLANE_FILL_PAYLOAD_OFFSET_BYTES,
        params.origin_q16.x,
        params.origin_q16.y,
        params.origin_q16.z,
        params.origin_q16.pad,
        params.axis_u_q16.x,
        params.axis_u_q16.y,
        params.axis_u_q16.z,
        params.axis_u_q16.pad,
        params.axis_v_q16.x,
        params.axis_v_q16.y,
        params.axis_v_q16.z,
        params.axis_v_q16.pad,
        params.color_rgba,
        DIRECT_RCS_GPU_VA_RING_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_RESULT_BASE,
        intel::mmio_read(dev, RCS_RING_HEAD),
        intel::mmio_read(dev, RCS_RING_TAIL),
        intel::mmio_read(dev, RCS_RING_ACTHD),
        intel::mmio_read(dev, RCS_RING_IPEIR),
        intel::mmio_read(dev, RCS_RING_IPEHR),
        intel::mmio_read(dev, RCS_RING_EIR),
    );

    ok
}

pub(crate) fn submit_canvas3d_plane_patch_worklist_rgba8_once() -> bool {
    if !DIRECT_RCS_ENABLED || CANVAS3D_PLANE_PATCH_WORKLIST_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }

    let Some(dev) = intel::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-patch-worklist skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-patch-worklist failed rung=alloc\n"
        );
        return false;
    };
    let Some(upload) = upload_canvas3d_plane_patch_worklist_rgba8_kernel() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-patch-worklist skipped reason=no-kernel-upload\n"
        );
        return false;
    };
    if CANVAS3D_PLANE_FILL_TEST_BYTES > CANVAS3D_PROJECT_OUT_ALLOC_BYTES
        || CANVAS3D_PLANE_PATCH_WORKLIST_DESC_BYTES > CANVAS3D_PROJECT_OUT_ALLOC_BYTES
        || core::mem::size_of::<Canvas3dPlanePatchWorklistRgba8Desc>()
            != CANVAS3D_PLANE_PATCH_WORKLIST_DESC_DWORDS * core::mem::size_of::<u32>()
    {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-plane-patch-worklist failed rung=buffer-cap dst_bytes=0x{:X} desc_bytes=0x{:X} cap=0x{:X} desc_struct=0x{:X}\n",
            CANVAS3D_PLANE_FILL_TEST_BYTES,
            CANVAS3D_PLANE_PATCH_WORKLIST_DESC_BYTES,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
            core::mem::size_of::<Canvas3dPlanePatchWorklistRgba8Desc>(),
        );
        return false;
    }

    let q = CANVAS3D_PROJECT_Q16_ONE;
    let square_constraints = (
        Canvas3dVec3Q16 {
            x: q,
            y: 0,
            z: q,
            pad: 0,
        },
        Canvas3dVec3Q16 {
            x: -q,
            y: 0,
            z: q,
            pad: 0,
        },
        Canvas3dVec3Q16 {
            x: 0,
            y: q,
            z: q,
            pad: 0,
        },
        Canvas3dVec3Q16 {
            x: 0,
            y: -q,
            z: q,
            pad: 0,
        },
    );
    let make_desc = |origin_x: i32,
                     origin_y: i32,
                     rect_x: u32,
                     rect_y: u32,
                     rect_width: u32,
                     rect_height: u32,
                     color_rgba: u32|
     -> Canvas3dPlanePatchWorklistRgba8Desc {
        Canvas3dPlanePatchWorklistRgba8Desc {
            dst_pitch_bytes: CANVAS3D_PLANE_FILL_TEST_PITCH_BYTES,
            dst_width: CANVAS3D_PLANE_FILL_TEST_WIDTH,
            dst_height: CANVAS3D_PLANE_FILL_TEST_HEIGHT,
            rect_x,
            rect_y,
            rect_width,
            rect_height,
            canvas_width: CANVAS3D_PLANE_FILL_TEST_WIDTH,
            canvas_height: CANVAS3D_PLANE_FILL_TEST_HEIGHT,
            reserved0: 0,
            origin_q16: Canvas3dVec3Q16 {
                x: origin_x,
                y: origin_y,
                z: q * 2,
                pad: 0,
            },
            axis_u_q16: Canvas3dVec3Q16 {
                x: (q * 3) / 4,
                y: 0,
                z: q / 4,
                pad: 0,
            },
            axis_v_q16: Canvas3dVec3Q16 {
                x: 0,
                y: (q * 5) / 8,
                z: 0,
                pad: 0,
            },
            constraint0_q16: square_constraints.0,
            constraint1_q16: square_constraints.1,
            constraint2_q16: square_constraints.2,
            constraint3_q16: square_constraints.3,
            constraint_count: 4,
            color_rgba,
        }
    };

    let descs = [
        make_desc(-q, 0, 0, 0, 32, 48, 0xFFFF_3048),
        make_desc(q, 0, 32, 0, 32, 48, 0xFF40_D060),
        make_desc(0, -q / 2, 16, 12, 32, 24, 0xFF44_88FF),
    ];

    let poison = 0x1020_3040u32;
    unsafe {
        let dst = state.canvas3d_out_virt as *mut u32;
        for index in 0..(CANVAS3D_PLANE_FILL_TEST_BYTES / core::mem::size_of::<u32>()) {
            core::ptr::write_volatile(dst.add(index), poison);
        }

        core::ptr::write_bytes(
            state.canvas3d_tmp_virt,
            0,
            CANVAS3D_PLANE_PATCH_WORKLIST_DESC_BYTES,
        );
        let desc_dst = state.canvas3d_tmp_virt as *mut Canvas3dPlanePatchWorklistRgba8Desc;
        for (index, desc) in descs.iter().copied().enumerate() {
            core::ptr::write_volatile(desc_dst.add(index), desc);
        }
    }
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    intel::dma_flush(state.canvas3d_tmp_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);

    let params = Canvas3dPlanePatchWorklistRgba8Params {
        dst_gpu: CANVAS3D_PROJECT_OUT_GPU,
        desc_gpu: CANVAS3D_TMP_GPU,
        desc_base: 0,
        desc_count: CANVAS3D_PLANE_PATCH_WORKLIST_TEST_DESCS as u32,
    };

    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let dst_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            CANVAS3D_PROJECT_OUT_GPU,
            state.canvas3d_out_phys,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
    let desc_ppgtt_ok = dst_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            CANVAS3D_TMP_GPU,
            state.canvas3d_tmp_phys,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
    let batch_ok = desc_ppgtt_ok
        && direct_rcs_encode_canvas3d_plane_patch_worklist_batch(
            state,
            upload,
            params,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
            CANVAS3D_PLANE_PATCH_WORKLIST_DESC_BYTES,
        );
    let submit_start_tick = direct_rcs_now_tick();
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let (observed, retire_ms) = if submitted {
        direct_rcs_poll_result_slot_elapsed(
            state,
            CANVAS3D_PLANE_PATCH_WORKLIST_POST_MARKER_SLOT,
            CANVAS3D_PLANE_PATCH_WORKLIST_POST_MARKER,
            submit_start_tick,
        )
    } else {
        (0, 0)
    };
    let pre_marker =
        direct_rcs_read_result_slot(state, CANVAS3D_PLANE_PATCH_WORKLIST_PRE_MARKER_SLOT);
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);

    let mut changed = 0usize;
    let mut red = 0usize;
    let mut green = 0usize;
    let mut blue = 0usize;
    let mut first_changed_index = usize::MAX;
    let mut first_changed = poison;
    let corner = unsafe {
        let dst = state.canvas3d_out_virt as *const u32;
        for index in 0..(CANVAS3D_PLANE_FILL_TEST_BYTES / core::mem::size_of::<u32>()) {
            let value = core::ptr::read_volatile(dst.add(index));
            if value != poison {
                changed += 1;
                if first_changed_index == usize::MAX {
                    first_changed_index = index;
                    first_changed = value;
                }
            }
            if value == descs[0].color_rgba {
                red += 1;
            } else if value == descs[1].color_rgba {
                green += 1;
            } else if value == descs[2].color_rgba {
                blue += 1;
            }
        }
        core::ptr::read_volatile(dst)
    };

    let retired = observed == CANVAS3D_PLANE_PATCH_WORKLIST_POST_MARKER;
    let ok = retired
        && pre_marker == CANVAS3D_PLANE_PATCH_WORKLIST_PRE_MARKER
        && changed > 0
        && red > 0
        && green > 0
        && blue > 0
        && corner == poison;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: canvas3d-plane-patch-worklist forcewake={} ggtt={} ppgtt={} kernel_ppgtt={} dst_ppgtt={} desc_ppgtt={} batch={} submitted={} retired={} retire_ms={} ok={} changed={} red={} green={} blue={} corner=0x{:08X} first_changed={} first_value=0x{:08X} poison=0x{:08X} dst={}x{} pitch={} descs={} desc_dwords={} desc_bytes=0x{:X} canvas={}x{} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} dst_gpu=0x{:X} desc_gpu=0x{:X} idd_off=0x{:X} payload_off=0x{:X} ring_gpu=0x{:X} batch_gpu=0x{:X} result_gpu=0x{:X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} path=direct-execlist no_guc_submit=1 next=patch-worklist-shade-or-depth\n",
        forcewake_ok as u8,
        mapped_ok as u8,
        ppgtt_ok as u8,
        kernel_ppgtt_ok as u8,
        dst_ppgtt_ok as u8,
        desc_ppgtt_ok as u8,
        batch_ok as u8,
        submitted as u8,
        retired as u8,
        retire_ms,
        ok as u8,
        changed,
        red,
        green,
        blue,
        corner,
        first_changed_index,
        first_changed,
        poison,
        CANVAS3D_PLANE_FILL_TEST_WIDTH,
        CANVAS3D_PLANE_FILL_TEST_HEIGHT,
        CANVAS3D_PLANE_FILL_TEST_PITCH_BYTES,
        params.desc_count,
        CANVAS3D_PLANE_PATCH_WORKLIST_DESC_DWORDS,
        CANVAS3D_PLANE_PATCH_WORKLIST_DESC_BYTES,
        CANVAS3D_PLANE_FILL_TEST_WIDTH,
        CANVAS3D_PLANE_FILL_TEST_HEIGHT,
        pre_marker,
        observed,
        CANVAS3D_PLANE_PATCH_WORKLIST_POST_MARKER,
        upload.gpu,
        upload.gpu + CANVAS3D_PLANE_PATCH_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        CANVAS3D_PROJECT_OUT_GPU,
        CANVAS3D_TMP_GPU,
        CANVAS3D_PLANE_FILL_IDD_OFFSET_BYTES,
        CANVAS3D_PLANE_FILL_PAYLOAD_OFFSET_BYTES,
        DIRECT_RCS_GPU_VA_RING_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_RESULT_BASE,
        intel::mmio_read(dev, RCS_RING_HEAD),
        intel::mmio_read(dev, RCS_RING_TAIL),
        intel::mmio_read(dev, RCS_RING_ACTHD),
        intel::mmio_read(dev, RCS_RING_IPEIR),
        intel::mmio_read(dev, RCS_RING_IPEHR),
        intel::mmio_read(dev, RCS_RING_EIR),
    );

    ok
}

pub(crate) fn shell_cube20_project_spin(
    duration_ms: u64,
    cadence_us: u64,
) -> Option<GpgpuShellCube20ProjectResult> {
    let total_start_tick = direct_rcs_now_tick();
    let duration_ms = duration_ms.clamp(1, CUBE20_PROJECT_MAX_DURATION_MS);
    let cadence_us = if cadence_us == 0 {
        CUBE20_PROJECT_DEFAULT_CADENCE_US
    } else {
        cadence_us.clamp(CUBE20_PROJECT_MIN_CADENCE_US, CUBE20_PROJECT_MAX_CADENCE_US)
    };
    let Some(dev) = intel::claimed_device() else {
        return None;
    };
    let Some(project_upload) = upload_canvas3d_project_rgba8_kernel() else {
        return None;
    };
    let Some(transform_upload) = upload_canvas3d_transform_q16_kernel() else {
        return None;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return None;
    };
    let target = intel::display::primary_surface_gpgpu_marker_target()?;
    if target.virt.is_null()
        || target.width == 0
        || target.height == 0
        || CANVAS3D_PROJECT_TEST_BYTES > CLEAR_RECT_TEST_BYTES
        || CANVAS3D_PROJECT_OUT_BYTES > CANVAS3D_PROJECT_OUT_ALLOC_BYTES
    {
        return None;
    }

    let canvas_x = 0;
    let canvas_y = 0;
    let canvas_xy = GpgpuPoint::new(canvas_x as i32, canvas_y as i32);
    let flush_offset = 0;
    let flush_bytes = (target.height as usize)
        .saturating_sub(1)
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add((target.width as usize).saturating_mul(core::mem::size_of::<u32>()));
    let deadline_tick = total_start_tick.saturating_add(direct_rcs_ticks_from_ms(duration_ms));
    let cadence_ticks = direct_rcs_ticks_from_us(cadence_us);
    let mut next_tick = total_start_tick;
    let mut frames = 0u32;
    let mut submitted = 0u32;
    let mut presented = 0u32;
    let mut visible_points = 0usize;
    let mut stamped_pixels = 0usize;
    let mut total_submit_ms = 0u64;
    let mut max_submit_ms = 0u64;
    let mut angle_deg = 0u32;

    while direct_rcs_now_tick() < deadline_tick {
        while direct_rcs_now_tick() < next_tick {
            core::hint::spin_loop();
        }
        if direct_rcs_now_tick() >= deadline_tick {
            break;
        }

        shell_canvas3d_seed_visual_vertices(state);
        let translate_x_q16 = shell_cube20_translate_x_q16(frames);
        let translate_y_q16 = shell_tetra10_translate_y_q16(frames);
        let y_spin_half_deg = (angle_deg / 2) % 180;
        let cube_rotate_q16 = shell_canvas3d_y_quat_q16(y_spin_half_deg, true);
        let tetra_rotate_q16 = shell_canvas3d_y_quat_q16(y_spin_half_deg, false);
        let scene_scale_q16 =
            shell_canvas3d_cube_scale_pulse_q16(direct_rcs_elapsed_ms_since(total_start_tick));
        let Some(cube_transform_ms) = submit_canvas3d_transform_fused_frame_range(
            dev,
            state,
            transform_upload,
            DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
            state.clear_test_phys,
            CANVAS3D_TMP_GPU,
            state.canvas3d_tmp_phys,
            0,
            0,
            CUBE20_VISUAL_VERTEX_COUNT as u32,
            Canvas3dVec3Q16 {
                x: scene_scale_q16,
                y: scene_scale_q16,
                z: scene_scale_q16,
                pad: 0,
            },
            Canvas3dVec3Q16 {
                x: cube_rotate_q16.x,
                y: cube_rotate_q16.y,
                z: cube_rotate_q16.z,
                pad: cube_rotate_q16.pad,
            },
            Canvas3dVec3Q16 {
                x: translate_x_q16,
                y: 0,
                z: CANVAS3D_PROJECT_Q16_ONE * 2,
                pad: 0,
            },
            CANVAS3D_TRANSFORM_FUSED_PRE_MARKER,
            CANVAS3D_TRANSFORM_FUSED_POST_MARKER,
        ) else {
            break;
        };
        let Some(tetra_transform_ms) = submit_canvas3d_transform_fused_frame_range(
            dev,
            state,
            transform_upload,
            DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
            state.clear_test_phys,
            CANVAS3D_TMP_GPU,
            state.canvas3d_tmp_phys,
            TETRA10_BASE_VERTEX as u32,
            TETRA10_BASE_VERTEX as u32,
            TETRA10_VERTEX_COUNT as u32,
            Canvas3dVec3Q16 {
                x: scene_scale_q16,
                y: scene_scale_q16,
                z: scene_scale_q16,
                pad: 0,
            },
            Canvas3dVec3Q16 {
                x: tetra_rotate_q16.x,
                y: tetra_rotate_q16.y,
                z: tetra_rotate_q16.z,
                pad: tetra_rotate_q16.pad,
            },
            Canvas3dVec3Q16 {
                x: CANVAS3D_PROJECT_Q16_ONE,
                y: translate_y_q16,
                z: CANVAS3D_PROJECT_Q16_ONE * 2,
                pad: 0,
            },
            CANVAS3D_TRANSFORM_FUSED_PRE_MARKER,
            CANVAS3D_TRANSFORM_FUSED_POST_MARKER,
        ) else {
            break;
        };
        let Some(project_ms) = submit_canvas3d_project_frame_from(
            dev,
            state,
            project_upload,
            CANVAS3D_TMP_GPU,
            state.canvas3d_tmp_phys,
            CANVAS3D_VISUAL_VERTEX_COUNT as u32,
            target.width,
            target.height,
        ) else {
            break;
        };
        submitted = submitted.saturating_add(3);
        let submit_ms = cube_transform_ms
            .saturating_add(tetra_transform_ms)
            .saturating_add(project_ms);
        total_submit_ms = total_submit_ms.saturating_add(submit_ms);
        max_submit_ms = max_submit_ms
            .max(cube_transform_ms)
            .max(tetra_transform_ms)
            .max(project_ms);

        let (frame_visible, frame_stamped) =
            shell_canvas3d_cpu_copy_projected_points_to_primary(state, target, canvas_x, canvas_y);
        visible_points = frame_visible;
        stamped_pixels = frame_stamped;
        if intel::display::notify_primary_surface_external_write(
            "gpgpu-cube20-project",
            flush_offset,
            flush_bytes,
        ) {
            presented = presented.saturating_add(1);
        }

        frames = frames.saturating_add(1);
        angle_deg = angle_deg.wrapping_add(2) % 360;
        next_tick = next_tick.saturating_add(cadence_ticks);
    }

    let elapsed_ms = direct_rcs_elapsed_ms_since(total_start_tick);
    let expected_frame_submits = 3;
    Some(GpgpuShellCube20ProjectResult {
        ok: frames != 0
            && submitted == frames.saturating_mul(expected_frame_submits)
            && presented != 0,
        frames,
        submitted,
        presented,
        visible_points,
        stamped_pixels,
        duration_ms,
        elapsed_ms,
        cadence_us,
        total_submit_ms,
        max_submit_ms,
        primary_width: target.width,
        primary_height: target.height,
        canvas_xy,
        vertex_count: CANVAS3D_VISUAL_VERTEX_COUNT,
        radius_px: (CUBE20_SEED_HALF_Q16 as u32).saturating_mul(target.width.min(target.height))
            / CANVAS3D_PROJECT_Q16_ONE as u32,
        last_angle_deg: angle_deg,
    })
}

pub(crate) fn ui2_canvas3d_archaeology_project_frame(
    frame: u32,
) -> Option<GpgpuShellCube20ProjectResult> {
    let target = intel::display::primary_surface_gpgpu_marker_target()?;
    ui2_canvas3d_archaeology_project_frame_in_rect(frame, 0, 0, target.width, target.height)
}

pub(crate) fn ui2_canvas3d_archaeology_project_frame_in_rect(
    frame: u32,
    rect_x: i32,
    rect_y: i32,
    rect_w: u32,
    rect_h: u32,
) -> Option<GpgpuShellCube20ProjectResult> {
    const CADENCE_US: u64 = 33_000;

    let total_start_tick = direct_rcs_now_tick();
    let Some(dev) = intel::claimed_device() else {
        return None;
    };
    let Some(project_upload) = upload_canvas3d_project_rgba8_kernel() else {
        return None;
    };
    let Some(transform_upload) = upload_canvas3d_transform_q16_kernel() else {
        return None;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return None;
    };
    let target = intel::display::primary_surface_gpgpu_marker_target()?;
    if target.virt.is_null()
        || target.width == 0
        || target.height == 0
        || CANVAS3D_PROJECT_TEST_BYTES > CLEAR_RECT_TEST_BYTES
        || CANVAS3D_PROJECT_OUT_BYTES > CANVAS3D_PROJECT_OUT_ALLOC_BYTES
    {
        return None;
    }

    let canvas_x = rect_x.max(0) as u32;
    let canvas_y = rect_y.max(0) as u32;
    if canvas_x >= target.width || canvas_y >= target.height {
        return None;
    }
    let canvas_width = rect_w.min(target.width.saturating_sub(canvas_x));
    let canvas_height = rect_h.min(target.height.saturating_sub(canvas_y));
    if canvas_width == 0 || canvas_height == 0 {
        return None;
    }
    let canvas_xy = GpgpuPoint::new(canvas_x as i32, canvas_y as i32);
    let flush_offset = (canvas_y as usize)
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add((canvas_x as usize).saturating_mul(core::mem::size_of::<u32>()));
    let flush_bytes = (canvas_height as usize)
        .saturating_sub(1)
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add((canvas_width as usize).saturating_mul(core::mem::size_of::<u32>()));

    shell_canvas3d_seed_archaeology_vertices(state);
    let y_spin_half_deg = frame % 180;
    let rotate_q16 = shell_canvas3d_y_quat_q16(y_spin_half_deg, true);
    let scene_scale_q16 = shell_canvas3d_cube_scale_pulse_q16(u64::from(frame) * 33);
    let transform_ms = submit_canvas3d_transform_fused_frame_range(
        dev,
        state,
        transform_upload,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        state.clear_test_phys,
        CANVAS3D_TMP_GPU,
        state.canvas3d_tmp_phys,
        0,
        0,
        ICO90_VERTEX_COUNT as u32,
        Canvas3dVec3Q16 {
            x: scene_scale_q16,
            y: scene_scale_q16,
            z: scene_scale_q16,
            pad: 0,
        },
        rotate_q16,
        Canvas3dVec3Q16 {
            x: 0,
            y: 0,
            z: CANVAS3D_PROJECT_Q16_ONE * 2,
            pad: 0,
        },
        CANVAS3D_TRANSFORM_FUSED_PRE_MARKER,
        CANVAS3D_TRANSFORM_FUSED_POST_MARKER,
    )?;
    let project_ms = submit_canvas3d_project_frame_from(
        dev,
        state,
        project_upload,
        CANVAS3D_TMP_GPU,
        state.canvas3d_tmp_phys,
        ICO90_VERTEX_COUNT as u32,
        canvas_width,
        canvas_height,
    )?;
    let (visible_points, stamped_pixels) =
        shell_canvas3d_cpu_copy_projected_points_to_primary_count(
            state,
            target,
            canvas_x,
            canvas_y,
            canvas_width,
            canvas_height,
            ICO90_VERTEX_COUNT,
            true,
        );
    let presented = intel::display::notify_primary_surface_external_write(
        "ui2-intel-canvas3d-demo",
        flush_offset,
        flush_bytes,
    ) as u32;
    let submitted = 2;
    let total_submit_ms = transform_ms.saturating_add(project_ms);

    Some(GpgpuShellCube20ProjectResult {
        ok: presented != 0 && visible_points != 0,
        frames: 1,
        submitted,
        presented,
        visible_points,
        stamped_pixels,
        duration_ms: 0,
        elapsed_ms: direct_rcs_elapsed_ms_since(total_start_tick),
        cadence_us: CADENCE_US,
        total_submit_ms,
        max_submit_ms: transform_ms.max(project_ms),
        primary_width: target.width,
        primary_height: target.height,
        canvas_xy,
        vertex_count: ICO90_VERTEX_COUNT,
        radius_px: (CUBE20_SEED_HALF_Q16 as u32).saturating_mul(canvas_width.min(canvas_height))
            / CANVAS3D_PROJECT_Q16_ONE as u32,
        last_angle_deg: frame.wrapping_mul(2) % 360,
    })
}

pub(crate) fn ui2_canvas3d_archaeology_project_texture_frame(
    frame: u32,
    width: u32,
    height: u32,
) -> Option<GpgpuCanvas3dUi2TextureFrame> {
    const CADENCE_US: u64 = 33_000;

    let width = width.max(1);
    let height = height.max(1);
    let pixel_count = (width as usize).checked_mul(height as usize)?;
    let byte_count = pixel_count.checked_mul(core::mem::size_of::<u32>())?;
    let total_start_tick = direct_rcs_now_tick();
    let Some(dev) = intel::claimed_device() else {
        return None;
    };
    let Some(project_upload) = upload_canvas3d_project_rgba8_kernel() else {
        return None;
    };
    let Some(transform_upload) = upload_canvas3d_transform_q16_kernel() else {
        return None;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return None;
    };
    if CANVAS3D_PROJECT_TEST_BYTES > CLEAR_RECT_TEST_BYTES
        || CANVAS3D_PROJECT_OUT_BYTES > CANVAS3D_PROJECT_OUT_ALLOC_BYTES
    {
        return None;
    }

    shell_canvas3d_seed_archaeology_vertices(state);
    let y_spin_half_deg = frame % 180;
    let rotate_q16 = shell_canvas3d_y_quat_q16(y_spin_half_deg, true);
    let scene_scale_q16 = shell_canvas3d_cube_scale_pulse_q16(u64::from(frame) * 33);
    let transform_ms = submit_canvas3d_transform_fused_frame_range(
        dev,
        state,
        transform_upload,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        state.clear_test_phys,
        CANVAS3D_TMP_GPU,
        state.canvas3d_tmp_phys,
        0,
        0,
        ICO90_VERTEX_COUNT as u32,
        Canvas3dVec3Q16 {
            x: scene_scale_q16,
            y: scene_scale_q16,
            z: scene_scale_q16,
            pad: 0,
        },
        rotate_q16,
        Canvas3dVec3Q16 {
            x: 0,
            y: 0,
            z: CANVAS3D_PROJECT_Q16_ONE * 2,
            pad: 0,
        },
        CANVAS3D_TRANSFORM_FUSED_PRE_MARKER,
        CANVAS3D_TRANSFORM_FUSED_POST_MARKER,
    )?;
    let project_ms = submit_canvas3d_project_frame_from(
        dev,
        state,
        project_upload,
        CANVAS3D_TMP_GPU,
        state.canvas3d_tmp_phys,
        ICO90_VERTEX_COUNT as u32,
        width,
        height,
    )?;
    let mut rgba = vec![0u8; byte_count];
    let (visible_points, stamped_pixels) = shell_canvas3d_copy_projected_points_to_rgba8(
        state,
        width as usize,
        height as usize,
        ICO90_VERTEX_COUNT,
        true,
        rgba.as_mut_slice(),
    );
    let submitted = 2;
    let total_submit_ms = transform_ms.saturating_add(project_ms);
    let result = GpgpuShellCube20ProjectResult {
        ok: visible_points != 0 && stamped_pixels != 0,
        frames: 1,
        submitted,
        presented: 0,
        visible_points,
        stamped_pixels,
        duration_ms: 0,
        elapsed_ms: direct_rcs_elapsed_ms_since(total_start_tick),
        cadence_us: CADENCE_US,
        total_submit_ms,
        max_submit_ms: transform_ms.max(project_ms),
        primary_width: width,
        primary_height: height,
        canvas_xy: GpgpuPoint::new(0, 0),
        vertex_count: ICO90_VERTEX_COUNT,
        radius_px: (CUBE20_SEED_HALF_Q16 as u32).saturating_mul(width.min(height))
            / CANVAS3D_PROJECT_Q16_ONE as u32,
        last_angle_deg: frame.wrapping_mul(2) % 360,
    };

    Some(GpgpuCanvas3dUi2TextureFrame {
        result,
        width,
        height,
        rgba,
    })
}

pub(crate) fn ui2_canvas3d_plane_patch_texture_frame(
    frame: u32,
    width: u32,
    height: u32,
) -> Option<GpgpuCanvas3dUi2TextureFrame> {
    const CADENCE_US: u64 = 33_000;

    let width = width.clamp(1, 512);
    let height = height.clamp(1, 512);
    let pixel_count = (width as usize).checked_mul(height as usize)?;
    let byte_count = pixel_count.checked_mul(core::mem::size_of::<u32>())?;
    let total_start_tick = direct_rcs_now_tick();
    let Some(dev) = intel::claimed_device() else {
        return None;
    };
    let Some(upload) = upload_canvas3d_plane_patch_worklist_rgba8_kernel() else {
        return None;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return None;
    };
    let Some(staging) = present_staging_surface_once(width, height) else {
        return None;
    };
    if byte_count > staging.surface.bytes {
        return None;
    }

    unsafe {
        let dst = staging.virt as *mut u32;
        for y in 0..height as usize {
            let row = dst.add(y * width as usize);
            for x in 0..width as usize {
                let checker = (((x >> 5) ^ (y >> 5)) & 1) as u32;
                let shade = if checker == 0 { 0x10 } else { 0x18 };
                core::ptr::write_volatile(
                    row.add(x),
                    0xFF00_0000 | (shade << 16) | (shade << 8) | shade,
                );
            }
        }
    }
    intel::dma_flush(staging.virt, byte_count);

    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let dst_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            staging.surface.gpu,
            staging.surface.phys,
            staging.surface.bytes,
        );
    let mut submitted_count = 0u32;
    let mut retired_count = 0u32;
    let mut total_submit_ms = 0u64;
    let mut max_submit_ms = 0u64;
    let q = CANVAS3D_PROJECT_Q16_ONE;
    let half = (q * 5) / 8;
    if dst_ppgtt_ok {
        let center = Canvas3dVec3Q16 {
            x: 0,
            y: 0,
            z: (q * 11) / 4,
            pad: 0,
        };
        let right = Canvas3dVec3Q16 {
            x: (half * 82) / 100,
            y: 0,
            z: -(half * 36) / 100,
            pad: 0,
        };
        let up = Canvas3dVec3Q16 {
            x: -(half * 10) / 100,
            y: (half * 94) / 100,
            z: -(half * 22) / 100,
            pad: 0,
        };
        let forward = Canvas3dVec3Q16 {
            x: (half * 34) / 100,
            y: (half * 24) / 100,
            z: (half * 91) / 100,
            pad: 0,
        };
        let angle_deg = frame.wrapping_mul(4) % 360;
        let roll_deg = frame % 360;
        let right =
            canvas3d_vec3_rotate_z_q16(canvas3d_vec3_rotate_y_q16(right, angle_deg), roll_deg);
        let up = canvas3d_vec3_rotate_z_q16(canvas3d_vec3_rotate_y_q16(up, angle_deg), roll_deg);
        let forward =
            canvas3d_vec3_rotate_z_q16(canvas3d_vec3_rotate_y_q16(forward, angle_deg), roll_deg);
        let constraints = [
            Canvas3dVec3Q16 {
                x: q,
                y: 0,
                z: q,
                pad: 0,
            },
            Canvas3dVec3Q16 {
                x: -q,
                y: 0,
                z: q,
                pad: 0,
            },
            Canvas3dVec3Q16 {
                x: 0,
                y: q,
                z: q,
                pad: 0,
            },
            Canvas3dVec3Q16 {
                x: 0,
                y: -q,
                z: q,
                pad: 0,
            },
        ];
        let face_descs = [
            canvas3d_plane_patch_ui2_face_desc(
                staging,
                width,
                height,
                canvas3d_vec3_add(center, forward),
                right,
                up,
                constraints,
                0xFF22_3355,
            ),
            canvas3d_plane_patch_ui2_face_desc(
                staging,
                width,
                height,
                canvas3d_vec3_sub(center, right),
                forward,
                up,
                constraints,
                0xFF2F_80ED,
            ),
            canvas3d_plane_patch_ui2_face_desc(
                staging,
                width,
                height,
                canvas3d_vec3_sub(center, up),
                right,
                forward,
                constraints,
                0xFF36_4A58,
            ),
            canvas3d_plane_patch_ui2_face_desc(
                staging,
                width,
                height,
                canvas3d_vec3_add(center, right),
                up,
                forward,
                constraints,
                0xFFFF_8844,
            ),
            canvas3d_plane_patch_ui2_face_desc(
                staging,
                width,
                height,
                canvas3d_vec3_add(center, up),
                forward,
                right,
                constraints,
                0xFFFF_D166,
            ),
            canvas3d_plane_patch_ui2_face_desc(
                staging,
                width,
                height,
                canvas3d_vec3_sub(center, forward),
                up,
                right,
                constraints,
                0xFF66_CCFF,
            ),
        ];
        unsafe {
            core::ptr::write_bytes(
                state.canvas3d_tmp_virt,
                0,
                CANVAS3D_PLANE_PATCH_WORKLIST_DESC_BYTES,
            );
            let desc_dst = state.canvas3d_tmp_virt as *mut Canvas3dPlanePatchWorklistRgba8Desc;
            for (index, desc) in face_descs.iter().copied().enumerate() {
                core::ptr::write_volatile(desc_dst.add(index), desc);
            }
        }
        intel::dma_flush(state.canvas3d_tmp_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);

        let desc_ppgtt_ok = direct_rcs_map_ppgtt_kernel(
            state,
            CANVAS3D_TMP_GPU,
            state.canvas3d_tmp_phys,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
        let params = Canvas3dPlanePatchWorklistRgba8Params {
            dst_gpu: staging.surface.gpu,
            desc_gpu: CANVAS3D_TMP_GPU,
            desc_base: 0,
            desc_count: face_descs.len() as u32,
        };
        let batch_ok = desc_ppgtt_ok
            && direct_rcs_encode_canvas3d_plane_patch_worklist_batch(
                state,
                upload,
                params,
                staging.surface.bytes,
                CANVAS3D_PLANE_PATCH_WORKLIST_DESC_BYTES,
            );
        let submit_start_tick = direct_rcs_now_tick();
        let submitted_ok = batch_ok && direct_rcs_submit_batch(dev, state);
        if submitted_ok {
            submitted_count = submitted_count.saturating_add(1);
        }
        let (observed, submit_ms) = if submitted_ok {
            direct_rcs_poll_result_slot_elapsed(
                state,
                CANVAS3D_PLANE_PATCH_WORKLIST_POST_MARKER_SLOT,
                CANVAS3D_PLANE_PATCH_WORKLIST_POST_MARKER,
                submit_start_tick,
            )
        } else {
            (0, 0)
        };
        total_submit_ms = total_submit_ms.saturating_add(submit_ms);
        max_submit_ms = max_submit_ms.max(submit_ms);
        let pre_marker =
            direct_rcs_read_result_slot(state, CANVAS3D_PLANE_PATCH_WORKLIST_PRE_MARKER_SLOT);
        if observed == CANVAS3D_PLANE_PATCH_WORKLIST_POST_MARKER
            && pre_marker == CANVAS3D_PLANE_PATCH_WORKLIST_PRE_MARKER
        {
            retired_count = retired_count.saturating_add(1);
        }
    }
    intel::dma_flush(staging.virt, byte_count);

    let mut rgba = vec![0u8; byte_count];
    let mut colored = 0usize;
    unsafe {
        let src = staging.virt as *const u8;
        core::ptr::copy_nonoverlapping(src, rgba.as_mut_ptr(), byte_count);
        let pixels = staging.virt as *const u32;
        for y in 0..height as usize {
            for x in 0..width as usize {
                let index = y * width as usize + x;
                let checker = (((x >> 5) ^ (y >> 5)) & 1) as u32;
                let shade = if checker == 0 { 0x10 } else { 0x18 };
                let background = 0xFF00_0000 | (shade << 16) | (shade << 8) | shade;
                if core::ptr::read_volatile(pixels.add(index)) != background {
                    colored += 1;
                }
            }
        }
    }
    let retired = submitted_count > 0 && submitted_count == retired_count;
    let elapsed_ms = direct_rcs_elapsed_ms_since(total_start_tick);
    let result = GpgpuShellCube20ProjectResult {
        ok: retired && colored > 0,
        frames: 1,
        submitted: submitted_count,
        presented: 0,
        visible_points: colored,
        stamped_pixels: colored,
        duration_ms: 0,
        elapsed_ms,
        cadence_us: CADENCE_US,
        total_submit_ms,
        max_submit_ms,
        primary_width: width,
        primary_height: height,
        canvas_xy: GpgpuPoint::new(0, 0),
        vertex_count: 6,
        radius_px: (half as u32).saturating_mul(width.min(height))
            / CANVAS3D_PROJECT_Q16_ONE as u32,
        last_angle_deg: frame.wrapping_mul(4) % 360,
    };

    Some(GpgpuCanvas3dUi2TextureFrame {
        result,
        width,
        height,
        rgba,
    })
}

fn canvas3d_vec3_add(a: Canvas3dVec3Q16, b: Canvas3dVec3Q16) -> Canvas3dVec3Q16 {
    Canvas3dVec3Q16 {
        x: a.x.saturating_add(b.x),
        y: a.y.saturating_add(b.y),
        z: a.z.saturating_add(b.z),
        pad: 0,
    }
}

fn canvas3d_vec3_sub(a: Canvas3dVec3Q16, b: Canvas3dVec3Q16) -> Canvas3dVec3Q16 {
    Canvas3dVec3Q16 {
        x: a.x.saturating_sub(b.x),
        y: a.y.saturating_sub(b.y),
        z: a.z.saturating_sub(b.z),
        pad: 0,
    }
}

fn canvas3d_sin_360_q16(deg: u32) -> i32 {
    let deg = deg % 360;
    if deg < 180 {
        shell_canvas3d_sin_0_180_q16(deg)
    } else {
        -shell_canvas3d_sin_0_180_q16(deg - 180)
    }
}

fn canvas3d_cos_360_q16(deg: u32) -> i32 {
    canvas3d_sin_360_q16(deg.wrapping_add(90))
}

fn canvas3d_vec3_rotate_y_q16(vertex: Canvas3dVec3Q16, deg: u32) -> Canvas3dVec3Q16 {
    let sin = canvas3d_sin_360_q16(deg) as i64;
    let cos = canvas3d_cos_360_q16(deg) as i64;
    let x = vertex.x as i64;
    let z = vertex.z as i64;
    Canvas3dVec3Q16 {
        x: (((x * cos) + (z * sin)) >> 16) as i32,
        y: vertex.y,
        z: (((z * cos) - (x * sin)) >> 16) as i32,
        pad: 0,
    }
}

fn canvas3d_vec3_rotate_z_q16(vertex: Canvas3dVec3Q16, deg: u32) -> Canvas3dVec3Q16 {
    let sin = canvas3d_sin_360_q16(deg) as i64;
    let cos = canvas3d_cos_360_q16(deg) as i64;
    let x = vertex.x as i64;
    let y = vertex.y as i64;
    Canvas3dVec3Q16 {
        x: (((x * cos) - (y * sin)) >> 16) as i32,
        y: (((x * sin) + (y * cos)) >> 16) as i32,
        z: vertex.z,
        pad: 0,
    }
}

fn canvas3d_plane_patch_ui2_face_desc(
    staging: GpgpuPresentStagingSurface,
    width: u32,
    height: u32,
    origin_q16: Canvas3dVec3Q16,
    axis_u_q16: Canvas3dVec3Q16,
    axis_v_q16: Canvas3dVec3Q16,
    constraints: [Canvas3dVec3Q16; 4],
    color_rgba: u32,
) -> Canvas3dPlanePatchWorklistRgba8Desc {
    Canvas3dPlanePatchWorklistRgba8Desc {
        dst_pitch_bytes: staging.surface.pitch_bytes,
        dst_width: width,
        dst_height: height,
        rect_x: 0,
        rect_y: 0,
        rect_width: width,
        rect_height: height,
        canvas_width: width,
        canvas_height: height,
        reserved0: 0,
        origin_q16,
        axis_u_q16,
        axis_v_q16,
        constraint0_q16: constraints[0],
        constraint1_q16: constraints[1],
        constraint2_q16: constraints[2],
        constraint3_q16: constraints[3],
        constraint_count: 4,
        color_rgba,
    }
}

fn shell_cube20_translate_x_q16(frame: u32) -> i32 {
    const PERIOD_FRAMES: u32 = 240;
    let half_period = PERIOD_FRAMES / 2;
    let phase = frame % PERIOD_FRAMES;
    let span = CANVAS3D_PROJECT_Q16_ONE;
    let offset = if phase < half_period {
        -CUBE20_HALF_Q16 + ((span as i64 * phase as i64) / half_period as i64) as i32
    } else {
        CUBE20_HALF_Q16 - ((span as i64 * (phase - half_period) as i64) / half_period as i64) as i32
    };
    offset.clamp(-CUBE20_HALF_Q16, CUBE20_HALF_Q16)
}

fn shell_tetra10_translate_y_q16(frame: u32) -> i32 {
    const PERIOD_FRAMES: u32 = 240;
    let half_period = PERIOD_FRAMES / 2;
    let phase = frame % PERIOD_FRAMES;
    let span = CANVAS3D_PROJECT_Q16_ONE;
    let offset = if phase < half_period {
        -CUBE20_HALF_Q16 + ((span as i64 * phase as i64) / half_period as i64) as i32
    } else {
        CUBE20_HALF_Q16 - ((span as i64 * (phase - half_period) as i64) / half_period as i64) as i32
    };
    offset.clamp(-CUBE20_HALF_Q16, CUBE20_HALF_Q16)
}

fn shell_canvas3d_cube_scale_pulse_q16(elapsed_ms: u64) -> i32 {
    const PERIOD_MS: u64 = 5_000;
    const MIN_SCALE_Q16: u64 = CANVAS3D_PROJECT_Q16_ONE as u64 / 10;
    let half_period = PERIOD_MS / 2;
    let phase = elapsed_ms % PERIOD_MS;
    let max_scale = CANVAS3D_PROJECT_Q16_ONE as u64;
    let span = max_scale - MIN_SCALE_Q16;
    if phase < half_period {
        (max_scale - (span * phase) / half_period) as i32
    } else {
        (MIN_SCALE_Q16 + (span * (phase - half_period)) / half_period) as i32
    }
}

const CANVAS3D_SIN_Q16_DEG_0_90: [i32; 91] = [
    0, 1144, 2287, 3430, 4572, 5712, 6850, 7987, 9121, 10252, 11380, 12505, 13626, 14742, 15855,
    16962, 18064, 19161, 20252, 21336, 22415, 23486, 24550, 25607, 26656, 27697, 28729, 29753,
    30767, 31772, 32768, 33754, 34729, 35693, 36647, 37590, 38521, 39441, 40348, 41243, 42126,
    42995, 43852, 44695, 45525, 46341, 47143, 47930, 48703, 49461, 50203, 50931, 51643, 52339,
    53020, 53684, 54332, 54963, 55578, 56175, 56756, 57319, 57865, 58393, 58903, 59396, 59870,
    60326, 60764, 61183, 61584, 61966, 62328, 62672, 62997, 63303, 63589, 63856, 64104, 64332,
    64540, 64729, 64898, 65048, 65177, 65287, 65376, 65446, 65496, 65526, 65536,
];

fn shell_canvas3d_sin_0_180_q16(deg: u32) -> i32 {
    let deg = deg % 180;
    if deg <= 90 {
        CANVAS3D_SIN_Q16_DEG_0_90[deg as usize]
    } else {
        CANVAS3D_SIN_Q16_DEG_0_90[(180 - deg) as usize]
    }
}

fn shell_canvas3d_cos_0_180_q16(deg: u32) -> i32 {
    let deg = deg % 180;
    if deg <= 90 {
        CANVAS3D_SIN_Q16_DEG_0_90[(90 - deg) as usize]
    } else {
        -CANVAS3D_SIN_Q16_DEG_0_90[(deg - 90) as usize]
    }
}

fn shell_canvas3d_y_quat_q16(half_angle_deg: u32, clockwise: bool) -> Canvas3dVec3Q16 {
    let mut y = shell_canvas3d_sin_0_180_q16(half_angle_deg);
    if clockwise {
        y = -y;
    }
    Canvas3dVec3Q16 {
        x: 0,
        y,
        z: 0,
        pad: shell_canvas3d_cos_0_180_q16(half_angle_deg),
    }
}

fn shell_canvas3d_seed_visual_vertices(state: DirectRcsState) {
    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CANVAS3D_PROJECT_TEST_BYTES);
        core::ptr::write_bytes(state.canvas3d_out_virt, 0, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
        core::ptr::write_bytes(state.canvas3d_tmp_virt, 0, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
        let vertices = state.clear_test_virt as *mut Canvas3dVec3Q16;
        for cube_index in 0..CUBE20_INSTANCE_COUNT {
            let base = cube_index * CUBE20_VERTEX_COUNT;
            for local_index in 0..CUBE20_VERTEX_COUNT {
                let vertex = shell_canvas3d_seed_cube_vertex(cube_index, local_index);
                core::ptr::write_volatile(vertices.add(base + local_index), vertex);
            }
        }
        for local_index in 0..TETRA10_VERTEX_COUNT {
            let vertex = shell_canvas3d_seed_tetra_vertex(local_index);
            core::ptr::write_volatile(vertices.add(TETRA10_BASE_VERTEX + local_index), vertex);
        }
    }
    intel::dma_flush(state.clear_test_virt, CANVAS3D_PROJECT_TEST_BYTES);
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    intel::dma_flush(state.canvas3d_tmp_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
}

const ICO30_VERTS_Q16: [Canvas3dVec3Q16; ICO30_CORNER_COUNT] = [
    Canvas3dVec3Q16 {
        x: 0,
        y: 0,
        z: 32768,
        pad: 0,
    },
    Canvas3dVec3Q16 {
        x: 16384,
        y: 10126,
        z: 26510,
        pad: 1,
    },
    Canvas3dVec3Q16 {
        x: -16384,
        y: 10126,
        z: 26510,
        pad: 2,
    },
    Canvas3dVec3Q16 {
        x: 16384,
        y: -10126,
        z: 26510,
        pad: 3,
    },
    Canvas3dVec3Q16 {
        x: -16384,
        y: -10126,
        z: 26510,
        pad: 4,
    },
    Canvas3dVec3Q16 {
        x: 10126,
        y: 26510,
        z: 16384,
        pad: 5,
    },
    Canvas3dVec3Q16 {
        x: -10126,
        y: 26510,
        z: 16384,
        pad: 6,
    },
    Canvas3dVec3Q16 {
        x: 26510,
        y: 16384,
        z: 10126,
        pad: 7,
    },
    Canvas3dVec3Q16 {
        x: 0,
        y: 32768,
        z: 0,
        pad: 8,
    },
    Canvas3dVec3Q16 {
        x: -26510,
        y: 16384,
        z: 10126,
        pad: 9,
    },
    Canvas3dVec3Q16 {
        x: 26510,
        y: 16384,
        z: -10126,
        pad: 10,
    },
    Canvas3dVec3Q16 {
        x: 32768,
        y: 0,
        z: 0,
        pad: 11,
    },
    Canvas3dVec3Q16 {
        x: -26510,
        y: 16384,
        z: -10126,
        pad: 12,
    },
    Canvas3dVec3Q16 {
        x: -32768,
        y: 0,
        z: 0,
        pad: 13,
    },
    Canvas3dVec3Q16 {
        x: 10126,
        y: 26510,
        z: -16384,
        pad: 14,
    },
    Canvas3dVec3Q16 {
        x: -10126,
        y: 26510,
        z: -16384,
        pad: 15,
    },
    Canvas3dVec3Q16 {
        x: 16384,
        y: 10126,
        z: -26510,
        pad: 16,
    },
    Canvas3dVec3Q16 {
        x: -16384,
        y: 10126,
        z: -26510,
        pad: 17,
    },
    Canvas3dVec3Q16 {
        x: 16384,
        y: -10126,
        z: -26510,
        pad: 18,
    },
    Canvas3dVec3Q16 {
        x: -16384,
        y: -10126,
        z: -26510,
        pad: 19,
    },
    Canvas3dVec3Q16 {
        x: 0,
        y: 0,
        z: -32768,
        pad: 20,
    },
    Canvas3dVec3Q16 {
        x: 10126,
        y: -26510,
        z: 16384,
        pad: 21,
    },
    Canvas3dVec3Q16 {
        x: -10126,
        y: -26510,
        z: 16384,
        pad: 22,
    },
    Canvas3dVec3Q16 {
        x: 26510,
        y: -16384,
        z: 10126,
        pad: 23,
    },
    Canvas3dVec3Q16 {
        x: 0,
        y: -32768,
        z: 0,
        pad: 24,
    },
    Canvas3dVec3Q16 {
        x: -26510,
        y: -16384,
        z: 10126,
        pad: 25,
    },
    Canvas3dVec3Q16 {
        x: 26510,
        y: -16384,
        z: -10126,
        pad: 26,
    },
    Canvas3dVec3Q16 {
        x: 10126,
        y: -26510,
        z: -16384,
        pad: 27,
    },
    Canvas3dVec3Q16 {
        x: -10126,
        y: -26510,
        z: -16384,
        pad: 28,
    },
    Canvas3dVec3Q16 {
        x: -26510,
        y: -16384,
        z: -10126,
        pad: 29,
    },
];

const ICO60_EDGES: [(usize, usize); ICO60_EDGE_COUNT] = [
    (0, 1),
    (0, 2),
    (0, 3),
    (0, 4),
    (5, 6),
    (5, 1),
    (5, 7),
    (5, 8),
    (6, 2),
    (6, 9),
    (6, 8),
    (1, 3),
    (1, 7),
    (2, 4),
    (2, 9),
    (7, 10),
    (7, 11),
    (9, 12),
    (9, 13),
    (14, 15),
    (14, 16),
    (14, 10),
    (14, 8),
    (15, 17),
    (15, 12),
    (15, 8),
    (16, 18),
    (16, 10),
    (17, 19),
    (17, 12),
    (20, 16),
    (20, 17),
    (20, 18),
    (20, 19),
    (21, 22),
    (21, 3),
    (21, 23),
    (21, 24),
    (22, 4),
    (22, 25),
    (22, 24),
    (3, 23),
    (4, 25),
    (23, 26),
    (26, 11),
    (10, 11),
    (27, 28),
    (27, 18),
    (27, 26),
    (27, 24),
    (28, 19),
    (28, 29),
    (28, 24),
    (18, 26),
    (19, 29),
    (25, 29),
    (25, 13),
    (23, 11),
    (12, 13),
    (29, 13),
];

fn shell_canvas3d_seed_archaeology_vertices(state: DirectRcsState) {
    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CANVAS3D_PROJECT_TEST_BYTES);
        core::ptr::write_bytes(state.canvas3d_out_virt, 0, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
        core::ptr::write_bytes(state.canvas3d_tmp_virt, 0, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);

        let vertices = state.clear_test_virt as *mut Canvas3dVec3Q16;
        for (index, vertex) in ICO30_VERTS_Q16.iter().copied().enumerate() {
            let mut vertex = shell_canvas3d_scale_seed_vertex(vertex);
            vertex.pad = index as i32;
            core::ptr::write_volatile(vertices.add(index), vertex);
        }
        for (edge_index, &(a, b)) in ICO60_EDGES.iter().enumerate() {
            let va = shell_canvas3d_scale_seed_vertex(ICO30_VERTS_Q16[a]);
            let vb = shell_canvas3d_scale_seed_vertex(ICO30_VERTS_Q16[b]);
            let out_index = ICO30_CORNER_COUNT + edge_index;
            core::ptr::write_volatile(
                vertices.add(out_index),
                Canvas3dVec3Q16 {
                    x: va.x + ((vb.x - va.x) / 2),
                    y: va.y + ((vb.y - va.y) / 2),
                    z: va.z + ((vb.z - va.z) / 2),
                    pad: out_index as i32,
                },
            );
        }
    }
    intel::dma_flush(state.clear_test_virt, CANVAS3D_PROJECT_TEST_BYTES);
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    intel::dma_flush(state.canvas3d_tmp_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
}

fn shell_canvas3d_scale_seed_vertex(vertex: Canvas3dVec3Q16) -> Canvas3dVec3Q16 {
    Canvas3dVec3Q16 {
        x: vertex.x.saturating_mul(CUBE20_SEED_SCALE),
        y: vertex.y.saturating_mul(CUBE20_SEED_SCALE),
        z: vertex.z.saturating_mul(CUBE20_SEED_SCALE),
        pad: vertex.pad,
    }
}

fn shell_canvas3d_seed_cube_vertex(cube_index: usize, local_index: usize) -> Canvas3dVec3Q16 {
    let half = CUBE20_SEED_HALF_Q16;
    let corners = [
        Canvas3dVec3Q16 {
            x: -half,
            y: -half,
            z: -half,
            pad: 0,
        },
        Canvas3dVec3Q16 {
            x: half,
            y: -half,
            z: -half,
            pad: 1,
        },
        Canvas3dVec3Q16 {
            x: -half,
            y: half,
            z: -half,
            pad: 2,
        },
        Canvas3dVec3Q16 {
            x: half,
            y: half,
            z: -half,
            pad: 3,
        },
        Canvas3dVec3Q16 {
            x: -half,
            y: -half,
            z: half,
            pad: 4,
        },
        Canvas3dVec3Q16 {
            x: half,
            y: -half,
            z: half,
            pad: 5,
        },
        Canvas3dVec3Q16 {
            x: -half,
            y: half,
            z: half,
            pad: 6,
        },
        Canvas3dVec3Q16 {
            x: half,
            y: half,
            z: half,
            pad: 7,
        },
    ];
    let mut vertex = if local_index < CUBE20_CORNER_COUNT {
        corners[local_index]
    } else {
        let edge_sample = local_index - CUBE20_CORNER_COUNT;
        let edge_index = edge_sample / CUBE20_EDGE_SAMPLE_COUNT;
        let sample_index = edge_sample % CUBE20_EDGE_SAMPLE_COUNT;
        let (a, b) = CUBE20_EDGES[edge_index];
        let va = corners[a];
        let vb = corners[b];
        let step = (sample_index + 1) as i32;
        let denom = (CUBE20_EDGE_SAMPLE_COUNT + 1) as i32;
        Canvas3dVec3Q16 {
            x: va.x + ((vb.x - va.x) * step) / denom,
            y: va.y + ((vb.y - va.y) * step) / denom,
            z: va.z + ((vb.z - va.z) * step) / denom,
            pad: local_index as i32,
        }
    };

    vertex.pad = (cube_index * CUBE20_VERTEX_COUNT + local_index) as i32;
    vertex
}

fn shell_canvas3d_seed_tetra_vertex(local_index: usize) -> Canvas3dVec3Q16 {
    let half = CUBE20_HALF_Q16;
    let base_x = ((half as i64 * 56_756) / CANVAS3D_PROJECT_Q16_ONE as i64) as i32;
    let base_y = -half / 2;
    let corners = [
        // Apex sits on the local y-axis; the base triangle centroid is x=0,z=0.
        Canvas3dVec3Q16 {
            x: 0,
            y: half,
            z: 0,
            pad: 0,
        },
        Canvas3dVec3Q16 {
            x: 0,
            y: base_y,
            z: half,
            pad: 1,
        },
        Canvas3dVec3Q16 {
            x: -base_x,
            y: base_y,
            z: -half / 2,
            pad: 2,
        },
        Canvas3dVec3Q16 {
            x: base_x,
            y: base_y,
            z: -half / 2,
            pad: 3,
        },
    ];
    let mut vertex = if local_index < TETRA10_CORNER_COUNT {
        corners[local_index]
    } else {
        let edge_sample = local_index - TETRA10_CORNER_COUNT;
        let edge_index = edge_sample / TETRA10_EDGE_SAMPLE_COUNT;
        let sample_index = edge_sample % TETRA10_EDGE_SAMPLE_COUNT;
        let (a, b) = TETRA10_EDGES[edge_index];
        let va = corners[a];
        let vb = corners[b];
        let step = (sample_index + 1) as i32;
        let denom = (TETRA10_EDGE_SAMPLE_COUNT + 1) as i32;
        Canvas3dVec3Q16 {
            x: va.x + ((vb.x - va.x) * step) / denom,
            y: va.y + ((vb.y - va.y) * step) / denom,
            z: va.z + ((vb.z - va.z) * step) / denom,
            pad: local_index as i32,
        }
    };

    vertex.pad = (TETRA10_BASE_VERTEX + local_index) as i32;
    vertex
}

fn shell_canvas3d_write_pixel(
    primary: *mut u32,
    pitch_pixels: usize,
    canvas_x: usize,
    canvas_y: usize,
    canvas_width: usize,
    canvas_height: usize,
    x: i32,
    y: i32,
    color: u32,
) -> bool {
    if x < 0 || y < 0 || x >= canvas_width as i32 || y >= canvas_height as i32 {
        return false;
    }
    unsafe {
        let dst = primary.add((canvas_y + y as usize) * pitch_pixels + canvas_x + x as usize);
        core::ptr::write_volatile(dst, color);
    }
    true
}

fn shell_canvas3d_stamp_dot(
    primary: *mut u32,
    pitch_pixels: usize,
    canvas_x: usize,
    canvas_y: usize,
    canvas_width: usize,
    canvas_height: usize,
    x: i32,
    y: i32,
    radius: i32,
    color: u32,
) -> usize {
    let mut stamped = 0usize;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if shell_canvas3d_write_pixel(
                primary,
                pitch_pixels,
                canvas_x,
                canvas_y,
                canvas_width,
                canvas_height,
                x + dx,
                y + dy,
                color,
            ) {
                stamped = stamped.saturating_add(1);
            }
        }
    }
    stamped
}

fn shell_canvas3d_write_rgba_pixel(
    rgba: &mut [u8],
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    color: u32,
) -> bool {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return false;
    }
    let offset = ((y as usize) * width + x as usize) * core::mem::size_of::<u32>();
    if offset + 3 >= rgba.len() {
        return false;
    }
    rgba[offset] = ((color >> 16) & 0xFF) as u8;
    rgba[offset + 1] = ((color >> 8) & 0xFF) as u8;
    rgba[offset + 2] = (color & 0xFF) as u8;
    rgba[offset + 3] = ((color >> 24) & 0xFF) as u8;
    true
}

fn shell_canvas3d_stamp_rgba_dot(
    rgba: &mut [u8],
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    radius: i32,
    color: u32,
) -> usize {
    let mut stamped = 0usize;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if shell_canvas3d_write_rgba_pixel(rgba, width, height, x + dx, y + dy, color) {
                stamped = stamped.saturating_add(1);
            }
        }
    }
    stamped
}

fn shell_canvas3d_present_color(vertex_index: usize) -> u32 {
    if vertex_index < CUBE20_VISUAL_VERTEX_COUNT {
        CUBE20_PRESENT_COLORS[0]
    } else {
        0xFF30_D8FF
    }
}

fn shell_canvas3d_archaeology_present_color(vertex_index: usize) -> u32 {
    if vertex_index < ICO30_CORNER_COUNT {
        const CORNER_COLORS: [u32; 6] = [
            0xFFFF_4FD8,
            0xFFFF_D050,
            0xFF53_FFD2,
            0xFF60_9CFF,
            0xFFE8_7CFF,
            0xFFFFFFFF,
        ];
        CORNER_COLORS[vertex_index % CORNER_COLORS.len()]
    } else {
        0xFF6C_B7FF
    }
}

fn shell_canvas3d_cpu_copy_projected_points_to_primary(
    state: DirectRcsState,
    target: intel::display::PrimarySurfaceGpgpuTarget,
    canvas_x: u32,
    canvas_y: u32,
) -> (usize, usize) {
    let canvas_width = target.width.saturating_sub(canvas_x);
    let canvas_height = target.height.saturating_sub(canvas_y);
    shell_canvas3d_cpu_copy_projected_points_to_primary_count(
        state,
        target,
        canvas_x,
        canvas_y,
        canvas_width,
        canvas_height,
        CANVAS3D_VISUAL_VERTEX_COUNT,
        false,
    )
}

fn shell_canvas3d_cpu_copy_projected_points_to_primary_count(
    state: DirectRcsState,
    target: intel::display::PrimarySurfaceGpgpuTarget,
    canvas_x: u32,
    canvas_y: u32,
    canvas_width: u32,
    canvas_height: u32,
    vertex_count: usize,
    archaeology_colors: bool,
) -> (usize, usize) {
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    let pitch_pixels = (target.pitch_bytes as usize) / core::mem::size_of::<u32>();
    let canvas_x = canvas_x as usize;
    let canvas_y = canvas_y as usize;
    if canvas_x >= target.width as usize || canvas_y >= target.height as usize {
        return (0, 0);
    }
    let canvas_width =
        (canvas_width as usize).min((target.width as usize).saturating_sub(canvas_x));
    let canvas_height =
        (canvas_height as usize).min((target.height as usize).saturating_sub(canvas_y));
    if canvas_width == 0 || canvas_height == 0 {
        return (0, 0);
    }
    let mut visible = 0usize;
    let mut stamped = 0usize;
    let vertex_count = vertex_count.min(CANVAS3D_PROJECT_VERTEX_COUNT);
    let mut point_xy = [(0i32, 0i32); CANVAS3D_PROJECT_VERTEX_COUNT];
    let mut point_visible = [false; CANVAS3D_PROJECT_VERTEX_COUNT];

    unsafe {
        let primary = target.virt as *mut u32;
        for y in 0..canvas_height {
            let row = primary.add((canvas_y + y) * pitch_pixels + canvas_x);
            for x in 0..canvas_width {
                core::ptr::write_volatile(row.add(x), 0xFF08_0810);
            }
        }

        let out = state.canvas3d_out_virt as *const Canvas3dProjectedRgba8;
        for index in 0..vertex_count {
            let point = core::ptr::read_volatile(out.add(index));
            if (point.packed_xy & 0x8000_0000) == 0 {
                continue;
            }
            visible = visible.saturating_add(1);
            let x = (point.packed_xy & 0xFFFF) as i32;
            let y = ((point.packed_xy >> 16) & 0x7FFF) as i32;
            point_xy[index] = (x, y);
            point_visible[index] = true;
        }

        for index in 0..vertex_count {
            if !point_visible[index] {
                continue;
            }
            let (x, y) = point_xy[index];
            let color = if archaeology_colors {
                shell_canvas3d_archaeology_present_color(index)
            } else {
                shell_canvas3d_present_color(index)
            };
            stamped = stamped.saturating_add(shell_canvas3d_stamp_dot(
                primary,
                pitch_pixels,
                canvas_x,
                canvas_y,
                canvas_width,
                canvas_height,
                x,
                y,
                CUBE20_PROJECT_DOT_RADIUS,
                color,
            ));
        }
    }

    (visible, stamped)
}

fn shell_canvas3d_copy_projected_points_to_rgba8(
    state: DirectRcsState,
    width: usize,
    height: usize,
    vertex_count: usize,
    archaeology_colors: bool,
    rgba: &mut [u8],
) -> (usize, usize) {
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    if width == 0 || height == 0 {
        return (0, 0);
    }
    for pixel in rgba.chunks_exact_mut(core::mem::size_of::<u32>()) {
        pixel[0] = 0x08;
        pixel[1] = 0x08;
        pixel[2] = 0x10;
        pixel[3] = 0xFF;
    }

    let mut visible = 0usize;
    let mut stamped = 0usize;
    let vertex_count = vertex_count.min(CANVAS3D_PROJECT_VERTEX_COUNT);

    unsafe {
        let out = state.canvas3d_out_virt as *const Canvas3dProjectedRgba8;
        for index in 0..vertex_count {
            let point = core::ptr::read_volatile(out.add(index));
            if (point.packed_xy & 0x8000_0000) == 0 {
                continue;
            }
            visible = visible.saturating_add(1);
            let x = (point.packed_xy & 0xFFFF) as i32;
            let y = ((point.packed_xy >> 16) & 0x7FFF) as i32;
            let color = if archaeology_colors {
                shell_canvas3d_archaeology_present_color(index)
            } else {
                shell_canvas3d_present_color(index)
            };
            stamped = stamped.saturating_add(shell_canvas3d_stamp_rgba_dot(
                rgba,
                width,
                height,
                x,
                y,
                CUBE20_PROJECT_DOT_RADIUS,
                color,
            ));
        }
    }

    (visible, stamped)
}

fn submit_canvas3d_transform_fused_smoke(
    dev: intel::Dev,
    state: DirectRcsState,
    upload: Option<UploadedKernelArtifact>,
    scale_q16: Canvas3dVec3Q16,
    rotate_q16: Canvas3dVec3Q16,
    translate_q16: Canvas3dVec3Q16,
) -> bool {
    let Some(upload) = upload else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: canvas3d-transform-fused_q16 skipped reason=no-kernel-upload\n"
        );
        return false;
    };

    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let expected =
        direct_rcs_seed_canvas3d_transform_fused(state, scale_q16, rotate_q16, translate_q16);
    let params = Canvas3dTransformFusedQ16Params {
        src_gpu: DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        dst_gpu: CANVAS3D_PROJECT_OUT_GPU,
        src_first_vertex: CANVAS3D_TRANSFORM_SRC_FIRST,
        dst_first_vertex: CANVAS3D_TRANSFORM_DST_FIRST,
        vertex_count: CANVAS3D_TRANSFORM_TEST_COUNT,
        scale_q16,
        rotate_q16,
        translate_q16,
    };
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let src_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
            state.clear_test_phys,
            CLEAR_RECT_TEST_BYTES,
        );
    let dst_ppgtt_ok = src_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            CANVAS3D_PROJECT_OUT_GPU,
            state.canvas3d_out_phys,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_canvas3d_transform_fused_batch(
            state,
            upload,
            params,
            CANVAS3D_TRANSFORM_FUSED_PRE_MARKER,
            CANVAS3D_TRANSFORM_FUSED_POST_MARKER,
            CANVAS3D_PROJECT_VERTEX_BYTES,
            CANVAS3D_PROJECT_VERTEX_BYTES,
        );
    let submit_start_tick = direct_rcs_now_tick();
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let (observed, retire_ms) = if submitted {
        direct_rcs_poll_result_slot_elapsed(
            state,
            CANVAS3D_TRANSFORM_POST_MARKER_SLOT,
            CANVAS3D_TRANSFORM_FUSED_POST_MARKER,
            submit_start_tick,
        )
    } else {
        (0, 0)
    };
    let pre_marker = direct_rcs_read_result_slot(state, CANVAS3D_TRANSFORM_PRE_MARKER_SLOT);
    let (matches, src_preserved, guards_ok, first, last) =
        direct_rcs_read_canvas3d_transform_result(state, expected);
    let retired = observed == CANVAS3D_TRANSFORM_FUSED_POST_MARKER;
    let ok = retired
        && pre_marker == CANVAS3D_TRANSFORM_FUSED_PRE_MARKER
        && matches == CANVAS3D_TRANSFORM_TEST_COUNT as usize
        && src_preserved == CANVAS3D_TRANSFORM_TEST_COUNT as usize
        && guards_ok;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: canvas3d-transform-fused_q16 forcewake={} ggtt={} ppgtt={} kernel_ppgtt={} src_ppgtt={} dst_ppgtt={} batch={} submitted={} retired={} retire_ms={} ok={} matched={}/{} src_preserved={}/{} guards_ok={} src_first={} dst_first={} count={} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} src_gpu=0x{:X} dst_gpu=0x{:X} first=[{}, {}, {}, {}] last=[{}, {}, {}, {}] scale=[{}, {}, {}, {}] quat=[{}, {}, {}, {}] translate=[{}, {}, {}, {}] ring_gpu=0x{:X} batch_gpu=0x{:X} result_gpu=0x{:X} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} path=direct-execlist no_guc_submit=1 next=project-or-visual\n",
        forcewake_ok as u8,
        mapped_ok as u8,
        ppgtt_ok as u8,
        kernel_ppgtt_ok as u8,
        src_ppgtt_ok as u8,
        dst_ppgtt_ok as u8,
        batch_ok as u8,
        submitted as u8,
        retired as u8,
        retire_ms,
        ok as u8,
        matches,
        CANVAS3D_TRANSFORM_TEST_COUNT,
        src_preserved,
        CANVAS3D_TRANSFORM_TEST_COUNT,
        guards_ok as u8,
        CANVAS3D_TRANSFORM_SRC_FIRST,
        CANVAS3D_TRANSFORM_DST_FIRST,
        CANVAS3D_TRANSFORM_TEST_COUNT,
        pre_marker,
        observed,
        CANVAS3D_TRANSFORM_FUSED_POST_MARKER,
        upload.gpu,
        upload.gpu + 0x40,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        CANVAS3D_PROJECT_OUT_GPU,
        first.x,
        first.y,
        first.z,
        first.pad,
        last.x,
        last.y,
        last.z,
        last.pad,
        scale_q16.x,
        scale_q16.y,
        scale_q16.z,
        scale_q16.pad,
        rotate_q16.x,
        rotate_q16.y,
        rotate_q16.z,
        rotate_q16.pad,
        translate_q16.x,
        translate_q16.y,
        translate_q16.z,
        translate_q16.pad,
        DIRECT_RCS_GPU_VA_RING_BASE,
        DIRECT_RCS_GPU_VA_BATCH_BASE,
        DIRECT_RCS_GPU_VA_RESULT_BASE,
        intel::mmio_read(dev, RCS_RING_HEAD),
        intel::mmio_read(dev, RCS_RING_TAIL),
        intel::mmio_read(dev, RCS_RING_ACTHD),
        intel::mmio_read(dev, RCS_RING_IPEIR),
        intel::mmio_read(dev, RCS_RING_IPEHR),
        intel::mmio_read(dev, RCS_RING_EIR),
    );

    ok
}

fn submit_canvas3d_transform_fused_frame_range(
    dev: intel::Dev,
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    src_gpu: u64,
    src_phys: u64,
    dst_gpu: u64,
    dst_phys: u64,
    src_first_vertex: u32,
    dst_first_vertex: u32,
    vertex_count: u32,
    scale_q16: Canvas3dVec3Q16,
    rotate_q16: Canvas3dVec3Q16,
    translate_q16: Canvas3dVec3Q16,
    pre_marker_value: u32,
    post_marker_value: u32,
) -> Option<u64> {
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let params = Canvas3dTransformFusedQ16Params {
        src_gpu,
        dst_gpu,
        src_first_vertex,
        dst_first_vertex,
        vertex_count,
        scale_q16,
        rotate_q16,
        translate_q16,
    };
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let src_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, src_gpu, src_phys, CANVAS3D_PROJECT_VERTEX_BYTES);
    let dst_ppgtt_ok = src_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, dst_gpu, dst_phys, CANVAS3D_PROJECT_VERTEX_BYTES);
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_canvas3d_transform_fused_batch(
            state,
            upload,
            params,
            pre_marker_value,
            post_marker_value,
            CANVAS3D_PROJECT_VERTEX_BYTES,
            CANVAS3D_PROJECT_VERTEX_BYTES,
        );
    let submit_start_tick = direct_rcs_now_tick();
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let (observed, retire_ms) = if submitted {
        direct_rcs_poll_result_slot_elapsed(
            state,
            CANVAS3D_TRANSFORM_POST_MARKER_SLOT,
            post_marker_value,
            submit_start_tick,
        )
    } else {
        (0, 0)
    };
    if observed == post_marker_value {
        Some(retire_ms)
    } else {
        None
    }
}

fn submit_canvas3d_clip_box_frame(
    dev: intel::Dev,
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    src_gpu: u64,
    src_phys: u64,
    dst_gpu: u64,
    dst_phys: u64,
    min_q16: Canvas3dVec3Q16,
    max_q16: Canvas3dVec3Q16,
) -> Option<u64> {
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let params = Canvas3dClipBoxQ16Params {
        src_gpu,
        dst_gpu,
        src_first_vertex: 0,
        dst_first_vertex: 0,
        vertex_count: CANVAS3D_PROJECT_VERTEX_COUNT as u32,
        min_q16,
        max_q16,
    };
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let src_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, src_gpu, src_phys, CANVAS3D_PROJECT_VERTEX_BYTES);
    let dst_ppgtt_ok = src_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, dst_gpu, dst_phys, CANVAS3D_PROJECT_VERTEX_BYTES);
    let batch_ok = dst_ppgtt_ok
        && direct_rcs_encode_canvas3d_clip_box_batch(
            state,
            upload,
            params,
            CANVAS3D_PROJECT_VERTEX_BYTES,
            CANVAS3D_PROJECT_VERTEX_BYTES,
        );
    let submit_start_tick = direct_rcs_now_tick();
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let (observed, retire_ms) = if submitted {
        direct_rcs_poll_result_slot_elapsed(
            state,
            CANVAS3D_CLIP_BOX_POST_MARKER_SLOT,
            CANVAS3D_CLIP_BOX_POST_MARKER,
            submit_start_tick,
        )
    } else {
        (0, 0)
    };
    if observed == CANVAS3D_CLIP_BOX_POST_MARKER {
        Some(retire_ms)
    } else {
        None
    }
}

fn submit_canvas3d_project_frame(
    dev: intel::Dev,
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
) -> Option<u64> {
    submit_canvas3d_project_frame_from(
        dev,
        state,
        upload,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        state.clear_test_phys,
        CANVAS3D_PROJECT_VERTEX_COUNT as u32,
        CUBE20_PROJECT_TILE_SIZE,
        CUBE20_PROJECT_TILE_SIZE,
    )
}

fn submit_canvas3d_project_frame_from(
    dev: intel::Dev,
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    vertices_gpu: u64,
    vertices_phys: u64,
    vertex_count: u32,
    canvas_width: u32,
    canvas_height: u32,
) -> Option<u64> {
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    if vertex_count == 0 || canvas_width == 0 || canvas_height == 0 {
        return None;
    }
    let params = Canvas3dProjectRgba8Params {
        vertices_gpu,
        out_gpu: CANVAS3D_PROJECT_OUT_GPU,
        src_first_vertex: 0,
        out_first_point: 0,
        vertex_count,
        canvas_width,
        canvas_height,
    };
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let vertices_ppgtt_ok = kernel_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            vertices_gpu,
            vertices_phys,
            CANVAS3D_PROJECT_VERTEX_BYTES,
        );
    let out_ppgtt_ok = vertices_ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            CANVAS3D_PROJECT_OUT_GPU,
            state.canvas3d_out_phys,
            CANVAS3D_PROJECT_OUT_ALLOC_BYTES,
        );
    let batch_ok = out_ppgtt_ok
        && direct_rcs_encode_canvas3d_project_batch(
            state,
            upload,
            params,
            CANVAS3D_PROJECT_VERTEX_BYTES,
            CANVAS3D_PROJECT_OUT_BYTES,
        );
    let submit_start_tick = direct_rcs_now_tick();
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let (observed, retire_ms) = if submitted {
        direct_rcs_poll_result_slot_elapsed(
            state,
            CANVAS3D_PROJECT_POST_MARKER_SLOT,
            CANVAS3D_PROJECT_POST_MARKER,
            submit_start_tick,
        )
    } else {
        (0, 0)
    };
    if observed == CANVAS3D_PROJECT_POST_MARKER {
        Some(retire_ms)
    } else {
        None
    }
}

fn direct_rcs_canvas3d_project_color(index: u32, z_q16: u32) -> u32 {
    let shade = 96u32 + ((index.wrapping_mul(29)) & 0x7F);
    let depth = (z_q16 >> 10) & 0x7F;
    let r = shade;
    let g = 255u32.saturating_sub(depth);
    let b = 96u32 + depth;
    0xFF00_0000 | (b << 16) | (g << 8) | r
}

fn direct_rcs_canvas3d_project_expected(
    index: usize,
    vertex: Canvas3dVec3Q16,
    canvas_width: u32,
    canvas_height: u32,
) -> Canvas3dProjectedRgba8 {
    let mut out = Canvas3dProjectedRgba8 {
        packed_xy: 0,
        rgba: 0,
        z_q16: vertex.z as u32,
        source_index: index as u32,
    };
    if vertex.z <= 0 {
        return out;
    }
    let focal = canvas_width.min(canvas_height) / 2;
    if canvas_width == 0 || canvas_height == 0 || focal == 0 {
        return out;
    }

    let sx_delta = ((vertex.x as i64) * focal as i64) / (vertex.z as i64);
    let sy_delta = ((vertex.y as i64) * focal as i64) / (vertex.z as i64);
    let sx = (canvas_width / 2) as i32 + sx_delta as i32;
    let sy = (canvas_height / 2) as i32 - sy_delta as i32;
    if (0..canvas_width as i32).contains(&sx) && (0..canvas_height as i32).contains(&sy) {
        out.packed_xy = 0x8000_0000 | (((sy as u32) & 0xFFFF) << 16) | ((sx as u32) & 0xFFFF);
        out.rgba = direct_rcs_canvas3d_project_color(index as u32, vertex.z as u32);
    }
    out
}

fn direct_rcs_canvas3d_project_seed_vertex(index: usize) -> Canvas3dVec3Q16 {
    let q = CANVAS3D_PROJECT_Q16_ONE;
    match index {
        0 => Canvas3dVec3Q16 {
            x: 0,
            y: 0,
            z: q,
            pad: 0,
        },
        1 => Canvas3dVec3Q16 {
            x: q / 2,
            y: 0,
            z: q,
            pad: 0,
        },
        2 => Canvas3dVec3Q16 {
            x: -(q / 2),
            y: 0,
            z: q,
            pad: 0,
        },
        3 => Canvas3dVec3Q16 {
            x: 0,
            y: q / 2,
            z: q,
            pad: 0,
        },
        4 => Canvas3dVec3Q16 {
            x: 0,
            y: -(q / 2),
            z: q,
            pad: 0,
        },
        5 => Canvas3dVec3Q16 {
            x: q / 2,
            y: q / 2,
            z: q * 2,
            pad: 0,
        },
        6 => Canvas3dVec3Q16 {
            x: q,
            y: 0,
            z: q,
            pad: 0,
        },
        7 => Canvas3dVec3Q16 {
            x: 0,
            y: 0,
            z: -q,
            pad: 0,
        },
        _ => {
            let ix = (index as i32 % 11) - 5;
            let iy = ((index as i32 / 11) % 11) - 5;
            let iz = 1 + (index as i32 % 5);
            Canvas3dVec3Q16 {
                x: ix * (q / 8),
                y: iy * (q / 8),
                z: q + iz * (q / 4),
                pad: index as i32,
            }
        }
    }
}

fn direct_rcs_seed_canvas3d_project(
    state: DirectRcsState,
) -> [Canvas3dProjectedRgba8; CANVAS3D_PROJECT_SAMPLE_COUNT] {
    let mut expected = [Canvas3dProjectedRgba8 {
        packed_xy: 0,
        rgba: 0,
        z_q16: 0,
        source_index: 0,
    }; CANVAS3D_PROJECT_SAMPLE_COUNT];

    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CANVAS3D_PROJECT_TEST_BYTES);
        core::ptr::write_bytes(state.canvas3d_out_virt, 0, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
        let vertices = state.clear_test_virt as *mut Canvas3dVec3Q16;
        let out = state.canvas3d_out_virt as *mut Canvas3dProjectedRgba8;
        for index in 0..CANVAS3D_PROJECT_VERTEX_COUNT {
            let vertex = direct_rcs_canvas3d_project_seed_vertex(index);
            core::ptr::write_volatile(vertices.add(index), vertex);
            core::ptr::write_volatile(
                out.add(index),
                Canvas3dProjectedRgba8 {
                    packed_xy: 0xDEAD_0000 | index as u32,
                    rgba: 0xA5A5_0000 | index as u32,
                    z_q16: 0,
                    source_index: 0xFFFF_FFFF,
                },
            );
        }
        for offset in 0..CANVAS3D_PROJECT_SAMPLE_COUNT {
            let src_index = CANVAS3D_PROJECT_SMOKE_SRC_FIRST as usize + offset;
            expected[offset] = direct_rcs_canvas3d_project_expected(
                src_index,
                direct_rcs_canvas3d_project_seed_vertex(src_index),
                CANVAS3D_PROJECT_SMOKE_CANVAS_WIDTH,
                CANVAS3D_PROJECT_SMOKE_CANVAS_HEIGHT,
            );
        }
    }
    intel::dma_flush(state.clear_test_virt, CANVAS3D_PROJECT_TEST_BYTES);
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    expected
}

fn direct_rcs_plane_sample_q16_lerp(index: u32, count: u32) -> i32 {
    if count <= 1 {
        0
    } else {
        -CANVAS3D_PROJECT_Q16_ONE
            + ((2 * CANVAS3D_PROJECT_Q16_ONE as i64 * index as i64) / (count - 1) as i64) as i32
    }
}

fn direct_rcs_plane_sample_constraint_ok(
    constraint: Canvas3dVec3Q16,
    u_q16: i32,
    v_q16: i32,
) -> bool {
    (((constraint.x as i64 * u_q16 as i64) >> 16)
        + ((constraint.y as i64 * v_q16 as i64) >> 16)
        + constraint.z as i64)
        >= 0
}

fn direct_rcs_plane_sample_dither_color(color_rgba: u32, u_index: u32, v_index: u32) -> u32 {
    let d = ((u_index ^ (v_index * 3)) & 3) * 10;
    let r = (color_rgba & 0xFF).saturating_add(d).min(255);
    let g = ((color_rgba >> 8) & 0xFF).saturating_add(d).min(255);
    let b = ((color_rgba >> 16) & 0xFF).saturating_add(d).min(255);
    let a = (color_rgba >> 24) & 0xFF;
    (a << 24) | (b << 16) | (g << 8) | r
}

fn direct_rcs_canvas3d_plane_sample_expected(
    params: Canvas3dPlaneSampleRgba8Params,
    offset: u32,
) -> Canvas3dProjectedRgba8 {
    let mut out = Canvas3dProjectedRgba8 {
        packed_xy: 0,
        rgba: 0,
        z_q16: 0,
        source_index: offset,
    };
    if params.u_steps == 0 || params.v_steps == 0 {
        return out;
    }

    let u_index = offset % params.u_steps;
    let v_index = offset / params.u_steps;
    if v_index >= params.v_steps {
        return out;
    }

    let u_q16 = direct_rcs_plane_sample_q16_lerp(u_index, params.u_steps);
    let v_q16 = direct_rcs_plane_sample_q16_lerp(v_index, params.v_steps);
    let constraints = [
        params.constraint0_q16,
        params.constraint1_q16,
        params.constraint2_q16,
        params.constraint3_q16,
    ];
    let mut keep = true;
    for index in 0..params.constraint_count.min(4) as usize {
        keep &= direct_rcs_plane_sample_constraint_ok(constraints[index], u_q16, v_q16);
    }

    let x = params.origin_q16.x
        + direct_rcs_q16_mul(params.axis_u_q16.x, u_q16)
        + direct_rcs_q16_mul(params.axis_v_q16.x, v_q16);
    let y = params.origin_q16.y
        + direct_rcs_q16_mul(params.axis_u_q16.y, u_q16)
        + direct_rcs_q16_mul(params.axis_v_q16.y, v_q16);
    let z = params.origin_q16.z
        + direct_rcs_q16_mul(params.axis_u_q16.z, u_q16)
        + direct_rcs_q16_mul(params.axis_v_q16.z, v_q16);
    out.z_q16 = z as u32;

    let focal = params.canvas_width.min(params.canvas_height) / 2;
    if keep && z > 0 && params.canvas_width > 0 && params.canvas_height > 0 && focal > 0 {
        let sx_delta = ((x as i64) * focal as i64) / z as i64;
        let sy_delta = ((y as i64) * focal as i64) / z as i64;
        let sx = (params.canvas_width / 2) as i32 + sx_delta as i32;
        let sy = (params.canvas_height / 2) as i32 - sy_delta as i32;
        if (0..params.canvas_width as i32).contains(&sx)
            && (0..params.canvas_height as i32).contains(&sy)
        {
            out.packed_xy = 0x8000_0000 | (((sy as u32) & 0xFFFF) << 16) | ((sx as u32) & 0xFFFF);
            out.rgba = direct_rcs_plane_sample_dither_color(params.color_rgba, u_index, v_index);
        }
    }

    out
}

fn direct_rcs_seed_canvas3d_plane_sample(
    state: DirectRcsState,
    params: Canvas3dPlaneSampleRgba8Params,
) -> [Canvas3dProjectedRgba8; CANVAS3D_PLANE_SAMPLE_COUNT] {
    let mut expected = [Canvas3dProjectedRgba8 {
        packed_xy: 0,
        rgba: 0,
        z_q16: 0,
        source_index: 0,
    }; CANVAS3D_PLANE_SAMPLE_COUNT];

    unsafe {
        core::ptr::write_bytes(state.canvas3d_out_virt, 0, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
        let out = state.canvas3d_out_virt as *mut Canvas3dProjectedRgba8;
        for index in 0..CANVAS3D_PROJECT_VERTEX_COUNT {
            core::ptr::write_volatile(
                out.add(index),
                Canvas3dProjectedRgba8 {
                    packed_xy: 0xDEAD_1000 | index as u32,
                    rgba: 0xA5A5_1000 | index as u32,
                    z_q16: 0,
                    source_index: 0xFFFF_FFFF,
                },
            );
        }
        for offset in 0..CANVAS3D_PLANE_SAMPLE_COUNT {
            expected[offset] = direct_rcs_canvas3d_plane_sample_expected(params, offset as u32);
        }
    }
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    expected
}

fn direct_rcs_q16_mul(a: i32, b: i32) -> i32 {
    (((a as i64) * (b as i64)) >> 16) as i32
}

fn direct_rcs_transform_seed_vertex(index: usize) -> Canvas3dVec3Q16 {
    let q = CANVAS3D_PROJECT_Q16_ONE;
    let x_step = ((index as i32 % 17) - 8) * (q / 8);
    let y_step = (((index as i32 * 3) % 19) - 9) * (q / 16);
    let z_step = q + ((index as i32 % 11) * (q / 16));
    Canvas3dVec3Q16 {
        x: x_step,
        y: y_step,
        z: z_step,
        pad: 0x5100 + index as i32,
    }
}

fn direct_rcs_transform_translate_expected(
    vertex: Canvas3dVec3Q16,
    delta: Canvas3dVec3Q16,
) -> Canvas3dVec3Q16 {
    Canvas3dVec3Q16 {
        x: vertex.x + delta.x,
        y: vertex.y + delta.y,
        z: vertex.z + delta.z,
        pad: vertex.pad,
    }
}

fn direct_rcs_transform_scale_expected(
    vertex: Canvas3dVec3Q16,
    scale: Canvas3dVec3Q16,
) -> Canvas3dVec3Q16 {
    Canvas3dVec3Q16 {
        x: direct_rcs_q16_mul(vertex.x, scale.x),
        y: direct_rcs_q16_mul(vertex.y, scale.y),
        z: direct_rcs_q16_mul(vertex.z, scale.z),
        pad: vertex.pad,
    }
}

fn direct_rcs_transform_rotate_z_180_expected(
    vertex: Canvas3dVec3Q16,
    _quat: Canvas3dVec3Q16,
) -> Canvas3dVec3Q16 {
    Canvas3dVec3Q16 {
        x: -vertex.x,
        y: -vertex.y,
        z: vertex.z,
        pad: vertex.pad,
    }
}

fn direct_rcs_transform_fused_z_180_expected(
    vertex: Canvas3dVec3Q16,
    scale: Canvas3dVec3Q16,
    quat: Canvas3dVec3Q16,
    translate: Canvas3dVec3Q16,
) -> Canvas3dVec3Q16 {
    direct_rcs_transform_translate_expected(
        direct_rcs_transform_rotate_z_180_expected(
            direct_rcs_transform_scale_expected(vertex, scale),
            quat,
        ),
        translate,
    )
}

fn direct_rcs_clip_box_expected(
    vertex: Canvas3dVec3Q16,
    min_q16: Canvas3dVec3Q16,
    max_q16: Canvas3dVec3Q16,
) -> Canvas3dVec3Q16 {
    Canvas3dVec3Q16 {
        x: vertex.x.clamp(min_q16.x, max_q16.x),
        y: vertex.y.clamp(min_q16.y, max_q16.y),
        z: vertex.z.clamp(min_q16.z, max_q16.z),
        pad: vertex.pad,
    }
}

fn direct_rcs_seed_canvas3d_transform(
    state: DirectRcsState,
    param_q16: Canvas3dVec3Q16,
    expected_fn: fn(Canvas3dVec3Q16, Canvas3dVec3Q16) -> Canvas3dVec3Q16,
) -> [Canvas3dVec3Q16; CANVAS3D_TRANSFORM_TEST_COUNT_USIZE] {
    let mut expected = [Canvas3dVec3Q16::default(); CANVAS3D_TRANSFORM_TEST_COUNT_USIZE];

    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CANVAS3D_PROJECT_VERTEX_BYTES);
        core::ptr::write_bytes(state.canvas3d_out_virt, 0, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
        let src = state.clear_test_virt as *mut Canvas3dVec3Q16;
        let dst = state.canvas3d_out_virt as *mut Canvas3dVec3Q16;
        for index in 0..CANVAS3D_PROJECT_VERTEX_COUNT {
            let vertex = direct_rcs_transform_seed_vertex(index);
            core::ptr::write_volatile(src.add(index), vertex);
            core::ptr::write_volatile(dst.add(index), CANVAS3D_TRANSFORM_DST_POISON);
        }
        for offset in 0..CANVAS3D_TRANSFORM_TEST_COUNT_USIZE {
            let src_index = CANVAS3D_TRANSFORM_SRC_FIRST as usize + offset;
            expected[offset] = expected_fn(direct_rcs_transform_seed_vertex(src_index), param_q16);
        }
    }
    intel::dma_flush(state.clear_test_virt, CANVAS3D_PROJECT_VERTEX_BYTES);
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    expected
}

fn direct_rcs_seed_canvas3d_transform_fused(
    state: DirectRcsState,
    scale_q16: Canvas3dVec3Q16,
    rotate_q16: Canvas3dVec3Q16,
    translate_q16: Canvas3dVec3Q16,
) -> [Canvas3dVec3Q16; CANVAS3D_TRANSFORM_TEST_COUNT_USIZE] {
    let mut expected = [Canvas3dVec3Q16::default(); CANVAS3D_TRANSFORM_TEST_COUNT_USIZE];

    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CANVAS3D_PROJECT_VERTEX_BYTES);
        core::ptr::write_bytes(state.canvas3d_out_virt, 0, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
        let src = state.clear_test_virt as *mut Canvas3dVec3Q16;
        let dst = state.canvas3d_out_virt as *mut Canvas3dVec3Q16;
        for index in 0..CANVAS3D_PROJECT_VERTEX_COUNT {
            let vertex = direct_rcs_transform_seed_vertex(index);
            core::ptr::write_volatile(src.add(index), vertex);
            core::ptr::write_volatile(dst.add(index), CANVAS3D_TRANSFORM_DST_POISON);
        }
        for offset in 0..CANVAS3D_TRANSFORM_TEST_COUNT_USIZE {
            let src_index = CANVAS3D_TRANSFORM_SRC_FIRST as usize + offset;
            expected[offset] = direct_rcs_transform_fused_z_180_expected(
                direct_rcs_transform_seed_vertex(src_index),
                scale_q16,
                rotate_q16,
                translate_q16,
            );
        }
    }
    intel::dma_flush(state.clear_test_virt, CANVAS3D_PROJECT_VERTEX_BYTES);
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    expected
}

fn direct_rcs_seed_canvas3d_clip_box(
    state: DirectRcsState,
    min_q16: Canvas3dVec3Q16,
    max_q16: Canvas3dVec3Q16,
) -> [Canvas3dVec3Q16; CANVAS3D_TRANSFORM_TEST_COUNT_USIZE] {
    let mut expected = [Canvas3dVec3Q16::default(); CANVAS3D_TRANSFORM_TEST_COUNT_USIZE];

    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CANVAS3D_PROJECT_VERTEX_BYTES);
        core::ptr::write_bytes(state.canvas3d_out_virt, 0, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
        let src = state.clear_test_virt as *mut Canvas3dVec3Q16;
        let dst = state.canvas3d_out_virt as *mut Canvas3dVec3Q16;
        for index in 0..CANVAS3D_PROJECT_VERTEX_COUNT {
            let vertex = direct_rcs_transform_seed_vertex(index);
            core::ptr::write_volatile(src.add(index), vertex);
            core::ptr::write_volatile(dst.add(index), CANVAS3D_TRANSFORM_DST_POISON);
        }
        for offset in 0..CANVAS3D_TRANSFORM_TEST_COUNT_USIZE {
            let src_index = CANVAS3D_TRANSFORM_SRC_FIRST as usize + offset;
            expected[offset] = direct_rcs_clip_box_expected(
                direct_rcs_transform_seed_vertex(src_index),
                min_q16,
                max_q16,
            );
        }
    }
    intel::dma_flush(state.clear_test_virt, CANVAS3D_PROJECT_VERTEX_BYTES);
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    expected
}

fn direct_rcs_read_canvas3d_project_samples(
    state: DirectRcsState,
    out_first: usize,
) -> [Canvas3dProjectedRgba8; CANVAS3D_PROJECT_SAMPLE_COUNT] {
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    let mut values = [Canvas3dProjectedRgba8 {
        packed_xy: 0,
        rgba: 0,
        z_q16: 0,
        source_index: 0,
    }; CANVAS3D_PROJECT_SAMPLE_COUNT];
    unsafe {
        let out = state.canvas3d_out_virt as *const Canvas3dProjectedRgba8;
        for (index, value) in values.iter_mut().enumerate() {
            *value = core::ptr::read_volatile(out.add(out_first + index));
        }
    }
    values
}

fn direct_rcs_read_canvas3d_plane_sample_result(
    state: DirectRcsState,
) -> [Canvas3dProjectedRgba8; CANVAS3D_PLANE_SAMPLE_COUNT] {
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    let mut values = [Canvas3dProjectedRgba8 {
        packed_xy: 0,
        rgba: 0,
        z_q16: 0,
        source_index: 0,
    }; CANVAS3D_PLANE_SAMPLE_COUNT];
    unsafe {
        let out = state.canvas3d_out_virt as *const Canvas3dProjectedRgba8;
        for (index, value) in values.iter_mut().enumerate() {
            *value =
                core::ptr::read_volatile(out.add(CANVAS3D_PLANE_SAMPLE_OUT_FIRST as usize + index));
        }
    }
    values
}

fn direct_rcs_read_canvas3d_transform_result(
    state: DirectRcsState,
    expected: [Canvas3dVec3Q16; CANVAS3D_TRANSFORM_TEST_COUNT_USIZE],
) -> (usize, usize, bool, Canvas3dVec3Q16, Canvas3dVec3Q16) {
    intel::dma_flush(state.clear_test_virt, CANVAS3D_PROJECT_VERTEX_BYTES);
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    let mut matches = 0usize;
    let mut src_preserved = 0usize;
    let mut guards_ok = true;
    let mut first = Canvas3dVec3Q16::default();
    let mut last = Canvas3dVec3Q16::default();

    unsafe {
        let src = state.clear_test_virt as *const Canvas3dVec3Q16;
        let dst = state.canvas3d_out_virt as *const Canvas3dVec3Q16;
        for offset in 0..CANVAS3D_TRANSFORM_TEST_COUNT_USIZE {
            let src_index = CANVAS3D_TRANSFORM_SRC_FIRST as usize + offset;
            let dst_index = CANVAS3D_TRANSFORM_DST_FIRST as usize + offset;
            let src_value = core::ptr::read_volatile(src.add(src_index));
            let dst_value = core::ptr::read_volatile(dst.add(dst_index));
            if src_value == direct_rcs_transform_seed_vertex(src_index) {
                src_preserved += 1;
            }
            if dst_value == expected[offset] {
                matches += 1;
            }
            if offset == 0 {
                first = dst_value;
            }
            if offset + 1 == CANVAS3D_TRANSFORM_TEST_COUNT_USIZE {
                last = dst_value;
            }
        }
        let before_index = CANVAS3D_TRANSFORM_DST_FIRST as usize - 1;
        let after_index =
            CANVAS3D_TRANSFORM_DST_FIRST as usize + CANVAS3D_TRANSFORM_TEST_COUNT_USIZE;
        guards_ok &=
            core::ptr::read_volatile(dst.add(before_index)) == CANVAS3D_TRANSFORM_DST_POISON;
        guards_ok &=
            core::ptr::read_volatile(dst.add(after_index)) == CANVAS3D_TRANSFORM_DST_POISON;
    }

    (matches, src_preserved, guards_ok, first, last)
}

fn direct_rcs_read_canvas3d_clip_box_result(
    state: DirectRcsState,
    expected: [Canvas3dVec3Q16; CANVAS3D_TRANSFORM_TEST_COUNT_USIZE],
) -> (usize, usize, bool, Canvas3dVec3Q16, Canvas3dVec3Q16) {
    intel::dma_flush(state.clear_test_virt, CANVAS3D_PROJECT_VERTEX_BYTES);
    intel::dma_flush(state.canvas3d_out_virt, CANVAS3D_PROJECT_OUT_ALLOC_BYTES);
    let mut matches = 0usize;
    let mut src_preserved = 0usize;
    let mut guards_ok = true;
    let mut first = Canvas3dVec3Q16::default();
    let mut last = Canvas3dVec3Q16::default();

    unsafe {
        let src = state.clear_test_virt as *const Canvas3dVec3Q16;
        let dst = state.canvas3d_out_virt as *const Canvas3dVec3Q16;
        for offset in 0..CANVAS3D_TRANSFORM_TEST_COUNT_USIZE {
            let src_index = CANVAS3D_TRANSFORM_SRC_FIRST as usize + offset;
            let dst_index = CANVAS3D_TRANSFORM_DST_FIRST as usize + offset;
            let src_value = core::ptr::read_volatile(src.add(src_index));
            let dst_value = core::ptr::read_volatile(dst.add(dst_index));
            if src_value == direct_rcs_transform_seed_vertex(src_index) {
                src_preserved += 1;
            }
            if dst_value == expected[offset] {
                matches += 1;
            }
            if offset == 0 {
                first = dst_value;
            }
            if offset + 1 == CANVAS3D_TRANSFORM_TEST_COUNT_USIZE {
                last = dst_value;
            }
        }
        let before_index = CANVAS3D_TRANSFORM_DST_FIRST as usize - 1;
        let after_index =
            CANVAS3D_TRANSFORM_DST_FIRST as usize + CANVAS3D_TRANSFORM_TEST_COUNT_USIZE;
        guards_ok &=
            core::ptr::read_volatile(dst.add(before_index)) == CANVAS3D_TRANSFORM_DST_POISON;
        guards_ok &=
            core::ptr::read_volatile(dst.add(after_index)) == CANVAS3D_TRANSFORM_DST_POISON;
    }

    (matches, src_preserved, guards_ok, first, last)
}

fn direct_rcs_canvas3d_project_count_matching(
    values: [Canvas3dProjectedRgba8; CANVAS3D_PROJECT_SAMPLE_COUNT],
    expected: [Canvas3dProjectedRgba8; CANVAS3D_PROJECT_SAMPLE_COUNT],
) -> usize {
    let mut count = 0usize;
    for index in 0..values.len() {
        if values[index] == expected[index] {
            count += 1;
        }
    }
    count
}

fn direct_rcs_canvas3d_project_count_visible(
    values: [Canvas3dProjectedRgba8; CANVAS3D_PROJECT_SAMPLE_COUNT],
) -> usize {
    let mut count = 0usize;
    for value in values {
        if (value.packed_xy & 0x8000_0000) != 0 {
            count += 1;
        }
    }
    count
}

fn direct_rcs_canvas3d_plane_sample_count_matching(
    values: [Canvas3dProjectedRgba8; CANVAS3D_PLANE_SAMPLE_COUNT],
    expected: [Canvas3dProjectedRgba8; CANVAS3D_PLANE_SAMPLE_COUNT],
) -> usize {
    let mut count = 0usize;
    for index in 0..values.len() {
        if values[index] == expected[index] {
            count += 1;
        }
    }
    count
}

fn direct_rcs_canvas3d_plane_sample_count_visible(
    values: [Canvas3dProjectedRgba8; CANVAS3D_PLANE_SAMPLE_COUNT],
) -> usize {
    let mut count = 0usize;
    for value in values {
        if (value.packed_xy & 0x8000_0000) != 0 {
            count += 1;
        }
    }
    count
}
