//! Retained bootstrap breadcrumbs and read-only plane register evidence.
//! No GPU submission, register writes, retry loops or readiness changes.
use super::*;

#[derive(Clone, Copy)]
struct Evidence {
    stage: &'static str,
    failure: &'static str,
    plane_slot: usize,
    pipe_slot: usize,
    wanted: [u32; MAX_UNIVERSAL_PLANE_SLOTS],
    live_mask: u32,
    polls: usize,
}

static EVIDENCE: Mutex<Evidence> = Mutex::new(Evidence {
    stage: "not-attempted",
    failure: "none",
    plane_slot: usize::MAX,
    pipe_slot: usize::MAX,
    wanted: [0; MAX_UNIVERSAL_PLANE_SLOTS],
    live_mask: 0,
    polls: 0,
});
static BOOT_REGISTERS: spin::Once<String> = spin::Once::new();
static FIRMWARE_REGISTERS: spin::Once<String> = spin::Once::new();

// Linux i915 intel_display_device.c at 8934827db5403eae57d4537114a9ff88b0a8460f:
// Tiger Lake display IP 12 has six sprites plus primary (and a separate cursor).
// The five UI4 slots are a software contract, not this physical capability.
// Consult the raw PCI ID because the TGL experiment aliases it to ADL-S.
fn physical_plane_count(dev: crate::intel::Dev) -> usize {
    if crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 0) == 0x9A49_8086 {
        7
    } else {
        UNIVERSAL_PLANE_SLOTS
    }
}

pub(super) fn capture_firmware(dev: crate::intel::Dev) {
    FIRMWARE_REGISTERS.call_once(|| {
        let mut out = String::new();
        append(&mut out, dev);
        crate::log_important!(target: "gfx"; "intel/display-firmware-handoff-before-plane-writes:\n{}", out);
        out
    });
}

pub(super) fn select_pipe(pipe: PipeInfo) {
    EVIDENCE.lock().pipe_slot = pipe.slot;
}

pub(super) fn checkpoint(stage: &'static str, plane_slot: usize) {
    let mut evidence = EVIDENCE.lock();
    evidence.stage = stage;
    evidence.plane_slot = plane_slot;
    drop(evidence);
    crate::log_important!(target: "gfx";
        "intel/ui4-bootstrap: stage={} plane_slot={} stack_state={}\n",
        stage, plane_slot, UI4_RGBA8_PLANE_STACK_STATE.load(Ordering::Acquire));
}

pub(super) fn latch(wanted: [u32; MAX_UNIVERSAL_PLANE_SLOTS], live_mask: u32, polls: usize) {
    let mut evidence = EVIDENCE.lock();
    evidence.wanted = wanted;
    evidence.live_mask = live_mask;
    evidence.polls = polls;
}

pub(super) fn failed(reason: &'static str, dev: crate::intel::Dev) {
    EVIDENCE.lock().failure = reason;
    let mut out = String::new();
    append(&mut out, dev);
    BOOT_REGISTERS.call_once(|| out.clone());
    crate::log_error!(target: "gfx"; "intel/ui4-bootstrap-failure: reason={}\n{}", reason, out);
}

pub(super) fn log_ready(dev: crate::intel::Dev) {
    let mut out = String::new();
    append(&mut out, dev);
    BOOT_REGISTERS.call_once(|| out.clone());
    crate::log_important!(target: "gfx"; "intel/ui4-bootstrap-ready:\n{}", out);
}

pub(super) fn append_retained(out: &mut String) {
    let _ = writeln!(out, "retained_firmware_handoff_registers:");
    out.push_str(
        FIRMWARE_REGISTERS
            .get()
            .map(|text| text.as_str())
            .unwrap_or("firmware-handoff=not-captured\n"),
    );
    let _ = writeln!(out, "retained_ui4_bootstrap_registers:");
    out.push_str(
        BOOT_REGISTERS
            .get()
            .map(|text| text.as_str())
            .unwrap_or("bootstrap-result=not-reached\n"),
    );
}

