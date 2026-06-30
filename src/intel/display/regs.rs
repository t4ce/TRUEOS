pub(in crate::intel) const PIPE_A_SRC: usize = 0x7001C;
pub(in crate::intel) const PIPE_B_SRC: usize = 0x7101C;
pub(in crate::intel) const PIPE_C_SRC: usize = 0x7201C;
pub(in crate::intel) const PIPE_D_SRC: usize = 0x7301C;
pub(in crate::intel) const PIPECONF_A: usize = 0x70008;
pub(in crate::intel) const TRANS_HTOTAL_A: usize = 0x60000;
pub(in crate::intel) const TRANS_HSYNC_A: usize = 0x60008;
pub(in crate::intel) const TRANS_VTOTAL_A: usize = 0x6000C;
pub(in crate::intel) const TRANS_VSYNC_A: usize = 0x60014;
pub(in crate::intel) const TRANS_DDI_FUNC_CTL_A: usize = 0x60400;
pub(in crate::intel) const TRANS_PSR_CTL_A: usize = 0x60800;
pub(in crate::intel) const TRANS_PSR_STATUS_A: usize = 0x60840;
pub(in crate::intel) const TRANS_PSR2_CTL_A: usize = 0x60900;
pub(in crate::intel) const TRANS_PSR2_STATUS_A: usize = 0x60940;
pub(in crate::intel) const HSW_PWR_WELL_CTL1: usize = 0x45400;
pub(in crate::intel) const HSW_PWR_WELL_CTL2: usize = 0x45404;
pub(in crate::intel) const HSW_PWR_WELL_CTL3: usize = 0x45408;
pub(in crate::intel) const HSW_PWR_WELL_CTL4: usize = 0x4540C;
pub(in crate::intel) const ICL_PWR_WELL_CTL_AUX1: usize = 0x45440;
pub(in crate::intel) const ICL_PWR_WELL_CTL_AUX2: usize = 0x45444;
pub(in crate::intel) const ICL_PWR_WELL_CTL_AUX4: usize = 0x4544C;
pub(in crate::intel) const ICL_PWR_WELL_CTL_DDI1: usize = 0x45450;
pub(in crate::intel) const ICL_PWR_WELL_CTL_DDI2: usize = 0x45454;
pub(in crate::intel) const ICL_PWR_WELL_CTL_DDI4: usize = 0x4545C;
pub(in crate::intel) const SKL_FUSE_STATUS: usize = 0x42000;
pub(in crate::intel) const CUR_SURFLIVE_A: usize = 0x700AC;
pub(in crate::intel) const PIPE_FRMCOUNT_A: usize = 0x70040;
pub(in crate::intel) const PIPE_MMIO_STRIDE: usize = 0x1000;
pub(in crate::intel) const SKL_BOTTOM_COLOR_A: usize = 0x70034;
pub(in crate::intel) const SKL_BOTTOM_COLOR_PIPE_STRIDE: usize = 0x1000;
pub(in crate::intel) const UNI_PLANE_BASE: usize = 0x70180;
pub(in crate::intel) const UNI_PLANE_PIPE_STRIDE: usize = 0x1000;
pub(in crate::intel) const UNI_PLANE_SLOT_STRIDE: usize = 0x100;
pub(in crate::intel) const UNI_PLANE_CTL_OFF: usize = 0x00;
pub(in crate::intel) const UNI_PLANE_STRIDE_OFF: usize = 0x08;
pub(in crate::intel) const UNI_PLANE_POS_OFF: usize = 0x0C;
pub(in crate::intel) const UNI_PLANE_SIZE_OFF: usize = 0x10;
pub(in crate::intel) const UNI_PLANE_KEYVAL_OFF: usize = 0x14;
pub(in crate::intel) const UNI_PLANE_KEYMSK_OFF: usize = 0x18;
pub(in crate::intel) const UNI_PLANE_SURF_OFF: usize = 0x1C;
pub(in crate::intel) const UNI_PLANE_KEYMAX_OFF: usize = 0x20;
pub(in crate::intel) const UNI_PLANE_OFFSET_OFF: usize = 0x24;
pub(in crate::intel) const UNI_PLANE_SURFLIVE_OFF: usize = 0x2C;
pub(in crate::intel) const UNI_PLANE_AUX_DIST_OFF: usize = 0x40;
pub(in crate::intel) const UNI_PLANE_AUX_OFFSET_OFF: usize = 0x44;
pub(in crate::intel) const UNI_PLANE_CUS_CTL_OFF: usize = 0x48;
pub(in crate::intel) const UNI_PLANE_COLOR_CTL_OFF: usize = 0x4C;
pub(in crate::intel) const UNI_PLANE_INPUT_CSC_COEFF_OFF: usize = 0x60;
pub(in crate::intel) const UNI_PLANE_INPUT_CSC_PREOFF_OFF: usize = 0x78;
pub(in crate::intel) const UNI_PLANE_INPUT_CSC_POSTOFF_OFF: usize = 0x84;
pub(in crate::intel) const UNI_PLANE_WM_0_OFF: usize = 0xC0;
pub(in crate::intel) const UNI_PLANE_WM_LEVELS: usize = 8;
pub(in crate::intel) const UNI_PLANE_WM_SAGV_OFF: usize = 0xD8;
pub(in crate::intel) const UNI_PLANE_WM_SAGV_TRANS_OFF: usize = 0xDC;
pub(in crate::intel) const UNI_PLANE_WM_TRANS_OFF: usize = 0xE8;
pub(in crate::intel) const UNI_PLANE_BUF_CFG_OFF: usize = 0xFC;
pub(in crate::intel) const PLANE_CTL_ENABLE: u32 = 1 << 31;
pub(in crate::intel) const PLANE_CTL_ARB_SLOTS_MASK: u32 = 0x07 << 28;
pub(in crate::intel) const PLANE_CTL_ARB_SLOTS_4BPP: u32 = 1 << 28;
pub(in crate::intel) const PLANE_CTL_FORMAT_MASK_SKL: u32 = 0x0F << 24;
pub(in crate::intel) const PLANE_CTL_ORDER_RGBX: u32 = 1 << 20;
pub(in crate::intel) const PLANE_CTL_YUV420_Y_PLANE: u32 = 1 << 19;
pub(in crate::intel) const PLANE_CTL_KEY_ENABLE_MASK: u32 = 0x03 << 21;
pub(in crate::intel) const PLANE_CTL_TILED_MASK: u32 = 0x07 << 10;
pub(in crate::intel) const PLANE_CTL_ROTATE_MASK: u32 = 0x03;
pub(in crate::intel) const PLANE_CTL_FORMAT_NV12: u32 = 1 << 24;
pub(in crate::intel) const PLANE_CTL_FORMAT_XRGB_8888: u32 = 4 << 24;
pub(in crate::intel) const PLANE_CTL_TILED_LINEAR: u32 = 0 << 10;
pub(in crate::intel) const PLANE_CTL_TILED_Y: u32 = 4 << 10;
pub(in crate::intel) const PLANE_CTL_TILED_YF: u32 = 5 << 10;
pub(in crate::intel) const PLANE_COLOR_ALPHA_MASK: u32 = 0x03 << 4;
pub(in crate::intel) const PLANE_COLOR_ALPHA_DISABLE: u32 = 0x00 << 4;
pub(in crate::intel) const PLANE_COLOR_ALPHA_SW_PREMULT: u32 = 0x02 << 4;
pub(in crate::intel) const PLANE_COLOR_ALPHA_HW_PREMULT: u32 = 0x03 << 4;
pub(in crate::intel) const PLANE_COLOR_YUV_RANGE_CORRECTION_DISABLE: u32 = 1 << 28;
pub(in crate::intel) const PLANE_COLOR_PIPE_CSC_ENABLE: u32 = 1 << 23;
pub(in crate::intel) const PLANE_COLOR_PLANE_CSC_ENABLE: u32 = 1 << 21;
pub(in crate::intel) const PLANE_COLOR_INPUT_CSC_ENABLE: u32 = 1 << 20;
pub(in crate::intel) const PLANE_COLOR_CSC_MODE_MASK: u32 = 0x07 << 17;
pub(in crate::intel) const PLANE_COLOR_CSC_MODE_BYPASS: u32 = 0x00 << 17;
pub(in crate::intel) const PLANE_COLOR_CSC_MODE_YUV601_TO_RGB601: u32 = 0x01 << 17;
pub(in crate::intel) const PLANE_COLOR_CSC_MODE_YUV709_TO_RGB709: u32 = 0x02 << 17;
pub(in crate::intel) const PLANE_COLOR_CSC_MODE_YUV2020_TO_RGB2020: u32 = 0x03 << 17;
pub(in crate::intel) const PLANE_COLOR_PLANE_GAMMA_DISABLE: u32 = 1 << 13;
pub(in crate::intel) const PLANE_CUS_ENABLE: u32 = 1 << 31;
pub(in crate::intel) const PLANE_CUS_Y_PLANE: u32 = 1 << 30;
pub(in crate::intel) const PLANE_CUS_HPHASE_SIGN_NEGATIVE: u32 = 1 << 19;
pub(in crate::intel) const PLANE_CUS_HPHASE_MASK: u32 = 0x03 << 16;
pub(in crate::intel) const PLANE_CUS_HPHASE_0: u32 = 0 << 16;
pub(in crate::intel) const PLANE_CUS_HPHASE_0_25: u32 = 1 << 16;
pub(in crate::intel) const PLANE_CUS_HPHASE_0_5: u32 = 2 << 16;
pub(in crate::intel) const PLANE_CUS_VPHASE_SIGN_NEGATIVE: u32 = 1 << 15;
pub(in crate::intel) const PLANE_CUS_VPHASE_MASK: u32 = 0x03 << 12;
pub(in crate::intel) const PLANE_CUS_VPHASE_0: u32 = 0 << 12;
pub(in crate::intel) const PLANE_CUS_VPHASE_0_25: u32 = 1 << 12;
pub(in crate::intel) const PLANE_CUS_VPHASE_0_5: u32 = 2 << 12;
pub(in crate::intel) const PLANE_WM_ENABLE: u32 = 1 << 31;
pub(in crate::intel) const PLANE_WM_LEVEL0_BOOT_SAFE: u32 = PLANE_WM_ENABLE | (2 << 14) | 160;
pub(in crate::intel) const PLANE_DBUF_PRIMARY_STACK_START: u16 = 0;
pub(in crate::intel) const PLANE_DBUF_PRIMARY_STACK_END: u16 = 511;
pub(in crate::intel) const PLANE_DBUF_UI_OVERLAY_STACK_START: u16 = 512;
pub(in crate::intel) const PLANE_DBUF_UI_OVERLAY_STACK_END: u16 = 767;
pub(in crate::intel) const PLANE_DBUF_VIDEO_NV12_UV_STACK_START: u16 = 768;
pub(in crate::intel) const PLANE_DBUF_VIDEO_NV12_UV_STACK_END: u16 = 895;
pub(in crate::intel) const PLANE_DBUF_VIDEO_NV12_Y_STACK_START: u16 = 896;
pub(in crate::intel) const PLANE_DBUF_VIDEO_NV12_Y_STACK_END: u16 = 1023;

