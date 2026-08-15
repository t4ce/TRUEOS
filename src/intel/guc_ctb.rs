use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use spin::Mutex;

const CT_DESC_BYTES: usize = 64;
const CT_DESC_DWORDS: usize = CT_DESC_BYTES / 4;
const CT_DESC_ALIGN_BYTES: usize = 2048;
const CT_H2G_DESC_OFFSET: usize = 0;
const CT_G2H_DESC_OFFSET: usize = CT_DESC_ALIGN_BYTES;
const CT_H2G_OFFSET: usize = 4096;
const CT_G2H_OFFSET: usize = 8192;
const CT_H2G_RING_BYTES: usize = 4096;
const CT_G2H_RING_BYTES: usize = 4 * CT_H2G_RING_BYTES;
const CT_H2G_RING_DWORDS: usize = CT_H2G_RING_BYTES / 4;
const CT_G2H_RING_DWORDS: usize = CT_G2H_RING_BYTES / 4;
const CT_BLOB_BYTES: usize = CT_G2H_OFFSET + CT_G2H_RING_BYTES;
const _: () = assert!(
    crate::intel::GPU_VA_GUC_CTB_BASE + CT_BLOB_BYTES as u64
        <= crate::intel::GPU_VA_GUC_RUNTIME_LIMIT
);
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
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const GUC_HXG_TYPE_REQUEST: u32 = 0;
const GUC_HXG_TYPE_EVENT: u32 = 1;
const GUC_HXG_TYPE_FAST_REQUEST: u32 = 2;
const GUC_HXG_TYPE_RESPONSE_FAILURE: u32 = 6;
const GUC_HXG_TYPE_RESPONSE_SUCCESS: u32 = 7;
const GEN11_GUC_HOST_INTERRUPT: usize = 0x0019_01F0;
const GUC_SEND_TRIGGER: u32 = 1 << 0;
const CT_H2G_ROOM_POLL_ITERS: usize = 8_192;
const CT_RESPONSE_POLL_ITERS: usize = 8_192;
const CT_G2H_EVENT_QUEUE_CAPACITY: usize = 64;
const CT_G2H_EVENT_PAYLOAD_DWORDS: usize = 4;

static CTB_ENABLED: AtomicBool = AtomicBool::new(false);
static NEXT_FENCE: AtomicU16 = AtomicU16::new(1);
static STATE: Mutex<Option<CtbState>> = Mutex::new(None);
static G2H_EVENTS: Mutex<CtbG2hEventQueue> = Mutex::new(CtbG2hEventQueue::EMPTY);

#[derive(Copy, Clone)]
struct CtbState {
    phys: u64,
    virt: *mut u8,
    len: usize,
    gpu: u64,
    h2g_tail: u32,
    h2g_observed_head: u32,
    h2g_published_dwords: u64,
    h2g_consumed_dwords: u64,
    g2h_head: u32,
}

unsafe impl Send for CtbState {}

/// One asynchronous GuC-to-host HXG event retained outside the CTB ring.
///
/// GuC submission lifecycle events use at most two payload dwords. Four are
/// retained so context-reset and engine-failure notifications can also be
/// diagnosed without making the transport depend on the submission module.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct CtbG2hEvent {
    pub(crate) action: u32,
    pub(crate) payload_len: usize,
    payload: [u32; CT_G2H_EVENT_PAYLOAD_DWORDS],
}

impl CtbG2hEvent {
    const EMPTY: Self = Self {
        action: 0,
        payload_len: 0,
        payload: [0; CT_G2H_EVENT_PAYLOAD_DWORDS],
    };

    pub(crate) const fn payload(self, index: usize) -> Option<u32> {
        if index < self.payload_len && index < CT_G2H_EVENT_PAYLOAD_DWORDS {
            Some(self.payload[index])
        } else {
            None
        }
    }