pub(super) fn append(out: &mut String, dev: crate::intel::Dev) {
    // Release the breadcrumb lock before any MMIO or other subsystem access.
    let evidence = *EVIDENCE.lock();
    let _ = writeln!(
        out,
        "ui4_bootstrap state={} stage={} failure={} plane_slot={} pipe_slot={} primary_attempted={} native_handoff={} last_live_mask=0x{:X} last_polls={} wanted={:08X?}",
        UI4_RGBA8_PLANE_STACK_STATE.load(Ordering::Acquire),
        evidence.stage,
        evidence.failure,
        evidence.plane_slot,
        evidence.pipe_slot,
        PRIMARY_BOOT_SURFACE_INIT.load(Ordering::Acquire),
        crate::intel::tgl_native_panel::ui4_handoff_active(),
        evidence.live_mask,
        evidence.polls,
        evidence.wanted
    );
    // The seven-plane laptop layout needs both slices. Preserve request and
    // power-state bits independently: a plane/WM readback cannot prove that a
    // requested slice is powered. Addresses match Spirit's existing DBUF path.
    // Read each control once; this snapshot never requests power or polls it.
    const DBUF_CTL_S1: usize = 0x45008;
    for (slice, register) in [
        ("s1", DBUF_CTL_S1),
        ("s2", tgl_ui4_layout::DBUF_CTL_S2),
    ] {
        let raw = crate::intel::mmio_read(dev, register);
        let _ = writeln!(out,
            "display_dbuf slice={} register=0x{:X} raw=0x{:08X} power_request={} power_state={} capture=read-only-no-poll",
            slice, register, raw,
            raw & tgl_ui4_layout::DBUF_POWER_REQUEST != 0,
            raw & tgl_ui4_layout::DBUF_POWER_STATE != 0);
    }
    // A failed bootstrap may have disabled the very primary plane needed by
    // active_pipe(). Retain the attempted pipe so failures still expose it.
    let Some(pipe) = PIPES
        .get(evidence.pipe_slot)
        .copied()
        .or_else(|| active_pipe(dev))
    else {
        let _ = writeln!(out, "ui4_plane_registers active_pipe=none");
        return;
    };
    let physical_count = physical_plane_count(dev);
    let _ = writeln!(
        out,
        "display_plane_inventory attempted_pipe={} logical_ui4_slots={} physical_universal_planes_per_pipe={} separate_cursor=1 count_source=physical-platform-policy capture=read-only-no-commit",
        pipe.name,
        super::plane_slot_count(pipe),
        physical_count
    );
    if physical_count == 7 {
        for observed_pipe in PIPES {
            append_pipe(out, dev, observed_pipe, physical_count);
        }
    } else {
        append_pipe(out, dev, pipe, physical_count);
    }
}