#[derive(Copy, Clone)]
pub(in crate::intel) struct PipeInfo {
    pub(in crate::intel) name: &'static str,
    pub(in crate::intel) slot: usize,
    pub(in crate::intel) pipe_src_off: usize,
}

impl PipeInfo {
    pub(in crate::intel) const fn plane(self, slot: usize) -> PlaneId {
        PlaneId { pipe: self, slot }
    }

    pub(in crate::intel) const fn primary_plane(self) -> PlaneId {
        self.plane(0)
    }
}

#[derive(Copy, Clone)]
pub(in crate::intel) struct PlaneId {
    pub(in crate::intel) pipe: PipeInfo,
    pub(in crate::intel) slot: usize,
}

impl PlaneId {
    pub(in crate::intel) const fn base(self) -> usize {
        UNI_PLANE_BASE + self.pipe.slot * UNI_PLANE_PIPE_STRIDE + self.slot * UNI_PLANE_SLOT_STRIDE
    }

    pub(in crate::intel) const fn ctl(self) -> usize {
        self.base() + UNI_PLANE_CTL_OFF
    }

    pub(in crate::intel) const fn stride(self) -> usize {
        self.base() + UNI_PLANE_STRIDE_OFF
    }

    pub(in crate::intel) const fn surf(self) -> usize {
        self.base() + UNI_PLANE_SURF_OFF
    }

