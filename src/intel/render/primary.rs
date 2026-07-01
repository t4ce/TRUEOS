pub(crate) fn submit_primary_triangle_once() {
    if PRIMARY_TRIANGLE_SUBMITTED.swap(true, Ordering::AcqRel) {
        return;
    }

    let _ = submit_primary_probe_now("boot-once");
}

pub(crate) fn submit_primary_probe_periodic() {
    let _ = submit_primary_probe_now("periodic");
}

pub(crate) struct RenderJokerResult {
    pub(crate) variant: &'static str,
    pub(crate) submit_name: &'static str,
    pub(crate) target: &'static str,
    pub(crate) completed: bool,
}

pub(crate) struct RenderOaControlResult {
    pub(crate) action: &'static str,
    pub(crate) oactx: u32,
    pub(crate) oar: u32,
    pub(crate) ctx_ctrl: u32,
}

pub(crate) struct RenderArtificialFragmentResult {
    pub(crate) mode: &'static str,
    pub(crate) ok: bool,
    pub(crate) descs: usize,
    pub(crate) before: u32,
    pub(crate) after: u32,
    pub(crate) rt_gpu: u64,
    pub(crate) remapped_render: bool,
}

const RENDER_JOKER_VARIANTS: &[&str] = &[
    "canonical",
    "mesa",
    "mesa-retire",
    "bt0",
    "bt0-primary",
    "scratch",
    "oa",
    "point",
    "point-scratch",
    "point-oa",
    "point-oa-pos0",
    "point-oa-header",
    "point-oa-killoff",
    "point-oa-smooth",
    "point-oa-msrast",
    "point-oa-msrast-force",
    "point-oa-deref0",
    "point-oa-hz0",
    "point-oa-wm-normal",
    "point-oa-wm-reemit",
    "point-oa-hz-omit",
    "point-oa-ps-off",
    "point-oa-bt1",
    "point-oa-early",
    "point-oa-early-killoff",
    "point-oa-clip-normal",
    "point-oa-clip-persp",
    "point-oa-clip-disable",
    "point-oa-clip-disable-arm",
    "point-oa-clip-force",
    "point-oa-clip-d3d",
    "point-oa-clip-xy",
    "point-oa-sbe0",
    "point-oa-sbe-pre-clip",
    "point-oa-sbe-pre-sf",
    "point-oa-no-pr",
    "point-oa-vfg",
    "point-oa-w64",
    "point-oa-w64-early",
    "point-oa-w64-early-scissor",
    "point-oa-screen-w64",
    "point-oa-w64-arm",
    "point-oa-w64-wm-normal",
    "point-oa-w64-wm-reemit",
    "point-oa-w64-hz-omit",
    "point-oa-w64-ps-off",
    "point-oa-w64-payload-attr",
    "point-oa-w64-payload-depthw",
    "point-oa-w64-payload-bary",
    "point-oa-w64-sbe-pre-clip",
    "point-oa-w64-sbe-pre-sf",
    "point-oa-w1023",
    "point-oa-w1023-nowmpoint",
    "point-oa-w1023-scissor",
    "point-oa-vtxw",
    "point-oa-early-w1023",
    "point-oa-early-msrast-force",
    "point-bt1",
    "point-slot0",
    "screen-vs-scratch",
    "screen-vs-oa",
    "screen-vs-ndc-oa",
    "screen-vs-ndc-oa-hz0",
    "screen-vs-sbe0",
    "screen-vs-slot0-oa",
    "screen-vs-urb2-oa",
    "screen-vs-urb2-slot0-oa",
    "vf-rect-oa",
    "vf-rect-oa-pos0",
    "vf-rect-oa-header",
    "vf-rect-oa-deref0",
    "vf-rect-ndc-oa",
    "vf-rect-ndc-oa-sbe-pre-clip",
    "vf-rect-ndc-oa-sbe-pre-sf",
    "vf-rect-ndc-oa-drawrect-early",
    "vf-rect-ndc-oa-sample-early",
    "vf-rect-ndc-oa-pc-clip-sf",
    "vf-rect-ndc-oa-hz-pre-wm",
    "vf-rect-ndc-oa-hz-post-extra",
    "vf-rect-ndc-oa-payload-attr",
    "vf-rect-ndc-oa-payload-depthw",
    "vf-rect-ndc-oa-payload-bary",
    "vf-rect-ndc-oa-persp",
    "vf-rect-ndc-oa-clipxy",
    "vf-rect-ndc-oa-clip-disable",
    "vf-rect-ndc-oa-clip-force",
    "vf-rect-ndc-oa-clip-d3d",
    "vf-rect-ndc-oa-early-clipxy",
    "vf-rect-ndc-oa-frontccw",
    "vf-rect-ndc-oa-hz0",
    "vf-rect-ndc-oa-early",
    "vf-rect-ndc-oa-bt1",
    "vf-rect-ndc-order-b-oa",
    "vf-rect-ndc-order-c-oa",
    "vf-rect-ndc-order-c-early-oa",
    "vf-rect-ndc-order-c-clip-disable-oa",
    "vf-rect-ndc-mesa-simple-oa",
    "vf-rect-ndc-mesa-nosrc-header-oa",
    "vf-rect-ndc-small-oa",
    "vf-rect-ndc-cw-oa",
    "vf-rect-ndc-alt-oa",
    "vf-rect-order-b-oa",
    "vf-rect-order-b-early-oa",
    "vf-rect-order-b-scissor-oa",
    "vf-rect-mesa-simple-oa",
    "vf-rect-mesa-simple-oa-early",
    "vf-rect-mesa-simple-oa-arm",
    "vf-rect-mesa-nosrc-header-oa",
    "vf-rect-order-c-oa",
    "vf-tri-ndc-oa",
    "vf-tri-ndc-oa-early",
    "vf-tri-ndc-oa-early-clipxy",
    "vf-tri-ndc-cw-oa-early",
    "screen-rect-scratch",
    "screen-rect-oa-early",
    "so-vf",
    "so-vf-header",
    "so-vs",
    "so-vs-header",
    "bt1",
    "wm-normal",
    "slot0",
    "slot1",
    "slot2",
    "all",
    "simd16",
    "simd16-retire",
    "eot",
    "eot-retire",
    "cps",
    "cps-retire",
    "hz",
    "hz-retire",
    "reemit",
    "reemit-retire",
    "reemit-vs-retire",
    "reemit-vs-slot0-retire",
    "reemit-vs-urb2-retire",
    "reemit-vs-urb2-slot0-retire",
    "payload-push",
    "payload-attr",
    "payload-simple",
    "payload-depthw",
    "payload-bary",
    "grf1",
    "grf2",
    "grf4",
    "mt31",
    "mt15",
    "sync-light",
    "sync-post-no-cs",
    "sync-cs-no-post",
];

pub(crate) fn render_joker_variant_names() -> &'static [&'static str] {
    RENDER_JOKER_VARIANTS
}

pub(crate) fn render_oa_control_action_names() -> &'static [&'static str] {
    &[
        "status",
        "selectors",
        "ctx-on",
        "ctx-off",
        "oactx-on",
        "oactx-off",
        "oar-on",
        "oar-off",
        "full-on",
        "full-off",
    ]
}

fn retired_render_joker_variant_reason(name: &str) -> Option<&'static str> {
    if name.eq_ignore_ascii_case("point-oa-w8")
        || name.eq_ignore_ascii_case("point-oa-w8-clipmax")
        || name.eq_ignore_ascii_case("point-oa-w64-clipmax")
    {
        Some("retired-invalid-point-width-hw-contract")
    } else {
        None
    }
}

pub(crate) fn render_oa_control_action(
    action: &str,
) -> Result<RenderOaControlResult, &'static str> {
    let Some(dev) = crate::intel::claimed_device() else {
        return Err("no-device");
    };
    if !forcewake_render_acquire(warm_once(dev)) {
        return Err("forcewake");
    }

    let action = if action.eq_ignore_ascii_case("status") {
        "status"
    } else if action.eq_ignore_ascii_case("selectors") {
        "selectors"
    } else if action.eq_ignore_ascii_case("ctx-on") {
        "ctx-on"
    } else if action.eq_ignore_ascii_case("ctx-off") {
        "ctx-off"
    } else if action.eq_ignore_ascii_case("oactx-on") {
        "oactx-on"
    } else if action.eq_ignore_ascii_case("oactx-off") {
        "oactx-off"
    } else if action.eq_ignore_ascii_case("oar-on") {
        "oar-on"
    } else if action.eq_ignore_ascii_case("oar-off") {
        "oar-off"
    } else if action.eq_ignore_ascii_case("full-on") {
        "full-on"
    } else if action.eq_ignore_ascii_case("full-off") {
        "full-off"
    } else {
        return Err("unknown-action");
    };

    let before_oactx = crate::intel::mmio_read(dev, RCS_OACTXCONTROL);
    let before_oar = crate::intel::mmio_read(dev, OAR_OACONTROL);
    let before_ctx = crate::intel::mmio_read(dev, RCS_RING_CONTEXT_CONTROL);
    intel_render_focus_log!(
        "intel/render: oa-control begin action={} oactx=0x{:08X} oar=0x{:08X} ctx_ctrl=0x{:08X}\n",
        action,
        before_oactx,
        before_oar,
        before_ctx,
    );

    match action {
        "status" => {}
        "selectors" => write_raster_wm_oa_selectors(dev),
        "ctx-on" => crate::intel::mmio_write(
            dev,
            RCS_RING_CONTEXT_CONTROL,
            masked_bits_update(CTX_CTRL_OAC_CONTEXT_ENABLE, 0),
        ),
        "ctx-off" => crate::intel::mmio_write(
            dev,
            RCS_RING_CONTEXT_CONTROL,
            masked_bits_update(0, CTX_CTRL_OAC_CONTEXT_ENABLE),
        ),
        "oactx-on" => crate::intel::mmio_write(dev, RCS_OACTXCONTROL, OACTXCONTROL_COUNTER_RESUME),
        "oactx-off" => crate::intel::mmio_write(dev, RCS_OACTXCONTROL, 0),
        "oar-on" => crate::intel::mmio_write(
            dev,
            OAR_OACONTROL,
            OAR_OACONTROL_FORMAT_A24_A14_B8_C8 | OAR_OACONTROL_COUNTER_ENABLE,
        ),
        "oar-off" => crate::intel::mmio_write(dev, OAR_OACONTROL, 0),
        "full-on" => {
            write_raster_wm_oa_selectors(dev);
            crate::intel::mmio_write(dev, RCS_OACTXCONTROL, OACTXCONTROL_COUNTER_RESUME);
            crate::intel::mmio_write(
                dev,
                OAR_OACONTROL,
                OAR_OACONTROL_FORMAT_A24_A14_B8_C8 | OAR_OACONTROL_COUNTER_ENABLE,
            );
            crate::intel::mmio_write(
                dev,
                RCS_RING_CONTEXT_CONTROL,
                masked_bits_update(CTX_CTRL_OAC_CONTEXT_ENABLE, 0),
            );
        }
        "full-off" => {
            crate::intel::mmio_write(dev, RCS_OACTXCONTROL, 0);
            crate::intel::mmio_write(dev, OAR_OACONTROL, 0);
            crate::intel::mmio_write(
                dev,
                RCS_RING_CONTEXT_CONTROL,
                masked_bits_update(0, CTX_CTRL_OAC_CONTEXT_ENABLE),
            );
        }
        _ => return Err("unknown-action"),
    }

    let after_oactx = crate::intel::mmio_read(dev, RCS_OACTXCONTROL);
    let after_oar = crate::intel::mmio_read(dev, OAR_OACONTROL);
    let after_ctx = crate::intel::mmio_read(dev, RCS_RING_CONTEXT_CONTROL);
    intel_render_focus_log!(
        "intel/render: oa-control end action={} oactx=0x{:08X}->0x{:08X} oar=0x{:08X}->0x{:08X} ctx_ctrl=0x{:08X}->0x{:08X}\n",
        action,
        before_oactx,
        after_oactx,
        before_oar,
        after_oar,
        before_ctx,
        after_ctx,
    );

    Ok(RenderOaControlResult {
        action,
        oactx: after_oactx,
        oar: after_oar,
        ctx_ctrl: after_ctx,
    })
}

fn write_raster_wm_oa_selectors(dev: crate::intel::Dev) {
    crate::intel::mmio_write(dev, OAG_OASTARTTRIG1, 0);
    crate::intel::mmio_write(dev, OAG_OASTARTTRIG2, 0x0080_0000);
    crate::intel::mmio_write(dev, OAG_OASTARTTRIG3, 0);
    crate::intel::mmio_write(dev, OAG_OASTARTTRIG4, 0x0080_0000);
    crate::intel::mmio_write(dev, OAG_OAREPORTTRIG1, 0);
    crate::intel::mmio_write(dev, OAG_SPCTR_CNF, 0);
    crate::intel::mmio_write(dev, OAA_LENABLE_REG, 0);
    crate::intel::mmio_write(dev, OAG_OA_PESS, 0);
}

#[derive(Copy, Clone)]
struct RenderJokerSpec {
    variant: &'static str,
    submit_name: &'static str,
    target: RenderJokerTarget,
    blend: TriangleBlendProbeMode,
    geometry: VfPrimitiveGeometry,
    backend: BackendProbeMode,
    sync: PostDrawSyncVariant,
}

#[derive(Copy, Clone)]
enum RenderJokerTarget {
    Primary,
    ScratchRt,
}