fn append_pipe(out: &mut String, dev: crate::intel::Dev, pipe: PipeInfo, physical_count: usize) {
    let pipe_delta = pipe.slot * PIPE_MMIO_STRIDE;
    let _ = writeln!(
        out,
        "display_pipe pipe={} frame={} ui4_stack_ready={} pipeconf=0x{:08X} source=0x{:08X} transcoder=0x{:08X} htotal=0x{:08X} hsync=0x{:08X} vtotal=0x{:08X} vsync=0x{:08X} psr_ctl=0x{:08X} psr_status=0x{:08X} psr2_ctl=0x{:08X} psr2_status=0x{:08X}",
        pipe.name,
        crate::intel::mmio_read(dev, PIPE_FRMCOUNT_A + pipe_delta),
        ui4_rgba8_plane_stack_ready(pipe),
        crate::intel::mmio_read(dev, PIPECONF_A + pipe_delta),
        crate::intel::mmio_read(dev, pipe.pipe_src_off),
        crate::intel::mmio_read(dev, TRANS_DDI_FUNC_CTL_A + pipe_delta),
        crate::intel::mmio_read(dev, TRANS_HTOTAL_A + pipe_delta),
        crate::intel::mmio_read(dev, TRANS_HSYNC_A + pipe_delta),
        crate::intel::mmio_read(dev, TRANS_VTOTAL_A + pipe_delta),
        crate::intel::mmio_read(dev, TRANS_VSYNC_A + pipe_delta),
        crate::intel::mmio_read(dev, TRANS_PSR_CTL_A + pipe_delta),
        crate::intel::mmio_read(dev, TRANS_PSR_STATUS_A + pipe_delta),
        crate::intel::mmio_read(dev, TRANS_PSR2_CTL_A + pipe_delta),
        crate::intel::mmio_read(dev, TRANS_PSR2_STATUS_A + pipe_delta)
    );
    let mut controls = [0u32; 7];
    let mut dbufs = [0u32; 7];
    for slot in 0..physical_count {
        // This inventory is physical, including before native adoption. Do
        // not apply UI4's logical interaction/extra-application remapping.
        let base =
            UNI_PLANE_BASE + pipe.slot * UNI_PLANE_PIPE_STRIDE + slot * UNI_PLANE_SLOT_STRIDE;
        let ctl = crate::intel::mmio_read(dev, base + UNI_PLANE_CTL_OFF);
        let dbuf = crate::intel::mmio_read(dev, base + UNI_PLANE_BUF_CFG_OFF);
        controls[slot] = ctl;
        dbufs[slot] = dbuf;
        let _ = writeln!(
            out,
            "physical_plane pipe={} slot={} hardware_plane={} logical_ui4_slot={:?} enabled={} base=0x{:X} ctl=0x{:08X} surf=0x{:08X} live=0x{:08X} stride=0x{:08X} pos=0x{:08X} size=0x{:08X} offset=0x{:08X} color=0x{:08X} dbuf=0x{:08X} dbuf_start={} dbuf_end_inclusive={}",
            pipe.name,
            slot,
            slot + 1,
            if native_ui4_for_pipe(pipe) {
                tgl_ui4_layout::logical_slot(slot)
            } else {
                (slot < UNIVERSAL_PLANE_SLOTS).then_some(slot)
            },
            ctl & PLANE_CTL_ENABLE != 0,
            base,
            ctl,
            crate::intel::mmio_read(dev, base + UNI_PLANE_SURF_OFF),
            crate::intel::mmio_read(dev, base + UNI_PLANE_SURFLIVE_OFF),
            crate::intel::mmio_read(dev, base + UNI_PLANE_STRIDE_OFF),
            crate::intel::mmio_read(dev, base + UNI_PLANE_POS_OFF),
            crate::intel::mmio_read(dev, base + UNI_PLANE_SIZE_OFF),
            crate::intel::mmio_read(dev, base + UNI_PLANE_OFFSET_OFF),
            crate::intel::mmio_read(dev, base + UNI_PLANE_COLOR_CTL_OFF),
            dbuf,
            dbuf & 0x1FFF,
            (dbuf >> 16) & 0x1FFF
        );
        let wm = core::array::from_fn::<u32, 8, _>(|level| {
            crate::intel::mmio_read(dev, base + UNI_PLANE_WM_0_OFF + level * 4)
        });
        let _ = writeln!(
            out,
            "physical_plane_watermarks pipe={} slot={} raw_wm_bank_0_7={:08X?} trans=0x{:08X} sagv_alias_6=0x{:08X} sagv_trans_alias_7=0x{:08X}",
            pipe.name,
            slot,
            wm,
            crate::intel::mmio_read(dev, base + UNI_PLANE_WM_TRANS_OFF),
            crate::intel::mmio_read(dev, base + UNI_PLANE_WM_SAGV_OFF),
            crate::intel::mmio_read(dev, base + UNI_PLANE_WM_SAGV_TRANS_OFF)
        );
    }
    for extra in 1..physical_count {
        for owned in 0..extra {
            if controls[extra] & controls[owned] & PLANE_CTL_ENABLE != 0
                && raw_dbuf_ranges_overlap(dbufs[extra], dbufs[owned])
            {
                let _ = writeln!(
                    out,
                    "display_dbuf_observation pipe={} extra_slot={} ui4_slot={} both_enabled=1 raw_range_overlap=1 inference=allocation-conflict-candidate-no-repair",
                    pipe.name, extra, owned
                );
            }
        }
    }
    for index in 0..2 {
        let base = PIPE_SCALER_0_A_CTRL + pipe.slot * 0x800 + index * 0x100;
        let ctl = crate::intel::mmio_read(dev, base);
        let _ = writeln!(
            out,
            "display_scaler pipe={} index={} ctl=0x{:08X} enabled={} binding={} pos=0x{:08X} size=0x{:08X} vphase=0x{:08X} hphase=0x{:08X}",
            pipe.name,
            index,
            ctl,
            ctl & PIPE_SCALER_ENABLE != 0,
            (ctl & PIPE_SCALER_BINDING_MASK) >> 25,
            crate::intel::mmio_read(dev, base - PIPE_SCALER_WIN_POS_FROM_CTRL),
            crate::intel::mmio_read(dev, base - PIPE_SCALER_WIN_SIZE_FROM_CTRL),
            crate::intel::mmio_read(dev, base + PIPE_SCALER_VPHASE_FROM_CTRL),
            crate::intel::mmio_read(dev, base + PIPE_SCALER_HPHASE_FROM_CTRL)
        );
    }
    let cursor = 0x70080 + pipe_delta;
    let wm = core::array::from_fn::<u32, 8, _>(|level| {
        crate::intel::mmio_read(dev, 0x70140 + pipe_delta + level * 4)
    });
    let _ = writeln!(
        out,
        "display_cursor pipe={} ctl=0x{:08X} base=0x{:08X} live=0x{:08X} pos=0x{:08X} fbc=0x{:08X} dbuf=0x{:08X} raw_wm_bank_0_7={:08X?} wm_trans=0x{:08X}",
        pipe.name,
        crate::intel::mmio_read(dev, cursor),
        crate::intel::mmio_read(dev, cursor + 4),
        crate::intel::mmio_read(dev, cursor + 0x2C),
        crate::intel::mmio_read(dev, cursor + 8),
        crate::intel::mmio_read(dev, cursor + 0x20),
        crate::intel::mmio_read(dev, 0x7017C + pipe_delta),
        wm,
        crate::intel::mmio_read(dev, 0x70168 + pipe_delta)
    );
}

const fn raw_dbuf_ranges_overlap(left: u32, right: u32) -> bool {
    let ls = left & 0x1FFF;
    let le = (left >> 16) & 0x1FFF;
    let rs = right & 0x1FFF;
    let re = (right >> 16) & 0x1FFF;
    le >= ls && re >= rs && ls <= re && rs <= le
}