    pub(in crate::intel) const fn surf_live(self) -> usize {
        self.base() + UNI_PLANE_SURFLIVE_OFF
    }

    pub(in crate::intel) const fn buf_cfg(self) -> usize {
        self.base() + UNI_PLANE_BUF_CFG_OFF
    }
}

pub(in crate::intel) const PIPES: [PipeInfo; 4] = [
    PipeInfo {
        name: "pipe-a",
        slot: 0,
        pipe_src_off: PIPE_A_SRC,
    },
    PipeInfo {
        name: "pipe-b",
        slot: 1,
        pipe_src_off: PIPE_B_SRC,
    },
    PipeInfo {
        name: "pipe-c",
        slot: 2,
        pipe_src_off: PIPE_C_SRC,
    },
    PipeInfo {
        name: "pipe-d",
        slot: 3,
        pipe_src_off: PIPE_D_SRC,
    },
];

pub(in crate::intel) fn plane_buf_cfg_for_pipe_slot(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    plane_slot: usize,
) -> u32 {
    crate::intel::mmio_read(dev, pipe.plane(plane_slot).buf_cfg())
}

pub(in crate::intel) fn decode_pipe_src(value: u32) -> Option<(u32, u32)> {
    if value == 0 || value == u32::MAX {
        return None;
    }
    let width = (value & 0xFFFF).saturating_add(1);
    let height = ((value >> 16) & 0xFFFF).saturating_add(1);
    if !(320..=8192).contains(&width) || !(200..=4320).contains(&height) {
        return None;
    }
    Some((width, height))
}

