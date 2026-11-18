//! Read-only scanout snapshots used during primary plane initialization.

use super::{
    PIPES, PLANE_CTL_ENABLE, PLANE_CTL_ORDER_RGBX, PipeInfo, UNI_PLANE_AUX_DIST_OFF,
    UNI_PLANE_AUX_OFFSET_OFF, UNI_PLANE_BUF_CFG_OFF, UNI_PLANE_COLOR_CTL_OFF, UNI_PLANE_KEYMAX_OFF,
    UNI_PLANE_KEYMSK_OFF, UNI_PLANE_KEYVAL_OFF, UNI_PLANE_OFFSET_OFF, UNI_PLANE_POS_OFF,
    UNI_PLANE_SIZE_OFF, decode_pipe_src, decode_plane_color_alpha, decode_plane_format,
    decode_plane_rotation, decode_plane_tiling, decode_xy_x, decode_xy_y,
};

pub(super) fn log_pipe_scanout_probe(dev: crate::intel::Dev, label: &str) {
    for pipe in PIPES {
        let pipe_src_raw = crate::intel::mmio_read(dev, pipe.pipe_src_off);
        let pipe_src_dims = decode_pipe_src(pipe_src_raw);
        let plane_ctl = crate::intel::mmio_read(dev, pipe.primary_plane().ctl());
        let plane_surf = crate::intel::mmio_read(dev, pipe.primary_plane().surf());
        let plane_surf_live = crate::intel::mmio_read(dev, pipe.primary_plane().surf_live());
        let (width, height) = pipe_src_dims.unwrap_or((0, 0));
        intel_display_verbose_log!(
            "intel/display: pipe-probe label={} pipe={} pipe_src=0x{:08X} dims={}x{} plane_enabled={} surf=0x{:08X} surf_live=0x{:08X}\n",
            label,
            pipe.name,
            pipe_src_raw,
            width,
            height,
            ((plane_ctl & PLANE_CTL_ENABLE) != 0) as u8,
            plane_surf,
            plane_surf_live
        );
    }
}

pub(super) fn log_primary_plane_probe(dev: crate::intel::Dev, pipe: PipeInfo, label: &str) {
    let ctl = crate::intel::mmio_read(dev, pipe.primary_plane().ctl());
    let stride = crate::intel::mmio_read(dev, pipe.primary_plane().stride());
    let pos = crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_POS_OFF);
    let size = crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_SIZE_OFF);
    let keyval = crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_KEYVAL_OFF);
    let keymsk = crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_KEYMSK_OFF);
    let surf = crate::intel::mmio_read(dev, pipe.primary_plane().surf());
    let keymax = crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_KEYMAX_OFF);
    let offset = crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_OFFSET_OFF);
    let surf_live = crate::intel::mmio_read(dev, pipe.primary_plane().surf_live());
    let aux_dist =
        crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_AUX_DIST_OFF);
    let aux_offset =
        crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_AUX_OFFSET_OFF);
    let color_ctl =
        crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_COLOR_CTL_OFF);
    let buf_cfg = crate::intel::mmio_read(dev, pipe.primary_plane().base() + UNI_PLANE_BUF_CFG_OFF);

    intel_display_verbose_log!(
        "intel/display: primary-probe label={} pipe={} ctl=0x{:08X} enabled={} format={} tiled={} rot={} rgbx={} stride=0x{:08X} pos={}x{} size={}x{} offset={}x{} surf=0x{:08X} surf_live=0x{:08X} color_ctl=0x{:08X} color_alpha={} key=0x{:08X}/0x{:08X}/0x{:08X} aux=0x{:08X}/0x{:08X} buf_cfg=0x{:08X}\n",
        label,
        pipe.name,
        ctl,
        ((ctl & PLANE_CTL_ENABLE) != 0) as u8,
        decode_plane_format(ctl),
        decode_plane_tiling(ctl),
        decode_plane_rotation(ctl),
        ((ctl & PLANE_CTL_ORDER_RGBX) != 0) as u8,
        stride,
        decode_xy_x(pos),
        decode_xy_y(pos),
        decode_xy_x(size).saturating_add(1),
        decode_xy_y(size).saturating_add(1),
        decode_xy_x(offset),
        decode_xy_y(offset),
        surf,
        surf_live,
        color_ctl,
        decode_plane_color_alpha(color_ctl),
        keyval,
        keymsk,
        keymax,
        aux_dist,
        aux_offset,
        buf_cfg
    );
}

pub(super) fn log_primary_dimensions_probe(
    pipe_name: &str,
    pipe_src_raw: u32,
    pipe_src_dims: Option<(u32, u32)>,
    fb_dims: Option<(u32, u32)>,
    chosen_from: &str,
) {
    let (pipe_w, pipe_h) = pipe_src_dims.unwrap_or((0, 0));
    let (fb_w, fb_h) = fb_dims.unwrap_or((0, 0));
    intel_display_verbose_log!(
        "intel/display: primary-dims pipe={} pipe_src=0x{:08X} pipe_decoded={}x{} fb_hint={}x{} chosen={}\n",
        pipe_name,
        pipe_src_raw,
        pipe_w,
        pipe_h,
        fb_w,
        fb_h,
        chosen_from
    );
}