fn parse_render_joker_spec(name: &str) -> Option<RenderJokerSpec> {
    let surface = RenderJokerTarget::Primary;
    let scratch = RenderJokerTarget::ScratchRt;
    let explicit = TriangleBlendProbeMode::ExplicitRt0;
    let zeroed = TriangleBlendProbeMode::MesaZeroedState;
    let canonical = VfPrimitiveGeometry::Canonical;
    let big = VfPrimitiveGeometry::Oversized;
    let point = VfPrimitiveGeometry::CenterPoint;
    let screen_point = VfPrimitiveGeometry::ScreenSpacePoint8x8;
    let screen_space = VfPrimitiveGeometry::ScreenSpace8x8;
    let screen_rect = VfPrimitiveGeometry::ScreenSpaceRect8x8;
    let screen_rect_order_b = VfPrimitiveGeometry::ScreenSpaceRect8x8OrderB;
    let screen_rect_order_c = VfPrimitiveGeometry::ScreenSpaceRect8x8OrderC;
    let ndc_triangle = VfPrimitiveGeometry::NdcTriangleLarge;
    let ndc_triangle_cw = VfPrimitiveGeometry::NdcTriangleLargeCw;
    let ndc_rect = VfPrimitiveGeometry::NdcRect;
    let ndc_rect_cw = VfPrimitiveGeometry::NdcRectCw;
    let ndc_rect_alt = VfPrimitiveGeometry::NdcRectAlt;
    let ndc_rect_order_c = VfPrimitiveGeometry::NdcRectUrLrUl;
    let ndc_rect_small = VfPrimitiveGeometry::NdcRectSmall;
    let heavy = PostDrawSyncVariant::HeavyAll;
    let light_post_no_cs = PostDrawSyncVariant::LightPostSyncNoCs;

    let spec = if name.eq_ignore_ascii_case("canonical") {
        RenderJokerSpec {
            variant: "canonical",
            submit_name: "vf-draw-path",
            target: surface,
            blend: explicit,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mesa") || name.eq_ignore_ascii_case("big") {
        RenderJokerSpec {
            variant: "mesa",
            submit_name: "ps-launch-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mesa-retire") {
        RenderJokerSpec {
            variant: "mesa-retire",
            submit_name: "ps-launch-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::MesaLike,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("bt0") || name.eq_ignore_ascii_case("scratch") {
        RenderJokerSpec {
            variant: if name.eq_ignore_ascii_case("scratch") {
                "scratch"
            } else {
                "bt0"
            },
            submit_name: "ps-bt0-scratch-rt",
            target: scratch,
            blend: zeroed,
            geometry: big,
            backend: BackendProbeMode::PsBindingTableCountZero,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("bt0-primary") {
        RenderJokerSpec {
            variant: "bt0-primary",
            submit_name: "ps-bt0-primary-rt",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsBindingTableCountZero,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("oa") {
        RenderJokerSpec {
            variant: "oa",
            submit_name: "raster-wm-oa-probe",
            target: scratch,
            blend: zeroed,
            geometry: big,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point") || name.eq_ignore_ascii_case("giant-point") {
        RenderJokerSpec {
            variant: "point",
            submit_name: "point-vf-giant",
            target: surface,
            blend: explicit,
            geometry: point,
            backend: BackendProbeMode::MesaLike,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-scratch") {
        RenderJokerSpec {
            variant: "point-scratch",
            submit_name: "point-vf-giant-scratch",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::PsBindingTableCountZero,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa") {
        RenderJokerSpec {
            variant: "point-oa",
            submit_name: "point-vf-giant-oa",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-pos0") {
        RenderJokerSpec {
            variant: "point-oa-pos0",
            submit_name: "point-vf-giant-oa-pos0",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-header") {
        RenderJokerSpec {
            variant: "point-oa-header",
            submit_name: "point-vf-giant-oa-header",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-killoff") {
        RenderJokerSpec {
            variant: "point-oa-killoff",
            submit_name: "point-vf-giant-oa-killoff",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaKillOff,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-smooth") {
        RenderJokerSpec {
            variant: "point-oa-smooth",
            submit_name: "point-vf-giant-oa-smooth",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaSmoothPoint,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-msrast") {
        RenderJokerSpec {
            variant: "point-oa-msrast",
            submit_name: "point-vf-giant-oa-msrast",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaMsRaster,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-msrast-force") {
        RenderJokerSpec {
            variant: "point-oa-msrast-force",
            submit_name: "point-vf-giant-oa-msrast-force",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaMsRasterForced,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-deref0") {
        RenderJokerSpec {
            variant: "point-oa-deref0",
            submit_name: "point-vf-giant-oa-deref0",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaDerefBlock0,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-hz0") {
        RenderJokerSpec {
            variant: "point-oa-hz0",
            submit_name: "point-vf-giant-oa-hz0",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaNoHzOp,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-wm-normal") {
        RenderJokerSpec {
            variant: "point-oa-wm-normal",
            submit_name: "point-vf-giant-oa-wm-normal",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaWmNormalDispatch,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-wm-reemit") {
        RenderJokerSpec {
            variant: "point-oa-wm-reemit",
            submit_name: "point-vf-giant-oa-wm-reemit",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaWmReemitAfterPsExtra,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-hz-omit") {
        RenderJokerSpec {
            variant: "point-oa-hz-omit",
            submit_name: "point-vf-giant-oa-hz-omit",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaOmitHzOp,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-ps-off") {
        RenderJokerSpec {
            variant: "point-oa-ps-off",
            submit_name: "point-vf-giant-oa-ps-off",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPsDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-bt1") {
        RenderJokerSpec {
            variant: "point-oa-bt1",
            submit_name: "point-vf-giant-oa-bt1",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaBtCountOne,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-early") {
        RenderJokerSpec {
            variant: "point-oa-early",
            submit_name: "point-vf-giant-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-early-killoff") {
        RenderJokerSpec {
            variant: "point-oa-early-killoff",
            submit_name: "point-vf-giant-oa-early-killoff",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaEarlyKillOff,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-normal") {
        RenderJokerSpec {
            variant: "point-oa-clip-normal",
            submit_name: "point-vf-giant-oa-clip-normal",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipNormal,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-persp") {
        RenderJokerSpec {
            variant: "point-oa-clip-persp",
            submit_name: "point-vf-giant-oa-clip-persp",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipPerspective,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-disable") {
        RenderJokerSpec {
            variant: "point-oa-clip-disable",
            submit_name: "point-vf-giant-oa-clip-disable",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-disable-arm") {
        RenderJokerSpec {
            variant: "point-oa-clip-disable-arm",
            submit_name: "point-vf-giant-oa-clip-disable-arm",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipDisabledArtificial,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-force") {
        RenderJokerSpec {
            variant: "point-oa-clip-force",
            submit_name: "point-vf-giant-oa-clip-force",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipForceMode,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-d3d") {
        RenderJokerSpec {
            variant: "point-oa-clip-d3d",
            submit_name: "point-vf-giant-oa-clip-d3d",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipApiD3d,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-clip-xy") {
        RenderJokerSpec {
            variant: "point-oa-clip-xy",
            submit_name: "point-vf-giant-oa-clip-xy",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaClipViewportXy,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-sbe0") {
        RenderJokerSpec {
            variant: "point-oa-sbe0",
            submit_name: "point-vf-giant-oa-sbe0",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaSbeRead0,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-sbe-pre-clip") {
        RenderJokerSpec {
            variant: "point-oa-sbe-pre-clip",
            submit_name: "point-vf-giant-oa-sbe-pre-clip",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaSbeBeforeClip,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-sbe-pre-sf") {
        RenderJokerSpec {
            variant: "point-oa-sbe-pre-sf",
            submit_name: "point-vf-giant-oa-sbe-pre-sf",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaSbeBeforeSf,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-no-pr") {
        RenderJokerSpec {
            variant: "point-oa-no-pr",
            submit_name: "point-vf-giant-oa-no-pr",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaNoPrimitiveReplication,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-vfg") {
        RenderJokerSpec {
            variant: "point-oa-vfg",
            submit_name: "point-vf-giant-oa-vfg",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaVfGeometryDistribution,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w8") {
        RenderJokerSpec {
            variant: "point-oa-w8",
            submit_name: "point-vf-giant-oa-w8",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth8,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w8-clipmax") {
        RenderJokerSpec {
            variant: "point-oa-w8-clipmax",
            submit_name: "point-vf-giant-oa-w8-clipmax",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth8ClipMax,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64") {
        RenderJokerSpec {
            variant: "point-oa-w64",
            submit_name: "point-vf-giant-oa-w64",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-halign128") {
        RenderJokerSpec {
            variant: "point-oa-w64-halign128",
            submit_name: "point-vf-giant-oa-w64-halign128",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64SurfaceHalign128,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-clipmax") {
        RenderJokerSpec {
            variant: "point-oa-w64-clipmax",
            submit_name: "point-vf-giant-oa-w64-clipmax",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64ClipMax,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-early") {
        RenderJokerSpec {
            variant: "point-oa-w64-early",
            submit_name: "point-vf-giant-oa-w64-early",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64Early,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-early-scissor") {
        RenderJokerSpec {
            variant: "point-oa-w64-early-scissor",
            submit_name: "point-vf-giant-oa-w64-early-scissor",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64EarlyScissor,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-screen-w64") {
        RenderJokerSpec {
            variant: "point-oa-screen-w64",
            submit_name: "point-vf-screen-oa-w64",
            target: scratch,
            blend: zeroed,
            geometry: screen_point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64Screen,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-arm") {
        RenderJokerSpec {
            variant: "point-oa-w64-arm",
            submit_name: "point-vf-giant-oa-w64-arm",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64Artificial,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-wm-normal") {
        RenderJokerSpec {
            variant: "point-oa-w64-wm-normal",
            submit_name: "point-vf-giant-oa-w64-wm-normal",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64WmNormalDispatch,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-wm-reemit") {
        RenderJokerSpec {
            variant: "point-oa-w64-wm-reemit",
            submit_name: "point-vf-giant-oa-w64-wm-reemit",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64WmReemitAfterPsExtra,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-hz-omit") {
        RenderJokerSpec {
            variant: "point-oa-w64-hz-omit",
            submit_name: "point-vf-giant-oa-w64-hz-omit",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64OmitHzOp,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-ps-off") {
        RenderJokerSpec {
            variant: "point-oa-w64-ps-off",
            submit_name: "point-vf-giant-oa-w64-ps-off",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64PsDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-payload-attr") {
        RenderJokerSpec {
            variant: "point-oa-w64-payload-attr",
            submit_name: "point-vf-giant-oa-w64-payload-attr",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64PayloadAttributeEnable,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-payload-depthw") {
        RenderJokerSpec {
            variant: "point-oa-w64-payload-depthw",
            submit_name: "point-vf-giant-oa-w64-payload-depthw",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64PayloadSourceDepthW,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-payload-bary") {
        RenderJokerSpec {
            variant: "point-oa-w64-payload-bary",
            submit_name: "point-vf-giant-oa-w64-payload-bary",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64PayloadBaryPlanes,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-sbe-pre-clip") {
        RenderJokerSpec {
            variant: "point-oa-w64-sbe-pre-clip",
            submit_name: "point-vf-giant-oa-w64-sbe-pre-clip",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64SbeBeforeClip,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w64-sbe-pre-sf") {
        RenderJokerSpec {
            variant: "point-oa-w64-sbe-pre-sf",
            submit_name: "point-vf-giant-oa-w64-sbe-pre-sf",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth64SbeBeforeSf,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w1023") {
        RenderJokerSpec {
            variant: "point-oa-w1023",
            submit_name: "point-vf-giant-oa-w1023",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth1023,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w1023-nowmpoint") {
        RenderJokerSpec {
            variant: "point-oa-w1023-nowmpoint",
            submit_name: "point-vf-giant-oa-w1023-nowmpoint",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth1023NoWmPoint,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-w1023-scissor") {
        RenderJokerSpec {
            variant: "point-oa-w1023-scissor",
            submit_name: "point-vf-giant-oa-w1023-scissor",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidth1023Scissor,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-vtxw") {
        RenderJokerSpec {
            variant: "point-oa-vtxw",
            submit_name: "point-vf-giant-oa-vtxw",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaPointWidthVertex,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-early-w1023") {
        RenderJokerSpec {
            variant: "point-oa-early-w1023",
            submit_name: "point-vf-giant-oa-early-w1023",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaEarlyPointWidth1023,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-oa-early-msrast-force") {
        RenderJokerSpec {
            variant: "point-oa-early-msrast-force",
            submit_name: "point-vf-giant-oa-early-msrast-force",
            target: scratch,
            blend: zeroed,
            geometry: point,
            backend: BackendProbeMode::RasterWmInputOaEarlyMsRasterForced,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-bt1") {
        RenderJokerSpec {
            variant: "point-bt1",
            submit_name: "point-vf-giant-bt1",
            target: surface,
            blend: explicit,
            geometry: point,
            backend: BackendProbeMode::PsBindingTableCountOne,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("point-slot0") {
        RenderJokerSpec {
            variant: "point-slot0",
            submit_name: "point-vf-giant-slot0",
            target: surface,
            blend: explicit,
            geometry: point,
            backend: BackendProbeMode::PsDispatchSlot0,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-scratch") {
        RenderJokerSpec {
            variant: "screen-vs-scratch",
            submit_name: "screen-vs-scratch",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::PsBindingTableCountZero,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-oa") {
        RenderJokerSpec {
            variant: "screen-vs-oa",
            submit_name: "screen-vs-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-ndc-oa") {
        RenderJokerSpec {
            variant: "screen-vs-ndc-oa",
            submit_name: "screen-vs-ndc-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-ndc-oa-hz0") {
        RenderJokerSpec {
            variant: "screen-vs-ndc-oa-hz0",
            submit_name: "screen-vs-ndc-oa-hz0",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle,
            backend: BackendProbeMode::RasterWmInputOaNoHzOp,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-sbe0") {
        RenderJokerSpec {
            variant: "screen-vs-sbe0",
            submit_name: "screen-vs-sbe0",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-slot0-oa") {
        RenderJokerSpec {
            variant: "screen-vs-slot0-oa",
            submit_name: "screen-vs-slot0-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-urb2-oa") {
        RenderJokerSpec {
            variant: "screen-vs-urb2-oa",
            submit_name: "screen-vs-urb2-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-vs-urb2-slot0-oa") {
        RenderJokerSpec {
            variant: "screen-vs-urb2-slot0-oa",
            submit_name: "screen-vs-urb2-slot0-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_space,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-oa") {
        RenderJokerSpec {
            variant: "vf-rect-oa",
            submit_name: "vf-rect-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-oa-pos0") {
        RenderJokerSpec {
            variant: "vf-rect-oa-pos0",
            submit_name: "vf-rect-oa-pos0",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-oa-header") {
        RenderJokerSpec {
            variant: "vf-rect-oa-header",
            submit_name: "vf-rect-oa-header",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-oa-deref0") {
        RenderJokerSpec {
            variant: "vf-rect-oa-deref0",
            submit_name: "vf-rect-oa-deref0",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::RasterWmInputOaDerefBlock0,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa",
            submit_name: "vf-rect-ndc-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-halign128") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-halign128",
            submit_name: "vf-rect-ndc-oa-halign128",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaSurfaceHalign128,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-sbe-pre-clip") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-sbe-pre-clip",
            submit_name: "vf-rect-ndc-oa-sbe-pre-clip",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaSbeBeforeClip,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-sbe-pre-sf") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-sbe-pre-sf",
            submit_name: "vf-rect-ndc-oa-sbe-pre-sf",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaSbeBeforeSf,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-drawrect-early") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-drawrect-early",
            submit_name: "vf-rect-ndc-oa-drawrect-early",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaDrawRectEarlyOnly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-sample-early") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-sample-early",
            submit_name: "vf-rect-ndc-oa-sample-early",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaSampleMaskEarlyOnly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-pc-clip-sf") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-pc-clip-sf",
            submit_name: "vf-rect-ndc-oa-pc-clip-sf",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaPipeControlClipSf,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-hz-pre-wm") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-hz-pre-wm",
            submit_name: "vf-rect-ndc-oa-hz-pre-wm",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaWmHzOpBeforeWm,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-hz-post-extra") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-hz-post-extra",
            submit_name: "vf-rect-ndc-oa-hz-post-extra",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaWmHzOpAfterPsExtra,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-payload-attr") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-payload-attr",
            submit_name: "vf-rect-ndc-oa-payload-attr",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaPayloadAttributeEnable,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-payload-depthw") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-payload-depthw",
            submit_name: "vf-rect-ndc-oa-payload-depthw",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaPayloadSourceDepthW,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-payload-bary") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-payload-bary",
            submit_name: "vf-rect-ndc-oa-payload-bary",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaPayloadBaryPlanes,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-persp") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-persp",
            submit_name: "vf-rect-ndc-oa-persp",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaClipPerspective,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-clipxy") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-clipxy",
            submit_name: "vf-rect-ndc-oa-clipxy",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaClipViewportXy,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-clip-disable") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-clip-disable",
            submit_name: "vf-rect-ndc-oa-clip-disable",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaClipDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-clip-force") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-clip-force",
            submit_name: "vf-rect-ndc-oa-clip-force",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaClipForceMode,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-clip-d3d") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-clip-d3d",
            submit_name: "vf-rect-ndc-oa-clip-d3d",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaClipApiD3d,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-early-clipxy") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-early-clipxy",
            submit_name: "vf-rect-ndc-oa-early-clipxy",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaEarlyClipViewportXy,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-frontccw") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-frontccw",
            submit_name: "vf-rect-ndc-oa-frontccw",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaFrontCcw,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-hz0") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-hz0",
            submit_name: "vf-rect-ndc-oa-hz0",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaNoHzOp,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-early") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-early",
            submit_name: "vf-rect-ndc-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-oa-bt1") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-oa-bt1",
            submit_name: "vf-rect-ndc-oa-bt1",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect,
            backend: BackendProbeMode::RasterWmInputOaBtCountOne,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-order-b-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-order-b-oa",
            submit_name: "vf-rect-ndc-order-b-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_cw,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-order-c-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-order-c-oa",
            submit_name: "vf-rect-ndc-order-c-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-order-c-early-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-order-c-early-oa",
            submit_name: "vf-rect-ndc-order-c-early-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-order-c-clip-disable-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-order-c-clip-disable-oa",
            submit_name: "vf-rect-ndc-order-c-clip-disable-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOaClipDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-mesa-simple-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-mesa-simple-oa",
            submit_name: "vf-rect-ndc-mesa-simple-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRect,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-mesa-nosrc-header-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-mesa-nosrc-header-oa",
            submit_name: "vf-rect-ndc-mesa-nosrc-header-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRectNoSrcHeader,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-small-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-small-oa",
            submit_name: "vf-rect-ndc-small-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_small,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-cw-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-cw-oa",
            submit_name: "vf-rect-ndc-cw-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_cw,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-ndc-alt-oa") {
        RenderJokerSpec {
            variant: "vf-rect-ndc-alt-oa",
            submit_name: "vf-rect-ndc-alt-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_rect_alt,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-order-b-oa") {
        RenderJokerSpec {
            variant: "vf-rect-order-b-oa",
            submit_name: "vf-rect-order-b-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-order-b-early-oa") {
        RenderJokerSpec {
            variant: "vf-rect-order-b-early-oa",
            submit_name: "vf-rect-order-b-early-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-order-b-scissor-oa") {
        RenderJokerSpec {
            variant: "vf-rect-order-b-scissor-oa",
            submit_name: "vf-rect-order-b-scissor-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaScissorOnly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-mesa-simple-oa") {
        RenderJokerSpec {
            variant: "vf-rect-mesa-simple-oa",
            submit_name: "vf-rect-mesa-simple-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRect,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-mesa-simple-oa-early") {
        RenderJokerSpec {
            variant: "vf-rect-mesa-simple-oa-early",
            submit_name: "vf-rect-mesa-simple-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRectEarly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-mesa-simple-oa-arm") {
        RenderJokerSpec {
            variant: "vf-rect-mesa-simple-oa-arm",
            submit_name: "vf-rect-mesa-simple-oa-arm",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRectArtificial,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-mesa-nosrc-header-oa") {
        RenderJokerSpec {
            variant: "vf-rect-mesa-nosrc-header-oa",
            submit_name: "vf-rect-mesa-nosrc-header-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_b,
            backend: BackendProbeMode::RasterWmInputOaMesaSimpleRectNoSrcHeader,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-rect-order-c-oa") {
        RenderJokerSpec {
            variant: "vf-rect-order-c-oa",
            submit_name: "vf-rect-order-c-oa",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect_order_c,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-tri-ndc-oa") {
        RenderJokerSpec {
            variant: "vf-tri-ndc-oa",
            submit_name: "vf-tri-ndc-oa",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle,
            backend: BackendProbeMode::RasterWmInputOa,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-tri-ndc-oa-early") {
        RenderJokerSpec {
            variant: "vf-tri-ndc-oa-early",
            submit_name: "vf-tri-ndc-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-tri-ndc-oa-early-clipxy") {
        RenderJokerSpec {
            variant: "vf-tri-ndc-oa-early-clipxy",
            submit_name: "vf-tri-ndc-oa-early-clipxy",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle,
            backend: BackendProbeMode::RasterWmInputOaEarlyClipViewportXy,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("vf-tri-ndc-cw-oa-early") {
        RenderJokerSpec {
            variant: "vf-tri-ndc-cw-oa-early",
            submit_name: "vf-tri-ndc-cw-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: ndc_triangle_cw,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-rect-scratch") {
        RenderJokerSpec {
            variant: "screen-rect-scratch",
            submit_name: "screen-rect-scratch",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::PsBindingTableCountZero,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("screen-rect-oa-early") {
        RenderJokerSpec {
            variant: "screen-rect-oa-early",
            submit_name: "screen-rect-oa-early",
            target: scratch,
            blend: zeroed,
            geometry: screen_rect,
            backend: BackendProbeMode::RasterWmInputOaEarlySample,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("so-vf") {
        RenderJokerSpec {
            variant: "so-vf",
            submit_name: "joker-vf-streamout",
            target: surface,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("so-vf-header") {
        RenderJokerSpec {
            variant: "so-vf-header",
            submit_name: "joker-vf-streamout-header",
            target: surface,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("so-vs") {
        RenderJokerSpec {
            variant: "so-vs",
            submit_name: "joker-vs-streamout",
            target: surface,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("so-vs-header") {
        RenderJokerSpec {
            variant: "so-vs-header",
            submit_name: "joker-vs-streamout-header",
            target: surface,
            blend: zeroed,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("bt1") {
        RenderJokerSpec {
            variant: "bt1",
            submit_name: "ps-bt1-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsBindingTableCountOne,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("wm-normal") || name.eq_ignore_ascii_case("wm") {
        RenderJokerSpec {
            variant: "wm-normal",
            submit_name: "ps-wm-normal-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmNormalDispatch,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("slot0") {
        RenderJokerSpec {
            variant: "slot0",
            submit_name: "ps-dispatch-slot0-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchSlot0,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("slot1") {
        RenderJokerSpec {
            variant: "slot1",
            submit_name: "ps-dispatch-slot1-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchSlot1,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("slot2") {
        RenderJokerSpec {
            variant: "slot2",
            submit_name: "ps-dispatch-slot2-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchSlot2,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("all") || name.eq_ignore_ascii_case("slots-all") {
        RenderJokerSpec {
            variant: "all",
            submit_name: "ps-dispatch-all-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsDispatchAllKspSlots,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("simd16") {
        RenderJokerSpec {
            variant: "simd16",
            submit_name: "ps-simd16-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsSimd16,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("simd16-retire") {
        RenderJokerSpec {
            variant: "simd16-retire",
            submit_name: "ps-simd16-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsSimd16,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("eot") {
        RenderJokerSpec {
            variant: "eot",
            submit_name: "ps-eot-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsEotOnly,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("eot-retire") {
        RenderJokerSpec {
            variant: "eot-retire",
            submit_name: "ps-eot-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsEotOnly,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("cps") || name.eq_ignore_ascii_case("cps-disabled") {
        RenderJokerSpec {
            variant: "cps",
            submit_name: "ps-cps-disabled-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsCpsDisabled,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("cps-retire") {
        RenderJokerSpec {
            variant: "cps-retire",
            submit_name: "ps-cps-disabled-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsCpsDisabled,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("hz") || name.eq_ignore_ascii_case("wm-hz") {
        RenderJokerSpec {
            variant: "hz",
            submit_name: "wm-hz-sample-mask-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmHzSampleMask,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("hz-retire") || name.eq_ignore_ascii_case("wm-hz-retire") {
        RenderJokerSpec {
            variant: "hz-retire",
            submit_name: "wm-hz-sample-mask-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmHzSampleMask,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit") || name.eq_ignore_ascii_case("late-reemit") {
        RenderJokerSpec {
            variant: "reemit",
            submit_name: "wm-late-reemit-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("reemit-retire")
        || name.eq_ignore_ascii_case("late-reemit-retire")
    {
        RenderJokerSpec {
            variant: "reemit-retire",
            submit_name: "wm-late-reemit-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit-vs-retire")
        || name.eq_ignore_ascii_case("late-reemit-vs-retire")
    {
        RenderJokerSpec {
            variant: "reemit-vs-retire",
            submit_name: "wm-late-reemit-vs-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit-vs-slot0-retire")
        || name.eq_ignore_ascii_case("late-reemit-vs-slot0-retire")
    {
        RenderJokerSpec {
            variant: "reemit-vs-slot0-retire",
            submit_name: "wm-late-reemit-vs-slot0-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit-vs-urb2-retire")
        || name.eq_ignore_ascii_case("late-reemit-vs-urb2-retire")
    {
        RenderJokerSpec {
            variant: "reemit-vs-urb2-retire",
            submit_name: "wm-late-reemit-vs-urb2-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("reemit-vs-urb2-slot0-retire")
        || name.eq_ignore_ascii_case("late-reemit-vs-urb2-slot0-retire")
    {
        RenderJokerSpec {
            variant: "reemit-vs-urb2-slot0-retire",
            submit_name: "wm-late-reemit-vs-urb2-slot0-big-primitive-retire",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::WmLateReemit,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("payload-push") {
        RenderJokerSpec {
            variant: "payload-push",
            submit_name: "ps-payload-push-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadPushConstant,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-attr") {
        RenderJokerSpec {
            variant: "payload-attr",
            submit_name: "ps-payload-attr-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadAttributeEnable,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-simple") {
        RenderJokerSpec {
            variant: "payload-simple",
            submit_name: "ps-payload-simple-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadSimpleHint,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-depthw") {
        RenderJokerSpec {
            variant: "payload-depthw",
            submit_name: "ps-payload-source-depth-w-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadSourceDepthW,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("payload-bary") || name.eq_ignore_ascii_case("bary") {
        RenderJokerSpec {
            variant: "payload-bary",
            submit_name: "ps-payload-bary-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsPayloadBaryPlanes,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("grf1") {
        RenderJokerSpec {
            variant: "grf1",
            submit_name: "ps-grf-start-r1-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfStartR1,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("grf2") {
        RenderJokerSpec {
            variant: "grf2",
            submit_name: "ps-grf-start-r2-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfStartR2,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("grf4") {
        RenderJokerSpec {
            variant: "grf4",
            submit_name: "ps-grf-start-r4-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfStartR4,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mt31") {
        RenderJokerSpec {
            variant: "mt31",
            submit_name: "ps-grf-maxthreads-31-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfMaxThreads31,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("mt15") {
        RenderJokerSpec {
            variant: "mt15",
            submit_name: "ps-grf-maxthreads-15-big-primitive",
            target: surface,
            blend: explicit,
            geometry: big,
            backend: BackendProbeMode::PsGrfMaxThreads15,
            sync: heavy,
        }
    } else if name.eq_ignore_ascii_case("sync-light") {
        RenderJokerSpec {
            variant: "sync-light",
            submit_name: "postdraw-light-only-retire",
            target: surface,
            blend: explicit,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: PostDrawSyncVariant::LightOnlyRetire,
        }
    } else if name.eq_ignore_ascii_case("sync-post-no-cs") {
        RenderJokerSpec {
            variant: "sync-post-no-cs",
            submit_name: "postdraw-pc-postsync-no-cs",
            target: surface,
            blend: explicit,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: light_post_no_cs,
        }
    } else if name.eq_ignore_ascii_case("sync-cs-no-post") {
        RenderJokerSpec {
            variant: "sync-cs-no-post",
            submit_name: "postdraw-pc-cs-no-postsync",
            target: surface,
            blend: explicit,
            geometry: canonical,
            backend: BackendProbeMode::MesaLike,
            sync: PostDrawSyncVariant::LightCsNoPostSync,
        }
    } else {
        return None;
    };
    Some(spec)
}

fn render_joker_real_vs_front_end_contract(variant: &str) -> Option<TriangleFrontEndContract> {
    match variant {
        "reemit-vs-retire" => Some(TRIANGLE_DEFAULT_FRONT_END_CONTRACT),
        "reemit-vs-slot0-retire" => Some(VS_DRAW_FRONTIER_CONTRACTS[1]),
        "reemit-vs-urb2-retire" => Some(VS_DRAW_FRONTIER_CONTRACTS[2]),
        "reemit-vs-urb2-slot0-retire" => Some(VS_DRAW_FRONTIER_CONTRACTS[3]),
        "screen-vs-sbe0" => Some(VS_DRAW_SBE_READ0_CONTRACT),
        "screen-vs-ndc-oa" | "screen-vs-ndc-oa-hz0" => Some(TRIANGLE_DEFAULT_FRONT_END_CONTRACT),
        "screen-vs-slot0-oa" => Some(VS_DRAW_FRONTIER_CONTRACTS[1]),
        "screen-vs-urb2-oa" => Some(VS_DRAW_FRONTIER_CONTRACTS[2]),
        "screen-vs-urb2-slot0-oa" => Some(VS_DRAW_FRONTIER_CONTRACTS[3]),
        "screen-vs-scratch" | "screen-vs-oa" | "screen-rect-scratch" | "screen-rect-oa-early" => {
            Some(TRIANGLE_DEFAULT_FRONT_END_CONTRACT)
        }
        _ => None,
    }
}

fn render_joker_vf_experiment(variant: &str) -> StreamoutProofExperiment {
    match variant {
        "point-oa-pos0" => StreamoutProofExperiment::PositionSlot0,
        "vf-rect-mesa-simple-oa"
        | "vf-rect-mesa-simple-oa-early"
        | "vf-rect-mesa-simple-oa-arm"
        | "vf-rect-ndc-mesa-simple-oa"
        | "vf-rect-mesa-nosrc-header-oa"
        | "vf-rect-ndc-mesa-nosrc-header-oa" => StreamoutProofExperiment::PositionSlot0,
        "vf-rect-oa-pos0" => StreamoutProofExperiment::PositionSlot0,
        "point-oa-header" | "vf-rect-oa-header" | "so-vf-header" | "so-vs-header" => {
            StreamoutProofExperiment::HeaderAndPositionSlots01
        }
        "point-oa-vtxw" => StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        _ => StreamoutProofExperiment::PositionSlot1,
    }
}

fn render_joker_streamout_kind(variant: &str) -> Option<&'static str> {
    match variant {
        "so-vf" | "so-vf-header" => Some("vf"),
        "so-vs" | "so-vs-header" => Some("vs"),
        _ => None,
    }
}

pub(crate) fn submit_render_joker_probe(name: &str) -> Result<RenderJokerResult, &'static str> {
    if let Some(reason) = retired_render_joker_variant_reason(name) {
        return Err(reason);
    }

    let Some(spec) = parse_render_joker_spec(name) else {
        return Err("unknown-variant");
    };

    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("in-flight");
    }

    let result = submit_render_joker_probe_locked(spec);
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    result
}

pub(crate) fn submit_render_artificial_fragment_sentinel()
-> Result<RenderArtificialFragmentResult, &'static str> {
    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("in-flight");
    }

    let result = submit_render_artificial_fragment_sentinel_locked();
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    result
}

fn submit_render_artificial_fragment_sentinel_locked()
-> Result<RenderArtificialFragmentResult, &'static str> {
    let Some(dev) = crate::intel::claimed_device() else {
        crate::log!("intel/render: artificial-fragment-sentinel skipped reason=no-device\n");
        return Err("no-device");
    };
    let warm = warm_once(dev);
    if warm.streamout_len < 8 * 8 * core::mem::size_of::<u32>()
        || warm.streamout_virt.is_null()
        || warm.streamout_phys == 0
    {
        crate::log!("intel/render: artificial-fragment-sentinel skipped reason=warm-scratch\n");
        return Err("warm-scratch");
    }
    if !forcewake_render_acquire(warm) {
        crate::log!("intel/render: artificial-fragment-sentinel skipped reason=forcewake\n");
        return Err("forcewake");
    }
    if !ensure_smoke_buffers_mapped(dev, warm) {
        crate::log!("intel/render: artificial-fragment-sentinel skipped reason=ggtt-map\n");
        return Err("ggtt-map");
    }

    const SENTINEL_COLOR: u32 = 0xA17F_F00D;
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
        core::ptr::write_volatile(warm.streamout_virt as *mut u32, 0xDEAD_BEEF);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.batch_virt, warm.batch_len);
    crate::intel::dma_flush(warm.ring_virt, warm.ring_len);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
    let before = unsafe { core::ptr::read_volatile(warm.streamout_virt as *const u32) };

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = encode_3d_no_draw_probe_batch(
        batch,
        warm,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_MI_PROBE_DONE,
        Some((GPU_VA_STREAMOUT_BASE, SENTINEL_COLOR)),
    )
    .map_err(|_| "batch")?;
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_MI_PROBE_DONE,
        RESULT_SLOT_PRE3D_DWORD,
        "artificial-fragment-sentinel",
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "artificial-fragment-sentinel");
    }

    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
    let after = unsafe { core::ptr::read_volatile(warm.streamout_virt as *const u32) };
    let remapped_render = ensure_smoke_buffers_mapped(dev, warm);
    WARM_BUFFERS_MAPPED.store(remapped_render, Ordering::Release);
    let ok = completed && after == SENTINEL_COLOR && remapped_render;
    intel_render_focus_log!(
        "intel/render: artificial-fragment-sentinel mode=mi-store ok={} completed={} stores=1 rt_gpu=0x{:X} size=8x8 pitch=0x{:X} before=0x{:08X} after=0x{:08X} remapped_render={} meaning=artificial-fragment-not-wm does_not_prove=raster_or_ps\n",
        ok as u8,
        completed as u8,
        GPU_VA_STREAMOUT_BASE,
        8 * core::mem::size_of::<u32>() as u32,
        before,
        after,
        remapped_render as u8,
    );

    Ok(RenderArtificialFragmentResult {
        mode: "mi-store",
        ok,
        descs: 1,
        before,
        after,
        rt_gpu: GPU_VA_STREAMOUT_BASE,
        remapped_render,
    })
}

fn submit_render_joker_probe_locked(
    spec: RenderJokerSpec,
) -> Result<RenderJokerResult, &'static str> {
    let probe_seq = PRIMARY_PROBE_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if PRIMARY_DISABLE_RENDER_BRINGUP {
        crate::log!(
            "intel/render: joker skipped reason=disabled variant={} seq={}\n",
            spec.variant,
            probe_seq
        );
        return Err("disabled");
    }

    let Some(dev) = crate::intel::claimed_device() else {
        crate::log!("intel/render: joker skipped reason=no-device variant={}\n", spec.variant);
        return Err("no-device");
    };
    let Some(surface_gpu) = crate::intel::display::primary_surface_gpu_addr() else {
        crate::log!("intel/render: joker skipped reason=no-surface variant={}\n", spec.variant);
        return Err("no-surface");
    };
    let Some((width, height)) = crate::intel::display::active_scanout_dimensions() else {
        crate::log!("intel/render: joker skipped reason=no-dimensions variant={}\n", spec.variant);
        return Err("no-dimensions");
    };
    let Some(pitch_bytes) = width
        .checked_mul(4)
        .and_then(|v| crate::intel::align_up(v as usize, 64))
    else {
        crate::log!("intel/render: joker skipped reason=bad-pitch width={}\n", width);
        return Err("bad-pitch");
    };

    let warm = warm_once(dev);
    if warm.ring_len == 0
        || warm.context_len == 0
        || warm.batch_len == 0
        || warm.draw_state_len == 0
        || warm.vertex_len == 0
        || warm.result_len == 0
        || warm.streamout_len == 0
    {
        crate::log!("intel/render: joker skipped reason=warm-buffers variant={}\n", spec.variant);
        return Err("warm-buffers");
    }
    if !forcewake_render_acquire(warm) {
        crate::log!("intel/render: joker skipped reason=forcewake variant={}\n", spec.variant);
        return Err("forcewake");
    }
    if !ensure_smoke_buffers_mapped(dev, warm) {
        crate::log!("intel/render: joker skipped reason=ggtt-map variant={}\n", spec.variant);
        return Err("ggtt-map");
    }

    let (target_gpu, target_pitch, target_w, target_h, target_label) = match spec.target {
        RenderJokerTarget::Primary => {
            (surface_gpu, pitch_bytes, width as usize, height as usize, "primary")
        }
        RenderJokerTarget::ScratchRt => {
            unsafe {
                core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
                core::ptr::write_volatile(warm.streamout_virt as *mut u32, 0xDEAD_BEEF);
            }
            crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
            (GPU_VA_STREAMOUT_BASE, 8 * core::mem::size_of::<u32>(), 8, 8, "scratch")
        }
    };

    let streamout_kind = render_joker_streamout_kind(spec.variant);
    let real_vs_contract = render_joker_real_vs_front_end_contract(spec.variant);
    let front_end_label = real_vs_contract
        .map(|contract| contract.label)
        .or(streamout_kind)
        .unwrap_or("vf-synthesized");
    intel_render_focus_log!(
        "intel/render: joker begin seq={} variant={} submit={} target={} backend={} geometry={} blend={} sync={} front_end={}\n",
        probe_seq,
        spec.variant,
        spec.submit_name,
        target_label,
        spec.backend.label(),
        spec.geometry.label(),
        spec.blend.label(),
        spec.sync.label(),
        front_end_label,
    );
    let completed = if let Some(kind) = streamout_kind {
        let experiment = render_joker_vf_experiment(spec.variant);
        if kind == "vs" {
            submit_triangle_vs_streamout_proof(
                dev,
                warm,
                target_gpu,
                target_pitch,
                target_w,
                target_h,
                experiment,
            )
        } else {
            submit_triangle_vf_streamout_proof(
                dev,
                warm,
                target_gpu,
                target_pitch,
                target_w,
                target_h,
                experiment,
            )
        }
    } else if let Some(front_end_contract) = real_vs_contract {
        submit_triangle_real_vs_draw_probe_to_surface_ext(
            dev,
            warm,
            target_gpu,
            target_pitch,
            target_w,
            target_h,
            spec.blend,
            spec.geometry,
            spec.submit_name,
            front_end_contract,
            spec.backend,
            spec.sync,
            None,
        )
    } else {
        submit_triangle_vf_draw_to_surface_ext(
            spec.submit_name,
            dev,
            warm,
            target_gpu,
            target_pitch,
            target_w,
            target_h,
            spec.blend,
            spec.geometry,
            spec.backend,
            spec.sync,
            render_joker_vf_experiment(spec.variant),
        )
    };
    intel_render_focus_log!(
        "intel/render: joker end seq={} variant={} submit={} target={} completed={}\n",
        probe_seq,
        spec.variant,
        spec.submit_name,
        target_label,
        completed as u8,
    );

    Ok(RenderJokerResult {
        variant: spec.variant,
        submit_name: spec.submit_name,
        target: target_label,
        completed,
    })
}

fn submit_primary_probe_now(reason: &'static str) -> bool {
    let probe_seq = PRIMARY_PROBE_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if PRIMARY_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        crate::log!("intel/render: primary-probe skipped reason=in-flight trigger={}\n", reason);
        return false;
    }

    if PRIMARY_DISABLE_RENDER_BRINGUP {
        crate::log!(
            "intel/render: primary-probe skipped reason=disabled trigger={} seq={}\n",
            reason,
            probe_seq
        );
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }

    let Some(dev) = crate::intel::claimed_device() else {
        crate::log!("intel/render: primary-triangle skipped reason=no-device\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };
    let Some(surface_gpu) = crate::intel::display::primary_surface_gpu_addr() else {
        crate::log!("intel/render: primary-triangle skipped reason=no-surface\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };
    let Some((width, height)) = crate::intel::display::active_scanout_dimensions() else {
        crate::log!("intel/render: primary-triangle skipped reason=no-dimensions\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };
    let Some(pitch_bytes) = width
        .checked_mul(4)
        .and_then(|v| crate::intel::align_up(v as usize, 64))
    else {
        crate::log!("intel/render: primary-triangle skipped reason=bad-pitch width={}\n", width);
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    };

    let warm = warm_once(dev);
    if warm.ring_len == 0
        || warm.context_len == 0
        || warm.batch_len == 0
        || warm.draw_state_len == 0
        || warm.vertex_len == 0
        || warm.result_len == 0
        || warm.streamout_len == 0
    {
        crate::log!("intel/render: primary-triangle skipped reason=warm-buffers\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }
    if !forcewake_render_acquire(warm) {
        crate::log!("intel/render: primary-triangle skipped reason=forcewake\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }
    if !ensure_smoke_buffers_mapped(dev, warm) {
        crate::log!("intel/render: primary-triangle skipped reason=ggtt-map\n");
        PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        return false;
    }
    if PRIMARY_USE_MI_SCANOUT_PROOF
        && reason == "boot-once"
        && !PRIMARY_MI_SCANOUT_PROOF_SUBMITTED.swap(true, Ordering::AcqRel)
    {
        let accepted = submit_mi_scanout_store_proof(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width as usize,
            height as usize,
        );
        if !accepted {
            intel_render_verbose_log!(
                "intel/render: primary-mi-scanout-store proof failed trigger={}\n",
                reason
            );
        }
    }
    let completed = if PRIMARY_USE_DRAW_PATH_BOOT_ONCE && reason == "boot-once" {
        let completed = submit_primary_triangle_with_retries(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width as usize,
            height as usize,
        );
        if !completed {
            intel_render_verbose_log!(
                "intel/render: primary-draw-path submit failed trigger={} mode=clean-boot-once\n",
                reason
            );
        }
        completed
    } else if PRIMARY_USE_MI_STRIPE_PROBE {
        let completed = submit_vertical_stripes_to_surface(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width as usize,
            height as usize,
        );
        if !completed {
            intel_render_verbose_log!(
                "intel/render: primary-mi-stripes submit failed trigger={}\n",
                reason
            );
        }
        completed
    } else if PRIMARY_USE_3D_NO_DRAW_PROBE {
        let completed = submit_3d_no_draw_probe(dev, warm);
        if !completed {
            intel_render_verbose_log!(
                "intel/render: primary-3d-no-draw submit failed trigger={}\n",
                reason
            );
        }
        completed
    } else if submit_primary_triangle_with_retries(
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width as usize,
        height as usize,
    ) {
        true
    } else {
        let completed = submit_triangle_to_surface(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width as usize,
            height as usize,
        );
        if !completed {
            intel_render_verbose_log!(
                "intel/render: primary-triangle submit failed trigger={}\n",
                reason
            );
        }
        completed
    };
    if should_log_primary_probe(reason, probe_seq) {
        intel_render_verbose_log!(
            "intel/render: primary-probe seq={} trigger={} completed={} mode={}\n",
            probe_seq,
            reason,
            completed as u8,
            if PRIMARY_USE_MI_STRIPE_PROBE {
                "mi-stripes"
            } else if PRIMARY_USE_DRAW_PATH_BOOT_ONCE && reason == "boot-once" {
                "draw-path"
            } else if PRIMARY_USE_3D_NO_DRAW_PROBE {
                "3d-no-draw"
            } else {
                "3d"
            }
        );
    }
    PRIMARY_PROBE_IN_FLIGHT.store(false, Ordering::Release);
    completed
}

fn seed_render_scratch_rt(warm: RenderWarmState) {
    unsafe {
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
        core::ptr::write_volatile(warm.streamout_virt as *mut u32, 0xDEAD_BEEF);
    }
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
}

fn submit_primary_triangle_with_retries(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    surface_gpu: u64,
    pitch_bytes: usize,
    width: usize,
    height: usize,
) -> bool {
    if !PRIMARY_BOOT_3D_PROBES_ENABLED {
        let completed = submit_triangle_vf_draw_to_surface(
            "primary-single-submit",
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            TriangleBlendProbeMode::ExplicitRt0,
            VfPrimitiveGeometry::Canonical,
            BackendProbeMode::MesaLike,
            PostDrawSyncVariant::HeavyAll,
        );
        intel_render_focus_log!(
            "intel/render: primary-single-submit completed={} action=stop-after-one-submit reason=boot-3d-probes-disabled\n",
            completed as u8,
        );
        return completed;
    }
    intel_render_focus_log!(
        "intel/render: primary-boot-3d-probes enabled=1 action=run-frontier-ladder vf_streamout=1 ps_spectrum=1 vs_frontier=1 revision=nonvisual-vs-scratch-rt32-trilist-split\n",
    );

    let initial_streamout_experiment =
        select_streamout_proof_experiment(PRIMARY_PROBE_SEQ.load(Ordering::Acquire));
    let vf_streamout_precheck = submit_triangle_vf_streamout_proof(
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        initial_streamout_experiment,
    );
    intel_render_verbose_log!(
        "intel/render: primary-vf-streamout-precheck experiment={} accepted={}\n",
        initial_streamout_experiment.label(),
        vf_streamout_precheck as u8,
    );
    if !vf_streamout_precheck {
        return false;
    }

    let vf_draw_precheck = submit_triangle_vf_draw_to_surface(
        "vf-draw-path",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Canonical,
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-vf-draw-precheck completed={}\n",
        vf_draw_precheck as u8,
    );
    if vf_draw_precheck {
        return true;
    }
    reset_fragment_boundary_probe();
    let ps_launch_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-launch-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-ps-launch-big-primitive completed={}\n",
        ps_launch_big_primitive as u8,
    );
    if ps_launch_big_primitive {
        return true;
    }

    run_postdraw_pc_retire_spectrum(dev, warm, surface_gpu, pitch_bytes, width, height);

    seed_render_scratch_rt(warm);
    let ps_bt0_scratch_rt = submit_triangle_vf_draw_to_surface(
        "ps-bt0-scratch-rt",
        dev,
        warm,
        GPU_VA_STREAMOUT_BASE,
        8 * core::mem::size_of::<u32>(),
        8,
        8,
        TriangleBlendProbeMode::MesaZeroedState,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsBindingTableCountZero,
        PostDrawSyncVariant::LightPostSyncNoCs,
    );
    intel_render_verbose_log!(
        "intel/render: primary-ps-bt0-scratch-rt completed={}\n",
        ps_bt0_scratch_rt as u8,
    );
    intel_render_focus_log!(
        "intel/render: primary-ps-bt0-scratch-rt diagnostic completed={} note=no-cs-tail-completion-is-not-a-fence\n",
        ps_bt0_scratch_rt as u8,
    );
    if ps_bt0_scratch_rt {
        recover_render_engine_after_nonretired_submit(dev, warm, "ps-bt0-scratch-rt");
    }

    seed_render_scratch_rt(warm);
    let raster_wm_oa_probe = submit_triangle_vf_draw_to_surface(
        "raster-wm-oa-probe",
        dev,
        warm,
        GPU_VA_STREAMOUT_BASE,
        8 * core::mem::size_of::<u32>(),
        8,
        8,
        TriangleBlendProbeMode::MesaZeroedState,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::RasterWmInputOa,
        PostDrawSyncVariant::LightPostSyncNoCs,
    );
    intel_render_verbose_log!(
        "intel/render: primary-raster-wm-oa-probe completed={}\n",
        raster_wm_oa_probe as u8,
    );
    intel_render_focus_log!(
        "intel/render: primary-raster-wm-oa-probe diagnostic completed={} note=no-cs-tail-completion-is-not-a-fence\n",
        raster_wm_oa_probe as u8,
    );
    if raster_wm_oa_probe {
        recover_render_engine_after_nonretired_submit(dev, warm, "raster-wm-oa-probe");
    }

    let fragment_candidate_ready = fragment_candidate_ready();
    let fragment_boundary_seen = fragment_boundary_observed();
    intel_render_focus_log!(
        "intel/render: primary-fragment-boundary-gate candidate_ready={} fragment_observed={} action={} reason=shape_to_fragment_boundary_precedes_ps_spectrum\n",
        fragment_candidate_ready as u8,
        fragment_boundary_seen as u8,
        if fragment_boundary_seen {
            "continue-ps-spectrum"
        } else {
            "continue-ps-spectrum-diagnostic"
        },
    );

    let ps_bt1_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-bt1-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsBindingTableCountOne,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-ps-bt1-big-primitive completed={}\n",
        ps_bt1_big_primitive as u8,
    );
    if ps_bt1_big_primitive {
        return true;
    }

    let ps_wm_normal_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-wm-normal-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::WmNormalDispatch,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-ps-wm-normal-big-primitive completed={}\n",
        ps_wm_normal_big_primitive as u8,
    );
    if ps_wm_normal_big_primitive {
        return true;
    }

    let ps_dispatch_slot0_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-dispatch-slot0-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsDispatchSlot0,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-ps-dispatch-slot0-big-primitive completed={}\n",
        ps_dispatch_slot0_big_primitive as u8,
    );
    if ps_dispatch_slot0_big_primitive {
        return true;
    }

    let ps_dispatch_slot1_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-dispatch-slot1-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsDispatchSlot1,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-ps-dispatch-slot1-big-primitive completed={}\n",
        ps_dispatch_slot1_big_primitive as u8,
    );
    if ps_dispatch_slot1_big_primitive {
        return true;
    }

    let ps_dispatch_slot2_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-dispatch-slot2-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsDispatchSlot2,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-ps-dispatch-slot2-big-primitive completed={}\n",
        ps_dispatch_slot2_big_primitive as u8,
    );
    if ps_dispatch_slot2_big_primitive {
        return true;
    }

    let payload_variants = [
        ("ps-payload-push-big-primitive", BackendProbeMode::PsPayloadPushConstant),
        ("ps-payload-attr-big-primitive", BackendProbeMode::PsPayloadAttributeEnable),
        ("ps-payload-simple-big-primitive", BackendProbeMode::PsPayloadSimpleHint),
        ("ps-payload-source-depth-w-big-primitive", BackendProbeMode::PsPayloadSourceDepthW),
        ("ps-payload-bary-big-primitive", BackendProbeMode::PsPayloadBaryPlanes),
    ];
    for (payload_submit_name, payload_mode) in payload_variants {
        let completed = submit_triangle_vf_draw_to_surface(
            payload_submit_name,
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            TriangleBlendProbeMode::ExplicitRt0,
            VfPrimitiveGeometry::Oversized,
            payload_mode,
            PostDrawSyncVariant::HeavyAll,
        );
        intel_render_verbose_log!(
            "intel/render: primary-{} completed={}\n",
            payload_submit_name,
            completed as u8,
        );
        if completed {
            return true;
        }
    }

    let grf_variants = [
        ("ps-grf-start-r1-big-primitive", BackendProbeMode::PsGrfStartR1),
        ("ps-grf-start-r2-big-primitive", BackendProbeMode::PsGrfStartR2),
        ("ps-grf-start-r4-big-primitive", BackendProbeMode::PsGrfStartR4),
        ("ps-grf-maxthreads-31-big-primitive", BackendProbeMode::PsGrfMaxThreads31),
        ("ps-grf-maxthreads-15-big-primitive", BackendProbeMode::PsGrfMaxThreads15),
    ];
    for (grf_submit_name, grf_mode) in grf_variants {
        let completed = submit_triangle_vf_draw_to_surface(
            grf_submit_name,
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            TriangleBlendProbeMode::ExplicitRt0,
            VfPrimitiveGeometry::Oversized,
            grf_mode,
            PostDrawSyncVariant::HeavyAll,
        );
        intel_render_verbose_log!(
            "intel/render: primary-{} completed={}\n",
            grf_submit_name,
            completed as u8,
        );
        if completed {
            return true;
        }
    }

    let ps_dispatch_all_big_primitive = submit_triangle_vf_draw_to_surface(
        "ps-dispatch-all-big-primitive",
        dev,
        warm,
        surface_gpu,
        pitch_bytes,
        width,
        height,
        TriangleBlendProbeMode::ExplicitRt0,
        VfPrimitiveGeometry::Oversized,
        BackendProbeMode::PsDispatchAllKspSlots,
        PostDrawSyncVariant::HeavyAll,
    );
    intel_render_verbose_log!(
        "intel/render: primary-ps-dispatch-all-big-primitive completed={}\n",
        ps_dispatch_all_big_primitive as u8,
    );
    if ps_dispatch_all_big_primitive {
        return true;
    }

    reset_fragment_boundary_probe();
    let fragment_shape_frontier = run_fragment_shape_frontier_spectrum(dev, warm);
    intel_render_focus_log!(
        "intel/render: primary-fragment-shape-spectrum completed={} observed={} note=shape_clip_sf_axis_after_ps_state_axis\n",
        fragment_shape_frontier as u8,
        fragment_boundary_observed() as u8,
    );
    if fragment_shape_frontier {
        return true;
    }

    let vs_draw_frontier_scratch = submit_triangle_vs_draw_frontier_to_scratch(dev, warm);
    intel_render_focus_log!(
        "intel/render: primary-vs-draw-frontier-scratch completed={} observed={} note=nonvisual-vs-clip-join-probe\n",
        vs_draw_frontier_scratch as u8,
        fragment_boundary_observed() as u8,
    );
    if vs_draw_frontier_scratch {
        return true;
    }
    intel_render_focus_log!(
        "intel/render: primary-vs-draw-frontier-precheck skipped reason=scratch-frontier-unobserved avoid_visible_scanout_flash surface=0x{:X} size={}x{} pitch=0x{:X}\n",
        surface_gpu,
        width,
        height,
        pitch_bytes,
    );

    let mut vs_streamout_experiment = initial_streamout_experiment;
    let mut vs_streamout_precheck = false;
    for attempt in 1..=3 {
        let accepted = submit_triangle_vs_streamout_proof(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            vs_streamout_experiment,
        );
        intel_render_verbose_log!(
            "intel/render: primary-vs-streamout-precheck experiment={} accepted={} attempt={}/3\n",
            vs_streamout_experiment.label(),
            accepted as u8,
            attempt
        );
        if accepted {
            vs_streamout_precheck = true;
            break;
        }
        vs_streamout_experiment = vs_streamout_experiment.alternate();
    }
    if !vs_streamout_precheck {
        return false;
    }

    let mut streamout_experiment = vs_streamout_experiment;
    let mut streamout_precheck = false;
    for attempt in 1..=3 {
        let accepted = submit_triangle_streamout_proof(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            streamout_experiment,
        );
        intel_render_verbose_log!(
            "intel/render: primary-streamout-precheck experiment={} accepted={} attempt={}/3\n",
            streamout_experiment.label(),
            accepted as u8,
            attempt
        );
        if accepted {
            streamout_precheck = true;
            break;
        }
        streamout_experiment = streamout_experiment.alternate();
    }
    if !streamout_precheck {
        return false;
    }

    let mut completed_any = false;
    for attempt in 1..=PRIMARY_TRIANGLE_SUBMIT_ATTEMPTS {
        let blend_mode = TriangleBlendProbeMode::for_attempt(attempt);
        let completed = submit_triangle_draw_to_surface(
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            blend_mode,
        );
        intel_render_verbose_log!(
            "intel/render: primary-triangle attempt={}/{} target=0x{:X} blend_probe={} completed={}\n",
            attempt,
            PRIMARY_TRIANGLE_SUBMIT_ATTEMPTS,
            surface_gpu,
            blend_mode.label(),
            completed as u8
        );
        completed_any |= completed;
        if !completed {
            intel_render_verbose_log!(
                "intel/render: primary-streamout-proof skipped trigger=draw-fail attempt={} reason=post-hang-state-not-clean\n",
                attempt,
            );
            break;
        }
    }
    completed_any
}

fn run_fragment_shape_frontier_spectrum(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    let scratch_pitch = 8 * core::mem::size_of::<u32>();
    let aligned_scratch_pitch = 32 * core::mem::size_of::<u32>();
    let probes = [
        (
            "point-vf-giant-oa-w64",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaPointWidth64,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-w64-halign128",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaPointWidth64SurfaceHalign128,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-w1023",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaPointWidth1023,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-msrast",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaMsRaster,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-msrast-force",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaMsRasterForced,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-early-msrast-force",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaEarlyMsRasterForced,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-early-w1023",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaEarlyPointWidth1023,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-w64-early",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaPointWidth64Early,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-w64-early-scissor",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaPointWidth64EarlyScissor,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-w1023-scissor",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaPointWidth1023Scissor,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-giant-oa-hammer",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaHammer,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "point-vf-screen-oa-w64",
            VfPrimitiveGeometry::ScreenSpacePoint8x8,
            BackendProbeMode::RasterWmInputOaPointWidth64Screen,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "point-vf-screen-oa-hammer",
            VfPrimitiveGeometry::ScreenSpacePoint8x8,
            BackendProbeMode::RasterWmInputOaScreenHammer,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "line-vf-screen-oa-hammer",
            VfPrimitiveGeometry::ScreenSpaceLine8x8,
            BackendProbeMode::RasterWmInputOaScreenHammer,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "point-vf-giant-oa-w64-wm-normal",
            VfPrimitiveGeometry::CenterPoint,
            BackendProbeMode::RasterWmInputOaPointWidth64WmNormalDispatch,
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOa,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-halign128",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaSurfaceHalign128,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-early",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaEarlySample,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-sample-early",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaSampleMaskEarlyOnly,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-pc-clip-sf",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaPipeControlClipSf,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-hz-pre-wm",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaWmHzOpBeforeWm,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-hz-post-extra",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaWmHzOpAfterPsExtra,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-frontccw",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaFrontCcw,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-hz0",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaNoHzOp,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-clip-disable",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaClipDisabled,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-bt1",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaBtCountOne,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-order-b-oa",
            VfPrimitiveGeometry::ScreenSpaceRect8x8OrderB,
            BackendProbeMode::RasterWmInputOa,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-order-b-scissor-oa",
            VfPrimitiveGeometry::ScreenSpaceRect8x8OrderB,
            BackendProbeMode::RasterWmInputOaScissorOnly,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-order-c-early-oa",
            VfPrimitiveGeometry::NdcRectUrLrUl,
            BackendProbeMode::RasterWmInputOaEarlySample,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-mesa-simple-oa-early",
            VfPrimitiveGeometry::ScreenSpaceRect8x8OrderB,
            BackendProbeMode::RasterWmInputOaMesaSimpleRectEarly,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "screen-rect-oa-early",
            VfPrimitiveGeometry::ScreenSpaceRect8x8,
            BackendProbeMode::RasterWmInputOaEarlySample,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "screen-rect-oa-hammer",
            VfPrimitiveGeometry::ScreenSpaceRect8x8OrderB,
            BackendProbeMode::RasterWmInputOaScreenHammer,
            StreamoutProofExperiment::PositionSlot1,
        ),
    ];
    let aligned_target_probes = [
        (
            "vf-rect-ndc-oa-rt32",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOa,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-halign128-rt32",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaSurfaceHalign128,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-early-rt32",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaEarlySample,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-ndc-oa-hammer-rt32",
            VfPrimitiveGeometry::NdcRect,
            BackendProbeMode::RasterWmInputOaHammer,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-line-ndc-oa-hammer-rt32",
            VfPrimitiveGeometry::NdcLine,
            BackendProbeMode::RasterWmInputOaHammer,
            StreamoutProofExperiment::PositionSlot1,
        ),
        (
            "vf-rect-mesa-simple-oa-early-rt32",
            VfPrimitiveGeometry::ScreenSpaceRect8x8OrderB,
            BackendProbeMode::RasterWmInputOaMesaSimpleRectEarly,
            StreamoutProofExperiment::PositionSlot1,
        ),
    ];

    intel_render_focus_log!(
        "intel/render: primary-fragment-shape-spectrum begin probes={} target=scratch-8x8+rt32 truth=fragment_boundary_observed\n",
        probes.len() + aligned_target_probes.len(),
    );
    for (submit_name, geometry, backend, vf_experiment) in probes {
        seed_render_scratch_rt(warm);
        let completed = submit_triangle_vf_draw_to_surface_ext(
            submit_name,
            dev,
            warm,
            GPU_VA_STREAMOUT_BASE,
            scratch_pitch,
            8,
            8,
            TriangleBlendProbeMode::MesaZeroedState,
            geometry,
            backend,
            PostDrawSyncVariant::LightPostSyncNoCs,
            vf_experiment,
        );
        let observed = fragment_boundary_observed();
        intel_render_focus_log!(
            "intel/render: primary-fragment-shape-spectrum submit={} geometry={} backend={} vf_contract={} completed={} candidate_ready={} observed={}\n",
            submit_name,
            geometry.label(),
            backend.label(),
            vf_experiment.vf_slot_contract(),
            completed as u8,
            fragment_candidate_ready() as u8,
            observed as u8,
        );
        if completed {
            recover_render_engine_after_nonretired_submit(dev, warm, submit_name);
        }
        if observed {
            return true;
        }
    }
    for (submit_name, geometry, backend, vf_experiment) in aligned_target_probes {
        seed_render_scratch_rt(warm);
        let completed = submit_triangle_vf_draw_to_surface_ext(
            submit_name,
            dev,
            warm,
            GPU_VA_STREAMOUT_BASE,
            aligned_scratch_pitch,
            32,
            32,
            TriangleBlendProbeMode::MesaZeroedState,
            geometry,
            backend,
            PostDrawSyncVariant::LightPostSyncNoCs,
            vf_experiment,
        );
        let observed = fragment_boundary_observed();
        intel_render_focus_log!(
            "intel/render: primary-fragment-shape-spectrum submit={} geometry={} backend={} vf_contract={} completed={} candidate_ready={} observed={}\n",
            submit_name,
            geometry.label(),
            backend.label(),
            vf_experiment.vf_slot_contract(),
            completed as u8,
            fragment_candidate_ready() as u8,
            observed as u8,
        );
        if completed {
            recover_render_engine_after_nonretired_submit(dev, warm, submit_name);
        }
        if observed {
            return true;
        }
    }
    false
}

fn run_postdraw_pc_retire_spectrum(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    surface_gpu: u64,
    pitch_bytes: usize,
    width: usize,
    height: usize,
) {
    for variant in POST_DRAW_PC_RETIRE_SPECTRUM {
        let submit_name = variant.submit_name();
        let completed = submit_triangle_vf_draw_to_surface(
            submit_name,
            dev,
            warm,
            surface_gpu,
            pitch_bytes,
            width,
            height,
            TriangleBlendProbeMode::ExplicitRt0,
            VfPrimitiveGeometry::Canonical,
            BackendProbeMode::MesaLike,
            variant,
        );
        intel_render_focus_log!(
            "intel/render: postdraw-pc-retire-spectrum submit={} variant={} completed={} note=diagnostic_only\n",
            submit_name,
            variant.label(),
            completed as u8,
        );
        if completed {
            intel_render_focus_log!(
                "intel/render: postdraw-pc-retire-spectrum cleanup submit={} variant={} reason=completed-diagnostic-not-a-fence\n",
                submit_name,
                variant.label(),
            );
            recover_render_engine_after_nonretired_submit(dev, warm, submit_name);
        }
    }
}

fn submit_triangle_vf_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> bool {
    let Some(draw) = prepare_vf_streamout_proof_resources(
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        experiment,
        VfPrimitiveGeometry::Canonical,
    ) else {
        crate::log!(
            "intel/render: vf-streamout-proof skipped reason=resource-layout size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };
    let slice_hash_table_offset = match write_vf_streamout_probe_state(warm) {
        Ok(offset) => offset,
        Err(reason) => {
            crate::log!(
                "intel/render: vf-streamout-proof skipped reason=probe-state detail={}\n",
                reason
            );
            return false;
        }
    };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_vf_streamout_proof_batch(
        batch,
        warm,
        draw,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        experiment,
        slice_hash_table_offset,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!("intel/render: vf-streamout-proof batch build failed detail={}\n", reason);
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    intel_render_verbose_log!(
        "intel/render: vf-streamout-proof batch-ready experiment={} bytes=0x{:X} so_gpu=0x{:X} so_pitch={} vertices={}\n",
        experiment.label(),
        batch_tail_bytes,
        GPU_VA_STREAMOUT_BASE,
        experiment.vertex_bytes(),
        draw.vertex_count
    );

    let stats_before = capture_triangle_stage_stats(dev);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        "vf-streamout-proof",
    );
    let stats_after = capture_triangle_stage_stats(dev);
    let accepted = completed
        || maybe_soft_accept_streamout_submit(
            "vf-streamout-proof",
            warm,
            stats_before,
            stats_after,
            false,
            experiment.vertex_bytes() * draw.vertex_count as usize,
        );
    log_streamout_proof_result(
        "vf-streamout-proof",
        warm,
        completed,
        draw.vertex_count as usize,
        experiment,
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "vf-streamout-proof");
    }
    accepted
}

fn submit_triangle_vf_draw_to_surface(
    submit_name: &'static str,
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
    geometry: VfPrimitiveGeometry,
    backend_probe_mode: BackendProbeMode,
    post_draw_sync_variant: PostDrawSyncVariant,
) -> bool {
    submit_triangle_vf_draw_to_surface_ext(
        submit_name,
        dev,
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        blend_mode,
        geometry,
        backend_probe_mode,
        post_draw_sync_variant,
        StreamoutProofExperiment::PositionSlot1,
    )
}

fn submit_triangle_vf_draw_to_surface_ext(
    submit_name: &'static str,
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
    geometry: VfPrimitiveGeometry,
    backend_probe_mode: BackendProbeMode,
    post_draw_sync_variant: PostDrawSyncVariant,
    vf_experiment: StreamoutProofExperiment,
) -> bool {
    let Some(draw) = prepare_vf_streamout_proof_resources(
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        vf_experiment,
        geometry,
    ) else {
        crate::log!(
            "intel/render: {} staging skipped reason=resource-layout size={}x{} pitch=0x{:X} geometry={}\n",
            submit_name,
            rect_w,
            rect_h,
            pitch,
            geometry.label(),
        );
        return false;
    };

    let (pipeline, pipeline_note) = match backend_probe_mode {
        BackendProbeMode::PsSimd16 => (
            crate::intel::shader::triangle_pipeline_simd16(),
            crate::intel::shader::triangle_pipeline_simd16_note(),
        ),
        BackendProbeMode::PsEotOnly => (
            crate::intel::shader::triangle_pipeline_ps_eot(),
            crate::intel::shader::triangle_pipeline_ps_eot_note(),
        ),
        _ => (
            crate::intel::shader::triangle_pipeline(),
            crate::intel::shader::triangle_pipeline_note(),
        ),
    };
    log_render_buffer_layout(warm, Some(dst_gpu_addr));
    log_render_packet_encodings();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!(
            "intel/render: {} staged rt=0x{:X} vb=0x{:X} state=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} geometry={} status=awaiting-igc-or-spec-triangle-shaders vs_src={} ps_src={} note={}\n",
            submit_name,
            draw.rt_gpu_addr,
            draw.vertex_gpu_addr,
            draw.state_gpu_addr,
            draw.target_w,
            draw.target_h,
            draw.rt_pitch,
            draw.vertex_count,
            draw.vertex_stride,
            geometry.label(),
            crate::intel::shader::TRIANGLE_VERTEX_SOURCE_PATH,
            crate::intel::shader::TRIANGLE_FRAGMENT_SOURCE_PATH,
            pipeline_note
        );
        return false;
    }

    intel_render_verbose_log!(
        "intel/render: {} ps-meta dispatch={:?} grf_start={} grf_used={} ksp_off=0x{:X} size={} header_only={} geometry={} vf_contract={} backend={} postdraw_sync={} note={}\n",
        submit_name,
        pipeline.ps.meta.kernel.dispatch_mode,
        pipeline.ps.meta.kernel.grf_start_register,
        pipeline.ps.meta.kernel.grf_used,
        pipeline.ps.meta.kernel.ksp_offset_bytes,
        pipeline.ps.meta.kernel.code_size_bytes,
        (pipeline.ps.meta.num_varying_inputs == 0
            && pipeline.ps.meta.kernel.push_constant_bytes == 0) as u8,
        geometry.label(),
        vf_experiment.vf_slot_contract(),
        backend_probe_mode.label(),
        post_draw_sync_variant.label(),
        pipeline_note
    );
    if geometry.fullscreen_candidate() {
        intel_render_focus_log!(
            "intel/render: {} fragment-candidate-shape accepted=1 geometry={} ndc=v0[-1.000,-1.000] v1[3.000,-1.000] v2[-1.000,3.000] screen_bbox=[0,0..{},{}] sample_points=full-surface coverage_contract=oversized-triangle does_not_prove=raster_samples_or_ps\n",
            submit_name,
            geometry.label(),
            draw.target_w.saturating_sub(1),
            draw.target_h.saturating_sub(1),
        );
    } else if geometry.point_candidate() {
        let point_width_raw = backend_probe_mode
            .point_width_raw_override()
            .unwrap_or(0x200);
        intel_render_focus_log!(
            "intel/render: {} fragment-candidate-shape accepted=1 geometry={} topology=pointlist ndc=center point_width_raw=0x{:X} point_width_source={} vf_contract={} screen_center=[{},{}] coverage_contract=giant-point does_not_prove=raster_samples_or_ps\n",
            submit_name,
            geometry.label(),
            point_width_raw,
            if backend_probe_mode.point_width_from_vertex() {
                "vertex"
            } else {
                "state"
            },
            vf_experiment.vf_slot_contract(),
            draw.target_w / 2,
            draw.target_h / 2,
        );
    } else if geometry.line_candidate() {
        intel_render_focus_log!(
            "intel/render: {} fragment-candidate-shape accepted=1 geometry={} topology=linelist vf_contract={} target={}x{} coverage_contract=diagonal-line does_not_prove=raster_samples_or_ps\n",
            submit_name,
            geometry.label(),
            vf_experiment.vf_slot_contract(),
            draw.target_w,
            draw.target_h,
        );
    }

    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: {} staging skipped reason=shader-layout-error detail={} note={}\n",
                submit_name,
                reason,
                pipeline_note
            );
            return false;
        }
    };
    let ps_ksp_code_dword_index =
        (pipeline.ps.meta.kernel.ksp_offset_bytes / core::mem::size_of::<u32>() as u32) as usize;
    let ps_ksp_packet_offset = shader_layout
        .ps
        .code_offset_bytes
        .saturating_add(shader_layout.ps.ksp_offset_bytes);
    let ps_ksp_base = ps_ksp_packet_offset & !0x3F;
    let ps_ksp0 = if matches!(backend_probe_mode.ps_dispatch_slot(), Some(1 | 2)) {
        0
    } else {
        ps_ksp_base
    };
    let ps_ksp1 = if matches!(
        backend_probe_mode,
        BackendProbeMode::PsDispatchSlot1
            | BackendProbeMode::PsDispatchAllKspSlots
            | BackendProbeMode::PsSimd16
    ) {
        ps_ksp_base
    } else {
        0
    };
    let ps_ksp2 = if matches!(
        backend_probe_mode,
        BackendProbeMode::PsDispatchSlot2 | BackendProbeMode::PsDispatchAllKspSlots
    ) {
        ps_ksp_base
    } else {
        0
    };
    let baked_ps_first = pipeline
        .ps
        .code
        .get(ps_ksp_code_dword_index)
        .copied()
        .unwrap_or(0);
    let uploaded_ps_first = unsafe {
        let ptr = (warm.draw_state_virt as *const u8).add(
            shader_layout.ps.code_offset_bytes as usize
                + shader_layout.ps.ksp_offset_bytes as usize,
        ) as *const u32;
        core::ptr::read_volatile(ptr)
    };
    let ps_ksp_contract_ok = baked_ps_first != 0 && baked_ps_first == uploaded_ps_first;
    intel_render_focus_log!(
        "intel/render: {} ps-ksp-proof accepted={} backend={} ksp0=0x{:X} ksp1=0x{:X} ksp2=0x{:X} ksp_off=0x{:X} first_dw=0x{:08X} baked_first=0x{:08X} dispatch={:?} does_not_prove=ps_thread_launch\n",
        submit_name,
        ps_ksp_contract_ok as u8,
        backend_probe_mode.label(),
        ps_ksp0,
        ps_ksp1,
        ps_ksp2,
        ps_ksp_packet_offset,
        uploaded_ps_first,
        baked_ps_first,
        pipeline.ps.meta.kernel.dispatch_mode,
    );

    intel_render_verbose_log!(
        "intel/render: {} staged rt=0x{:X} vb=0x{:X} state=0x{:X} used_end=0x{:X} state_off=0x{:X} state_region=0x{:X} free=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} geometry={} backend={} status=pipeline-ready vs_bytes={} vs_off=0x{:X} vs_gpu=0x{:X} vs_ksp_off=0x{:X} vs_ksp=0x{:X} ps_bytes={} ps_off=0x{:X} ps_gpu=0x{:X} ps_ksp_off=0x{:X} ps_ksp=0x{:X} varyings={} ps_dispatch={:?}\n",
        submit_name,
        draw.rt_gpu_addr,
        draw.vertex_gpu_addr,
        draw.state_gpu_addr,
        shader_layout.used_bytes,
        shader_layout.state_region_offset_bytes,
        shader_layout.state_region_gpu_addr,
        warm.draw_state_len
            .saturating_sub(shader_layout.state_region_offset_bytes as usize),
        draw.target_w,
        draw.target_h,
        draw.rt_pitch,
        draw.vertex_count,
        draw.vertex_stride,
        geometry.label(),
        backend_probe_mode.label(),
        shader_layout.vs.code_size_bytes,
        shader_layout.vs.code_offset_bytes,
        shader_layout.vs.code_gpu_addr,
        shader_layout.vs.ksp_offset_bytes,
        shader_layout.vs.ksp_gpu_addr,
        shader_layout.ps.code_size_bytes,
        shader_layout.ps.code_offset_bytes,
        shader_layout.ps.code_gpu_addr,
        shader_layout.ps.ksp_offset_bytes,
        shader_layout.ps.ksp_gpu_addr,
        pipeline.ps.meta.num_varying_inputs,
        pipeline.ps.meta.kernel.dispatch_mode
    );

    let probe_state =
        match write_triangle_probe_state(warm, draw, shader_layout, blend_mode, backend_probe_mode)
        {
            Ok(layout) => layout,
            Err(reason) => {
                crate::log!(
                    "intel/render: {} staging skipped reason=probe-state-error detail={}\n",
                    submit_name,
                    reason
                );
                return false;
            }
        };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_mode = if geometry.point_candidate() {
        TriangleBatchMode::VfPointDraw
    } else if geometry.line_candidate() {
        TriangleBatchMode::VfLineDraw
    } else if geometry.ndc_rect_candidate() {
        TriangleBatchMode::VfRectClipDraw
    } else if geometry.rect_candidate() {
        TriangleBatchMode::VfRectDraw
    } else {
        TriangleBatchMode::VfDraw
    };
    let batch_tail_bytes = match encode_triangle_probe_batch(
        submit_name,
        batch,
        warm,
        draw,
        blend_mode,
        pipeline,
        shader_layout,
        probe_state,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        batch_mode,
        vf_experiment,
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
        backend_probe_mode,
        post_draw_sync_variant,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "intel/render: {} staging skipped reason=probe-batch-error detail={}\n",
                submit_name,
                reason
            );
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    intel_render_verbose_log!(
        "intel/render: {} batch-ready bytes=0x{:X} bt_off=0x{:X} samp_off=0x{:X} blend_off=0x{:X} cc_state_off=0x{:X} cc_vp_off=0x{:X} sf_vp_off=0x{:X} geometry={}\n",
        submit_name,
        batch_tail_bytes,
        probe_state.binding_table_offset_bytes,
        probe_state.sampler_state_offset_bytes,
        probe_state.blend_state_offset_bytes,
        probe_state.color_calc_state_offset_bytes,
        probe_state.cc_viewport_offset_bytes,
        probe_state.sf_clip_viewport_offset_bytes,
        geometry.label(),
    );
    intel_render_verbose_log!(
        "intel/render: {} blend-probe={} geometry={}\n",
        submit_name,
        blend_mode.label(),
        geometry.label(),
    );
    log_triangle_probe_state(warm, shader_layout, probe_state);

    let scratch_rt_before = if is_scratch_rt_submit_name(submit_name) {
        crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
        let center_x = draw.target_w / 2;
        let center_y = draw.target_h / 2;
        let center_offset = center_y
            .saturating_mul(draw.rt_pitch)
            .saturating_add(center_x.saturating_mul(4)) as usize;
        let post_offset =
            center_offset.saturating_add(if center_x + 1 < draw.target_w { 4 } else { 0 });
        let read_scratch_dword = |byte_offset: usize| -> u32 {
            if byte_offset.saturating_add(core::mem::size_of::<u32>()) > warm.streamout_len {
                return 0;
            }
            unsafe {
                let ptr = (warm.streamout_virt as *const u8).add(byte_offset) as *const u32;
                core::ptr::read_volatile(ptr)
            }
        };
        Some((
            read_scratch_dword(0),
            read_scratch_dword(center_offset),
            read_scratch_dword(post_offset),
            center_offset,
            post_offset,
        ))
    } else {
        None
    };
    let scratch_stats_before = if scratch_rt_before.is_some() {
        Some(capture_triangle_stage_stats(dev))
    } else {
        None
    };

    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        submit_name,
    );
    if let (
        Some((scratch_before, center_before, post_before, center_offset, post_offset)),
        Some(stats_before),
    ) = (scratch_rt_before, scratch_stats_before)
    {
        crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
        let read_scratch_dword = |byte_offset: usize| -> u32 {
            if byte_offset.saturating_add(core::mem::size_of::<u32>()) > warm.streamout_len {
                return 0;
            }
            unsafe {
                let ptr = (warm.streamout_virt as *const u8).add(byte_offset) as *const u32;
                core::ptr::read_volatile(ptr)
            }
        };
        let scratch_after = read_scratch_dword(0);
        let center_after = read_scratch_dword(center_offset);
        let post_after = read_scratch_dword(post_offset);
        let delta = capture_triangle_stage_stats(dev).delta_since(stats_before);
        let ps_counter_accept =
            delta.ps_invocations > 0 || delta.cps_invocations > 0 || delta.ps_depth > 0;
        let rt_changed = scratch_after != scratch_before
            || center_after != center_before
            || post_after != post_before;
        let artificial_markers = is_artificial_fragment_marker_submit_name(submit_name);
        let artificial_pre_marker = center_after == RCS_ARTIFICIAL_FRAGMENT_PRE_COLOR;
        let artificial_post_marker = post_after == RCS_ARTIFICIAL_FRAGMENT_POST_COLOR;
        let possible_draw_window_write = artificial_markers
            && artificial_post_marker
            && center_after != RCS_ARTIFICIAL_FRAGMENT_PRE_COLOR;
        let accepted =
            ps_counter_accept || (!artificial_markers && rt_changed) || possible_draw_window_write;
        record_fragment_boundary_probe(true, accepted);
        intel_render_focus_log!(
            "intel/render: {} scratch-rt-fragment-proof accepted={} completed={} rt_gpu=0x{:X} size={}x{} pitch=0x{:X} before=0x{:08X} after=0x{:08X} center_before=0x{:08X} center_after=0x{:08X} post_before=0x{:08X} post_after=0x{:08X} changed={} artificial={} artificial_pre_marker={} artificial_post_marker={} possible_draw_window_write={} ps_delta={} cps_delta={} ps_depth_delta={} does_not_prove=display_scanout\n",
            submit_name,
            accepted as u8,
            completed as u8,
            draw.rt_gpu_addr,
            draw.target_w,
            draw.target_h,
            draw.rt_pitch,
            scratch_before,
            scratch_after,
            center_before,
            center_after,
            post_before,
            post_after,
            rt_changed as u8,
            artificial_markers as u8,
            artificial_pre_marker as u8,
            artificial_post_marker as u8,
            possible_draw_window_write as u8,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
        );
        if is_raster_wm_oa_submit_name(submit_name) {
            log_raster_wm_oa_probe(submit_name, warm, completed, draw, delta);
        }
    }
    if is_raster_wm_oa_submit_name(submit_name) {
        disable_raster_wm_oa_context(dev, submit_name);
    }
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, submit_name);
    }
    completed
}

fn disable_raster_wm_oa_context(dev: crate::intel::Dev, submit_name: &'static str) {
    crate::intel::mmio_write(dev, RCS_OACTXCONTROL, 0);
    crate::intel::mmio_write(dev, OAR_OACONTROL, 0);
    crate::intel::mmio_write(
        dev,
        RCS_RING_CONTEXT_CONTROL,
        masked_bits_update(0, CTX_CTRL_OAC_CONTEXT_ENABLE),
    );
    intel_render_focus_log!(
        "intel/render: {} raster-wm-oa cleanup oactx=0 oar=0 reason=diagnostic-counter-disable\n",
        submit_name,
    );
}

fn oa_report_slice(warm: RenderWarmState, base_dword: usize) -> Option<&'static [u32]> {
    if base_dword
        .checked_add(RESULT_OA_REPORT_DWORDS)?
        .checked_mul(core::mem::size_of::<u32>())?
        > warm.result_len
    {
        return None;
    }
    let dwords =
        unsafe { core::slice::from_raw_parts(warm.result_virt as *const u32, warm.result_len / 4) };
    dwords.get(base_dword..base_dword + RESULT_OA_REPORT_DWORDS)
}

fn oa_counter_delta(before: u64, after: u64, bits: u32) -> u64 {
    if after >= before {
        after - before
    } else {
        (1u64 << bits).saturating_add(after).saturating_sub(before)
    }
}

fn oa_a_counter_gfx125(report: &[u32], index: usize) -> Option<u64> {
    if report.len() < RESULT_OA_REPORT_DWORDS || index >= 36 {
        return None;
    }
    if index < 4 {
        Some(report[4 + index] as u64)
    } else if index < 24 {
        let high_bytes =
            unsafe { core::slice::from_raw_parts(report.as_ptr().add(40) as *const u8, 32) };
        Some(report[4 + index] as u64 | ((high_bytes[index] as u64) << 32))
    } else if index < 28 {
        Some(report[28 + (index - 24)] as u64)
    } else if index < 32 {
        let high_bytes =
            unsafe { core::slice::from_raw_parts(report.as_ptr().add(40) as *const u8, 32) };
        Some(report[4 + index] as u64 | ((high_bytes[index] as u64) << 32))
    } else {
        Some(report[36 + (index - 32)] as u64)
    }
}

fn oa_a_delta_gfx125(begin: &[u32], end: &[u32], index: usize) -> u64 {
    let Some(before) = oa_a_counter_gfx125(begin, index) else {
        return 0;
    };
    let Some(after) = oa_a_counter_gfx125(end, index) else {
        return 0;
    };
    let bits = if (4..24).contains(&index) || (28..32).contains(&index) {
        40
    } else {
        32
    };
    oa_counter_delta(before, after, bits)
}

fn log_raster_wm_oa_raw_deltas(submit_name: &'static str, begin: &[u32], end: &[u32]) {
    let mut a = [0u64; 36];
    let mut changed = 0usize;
    let mut i = 0usize;
    while i < a.len() {
        a[i] = oa_a_delta_gfx125(begin, end, i);
        if a[i] != 0 {
            changed += 1;
        }
        i += 1;
    }
    intel_render_verbose_log!(
        "intel/render: {} oa-raw-a-delta changed={} a00={} a01={} a02={} a03={} a04={} a05={} a06={} a07={} a08={} a09={} a10={} a11={}\n",
        submit_name,
        changed,
        a[0],
        a[1],
        a[2],
        a[3],
        a[4],
        a[5],
        a[6],
        a[7],
        a[8],
        a[9],
        a[10],
        a[11],
    );
    intel_render_verbose_log!(
        "intel/render: {} oa-raw-a-delta a12={} a13={} a14={} a15={} a16={} a17={} a18={} a19={} a20={} a21={} a22={} a23={}\n",
        submit_name,
        a[12],
        a[13],
        a[14],
        a[15],
        a[16],
        a[17],
        a[18],
        a[19],
        a[20],
        a[21],
        a[22],
        a[23],
    );
    intel_render_verbose_log!(
        "intel/render: {} oa-raw-a-delta a24={} a25={} a26={} a27={} a28={} a29={} a30={} a31={} a32={} a33={} a34={} a35={} note=raw-counter-index-audit\n",
        submit_name,
        a[24],
        a[25],
        a[26],
        a[27],
        a[28],
        a[29],
        a[30],
        a[31],
        a[32],
        a[33],
        a[34],
        a[35],
    );
}

fn log_raster_wm_oa_probe(
    submit_name: &'static str,
    warm: RenderWarmState,
    completed: bool,
    draw: TriangleDrawPrep,
    delta: TriangleStageStats,
) {
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let begin = oa_report_slice(warm, RESULT_OA_BEGIN_DWORD);
    let end = oa_report_slice(warm, RESULT_OA_END_DWORD);
    let begin_id = begin.and_then(|r| r.first().copied()).unwrap_or(0);
    let end_id = end.and_then(|r| r.first().copied()).unwrap_or(0);
    let reports_valid =
        begin_id == RESULT_OA_RASTER_WM_BEGIN_ID && end_id == RESULT_OA_RASTER_WM_END_ID;

    let (ps_threads_delta, raster_samples_delta, samples_killed_delta, postps_fail_delta) =
        if reports_valid {
            let begin = begin.unwrap_or(&[]);
            let end = end.unwrap_or(&[]);
            (
                oa_a_delta_gfx125(begin, end, 6),
                oa_a_delta_gfx125(begin, end, 21).saturating_mul(4),
                oa_a_delta_gfx125(begin, end, 24).saturating_mul(4),
                oa_a_delta_gfx125(begin, end, 25).saturating_mul(4),
            )
        } else {
            (0, 0, 0, 0)
        };
    let (pixel_write_delta, pixel_blend_delta) = if reports_valid {
        let begin = begin.unwrap_or(&[]);
        let end = end.unwrap_or(&[]);
        (
            oa_a_delta_gfx125(begin, end, 26).saturating_mul(4),
            oa_a_delta_gfx125(begin, end, 27).saturating_mul(4),
        )
    } else {
        (0, 0)
    };
    let accepted = reports_valid
        && (raster_samples_delta != 0
            || ps_threads_delta != 0
            || samples_killed_delta != 0
            || postps_fail_delta != 0
            || pixel_write_delta != 0
            || pixel_blend_delta != 0);
    if reports_valid && !accepted {
        let begin = begin.unwrap_or(&[]);
        let end = end.unwrap_or(&[]);
        log_raster_wm_oa_raw_deltas(submit_name, begin, end);
    }
    record_fragment_boundary_probe(true, accepted);
    intel_render_focus_log!(
        "intel/render: {} raster-wm-input-proof accepted={} completed={} reports_valid={} begin_id=0x{:08X} end_id=0x{:08X} rt_gpu=0x{:X} size={}x{} pitch=0x{:X} raster_samples_delta={} ps_threads_delta={} samples_killed_delta={} postps_fail_delta={} pixel_write_delta={} pixel_blend_delta={} ps_delta={} cps_delta={} ps_depth_delta={} observable=oar-mi-rpc-a21 does_not_prove=rt_visible\n",
        submit_name,
        accepted as u8,
        completed as u8,
        reports_valid as u8,
        begin_id,
        end_id,
        draw.rt_gpu_addr,
        draw.target_w,
        draw.target_h,
        draw.rt_pitch,
        raster_samples_delta,
        ps_threads_delta,
        samples_killed_delta,
        postps_fail_delta,
        pixel_write_delta,
        pixel_blend_delta,
        delta.ps_invocations,
        delta.cps_invocations,
        delta.ps_depth,
    );
}

fn submit_triangle_vs_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> bool {
    let Some(draw) = prepare_triangle_draw_resources(warm, dst_gpu_addr, pitch, rect_w, rect_h)
    else {
        crate::log!(
            "intel/render: vs-streamout-proof skipped reason=resource-layout size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };
    let pipeline = crate::intel::shader::triangle_pipeline();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!("intel/render: vs-streamout-proof skipped reason=placeholder-pipeline\n");
        return false;
    }
    let slice_hash_table_offset = match write_vf_streamout_probe_state(warm) {
        Ok(offset) => offset,
        Err(reason) => {
            crate::log!(
                "intel/render: vs-streamout-proof skipped reason=probe-state detail={}\n",
                reason
            );
            return false;
        }
    };
    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: vs-streamout-proof skipped reason=shader-layout detail={}\n",
                reason
            );
            return false;
        }
    };
    if slice_hash_table_offset != 0
        && usize::try_from(shader_layout.used_bytes)
            .ok()
            .unwrap_or(usize::MAX)
            > slice_hash_table_offset as usize
    {
        crate::log!(
            "intel/render: vs-streamout-proof skipped reason=slice-hash-overlap used_end=0x{:X} slice_hash_off=0x{:X}\n",
            shader_layout.used_bytes,
            slice_hash_table_offset
        );
        return false;
    }

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_vs_streamout_proof_batch(
        batch,
        warm,
        draw,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        experiment,
        slice_hash_table_offset,
        VsStreamoutProofConfig {
            pipeline,
            shader_layout,
        },
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!("intel/render: vs-streamout-proof batch build failed detail={}\n", reason);
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    intel_render_verbose_log!(
        "intel/render: vs-streamout-proof batch-ready experiment={} bytes=0x{:X} so_gpu=0x{:X} so_pitch={} vertices={}\n",
        experiment.label(),
        batch_tail_bytes,
        GPU_VA_STREAMOUT_BASE,
        experiment.vertex_bytes(),
        draw.vertex_count
    );

    let stats_before = capture_triangle_stage_stats(dev);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        "vs-streamout-proof",
    );
    let stats_after = capture_triangle_stage_stats(dev);
    let accepted = completed
        || maybe_soft_accept_streamout_submit(
            "vs-streamout-proof",
            warm,
            stats_before,
            stats_after,
            true,
            experiment.vertex_bytes() * draw.vertex_count as usize,
        );
    log_streamout_proof_result(
        "vs-streamout-proof",
        warm,
        completed,
        draw.vertex_count as usize,
        experiment,
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "vs-streamout-proof");
    }
    accepted
}

fn submit_triangle_streamout_proof(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    experiment: StreamoutProofExperiment,
) -> bool {
    let Some(draw) = prepare_triangle_draw_resources(warm, dst_gpu_addr, pitch, rect_w, rect_h)
    else {
        crate::log!(
            "intel/render: streamout-proof skipped reason=resource-layout size={}x{} pitch=0x{:X}\n",
            rect_w,
            rect_h,
            pitch
        );
        return false;
    };
    let pipeline = crate::intel::shader::triangle_pipeline();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!("intel/render: streamout-proof skipped reason=placeholder-pipeline\n");
        return false;
    }
    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: streamout-proof skipped reason=shader-layout detail={}\n",
                reason
            );
            return false;
        }
    };
    let probe_state = match write_triangle_probe_state(
        warm,
        draw,
        shader_layout,
        TriangleBlendProbeMode::ExplicitRt0,
        BackendProbeMode::MesaLike,
    ) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: streamout-proof skipped reason=probe-state detail={}\n",
                reason
            );
            return false;
        }
    };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_bytes(warm.streamout_virt, 0, warm.streamout_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_tail_bytes = match encode_triangle_probe_batch(
        "streamout-proof",
        batch,
        warm,
        draw,
        TriangleBlendProbeMode::ExplicitRt0,
        pipeline,
        shader_layout,
        probe_state,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        TriangleBatchMode::StreamoutProof,
        experiment,
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::HeavyAll,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!("intel/render: streamout-proof batch build failed detail={}\n", reason);
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    intel_render_verbose_log!(
        "intel/render: streamout-proof batch-ready experiment={} bytes=0x{:X} so_gpu=0x{:X} so_pitch={} vertices={}\n",
        experiment.label(),
        batch_tail_bytes,
        GPU_VA_STREAMOUT_BASE,
        experiment.vertex_bytes(),
        draw.vertex_count
    );

    let stats_before = capture_triangle_stage_stats(dev);
    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        "streamout-proof",
    );
    let stats_after = capture_triangle_stage_stats(dev);
    let accepted = completed
        || maybe_soft_accept_streamout_submit(
            "streamout-proof",
            warm,
            stats_before,
            stats_after,
            true,
            experiment.vertex_bytes() * draw.vertex_count as usize,
        );
    log_streamout_proof_result(
        "streamout-proof",
        warm,
        completed,
        draw.vertex_count as usize,
        experiment,
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, "streamout-proof");
    }
    accepted
}

fn submit_triangle_vs_draw_frontier_to_surface(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
) -> bool {
    for contract in VS_DRAW_FRONTIER_CONTRACTS {
        let completed = submit_triangle_real_vs_draw_probe_to_surface(
            dev,
            warm,
            dst_gpu_addr,
            pitch,
            rect_w,
            rect_h,
            blend_mode,
            "vs-draw-frontier",
            contract,
        );
        intel_render_verbose_log!(
            "intel/render: primary-vs-draw-frontier-contract variant={} completed={}\n",
            contract.label,
            completed as u8,
        );
        if completed {
            return true;
        }
    }
    false
}

fn submit_triangle_vs_draw_frontier_to_scratch(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
) -> bool {
    let scratch_pitch = 32 * core::mem::size_of::<u32>();
    if warm.streamout_len < scratch_pitch * 32 {
        intel_render_focus_log!(
            "intel/render: vs-draw-frontier-scratch skipped reason=streamout-too-small len=0x{:X} required=0x{:X}\n",
            warm.streamout_len,
            scratch_pitch * 32,
        );
        return false;
    }
    let variants = [
        ("vs-draw-frontier-scratch", VfPrimitiveGeometry::Canonical, None),
        ("vs-draw-frontier-scratch-ndc-rect", VfPrimitiveGeometry::NdcRect, None),
        (
            "vs-draw-frontier-scratch-ndc-rect-trilist",
            VfPrimitiveGeometry::NdcRect,
            Some(TriangleBatchMode::Draw),
        ),
        ("vs-draw-frontier-scratch-ndc-rect-cw", VfPrimitiveGeometry::NdcRectCw, None),
        (
            "vs-draw-frontier-scratch-ndc-rect-cw-trilist",
            VfPrimitiveGeometry::NdcRectCw,
            Some(TriangleBatchMode::Draw),
        ),
        (
            "vs-draw-frontier-scratch-screen-rect",
            VfPrimitiveGeometry::ScreenSpaceRect8x8OrderB,
            None,
        ),
        ("vs-draw-frontier-scratch-ndc-large", VfPrimitiveGeometry::NdcTriangleLarge, None),
    ];
    let contracts = [
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
        VS_DRAW_SBE_READ0_CONTRACT,
        VS_DRAW_FRONTIER_CONTRACTS[2],
    ];
    for (submit_name, geometry, batch_mode_override) in variants {
        for contract in contracts {
            seed_render_scratch_rt(warm);
            let completed = submit_triangle_real_vs_draw_probe_to_surface_ext(
                dev,
                warm,
                GPU_VA_STREAMOUT_BASE,
                scratch_pitch,
                32,
                32,
                TriangleBlendProbeMode::MesaZeroedState,
                geometry,
                submit_name,
                contract,
                BackendProbeMode::MesaLike,
                PostDrawSyncVariant::LightPostSyncNoCs,
                batch_mode_override,
            );
            let observed = fragment_boundary_observed();
            intel_render_focus_log!(
                "intel/render: vs-draw-frontier-scratch variant={} geometry={} contract={} completed={} observed={} target=scratch-rt32\n",
                submit_name,
                geometry.label(),
                contract.label,
                completed as u8,
                observed as u8,
            );
            if observed {
                return true;
            }
        }
    }
    false
}

fn wait_eq(dev: crate::intel::Dev, reg: usize, mask: u32, want: u32, n: usize) -> bool {
    for _ in 0..n {
        if (crate::intel::mmio_read(dev, reg) & mask) == want {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn map_smoke_buffers(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    let ok_ring = super::map_ggtt(dev, warm.ring_phys, warm.ring_len, GPU_VA_RING_BASE);
    let ok_context = super::map_ggtt(dev, warm.context_phys, warm.context_len, GPU_VA_CONTEXT_BASE);
    let ok_batch = super::map_ggtt(dev, warm.batch_phys, warm.batch_len, GPU_VA_BATCH_BASE);
    let ok_draw_state =
        super::map_ggtt(dev, warm.draw_state_phys, warm.draw_state_len, GPU_VA_DRAW_STATE_BASE);
    let ok_vertex = super::map_ggtt(dev, warm.vertex_phys, warm.vertex_len, GPU_VA_VERTEX_BASE);
    let ok_result = super::map_ggtt(dev, warm.result_phys, warm.result_len, GPU_VA_RESULT_BASE);
    let ok_streamout =
        super::map_ggtt(dev, warm.streamout_phys, warm.streamout_len, GPU_VA_STREAMOUT_BASE);
    if ok_ring && ok_context && ok_batch && ok_draw_state && ok_vertex && ok_result && ok_streamout
    {
        super::ggtt_invalidate(dev);
        true
    } else {
        false
    }
}

fn read_first_dword(virt: *mut u8, len: usize) -> u32 {
    if virt.is_null() || len < core::mem::size_of::<u32>() {
        return 0;
    }
    unsafe { core::ptr::read_volatile(virt as *const u32) }
}

fn log_render_memory_proof(warm: RenderWarmState) {
    crate::intel::dma_flush(warm.ring_virt, warm.ring_len);
    crate::intel::dma_flush(warm.context_virt, warm.context_len);
    crate::intel::dma_flush(warm.batch_virt, warm.batch_len);
    crate::intel::dma_flush(warm.draw_state_virt, warm.draw_state_len);
    crate::intel::dma_flush(warm.vertex_virt, warm.vertex_len);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len);

    let ring_rb = read_first_dword(warm.ring_virt, warm.ring_len);
    let context_rb = read_first_dword(warm.context_virt, warm.context_len);
    let batch_rb = read_first_dword(warm.batch_virt, warm.batch_len);
    let state_rb = read_first_dword(warm.draw_state_virt, warm.draw_state_len);
    let vertex_rb = read_first_dword(warm.vertex_virt, warm.vertex_len);
    let result_rb = read_first_dword(warm.result_virt, warm.result_len);
    let streamout_rb = read_first_dword(warm.streamout_virt, warm.streamout_len);

    intel_render_focus_log!(
        "intel/render: memory-proof accepted=1 map=1 ggtt_invalidated=1 flush=all readback=cpu-first-dword ring[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] context[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] batch[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] state[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] vertex[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] result[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] streamout[phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rb=0x{:08X}] does_not_prove=fragment_ps_rt_progress\n",
        warm.ring_phys,
        GPU_VA_RING_BASE,
        warm.ring_len,
        ring_rb,
        warm.context_phys,
        GPU_VA_CONTEXT_BASE,
        warm.context_len,
        context_rb,
        warm.batch_phys,
        GPU_VA_BATCH_BASE,
        warm.batch_len,
        batch_rb,
        warm.draw_state_phys,
        GPU_VA_DRAW_STATE_BASE,
        warm.draw_state_len,
        state_rb,
        warm.vertex_phys,
        GPU_VA_VERTEX_BASE,
        warm.vertex_len,
        vertex_rb,
        warm.result_phys,
        GPU_VA_RESULT_BASE,
        warm.result_len,
        result_rb,
        warm.streamout_phys,
        GPU_VA_STREAMOUT_BASE,
        warm.streamout_len,
        streamout_rb,
    );
}

fn ensure_smoke_buffers_mapped(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    if !map_smoke_buffers(dev, warm) {
        WARM_BUFFERS_MAPPED.store(false, Ordering::Release);
        return false;
    }
    if !MEMORY_PROOF_LOGGED.swap(true, Ordering::AcqRel) {
        log_render_memory_proof(warm);
    }
    WARM_BUFFERS_MAPPED.store(true, Ordering::Release);
    true
}

fn should_log_primary_probe(reason: &str, seq: u32) -> bool {
    reason == "boot-once" || seq <= 3 || seq.is_multiple_of(PRIMARY_PERIODIC_LOG_EVERY)
}

fn should_log_primary_probe_detail() -> bool {
    if crate::logflag::INTEL_STAGE1_LOGS {
        return false;
    }
    let seq = PRIMARY_PROBE_SEQ.load(Ordering::Acquire);
    seq <= 3 || seq.is_multiple_of(PRIMARY_PERIODIC_LOG_EVERY)
}

fn submit_triangle_draw_to_surface(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
) -> bool {
    submit_triangle_real_vs_draw_probe_to_surface(
        dev,
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        blend_mode,
        "draw-path",
        TRIANGLE_DEFAULT_FRONT_END_CONTRACT,
    )
}

fn submit_triangle_real_vs_draw_probe_to_surface(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
    submit_name: &'static str,
    front_end_contract: TriangleFrontEndContract,
) -> bool {
    submit_triangle_real_vs_draw_probe_to_surface_ext(
        dev,
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        blend_mode,
        VfPrimitiveGeometry::Canonical,
        submit_name,
        front_end_contract,
        BackendProbeMode::MesaLike,
        PostDrawSyncVariant::HeavyAll,
        None,
    )
}

fn submit_triangle_real_vs_draw_probe_to_surface_ext(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    blend_mode: TriangleBlendProbeMode,
    geometry: VfPrimitiveGeometry,
    submit_name: &'static str,
    front_end_contract: TriangleFrontEndContract,
    backend_probe_mode: BackendProbeMode,
    post_draw_sync_variant: PostDrawSyncVariant,
    batch_mode_override: Option<TriangleBatchMode>,
) -> bool {
    let Some(draw) = prepare_triangle_draw_resources_for_geometry(
        warm,
        dst_gpu_addr,
        pitch,
        rect_w,
        rect_h,
        geometry,
    ) else {
        crate::log!(
            "intel/render: {} staging skipped reason=resource-layout size={}x{} pitch=0x{:X} geometry={}\n",
            submit_name,
            rect_w,
            rect_h,
            pitch,
            geometry.label(),
        );
        return false;
    };

    let pipeline = crate::intel::shader::triangle_pipeline();
    log_render_buffer_layout(warm, Some(dst_gpu_addr));
    log_render_packet_encodings();
    if crate::intel::shader::triangle_pipeline_is_placeholder() {
        crate::log!(
            "intel/render: {} staged rt=0x{:X} vb=0x{:X} state=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} status=awaiting-igc-or-spec-triangle-shaders vs_src={} ps_src={} note={}\n",
            submit_name,
            draw.rt_gpu_addr,
            draw.vertex_gpu_addr,
            draw.state_gpu_addr,
            draw.target_w,
            draw.target_h,
            draw.rt_pitch,
            draw.vertex_count,
            draw.vertex_stride,
            crate::intel::shader::TRIANGLE_VERTEX_SOURCE_PATH,
            crate::intel::shader::TRIANGLE_FRAGMENT_SOURCE_PATH,
            crate::intel::shader::triangle_pipeline_note()
        );
        return false;
    }

    intel_render_verbose_log!(
        "intel/render: {} ps-meta dispatch={:?} grf_start={} grf_used={} ksp_off=0x{:X} size={} header_only={} geometry={} backend={} postdraw_sync={} note={}\n",
        submit_name,
        pipeline.ps.meta.kernel.dispatch_mode,
        pipeline.ps.meta.kernel.grf_start_register,
        pipeline.ps.meta.kernel.grf_used,
        pipeline.ps.meta.kernel.ksp_offset_bytes,
        pipeline.ps.meta.kernel.code_size_bytes,
        (pipeline.ps.meta.num_varying_inputs == 0
            && pipeline.ps.meta.kernel.push_constant_bytes == 0) as u8,
        geometry.label(),
        backend_probe_mode.label(),
        post_draw_sync_variant.label(),
        crate::intel::shader::triangle_pipeline_note()
    );
    if geometry.fullscreen_candidate() {
        intel_render_focus_log!(
            "intel/render: {} fragment-candidate-shape accepted=1 geometry={} ndc=v0[-1.000,-1.000] v1[3.000,-1.000] v2[-1.000,3.000] screen_bbox=[0,0..{},{}] sample_points=full-surface coverage_contract=oversized-triangle does_not_prove=raster_samples_or_ps\n",
            submit_name,
            geometry.label(),
            draw.target_w.saturating_sub(1),
            draw.target_h.saturating_sub(1),
        );
    } else if geometry.screen_space_candidate() {
        intel_render_focus_log!(
            "intel/render: {} fragment-candidate-shape accepted=1 geometry={} topology=trilist sf_viewport_transform=0 screen_vertices=v0[0.5,0.5] v1[7.5,0.5] v2[0.5,7.5] target={}x{} coverage_contract=screen-space-scratch-triangle does_not_prove=raster_samples_or_ps\n",
            submit_name,
            geometry.label(),
            draw.target_w,
            draw.target_h,
        );
    }
    let programmed_vs_urb_output_length = front_end_contract
        .vs_urb_output_length_override
        .or(TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE)
        .unwrap_or(pipeline.vs.meta.urb_entry_output_length);
    if submit_name == "vs-draw-frontier" {
        intel_render_focus_log!(
            "intel/render: {} contract variant={} baked_vs_urb_out_len={} programmed_vs_urb_out_len={} sbe[read_offset={} read_length={} force_offset={} force_length={} num_sf_attrs={}]\n",
            submit_name,
            front_end_contract.label,
            pipeline.vs.meta.urb_entry_output_length,
            programmed_vs_urb_output_length,
            front_end_contract.sbe_read_offset,
            front_end_contract.sbe_read_length,
            front_end_contract.force_sbe_read_offset as u8,
            front_end_contract.force_sbe_read_length as u8,
            pipeline.ps.meta.num_varying_inputs,
        );
    } else {
        intel_render_verbose_log!(
            "intel/render: {} contract variant={} baked_vs_urb_out_len={} programmed_vs_urb_out_len={} sbe[read_offset={} read_length={} force_offset={} force_length={} num_sf_attrs={}]\n",
            submit_name,
            front_end_contract.label,
            pipeline.vs.meta.urb_entry_output_length,
            programmed_vs_urb_output_length,
            front_end_contract.sbe_read_offset,
            front_end_contract.sbe_read_length,
            front_end_contract.force_sbe_read_offset as u8,
            front_end_contract.force_sbe_read_length as u8,
            pipeline.ps.meta.num_varying_inputs,
        );
    }

    let shader_layout = match upload_triangle_shader_pipeline(warm, pipeline) {
        Ok(layout) => layout,
        Err(reason) => {
            crate::log!(
                "intel/render: {} staging skipped reason=shader-layout-error detail={} note={}\n",
                submit_name,
                reason,
                crate::intel::shader::triangle_pipeline_note()
            );
            return false;
        }
    };
    log_uploaded_triangle_shader_verification(warm, pipeline, shader_layout, submit_name);

    intel_render_verbose_log!(
        "intel/render: {} staged rt=0x{:X} vb=0x{:X} state=0x{:X} used_end=0x{:X} state_off=0x{:X} state_region=0x{:X} free=0x{:X} size={}x{} pitch=0x{:X} vertices={} stride={} status=pipeline-ready vs_bytes={} vs_off=0x{:X} vs_gpu=0x{:X} vs_ksp_off=0x{:X} vs_ksp=0x{:X} ps_bytes={} ps_off=0x{:X} ps_gpu=0x{:X} ps_ksp_off=0x{:X} ps_ksp=0x{:X} varyings={} ps_dispatch={:?}\n",
        submit_name,
        draw.rt_gpu_addr,
        draw.vertex_gpu_addr,
        draw.state_gpu_addr,
        shader_layout.used_bytes,
        shader_layout.state_region_offset_bytes,
        shader_layout.state_region_gpu_addr,
        warm.draw_state_len
            .saturating_sub(shader_layout.state_region_offset_bytes as usize),
        draw.target_w,
        draw.target_h,
        draw.rt_pitch,
        draw.vertex_count,
        draw.vertex_stride,
        shader_layout.vs.code_size_bytes,
        shader_layout.vs.code_offset_bytes,
        shader_layout.vs.code_gpu_addr,
        shader_layout.vs.ksp_offset_bytes,
        shader_layout.vs.ksp_gpu_addr,
        shader_layout.ps.code_size_bytes,
        shader_layout.ps.code_offset_bytes,
        shader_layout.ps.code_gpu_addr,
        shader_layout.ps.ksp_offset_bytes,
        shader_layout.ps.ksp_gpu_addr,
        pipeline.ps.meta.num_varying_inputs,
        pipeline.ps.meta.kernel.dispatch_mode
    );

    let probe_state =
        match write_triangle_probe_state(warm, draw, shader_layout, blend_mode, backend_probe_mode)
        {
            Ok(layout) => layout,
            Err(reason) => {
                crate::log!(
                    "intel/render: {} staging skipped reason=probe-state-error detail={}\n",
                    submit_name,
                    reason
                );
                return false;
            }
        };

    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
    }
    seed_result_debug_slots(warm);
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let batch_mode = batch_mode_override.unwrap_or_else(|| {
        if geometry.rect_candidate() {
            TriangleBatchMode::DrawScreenSpaceRect
        } else if geometry.screen_space_candidate() {
            TriangleBatchMode::DrawScreenSpace
        } else {
            TriangleBatchMode::Draw
        }
    });
    let batch_tail_bytes = match encode_triangle_probe_batch(
        submit_name,
        batch,
        warm,
        draw,
        blend_mode,
        pipeline,
        shader_layout,
        probe_state,
        GPU_VA_RESULT_BASE,
        RCS_EXEC_RESULT_DRAW_PRE3D,
        RCS_EXEC_RESULT_DRAW_POST3D,
        RCS_EXEC_RESULT_DONE,
        batch_mode,
        StreamoutProofExperiment::PositionSlot1,
        front_end_contract,
        backend_probe_mode,
        post_draw_sync_variant,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "intel/render: {} staging skipped reason=probe-batch-error detail={}\n",
                submit_name,
                reason
            );
            return false;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);

    intel_render_verbose_log!(
        "intel/render: {} batch-ready bytes=0x{:X} bt_off=0x{:X} samp_off=0x{:X} blend_off=0x{:X} cc_state_off=0x{:X} cc_vp_off=0x{:X} sf_vp_off=0x{:X} geometry={} backend={}\n",
        submit_name,
        batch_tail_bytes,
        probe_state.binding_table_offset_bytes,
        probe_state.sampler_state_offset_bytes,
        probe_state.blend_state_offset_bytes,
        probe_state.color_calc_state_offset_bytes,
        probe_state.cc_viewport_offset_bytes,
        probe_state.sf_clip_viewport_offset_bytes,
        geometry.label(),
        backend_probe_mode.label(),
    );
    intel_render_verbose_log!("intel/render: {} blend-probe={}\n", submit_name, blend_mode.label());
    log_triangle_probe_state(warm, shader_layout, probe_state);

    let scratch_rt_before = if is_scratch_rt_submit_name(submit_name) {
        crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
        let center_x = draw.target_w / 2;
        let center_y = draw.target_h / 2;
        let center_offset = center_y
            .saturating_mul(draw.rt_pitch)
            .saturating_add(center_x.saturating_mul(4)) as usize;
        let post_offset =
            center_offset.saturating_add(if center_x + 1 < draw.target_w { 4 } else { 0 });
        let read_scratch_dword = |byte_offset: usize| -> u32 {
            if byte_offset.saturating_add(core::mem::size_of::<u32>()) > warm.streamout_len {
                return 0;
            }
            unsafe {
                let ptr = (warm.streamout_virt as *const u8).add(byte_offset) as *const u32;
                core::ptr::read_volatile(ptr)
            }
        };
        Some((
            read_scratch_dword(0),
            read_scratch_dword(center_offset),
            read_scratch_dword(post_offset),
            center_offset,
            post_offset,
        ))
    } else {
        None
    };
    let scratch_stats_before = if scratch_rt_before.is_some() {
        Some(capture_triangle_stage_stats(dev))
    } else {
        None
    };

    let completed = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_DONE,
        RESULT_SLOT_FINAL_DWORD,
        submit_name,
    );
    if !completed {
        recover_render_engine_after_nonretired_submit(dev, warm, submit_name);
    }
    if let (
        Some((scratch_before, center_before, post_before, center_offset, post_offset)),
        Some(stats_before),
    ) = (scratch_rt_before, scratch_stats_before)
    {
        crate::intel::dma_flush(warm.streamout_virt, warm.streamout_len.min(64));
        let read_scratch_dword = |byte_offset: usize| -> u32 {
            if byte_offset.saturating_add(core::mem::size_of::<u32>()) > warm.streamout_len {
                return 0;
            }
            unsafe {
                let ptr = (warm.streamout_virt as *const u8).add(byte_offset) as *const u32;
                core::ptr::read_volatile(ptr)
            }
        };
        let scratch_after = read_scratch_dword(0);
        let center_after = read_scratch_dword(center_offset);
        let post_after = read_scratch_dword(post_offset);
        let delta = capture_triangle_stage_stats(dev).delta_since(stats_before);
        let ps_counter_accept =
            delta.ps_invocations > 0 || delta.cps_invocations > 0 || delta.ps_depth > 0;
        let rt_changed = scratch_after != scratch_before
            || center_after != center_before
            || post_after != post_before;
        let accepted = ps_counter_accept || rt_changed;
        record_fragment_boundary_probe(true, accepted);
        intel_render_focus_log!(
            "intel/render: {} scratch-rt-fragment-proof accepted={} completed={} rt_gpu=0x{:X} size={}x{} pitch=0x{:X} before=0x{:08X} after=0x{:08X} center_before=0x{:08X} center_after=0x{:08X} post_before=0x{:08X} post_after=0x{:08X} changed={} ps_delta={} cps_delta={} ps_depth_delta={} source=real-vs does_not_prove=display_scanout\n",
            submit_name,
            accepted as u8,
            completed as u8,
            draw.rt_gpu_addr,
            draw.target_w,
            draw.target_h,
            draw.rt_pitch,
            scratch_before,
            scratch_after,
            center_before,
            center_after,
            post_before,
            post_after,
            rt_changed as u8,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
        );
    }
    completed
}

fn submit_result_store_probe(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let Ok(batch_tail_bytes) =
        encode_result_store_probe_batch(batch, GPU_VA_RESULT_BASE, RCS_EXEC_RESULT_MI_PROBE_DONE)
    else {
        crate::log!("intel/render: mi-store-probe batch build failed\n");
        return false;
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_MI_PROBE_DONE,
        RESULT_SLOT_PRE3D_DWORD,
        "mi-store-probe",
    )
}

fn submit_3d_no_draw_probe(dev: crate::intel::Dev, warm: RenderWarmState) -> bool {
    unsafe {
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::write_bytes(warm.result_virt, 0, warm.result_len);
        core::ptr::write_volatile(warm.result_virt as *mut u32, 0xC0DE_7700);
        core::ptr::write_volatile((warm.result_virt as *mut u32).add(1), 0xC0DE_7700);
        core::ptr::write_volatile((warm.result_virt as *mut u32).add(2), 0xC0DE_7700);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let total_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, total_dwords) };
    let Ok(batch_tail_bytes) = encode_3d_no_draw_probe_batch(
        batch,
        warm,
        GPU_VA_RESULT_BASE + (RESULT_SLOT_POST3D_DWORD as u64) * 4,
        RCS_EXEC_RESULT_3D_NO_DRAW_DONE,
        None,
    ) else {
        crate::log!("intel/render: 3d-no-draw-probe batch build failed\n");
        return false;
    };
    crate::intel::dma_flush(warm.batch_virt, batch_tail_bytes);
    submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_3D_NO_DRAW_DONE,
        RESULT_SLOT_POST3D_DWORD,
        "3d-no-draw",
    )
}