pub(in crate::intel) fn framebuffer_hint() -> Option<(u32, u32)> {
    let fb = crate::limine::framebuffer_response()?
        .framebuffers()
        .first()
        .copied()?;
    Some((fb.width as u32, fb.height as u32))
}

pub(in crate::intel) fn aligned_pitch_bytes(width: u32, bytes_per_pixel: u32) -> Option<u32> {
    let bytes = width.checked_mul(bytes_per_pixel)?;
    let aligned = crate::intel::align_up(bytes as usize, 64)?;
    u32::try_from(aligned).ok()
}

pub(in crate::intel) fn plane_pos_reg_value(x: u32, y: u32) -> u32 {
    ((y & 0xFFFF) << 16) | (x & 0xFFFF)
}

pub(in crate::intel) fn plane_size_reg_value(width: u32, height: u32) -> u32 {
    let enc_w = width.saturating_sub(1) & 0xFFFF;
    let enc_h = height.saturating_sub(1) & 0xFFFF;
    (enc_h << 16) | enc_w
}

pub(in crate::intel) fn plane_stride_reg_value(pitch_bytes: u32) -> Option<u32> {
    if pitch_bytes == 0 || !pitch_bytes.is_multiple_of(64) {
        None
    } else {
        Some(pitch_bytes / 64)
    }
}

pub(in crate::intel) fn fill_surface_color(
    ptr: *mut u8,
    pitch_bytes: usize,
    width: u32,
    height: u32,
    color: u32,
) {
    let width = width as usize;
    let height = height as usize;
    unsafe {
        for y in 0..height {
            let row = ptr.add(y.saturating_mul(pitch_bytes)) as *mut u32;
            for x in 0..width {
                core::ptr::write_volatile(row.add(x), color);
            }
        }
    }
}
