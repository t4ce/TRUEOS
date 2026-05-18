use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use spin::Mutex;

const CT_DESC_BYTES: usize = 64;
const CT_DESC_DWORDS: usize = CT_DESC_BYTES / 4;
const CT_H2G_OFFSET: usize = 4096;
const CT_G2H_OFFSET: usize = 8192;
const CT_RING_BYTES: usize = 4096;
const CT_RING_DWORDS: usize = CT_RING_BYTES / 4;
const CT_BLOB_BYTES: usize = CT_G2H_OFFSET + CT_RING_BYTES;
const CT_DESC_HEAD: usize = 0;
const CT_DESC_TAIL: usize = 4;
const CT_DESC_STATUS: usize = 8;
const GUC_ACTION_HOST2GUC_SELF_CFG: u32 = 0x0508;
const GUC_ACTION_HOST2GUC_CONTROL_CTB: u32 = 0x4509;
const GUC_CTB_CONTROL_ENABLE: u32 = 1;
const GUC_KLV_SELF_CFG_H2G_CTB_ADDR_KEY: u32 = 0x0902;
const GUC_KLV_SELF_CFG_H2G_CTB_DESCRIPTOR_ADDR_KEY: u32 = 0x0903;
const GUC_KLV_SELF_CFG_H2G_CTB_SIZE_KEY: u32 = 0x0904;
const GUC_KLV_SELF_CFG_G2H_CTB_ADDR_KEY: u32 = 0x0905;
const GUC_KLV_SELF_CFG_G2H_CTB_DESCRIPTOR_ADDR_KEY: u32 = 0x0906;
const GUC_KLV_SELF_CFG_G2H_CTB_SIZE_KEY: u32 = 0x0907;
const GUC_HXG_ORIGIN_GUC: u32 = 1;
const GUC_HXG_TYPE_REQUEST: u32 = 0;
const GUC_HXG_TYPE_RESPONSE_FAILURE: u32 = 6;
const GUC_HXG_TYPE_RESPONSE_SUCCESS: u32 = 7;
const GEN11_GUC_HOST_INTERRUPT: usize = 0x0019_01F0;
const GUC_SEND_TRIGGER: u32 = 1 << 0;
const CT_RESPONSE_POLL_ITERS: usize = 100_000;

static CTB_ENABLED: AtomicBool = AtomicBool::new(false);
static NEXT_FENCE: AtomicU16 = AtomicU16::new(1);
static STATE: Mutex<Option<CtbState>> = Mutex::new(None);

#[derive(Copy, Clone)]
struct CtbState {
    phys: u64,
    virt: *mut u8,
    len: usize,
    gpu: u64,
    h2g_tail: u32,
    g2h_head: u32,
}

unsafe impl Send for CtbState {}

pub(crate) struct CtbSendResult {
    pub(crate) accepted: bool,
    pub(crate) response: u32,
    pub(crate) response_type: u32,
    pub(crate) error: u32,
    pub(crate) h2g_poll_iters: usize,
    pub(crate) g2h_poll_iters: usize,
}

pub(crate) fn enabled() -> bool {
    CTB_ENABLED.load(Ordering::Acquire)
}