    pub(crate) const fn truncated(self) -> bool {
        self.payload_len > CT_G2H_EVENT_PAYLOAD_DWORDS
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct CtbG2hPollResult {
    pub(crate) events: usize,
    pub(crate) coalesced_events: u64,
    pub(crate) malformed_messages: u64,
    pub(crate) dropped_events: u64,
    pub(crate) unsolicited_responses: u64,
}

struct CtbG2hEventQueue {
    entries: [CtbG2hEvent; CT_G2H_EVENT_QUEUE_CAPACITY],
    head: usize,
    len: usize,
    coalesced_events: u64,
    malformed_messages: u64,
    dropped_events: u64,
    unsolicited_responses: u64,
}

impl CtbG2hEventQueue {
    const EMPTY: Self = Self {
        entries: [CtbG2hEvent::EMPTY; CT_G2H_EVENT_QUEUE_CAPACITY],
        head: 0,
        len: 0,
        coalesced_events: 0,
        malformed_messages: 0,
        dropped_events: 0,
        unsolicited_responses: 0,
    };

    fn push(&mut self, event: CtbG2hEvent) {
        // GuC may publish the same sticky CAT/reset notification repeatedly
        // until the exact context DISABLE reaches firmware. Retain one copy:
        // duplicate notifications carry no additional ownership evidence and
        // must not crowd an unrelated engine's event out of this queue.
        if (0..self.len).any(|offset| {
            let index = (self.head + offset) % CT_G2H_EVENT_QUEUE_CAPACITY;
            self.entries[index] == event
        }) {
            self.coalesced_events = self.coalesced_events.saturating_add(1);
            return;
        }
        if self.len == CT_G2H_EVENT_QUEUE_CAPACITY {
            self.dropped_events = self.dropped_events.saturating_add(1);
            return;
        }
        let tail = (self.head + self.len) % CT_G2H_EVENT_QUEUE_CAPACITY;
        self.entries[tail] = event;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<CtbG2hEvent> {
        if self.len == 0 {
            return None;
        }
        let event = self.entries[self.head];
        self.entries[self.head] = CtbG2hEvent::EMPTY;
        self.head = (self.head + 1) % CT_G2H_EVENT_QUEUE_CAPACITY;
        self.len -= 1;
        Some(event)
    }
}

pub(crate) struct CtbSendResult {
    pub(crate) accepted: bool,
    pub(crate) response: u32,
    pub(crate) response_type: u32,
    pub(crate) error: u32,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) h2g_poll_iters: usize,
    pub(crate) g2h_poll_iters: usize,
    /// Monotonic H2G stream position immediately after this message. A
    /// consumer can compare it with `h2g_sequence_consumed` without relying
    /// on a wrapping descriptor index or synchronously waiting for GuC.
    pub(crate) h2g_publish_sequence: u64,
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
        h2g_observed_head: 0,
        h2g_published_dwords: 0,
        h2g_consumed_dwords: 0,
        g2h_head: 0,
    };
    write_desc(state, CT_H2G_DESC_OFFSET, 0, 0, 0);
    write_desc(state, CT_G2H_DESC_OFFSET, 0, 0, 0);
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

    let h2g_desc = (state.gpu + CT_H2G_DESC_OFFSET as u64) as u32;
    let g2h_desc = (state.gpu + CT_G2H_DESC_OFFSET as u64) as u32;
    let h2g_buf = (state.gpu + CT_H2G_OFFSET as u64) as u32;
    let g2h_buf = (state.gpu + CT_G2H_OFFSET as u64) as u32;
    let regs = [
        self_cfg64(dev, GUC_KLV_SELF_CFG_G2H_CTB_DESCRIPTOR_ADDR_KEY, g2h_desc as u64),
        self_cfg64(dev, GUC_KLV_SELF_CFG_G2H_CTB_ADDR_KEY, g2h_buf as u64),
        self_cfg32(dev, GUC_KLV_SELF_CFG_G2H_CTB_SIZE_KEY, CT_G2H_RING_BYTES as u32),
        self_cfg64(dev, GUC_KLV_SELF_CFG_H2G_CTB_DESCRIPTOR_ADDR_KEY, h2g_desc as u64),
        self_cfg64(dev, GUC_KLV_SELF_CFG_H2G_CTB_ADDR_KEY, h2g_buf as u64),
        self_cfg32(dev, GUC_KLV_SELF_CFG_H2G_CTB_SIZE_KEY, CT_H2G_RING_BYTES as u32),
    ];
    let regs_ok = regs.iter().all(|r| r.accepted);
    if !regs_ok {
        crate::log!(
            "intel/guc-ctb: setup accepted=0 stage=self-cfg g2h_desc=0x{:X} g2h=0x{:X} h2g_desc=0x{:X} h2g=0x{:X} responses=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] next=submission-blocked\n",
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
        "intel/guc-ctb: setup accepted={} gpu=0x{:X} phys=0x{:X} bytes=0x{:X} h2g_desc=0x{:X} h2g=0x{:X} h2g_ring_bytes=0x{:X} g2h_desc=0x{:X} g2h=0x{:X} g2h_ring_bytes=0x{:X} control_response=0x{:08X} response_type={} error={} poll_iters={} next=guc-context-register\n",
        ok as u8,
        state.gpu,
        state.phys,
        state.len,
        h2g_desc,
        h2g_buf,
        CT_H2G_RING_BYTES,
        g2h_desc,
        g2h_buf,
        CT_G2H_RING_BYTES,
        enable.response,
        enable.response_type,
        enable.error,
        enable.poll_iters
    );
    ok
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn send_hxg_action(dev: crate::intel::Dev, action: u32, args: &[u32]) -> CtbSendResult {
    send_hxg(dev, action, args, GUC_HXG_TYPE_REQUEST, true)
}

/// Enqueue an asynchronous GuC action without waiting for a fenced response.
///
/// GuC submission actions such as SCHED_CONTEXT and
/// SCHED_CONTEXT_MODE_SET are FAST_REQUEST messages.  The latter reports its
/// state transition through a separate SCHED_CONTEXT_MODE_DONE G2H event; it
/// does not produce the synchronous response consumed by `send_hxg_action`.
pub(crate) fn send_hxg_fast_action(
    dev: crate::intel::Dev,
    action: u32,
    args: &[u32],
) -> CtbSendResult {
    send_hxg(dev, action, args, GUC_HXG_TYPE_FAST_REQUEST, false)
}

/// Non-blockingly observe whether GuC has advanced the H2G descriptor head
/// through one previously published message.
///
/// This is an observation boundary, not a wait. The monotonic sequence keeps
/// the result unambiguous across CTB ring wrap and across unrelated producers.
pub(crate) fn h2g_sequence_consumed(target_sequence: u64) -> bool {
    if target_sequence == 0 {
        return false;
    }
    let mut guard = STATE.lock();
    let Some(mut state) = *guard else {
        return false;
    };
    flush_blob_range(state, CT_H2G_DESC_OFFSET, CT_DESC_BYTES);
    let head = read_desc_head(state, CT_H2G_DESC_OFFSET) as usize;
    if head >= CT_H2G_RING_DWORDS {
        return false;
    }
    observe_h2g_head(&mut state, head);
    let consumed = state.h2g_consumed_dwords >= target_sequence;
    *guard = Some(state);
    consumed
}

/// Drain asynchronous G2H events without waiting for a new message.
///
/// The callback runs without either CTB mutex held. This lets the submission
/// layer update its context registry while keeping transport lock ordering
/// one-way. Events consumed while a synchronous response was being awaited are
/// delivered through the same queue, so they are never mistaken for fenced
/// responses or silently discarded.
pub(crate) fn poll_g2h_events(mut visit: impl FnMut(CtbG2hEvent)) -> CtbG2hPollResult {
    {
        let mut guard = STATE.lock();
        if let Some(mut state) = *guard {
            drain_available_g2h(&mut state);
            *guard = Some(state);
        }
    }

    let mut result = CtbG2hPollResult::default();
    loop {
        let event = G2H_EVENTS.lock().pop();
        let Some(event) = event else {
            break;
        };
        result.events = result.events.saturating_add(1);
        visit(event);
    }

    let mut queue = G2H_EVENTS.lock();
    result.coalesced_events = queue.coalesced_events;
    result.malformed_messages = queue.malformed_messages;
    result.dropped_events = queue.dropped_events;
    result.unsolicited_responses = queue.unsolicited_responses;
    queue.coalesced_events = 0;
    queue.malformed_messages = 0;
    queue.dropped_events = 0;
    queue.unsolicited_responses = 0;
    result
}

fn send_hxg(
    dev: crate::intel::Dev,
    action: u32,
    args: &[u32],
    request_type: u32,
    wait_for_response: bool,
) -> CtbSendResult {
    if !enabled() {
        return CtbSendResult {
            accepted: false,
            response: 0,
            response_type: 0,
            error: 1,
            h2g_poll_iters: 0,
            g2h_poll_iters: 0,
            h2g_publish_sequence: 0,
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
            h2g_publish_sequence: 0,
        };
    };

    let fence = next_fence(!wait_for_response);
    let payload_len = 1usize.saturating_add(args.len().min(14));
    let total_len = 1usize.saturating_add(payload_len);
    let mut h2g_poll_iters = 0usize;
    let required = loop {
        flush_blob_range(state, CT_H2G_DESC_OFFSET, CT_DESC_BYTES);
        let h2g_head = read_desc_head(state, CT_H2G_DESC_OFFSET) as usize;
        if h2g_head >= CT_H2G_RING_DWORDS {
            *guard = Some(state);
            return CtbSendResult {
                accepted: false,
                response: 0,
                response_type: 0,
                error: 6,
                h2g_poll_iters,
                g2h_poll_iters: 0,
                h2g_publish_sequence: 0,
            };
        }
        observe_h2g_head(&mut state, h2g_head);
        let tail = state.h2g_tail as usize;
        let required = if tail.saturating_add(total_len) > CT_H2G_RING_DWORDS {
            CT_H2G_RING_DWORDS
                .saturating_sub(tail)
                .saturating_add(total_len)
        } else {
            total_len
        };
        if ct_ring_space(tail, h2g_head, CT_H2G_RING_DWORDS) >= required {
            break required;
        }
        h2g_poll_iters = h2g_poll_iters.saturating_add(1);
        if h2g_poll_iters >= CT_H2G_ROOM_POLL_ITERS {
            *guard = Some(state);
            return CtbSendResult {
                accepted: false,
                response: 0,
                response_type: 0,
                error: 3,
                h2g_poll_iters,
                g2h_poll_iters: 0,
                h2g_publish_sequence: 0,
            };
        }
        core::hint::spin_loop();
    };

    let mut tail = state.h2g_tail as usize;
    if tail.saturating_add(total_len) > CT_H2G_RING_DWORDS {
        while tail < CT_H2G_RING_DWORDS {
            write_ct_dw(state, CT_H2G_OFFSET, tail, 0);
            tail += 1;
        }
        tail = 0;
    }
    write_ct_dw(state, CT_H2G_OFFSET, tail, ((fence as u32) << 16) | payload_len as u32);
    tail = (tail + 1) % CT_H2G_RING_DWORDS;
    write_ct_dw(state, CT_H2G_OFFSET, tail, hxg_action_header(request_type, action));
    tail = (tail + 1) % CT_H2G_RING_DWORDS;
    for value in args.iter().copied().take(payload_len.saturating_sub(1)) {
        write_ct_dw(state, CT_H2G_OFFSET, tail, value);
        tail = (tail + 1) % CT_H2G_RING_DWORDS;
    }
    state.h2g_tail = tail as u32;
    state.h2g_published_dwords = state.h2g_published_dwords.saturating_add(required as u64);
    let h2g_publish_sequence = state.h2g_published_dwords;
    write_desc_tail(state, CT_H2G_DESC_OFFSET, state.h2g_tail);
    flush_blob_range(state, CT_H2G_OFFSET, CT_H2G_RING_BYTES);
    flush_blob_range(state, CT_H2G_DESC_OFFSET, CT_DESC_BYTES);
    crate::intel::mmio_write(dev, GEN11_GUC_HOST_INTERRUPT, GUC_SEND_TRIGGER);

    if !wait_for_response {
        *guard = Some(state);
        return CtbSendResult {
            accepted: true,
            response: 0,
            response_type: request_type,
            error: 0,
            h2g_poll_iters: h2g_poll_iters.saturating_add(required),
            g2h_poll_iters: 0,
            h2g_publish_sequence,
        };
    }

    let mut response = 0u32;
    let mut response_type = 0u32;
    let mut error = 4u32;
    let mut g2h_poll_iters = 0usize;
    while g2h_poll_iters < CT_RESPONSE_POLL_ITERS {
        flush_blob_range(state, CT_G2H_DESC_OFFSET, CT_DESC_BYTES);
        let tail_now = read_desc_tail(state, CT_G2H_DESC_OFFSET) as usize;
        if tail_now >= CT_G2H_RING_DWORDS {
            error = 6;
            break;
        }
        if state.g2h_head as usize != tail_now {
            flush_blob_range(state, CT_G2H_OFFSET, CT_G2H_RING_BYTES);
        }
        let mut messages = 0usize;
        while state.g2h_head as usize != tail_now && messages < CT_G2H_RING_DWORDS {
            let msg_head = state.g2h_head as usize;
            let available = ct_ring_distance(msg_head, tail_now, CT_G2H_RING_DWORDS);
            let hdr = read_ct_dw(state, CT_G2H_OFFSET, msg_head);
            let msg_fence = (hdr >> 16) as u16;
            let msg_len = (hdr & 0xFF) as usize;
            let msg_total = 1usize.saturating_add(msg_len);
            if msg_len == 0 || msg_total > available {
                error = 7;
                break;
            }
            let hxg = read_ct_dw(state, CT_G2H_OFFSET, (msg_head + 1) % CT_G2H_RING_DWORDS);
            let origin = hxg_origin(hxg);
            let message_type = hxg_type(hxg);
            if message_type == GUC_HXG_TYPE_EVENT {
                if origin == GUC_HXG_ORIGIN_GUC {
                    queue_g2h_event(state, msg_head, msg_len, hxg);
                } else {
                    let mut queue = G2H_EVENTS.lock();
                    queue.malformed_messages = queue.malformed_messages.saturating_add(1);
                }
            }
            state.g2h_head = ((msg_head + msg_total) % CT_G2H_RING_DWORDS) as u32;
            write_desc_head(state, CT_G2H_DESC_OFFSET, state.g2h_head);
            flush_blob_range(state, CT_G2H_DESC_OFFSET, CT_DESC_BYTES);
            messages = messages.saturating_add(1);
            if message_type == GUC_HXG_TYPE_EVENT {
                continue;
            }
            if msg_fence == fence {
                response = hxg;
                response_type = message_type;
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
                    h2g_poll_iters: h2g_poll_iters.saturating_add(required),
                    g2h_poll_iters,
                    h2g_publish_sequence,
                };
            }
            let mut queue = G2H_EVENTS.lock();
            queue.unsolicited_responses = queue.unsolicited_responses.saturating_add(1);
        }
        if error == 6 || error == 7 || messages >= CT_G2H_RING_DWORDS {
            break;
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
        h2g_poll_iters: h2g_poll_iters.saturating_add(required),
        g2h_poll_iters,
        h2g_publish_sequence,
    }
}

fn drain_available_g2h(state: &mut CtbState) {
    flush_blob_range(*state, CT_G2H_DESC_OFFSET, CT_DESC_BYTES);
    let tail = read_desc_tail(*state, CT_G2H_DESC_OFFSET) as usize;
    if tail >= CT_G2H_RING_DWORDS {
        let mut queue = G2H_EVENTS.lock();
        queue.malformed_messages = queue.malformed_messages.saturating_add(1);
        return;
    }
    if state.g2h_head as usize != tail {
        flush_blob_range(*state, CT_G2H_OFFSET, CT_G2H_RING_BYTES);
    }

    let mut messages = 0usize;
    while state.g2h_head as usize != tail && messages < CT_G2H_RING_DWORDS {
        let msg_head = state.g2h_head as usize;
        let available = ct_ring_distance(msg_head, tail, CT_G2H_RING_DWORDS);
        let header = read_ct_dw(*state, CT_G2H_OFFSET, msg_head);
        let msg_len = (header & 0xFF) as usize;
        let msg_total = 1usize.saturating_add(msg_len);
        if msg_len == 0 || msg_total > available {
            let mut queue = G2H_EVENTS.lock();
            queue.malformed_messages = queue.malformed_messages.saturating_add(1);
            break;
        }

        let hxg = read_ct_dw(*state, CT_G2H_OFFSET, (msg_head + 1) % CT_G2H_RING_DWORDS);
        let origin = hxg_origin(hxg);
        let message_type = hxg_type(hxg);
        if origin == GUC_HXG_ORIGIN_GUC && message_type == GUC_HXG_TYPE_EVENT {
            queue_g2h_event(*state, msg_head, msg_len, hxg);
        } else if origin == GUC_HXG_ORIGIN_GUC
            && matches!(message_type, GUC_HXG_TYPE_RESPONSE_FAILURE | GUC_HXG_TYPE_RESPONSE_SUCCESS)
        {
            let mut queue = G2H_EVENTS.lock();
            queue.unsolicited_responses = queue.unsolicited_responses.saturating_add(1);
        } else {
            let mut queue = G2H_EVENTS.lock();
            queue.malformed_messages = queue.malformed_messages.saturating_add(1);
        }

        state.g2h_head = ((msg_head + msg_total) % CT_G2H_RING_DWORDS) as u32;
        write_desc_head(*state, CT_G2H_DESC_OFFSET, state.g2h_head);
        flush_blob_range(*state, CT_G2H_DESC_OFFSET, CT_DESC_BYTES);
        messages = messages.saturating_add(1);
    }
    if messages >= CT_G2H_RING_DWORDS {
        let mut queue = G2H_EVENTS.lock();
        queue.malformed_messages = queue.malformed_messages.saturating_add(1);
    }
}

fn queue_g2h_event(state: CtbState, msg_head: usize, msg_len: usize, hxg: u32) {
    let payload_len = msg_len.saturating_sub(1);
    let mut payload = [0u32; CT_G2H_EVENT_PAYLOAD_DWORDS];
    for (index, value) in payload.iter_mut().enumerate().take(payload_len) {
        *value = read_ct_dw(state, CT_G2H_OFFSET, (msg_head + 2 + index) % CT_G2H_RING_DWORDS);
    }
    G2H_EVENTS.lock().push(CtbG2hEvent {
        action: hxg & 0xFFFF,
        payload_len,
        payload,
    });
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

fn read_desc_head(state: CtbState, desc_off: usize) -> u32 {
    read_blob_u32(state, desc_off + CT_DESC_HEAD)
}

fn read_desc_tail(state: CtbState, desc_off: usize) -> u32 {
    read_blob_u32(state, desc_off + CT_DESC_TAIL)
}

fn write_ct_dw(state: CtbState, base: usize, idx: usize, value: u32) {
    write_blob_u32(state, base + idx * 4, value);
}

fn read_ct_dw(state: CtbState, base: usize, idx: usize) -> u32 {
    read_blob_u32(state, base + idx * 4)
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

fn next_fence(untracked: bool) -> u16 {
    let rolling = NEXT_FENCE.fetch_add(1, Ordering::AcqRel) & 0x7FFF;
    let rolling = rolling.max(1);
    rolling | if untracked { 0x8000 } else { 0 }
}

fn ct_ring_space(tail: usize, head: usize, size: usize) -> usize {
    if tail >= size || head >= size || size == 0 {
        return 0;
    }
    if head > tail {
        head - tail - 1
    } else {
        size - tail + head - 1
    }
}

fn ct_ring_distance(head: usize, tail: usize, size: usize) -> usize {
    if head >= size || tail >= size || size == 0 {
        return 0;
    }
    if tail >= head {
        tail - head
    } else {
        size - head + tail
    }
}

fn observe_h2g_head(state: &mut CtbState, head: usize) {
    let previous = state.h2g_observed_head as usize;
    if previous >= CT_H2G_RING_DWORDS || head >= CT_H2G_RING_DWORDS {
        return;
    }
    state.h2g_consumed_dwords = state.h2g_consumed_dwords.saturating_add(ct_ring_distance(
        previous,
        head,
        CT_H2G_RING_DWORDS,
    ) as u64);
    state.h2g_observed_head = head as u32;
}

fn flush_blob_range(state: CtbState, off: usize, len: usize) {
    if off.saturating_add(len) <= state.len {
        unsafe {
            crate::intel::dma_flush(state.virt.add(off), len);
        }
    }
}

fn hxg_action_header(request_type: u32, action: u32) -> u32 {
    ((request_type & 0x7) << 28) | (action & 0xFFFF)
}

fn hxg_origin(value: u32) -> u32 {
    (value >> 31) & 0x1
}

fn hxg_type(value: u32) -> u32 {
    (value >> 28) & 0x7
}
