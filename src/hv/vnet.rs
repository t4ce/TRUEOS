use alloc::collections::VecDeque;
use alloc::string::String as AllocString;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use spin::Mutex;

use crate::hv::hvlogf;

pub const VM_NET_INLINE_CAP: usize = 512;
pub const VM_NET_OP_TCP_WRITE: u32 = 0x10;
pub const VM_NET_OP_TCP_READ: u32 = 0x11;

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VmNetStatus {
    Ok = 0,
    BadArg = 1,
    NotReady = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct VmNetRequest {
    pub seq: u32,
    pub op: u32,
    pub arg0: u64,
    pub arg1: u64,
    pub len: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct VmNetCompletion {
    pub seq: u32,
    pub status: u32,
    pub data: u64,
    pub len: u32,
}

#[derive(Debug)]
struct VmNetContext {
    vm_id: u8,
    tx_bytes: u64,
    rx_bytes: u64,
    last_req: Option<VmNetRequest>,
    last_cpl: Option<VmNetCompletion>,
    recent_rx: VecDeque<u8>,
    pending_console_text: AllocString,
}

impl VmNetContext {
    const fn new(vm_id: u8) -> Self {
        Self {
            vm_id,
            tx_bytes: 0,
            rx_bytes: 0,
            last_req: None,
            last_cpl: None,
            recent_rx: VecDeque::new(),
            pending_console_text: AllocString::new(),
        }
    }
}

static VM_NET_CONTEXTS: [Mutex<VmNetContext>; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { Mutex::new(VmNetContext::new(0)) }; crate::allcaps::hv::VM_ID_LIMIT];
static VM_NET_SEQ: AtomicU32 = AtomicU32::new(1);
static VM_NET_DIAG_SEQ: AtomicU64 = AtomicU64::new(0);

fn current_vm_id_for_log() -> u8 {
    crate::hv::current_vm_id().unwrap_or(0)
}

fn context(vm_id: u8) -> Option<&'static Mutex<VmNetContext>> {
    VM_NET_CONTEXTS.get(vm_id as usize)
}

fn record_request(ctx: &mut VmNetContext, op: u32, arg0: u64, arg1: u64, len: u32) -> u32 {
    let seq = VM_NET_SEQ.fetch_add(1, Ordering::Relaxed);
    ctx.last_req = Some(VmNetRequest {
        seq,
        op,
        arg0,
        arg1,
        len,
    });
    seq
}

fn record_completion(
    ctx: &mut VmNetContext,
    seq: u32,
    status: VmNetStatus,
    data: u64,
    len: u32,
) -> VmNetCompletion {
    let cpl = VmNetCompletion {
        seq,
        status: status as u32,
        data,
        len,
    };
    ctx.last_cpl = Some(cpl);
    cpl
}

fn maybe_log(ctx: &VmNetContext, cpl: &VmNetCompletion) {
    let n = VM_NET_DIAG_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    if n <= 4 || n.is_power_of_two() {
        hvlogf(format_args!(
            "hv: vm{} reporting: vnet channel={} last-op=0x{:X} status={} data={} len={} tx_bytes={} rx_bytes={} recent_rx={}",
            current_vm_id_for_log(),
            ctx.vm_id,
            ctx.last_req.map(|r| r.op).unwrap_or(0),
            cpl.status,
            cpl.data,
            cpl.len,
            ctx.tx_bytes,
            ctx.rx_bytes,
            ctx.recent_rx.len()
        ));
    }
}

fn emit_console_line(text: &str) {
    crate::globalog::log(format_args!("{}\n", text));
}

fn push_console_bytes(ctx: &mut VmNetContext, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }

    let chunk = alloc::string::String::from_utf8_lossy(bytes);
    ctx.pending_console_text.push_str(chunk.as_ref());

    while let Some(newline_idx) = ctx.pending_console_text.find('\n') {
        let mut line = AllocString::from(&ctx.pending_console_text[..newline_idx]);
        if line.ends_with('\r') {
            line.pop();
        }
        emit_console_line(line.as_str());
        ctx.pending_console_text.drain(..=newline_idx);
    }
}

pub fn flush_console(vm_id: u8) {
    let Some(ctx_lock) = context(vm_id) else {
        return;
    };

    let mut ctx = ctx_lock.lock();
    ctx.vm_id = vm_id;
    if ctx.pending_console_text.is_empty() {
        return;
    }

    let mut line = core::mem::take(&mut ctx.pending_console_text);
    if line.ends_with('\r') {
        line.pop();
    }
    emit_console_line(line.as_str());
}

pub fn tcp_write(vm_id: u8, bytes: &[u8]) -> Result<usize, VmNetStatus> {
    let Some(ctx_lock) = context(vm_id) else {
        return Err(VmNetStatus::BadArg);
    };

    let mut ctx = ctx_lock.lock();
    ctx.vm_id = vm_id;
    push_console_bytes(&mut ctx, bytes);
    let seq = record_request(
        &mut ctx,
        VM_NET_OP_TCP_WRITE,
        0,
        0,
        bytes.len().min(u32::MAX as usize) as u32,
    );
    ctx.tx_bytes = ctx.tx_bytes.saturating_add(bytes.len() as u64);
    let cpl = record_completion(&mut ctx, seq, VmNetStatus::Ok, bytes.len() as u64, 0);
    maybe_log(&ctx, &cpl);
    Ok(bytes.len())
}

pub fn tcp_read(vm_id: u8, out: &mut [u8]) -> Result<usize, VmNetStatus> {
    if out.is_empty() {
        return Err(VmNetStatus::BadArg);
    }
    let Some(ctx_lock) = context(vm_id) else {
        return Err(VmNetStatus::BadArg);
    };

    let mut got = 0usize;
    while got < out.len() {
        match crate::shell2::backends::net_tcp::net_shell_read_byte() {
            Some(b) => {
                out[got] = b;
                got += 1;
            }
            None => break,
        }
    }

    let mut ctx = ctx_lock.lock();
    ctx.vm_id = vm_id;
    let seq =
        record_request(&mut ctx, VM_NET_OP_TCP_READ, out.len().min(u32::MAX as usize) as u64, 0, 0);
    for &b in &out[..got.min(VM_NET_INLINE_CAP)] {
        if ctx.recent_rx.len() >= VM_NET_INLINE_CAP {
            let _ = ctx.recent_rx.pop_front();
        }
        ctx.recent_rx.push_back(b);
    }
    ctx.rx_bytes = ctx.rx_bytes.saturating_add(got as u64);
    let cpl = record_completion(&mut ctx, seq, VmNetStatus::Ok, got as u64, got as u32);
    maybe_log(&ctx, &cpl);
    Ok(got)
}

pub fn last_completion(vm_id: u8) -> Option<VmNetCompletion> {
    let ctx = context(vm_id)?.lock();
    ctx.last_cpl
}