pub(crate) fn init_and_enable(dev: crate::intel::Dev) -> bool {
    if enabled() {
        return true;
    }
    if !crate::intel::guc_ready() {
        crate::log!("intel/guc-ctb: setup skipped reason=guc-not-ready\n");
        return false;
    }

    let Some((phys, virt)) = crate::dma::alloc(CT_BLOB_BYTES, crate::intel::WARM_ALIGN) else {
        crate::log!("intel/guc-ctb: setup failed reason=alloc bytes=0x{:X}\n", CT_BLOB_BYTES);
        return false;
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, CT_BLOB_BYTES);
    }
    let state = CtbState {
        phys,
        virt,
        len: CT_BLOB_BYTES,
        gpu: crate::intel::GPU_VA_GUC_CTB_BASE,
        h2g_tail: 0,
        g2h_head: 0,
    };
    write_desc(state, 0, 0, 0, 0);
    write_desc(state, CT_DESC_BYTES, 0, 0, 0);
    crate::intel::dma_flush(virt, CT_BLOB_BYTES);

    if !crate::intel::map_ggtt(dev, phys, CT_BLOB_BYTES, state.gpu) {
        crate::log!(
            "intel/guc-ctb: setup failed reason=ggtt-map phys=0x{:X} gpu=0x{:X} bytes=0x{:X}\n",
            phys,
            state.gpu,
            CT_BLOB_BYTES
        );
        return false;
    }
    super::ggtt_invalidate(dev);

    let h2g_desc = state.gpu as u32;
    let g2h_desc = (state.gpu + CT_DESC_BYTES as u64) as u32;
    let h2g_buf = (state.gpu + CT_H2G_OFFSET as u64) as u32;
    let g2h_buf = (state.gpu + CT_G2H_OFFSET as u64) as u32;
    let regs = [
        self_cfg64(dev, GUC_KLV_SELF_CFG_G2H_CTB_DESCRIPTOR_ADDR_KEY, g2h_desc as u64),
        self_cfg64(dev, GUC_KLV_SELF_CFG_G2H_CTB_ADDR_KEY, g2h_buf as u64),
        self_cfg32(dev, GUC_KLV_SELF_CFG_G2H_CTB_SIZE_KEY, CT_RING_BYTES as u32),
        self_cfg64(dev, GUC_KLV_SELF_CFG_H2G_CTB_DESCRIPTOR_ADDR_KEY, h2g_desc as u64),
        self_cfg64(dev, GUC_KLV_SELF_CFG_H2G_CTB_ADDR_KEY, h2g_buf as u64),
        self_cfg32(dev, GUC_KLV_SELF_CFG_H2G_CTB_SIZE_KEY, CT_RING_BYTES as u32),
    ];
    let regs_ok = regs.iter().all(|r| r.accepted);
    if !regs_ok {
        crate::log!(
            "intel/guc-ctb: setup accepted=0 stage=self-cfg g2h_desc=0x{:X} g2h=0x{:X} h2g_desc=0x{:X} h2g=0x{:X} responses=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] next=mmio-fallback\n",
            g2h_desc,
            g2h_buf,
            h2g_desc,
            h2g_buf,
            regs[0].response,
            regs[1].response,
            regs[2].response,
            regs[3].response,
            regs[4].response,
            regs[5].response
        );
        return false;
    }

    let enable = crate::intel::guc::send_h2g_mmio_action(
        dev,
        GUC_ACTION_HOST2GUC_CONTROL_CTB,
        &[GUC_CTB_CONTROL_ENABLE],
    );
    let ok = enable.accepted;
    CTB_ENABLED.store(ok, Ordering::Release);
    if ok {
        *STATE.lock() = Some(state);
    }
    crate::log!(
        "intel/guc-ctb: setup accepted={} gpu=0x{:X} phys=0x{:X} bytes=0x{:X} h2g_desc=0x{:X} h2g=0x{:X} g2h_desc=0x{:X} g2h=0x{:X} ring_bytes=0x{:X} control_response=0x{:08X} response_type={} error={} poll_iters={} next=huc-auth-ctb does_not_prove=guc_owned_render_submission\n",
        ok as u8,
        state.gpu,
        state.phys,
        state.len,
        h2g_desc,
        h2g_buf,
        g2h_desc,
        g2h_buf,
        CT_RING_BYTES,
        enable.response,
        enable.response_type,
        enable.error,
        enable.poll_iters
    );
    ok
}

pub(crate) fn send_hxg_action(dev: crate::intel::Dev, action: u32, args: &[u32]) -> CtbSendResult {
    if !enabled() {
        return CtbSendResult {
            accepted: false,
            response: 0,
            response_type: 0,
            error: 1,
            h2g_poll_iters: 0,
            g2h_poll_iters: 0,
        };
    }
    let mut guard = STATE.lock();
    let Some(mut state) = *guard else {
        return CtbSendResult {
            accepted: false,
            response: 0,
            response_type: 0,
            error: 2,
            h2g_poll_iters: 0,
            g2h_poll_iters: 0,
        };
    };

    let fence = NEXT_FENCE.fetch_add(1, Ordering::AcqRel).max(1);
    let payload_len = 1usize.saturating_add(args.len().min(14));
    let total_len = 1usize.saturating_add(payload_len);
    let mut tail = state.h2g_tail as usize;
    write_ct_dw(state, CT_H2G_OFFSET, tail, ((fence as u32) << 16) | payload_len as u32);
    tail = (tail + 1) % CT_RING_DWORDS;
    write_ct_dw(state, CT_H2G_OFFSET, tail, hxg_request_header(action));
    tail = (tail + 1) % CT_RING_DWORDS;
    for value in args.iter().copied().take(payload_len.saturating_sub(1)) {
        write_ct_dw(state, CT_H2G_OFFSET, tail, value);
        tail = (tail + 1) % CT_RING_DWORDS;
    }
    state.h2g_tail = tail as u32;
    write_desc_tail(state, 0, state.h2g_tail);
    crate::intel::dma_flush(state.virt, CT_BLOB_BYTES);
    crate::intel::mmio_write(dev, GEN11_GUC_HOST_INTERRUPT, GUC_SEND_TRIGGER);

    let mut response = 0u32;
    let mut response_type = 0u32;
    let mut error = 4u32;
    let mut g2h_poll_iters = 0usize;
    while g2h_poll_iters < CT_RESPONSE_POLL_ITERS {
        crate::intel::dma_flush(state.virt, CT_BLOB_BYTES);
        let tail_now = read_desc_tail(state, CT_DESC_BYTES);
        while state.g2h_head != tail_now {
            let msg_head = state.g2h_head as usize;
            let hdr = read_ct_dw(state, CT_G2H_OFFSET, msg_head);
            let msg_fence = (hdr >> 16) as u16;
            let msg_len = (hdr & 0xFF) as usize;
            let hxg = read_ct_dw(state, CT_G2H_OFFSET, (msg_head + 1) % CT_RING_DWORDS);
            state.g2h_head = ((msg_head + 1 + msg_len) % CT_RING_DWORDS) as u32;
            write_desc_head(state, CT_DESC_BYTES, state.g2h_head);
            if msg_fence == fence {
                response = hxg;
                response_type = hxg_type(hxg);
                error = match response_type {
                    GUC_HXG_TYPE_RESPONSE_SUCCESS => 0,
                    GUC_HXG_TYPE_RESPONSE_FAILURE => hxg & 0xFFFF,
                    _ => 5,
                };
                let accepted = hxg_origin(hxg) == GUC_HXG_ORIGIN_GUC
                    && response_type == GUC_HXG_TYPE_RESPONSE_SUCCESS;
                *guard = Some(state);
                return CtbSendResult {
                    accepted,
                    response,
                    response_type,
                    error,
                    h2g_poll_iters: total_len,
                    g2h_poll_iters,
                };
            }
        }
        g2h_poll_iters += 1;
        core::hint::spin_loop();
    }

    *guard = Some(state);
    CtbSendResult {
        accepted: false,
        response,
        response_type,
        error,
        h2g_poll_iters: total_len,
        g2h_poll_iters,
    }
}

fn self_cfg32(dev: crate::intel::Dev, key: u32, value: u32) -> crate::intel::guc::H2gMmioResult {
    crate::intel::guc::send_h2g_mmio_action(
        dev,
        GUC_ACTION_HOST2GUC_SELF_CFG,
        &[(key << 16) | 1, value],
    )
}

fn self_cfg64(dev: crate::intel::Dev, key: u32, value: u64) -> crate::intel::guc::H2gMmioResult {
    crate::intel::guc::send_h2g_mmio_action(
        dev,
        GUC_ACTION_HOST2GUC_SELF_CFG,
        &[(key << 16) | 2, value as u32, (value >> 32) as u32],
    )
}

fn write_desc(state: CtbState, desc_off: usize, head: u32, tail: u32, status: u32) {
    write_blob_u32(state, desc_off + CT_DESC_HEAD, head);
    write_blob_u32(state, desc_off + CT_DESC_TAIL, tail);
    write_blob_u32(state, desc_off + CT_DESC_STATUS, status);
    for i in 3..CT_DESC_DWORDS {
        write_blob_u32(state, desc_off + i * 4, 0);
    }
}

fn write_desc_head(state: CtbState, desc_off: usize, head: u32) {
    write_blob_u32(state, desc_off + CT_DESC_HEAD, head);
}

fn write_desc_tail(state: CtbState, desc_off: usize, tail: u32) {
    write_blob_u32(state, desc_off + CT_DESC_TAIL, tail);
}

fn read_desc_tail(state: CtbState, desc_off: usize) -> u32 {
    read_blob_u32(state, desc_off + CT_DESC_TAIL)
}

fn write_ct_dw(state: CtbState, base: usize, idx: usize, value: u32) {
    write_blob_u32(state, base + (idx % CT_RING_DWORDS) * 4, value);
}

fn read_ct_dw(state: CtbState, base: usize, idx: usize) -> u32 {
    read_blob_u32(state, base + (idx % CT_RING_DWORDS) * 4)
}

fn write_blob_u32(state: CtbState, off: usize, value: u32) {
    if off + 4 <= state.len {
        unsafe {
            core::ptr::write_volatile(state.virt.add(off) as *mut u32, value);
        }
    }
}

fn read_blob_u32(state: CtbState, off: usize) -> u32 {
    if off + 4 <= state.len {
        unsafe { core::ptr::read_volatile(state.virt.add(off) as *const u32) }
    } else {
        0
    }
}

fn hxg_request_header(action: u32) -> u32 {
    (GUC_HXG_TYPE_REQUEST << 28) | (action & 0xFFFF)
}

fn hxg_origin(value: u32) -> u32 {
    (value >> 31) & 0x1
}

fn hxg_type(value: u32) -> u32 {
    (value >> 28) & 0x7
}
