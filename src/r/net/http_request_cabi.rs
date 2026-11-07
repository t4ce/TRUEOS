//! Bounded, owner-scoped single HTTP exchange. Transport lives in request_stream.
//! Discard releases response buffers immediately; its capacity slot retires only
//! after the producer returns, including its transport cancellation fence.
extern crate alloc;
use crate::r::cabi_codes::*;
use alloc::{
    boxed::Box,
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    vec::Vec,
};
use core::{
    future::{Future, poll_fn},
    pin::{Pin, pin},
    task::{Context, Poll},
};
use spin::Mutex;
use v::vnetfs::http::{MAX_BODY_READ_BYTES, MAX_FRAME_BYTES, RequestOwned, ResponseHead, wire};

const MAX_OPERATIONS: usize = 64;
const MAX_OPERATIONS_PER_OWNER: usize = 32;
const BODY_WINDOW: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Owner {
    Kernel,
    Vm { id: u8, generation: u64 },
}
pub(crate) fn owner_for_vm(id: u8) -> Option<Owner> {
    crate::hv::vm_run_generation(id).map(|generation| Owner::Vm { id, generation })
}
fn owner_current(owner: Owner) -> bool {
    match owner {
        Owner::Kernel => true,
        Owner::Vm { id, generation } => crate::hv::vm_run_generation(id) == Some(generation),
    }
}
struct State {
    head: Option<Vec<u8>>,
    body: VecDeque<u8>,
    terminal: Option<Result<(), i32>>,
    cancelled: bool,
    producer_done: bool,
    received: u64,
}
struct Shared {
    id: u32,
    owner: Owner,
    state: Mutex<State>,
    changed: crate::wait::WaitQueue,
}
struct Pending {
    request: RequestOwned,
    sink: ResponseSink,
}
struct Registry {
    next: u32,
    entries: BTreeMap<u32, Arc<Shared>>,
    pending: VecDeque<Pending>,
    // Teardown closes admission under the same lock as insertion, even while
    // HV still reports the retiring generation. Fixed by the u8 VM namespace.
    closed_runs: [Option<u64>; 256],
}
impl Registry {
    fn owner_open(&self, owner: Owner) -> bool {
        match owner {
            Owner::Kernel => true,
            Owner::Vm { id, generation } => {
                self.closed_runs[id as usize].is_none_or(|closed| generation > closed)
            }
        }
    }
}
static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    next: 1,
    entries: BTreeMap::new(),
    pending: VecDeque::new(),
    closed_runs: [None; 256],
});
static ARRIVAL: crate::wait::WaitQueue = crate::wait::WaitQueue::new();

/// One producer owns body publication; clones are also usable for cancellation
/// observation. The body window is credited only by successful consumer reads.
#[derive(Clone)]
pub struct ResponseSink {
    shared: Arc<Shared>,
}
impl ResponseSink {
    pub fn is_cancelled(&self) -> bool {
        self.shared.state.lock().cancelled || !owner_current(self.shared.owner)
    }
    pub async fn cancelled(&self) {
        loop {
            let observed = self.shared.changed.observe();
            if self.is_cancelled() {
                return;
            }
            self.shared.changed.wait_after(observed).await;
        }
    }
    pub fn publish_head(&self, head: ResponseHead) -> Result<(), i32> {
        let bytes = crate::allocators::with_host_alloc_domain_strong(|| wire::encode_head(&head))?;
        let mut state = self.shared.state.lock();
        if state.cancelled || !owner_current(self.shared.owner) {
            return Err(FS_ERR_NOT_FOUND);
        }
        if state.head.is_some() || state.terminal.is_some() {
            return Err(FS_ERR_BAD_PARAM);
        }
        state.head = Some(bytes);
        Ok(())
    }
    pub async fn push_body(&self, bytes: &[u8]) -> Result<(), i32> {
        for piece in bytes.chunks(MAX_BODY_READ_BYTES) {
            loop {
                let observed = self.shared.changed.observe();
                {
                    let mut state = self.shared.state.lock();
                    if state.cancelled || !owner_current(self.shared.owner) {
                        return Err(FS_ERR_NOT_FOUND);
                    }
                    if state.head.is_none() || state.terminal.is_some() {
                        return Err(FS_ERR_BAD_PARAM);
                    }
                    if state.body.len() <= BODY_WINDOW - piece.len() {
                        state.received = state
                            .received
                            .checked_add(piece.len() as u64)
                            .ok_or(FS_ERR_TOO_LARGE)?;
                        // Start reserved the complete fixed window, so accepting
                        // bytes never allocates while holding this lock.
                        state.body.extend(piece.iter().copied());
                        break;
                    }
                }
                self.shared.changed.wait_after(observed).await;
            }
        }
        Ok(())
    }
    /// The first terminal result wins. Already accepted bytes precede errors.
    pub fn finish(&self, result: Result<(), i32>) {
        let mut state = self.shared.state.lock();
        if !state.cancelled && state.terminal.is_none() {
            state.terminal = Some(result.map_err(|code| {
                if code < 0 && code != NET_PENDING {
                    code
                } else {
                    FS_ERR_IO
                }
            }));
        }
        drop(state);
        self.shared.changed.notify_all();
    }
}

fn accessible(owner: Owner, id: u32) -> Result<Arc<Shared>, i32> {
    if id == 0 || id > i32::MAX as u32 {
        return Err(FS_ERR_BAD_PARAM);
    }
    if !owner_current(owner) {
        return Err(FS_ERR_NOT_FOUND);
    }
    let registry = REGISTRY.lock();
    if !registry.owner_open(owner) {
        return Err(FS_ERR_NOT_FOUND);
    }
    let shared = registry.entries.get(&id).ok_or(FS_ERR_NOT_FOUND)?;
    if shared.owner != owner || shared.state.lock().cancelled {
        return Err(FS_ERR_NOT_FOUND);
    }
    Ok(shared.clone())
}

pub(crate) fn start_host(owner: Owner, frame: &[u8]) -> i32 {
    crate::allocators::with_host_alloc_domain_strong(|| {
        if !owner_current(owner) {
            return FS_ERR_NOT_FOUND;
        }
        let request = match wire::decode_request(frame) {
            Ok(value) => value,
            Err(code) => return code,
        };
        let mut registry = REGISTRY.lock();
        if !owner_current(owner) || !registry.owner_open(owner) {
            return FS_ERR_NOT_FOUND;
        }
        if registry.entries.len() >= MAX_OPERATIONS
            || registry
                .entries
                .values()
                .filter(|shared| shared.owner == owner)
                .count()
                >= MAX_OPERATIONS_PER_OWNER
        {
            return FS_ERR_NO_SPACE;
        }
        // IDs are never reused. Exhaustion rejects starts rather than aliasing
        // a stale guest handle to another request in the same run.
        if registry.next > i32::MAX as u32 {
            return FS_ERR_NO_SPACE;
        }
        let mut body = VecDeque::new();
        if body.try_reserve_exact(BODY_WINDOW).is_err() || registry.pending.try_reserve(1).is_err()
        {
            return FS_ERR_NO_SPACE;
        }
        let id = registry.next;
        registry.next += 1;
        let shared = Arc::new(Shared {
            id,
            owner,
            state: Mutex::new(State {
                head: None,
                body,
                terminal: None,
                cancelled: false,
                producer_done: false,
                received: 0,
            }),
            changed: crate::wait::WaitQueue::new(),
        });
        registry.entries.insert(id, shared.clone());
        registry.pending.push_back(Pending {
            request,
            sink: ResponseSink { shared },
        });
        drop(registry);
        ARRIVAL.notify_one();
        crate::remote_work_wake::wake_cpu_for_remote_work(0);
        id as i32
    })
}

/// Empty output is a non-consuming length query, including after body EOF.
pub(crate) fn head_host(owner: Owner, id: u32, out: &mut [u8]) -> isize {
    if out.len() > MAX_FRAME_BYTES {
        return FS_ERR_BAD_PARAM as isize;
    }
    let shared = match accessible(owner, id) {
        Ok(value) => value,
        Err(code) => return code as isize,
    };
    let state = shared.state.lock();
    if state.cancelled {
        return FS_ERR_NOT_FOUND as isize;
    }
    match &state.head {
        Some(bytes) => {
            if !out.is_empty() {
                if out.len() < bytes.len() {
                    return FS_ERR_TOO_LARGE as isize;
                }
                out[..bytes.len()].copy_from_slice(bytes);
            }
            bytes.len() as isize
        }
        None => match state.terminal {
            Some(Err(code)) => code as isize,
            Some(Ok(())) => FS_ERR_IO as isize,
            None => NET_PENDING as isize,
        },
    }
}

pub(crate) fn read_host(owner: Owner, id: u32, out: &mut [u8]) -> isize {
    if out.is_empty() || out.len() > MAX_BODY_READ_BYTES {
        return FS_ERR_BAD_PARAM as isize;
    }
    let shared = match accessible(owner, id) {
        Ok(value) => value,
        Err(code) => return code as isize,
    };
    let mut state = shared.state.lock();
    if state.cancelled {
        return FS_ERR_NOT_FOUND as isize;
    }
    if !state.body.is_empty() {
        let len = out.len().min(state.body.len());
        for byte in &mut out[..len] {
            *byte = state.body.pop_front().unwrap();
        }
        drop(state);
        shared.changed.notify_all();
        return len as isize;
    }
    match state.terminal {
        Some(Ok(())) => 0,
        Some(Err(code)) => code as isize,
        None => NET_PENDING as isize,
    }
}

fn cancel_shared(shared: &Shared) {
    let mut state = shared.state.lock();
    state.cancelled = true;
    state.head = None;
    state.body = VecDeque::new();
    drop(state);
    shared.changed.notify_all();
}
pub(crate) fn discard_host(owner: Owner, id: u32) -> i32 {
    if id == 0 || id > i32::MAX as u32 {
        return FS_ERR_BAD_PARAM;
    }
    if !owner_current(owner) {
        return FS_ERR_NOT_FOUND;
    }
    let mut registry = REGISTRY.lock();
    if !registry.owner_open(owner) {
        return FS_ERR_NOT_FOUND;
    }
    let Some(shared) = registry.entries.get(&id).cloned() else {
        return 0;
    };
    if shared.owner != owner {
        return FS_ERR_NOT_FOUND;
    }
    cancel_shared(&shared);
    if shared.state.lock().producer_done {
        registry.entries.remove(&id);
    }
    drop(registry);
    ARRIVAL.notify_one();
    crate::remote_work_wake::wake_cpu_for_remote_work(0);
    0
}
pub(crate) fn discard_for_vm(id: u8, generation: u64) -> usize {
    let mut registry = REGISTRY.lock();
    let mut count = 0;
    let closed = &mut registry.closed_runs[id as usize];
    *closed = Some(closed.map_or(generation, |previous| previous.max(generation)));
    registry.entries.retain(|_, shared| {
        if shared.owner != (Owner::Vm { id, generation }) {
            return true;
        }
        count += 1;
        cancel_shared(shared);
        !shared.state.lock().producer_done
    });
    drop(registry);
    if count != 0 {
        ARRIVAL.notify_one();
        crate::remote_work_wake::wake_cpu_for_remote_work(0);
    }
    count
}
fn producer_retired(sink: &ResponseSink) {
    let mut registry = REGISTRY.lock();
    if !owner_current(sink.shared.owner) {
        cancel_shared(&sink.shared);
    }
    let mut state = sink.shared.state.lock();
    state.producer_done = true;
    let cancelled = state.cancelled;
    drop(state);
    if cancelled {
        registry.entries.remove(&sink.shared.id);
    }
}
async fn produce(pending: Pending) {
    if !pending.sink.is_cancelled() {
        super::request_stream::start_exchange(pending.request, pending.sink.clone()).await;
        // A forgotten completion must not leave an immortal pending handle.
        pending.sink.finish(Err(FS_ERR_IO));
    }
    producer_retired(&pending.sink);
}
type Active = Pin<Box<dyn Future<Output = ()>>>;
fn poll_requests(active: &mut Vec<Active>, cx: &mut Context<'_>) {
    loop {
        let pending = REGISTRY.lock().pending.pop_front();
        match pending {
            Some(pending) => active.push(Box::pin(produce(pending))),
            None => break,
        }
    }
    let mut index = 0;
    while index < active.len() {
        if active[index].as_mut().poll(cx).is_ready() {
            drop(active.swap_remove(index));
        } else {
            index += 1;
        }
    }
}
#[trueos_executor::task]
pub async fn service_task() {
    let mut active = Vec::with_capacity(MAX_OPERATIONS);
    loop {
        let observed = ARRIVAL.observe();
        let mut arrival = pin!(ARRIVAL.wait_after(observed));
        poll_fn(|cx| {
            let notified = arrival.as_mut().poll(cx).is_ready();
            poll_requests(&mut active, cx);
            if notified {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }
}

/// Validated VM wire arguments; no payload or body is consumed on rejection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VmCommand {
    Start,
    Head { id: u32, cap: usize },
    Read { id: u32, cap: usize },
    Discard { id: u32 },
}
pub(crate) fn decode_vm_command(
    op: u32,
    arg0: u64,
    arg1: u64,
    request_len: usize,
) -> Result<VmCommand, i32> {
    use trueos_vm::vmcall::*;
    if op == OP_BP_HTTP_REQUEST_START_V1 {
        return if arg0 == 0 && arg1 == 0 && request_len != 0 && request_len <= MAX_FRAME_BYTES {
            Ok(VmCommand::Start)
        } else {
            Err(FS_ERR_BAD_PARAM)
        };
    }
    if request_len != 0 || arg0 == 0 || arg0 > i32::MAX as u64 {
        return Err(FS_ERR_BAD_PARAM);
    }
    let id = arg0 as u32;
    match op {
        OP_BP_HTTP_REQUEST_HEAD_V1 if arg1 <= MAX_FRAME_BYTES as u64 => Ok(VmCommand::Head {
            id,
            cap: arg1 as usize,
        }),
        OP_BP_HTTP_REQUEST_READ_V1 if arg1 != 0 && arg1 <= MAX_BODY_READ_BYTES as u64 => {
            Ok(VmCommand::Read {
                id,
                cap: arg1 as usize,
            })
        }
        OP_BP_HTTP_REQUEST_DISCARD_V1 if arg1 == 0 => Ok(VmCommand::Discard { id }),
        _ => Err(FS_ERR_BAD_PARAM),
    }
}

fn guest_call(op: u32, arg0: u64, arg1: u64, request: &[u8], out: &mut [u8]) -> isize {
    let (status, value, reported) =
        trueos_vm::vmcall::call_with_payload_reported_len(op, arg0, arg1, request, out);
    if status != trueos_vm::vmcall::STATUS_OK {
        return FS_ERR_BAD_PARAM as isize;
    }
    let result = (value as i64) as isize;
    // Length queries/start/discard carry no response bytes. Reads must report
    // exactly their positive count; negative/EOF responses carry no payload.
    let expected = if !out.is_empty() && result > 0 {
        result as usize
    } else {
        0
    };
    if expected > out.len() || reported != expected {
        return FS_ERR_IO as isize;
    }
    result
}
/// # Safety
/// `payload` must reference `len` readable bytes for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_http_request_start_v1(
    payload: *const u8,
    len: usize,
) -> i32 {
    if payload.is_null() || len == 0 || len > MAX_FRAME_BYTES {
        return FS_ERR_BAD_PARAM;
    }
    let Some(bytes) = crate::native_abi::abi_read_bytes(payload, len) else {
        return FS_ERR_BAD_PARAM;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_call(
            trueos_vm::vmcall::OP_BP_HTTP_REQUEST_START_V1,
            0,
            0,
            bytes,
            &mut [],
        ) as i32
    } else {
        start_host(Owner::Kernel, bytes)
    }
}
/// # Safety
/// Nonzero `cap` requires `out` to reference that many writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_http_request_head_v1(
    id: u32,
    out: *mut u8,
    cap: usize,
) -> isize {
    if cap > MAX_FRAME_BYTES || (cap != 0 && out.is_null()) {
        return FS_ERR_BAD_PARAM as isize;
    }
    let Some(bytes) = crate::native_abi::abi_write_bytes(out, cap) else {
        return FS_ERR_BAD_PARAM as isize;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_call(
            trueos_vm::vmcall::OP_BP_HTTP_REQUEST_HEAD_V1,
            u64::from(id),
            cap as u64,
            &[],
            bytes,
        )
    } else {
        head_host(Owner::Kernel, id, bytes)
    }
}
/// # Safety
/// `out` must reference `cap` writable bytes; cap must be in 1..=32768.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_http_request_read_v1(
    id: u32,
    out: *mut u8,
    cap: usize,
) -> isize {
    if cap == 0 || cap > MAX_BODY_READ_BYTES || out.is_null() {
        return FS_ERR_BAD_PARAM as isize;
    }
    let Some(bytes) = crate::native_abi::abi_write_bytes(out, cap) else {
        return FS_ERR_BAD_PARAM as isize;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_call(
            trueos_vm::vmcall::OP_BP_HTTP_REQUEST_READ_V1,
            u64::from(id),
            cap as u64,
            &[],
            bytes,
        )
    } else {
        read_host(Owner::Kernel, id, bytes)
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_http_request_discard_v1(id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_call(
            trueos_vm::vmcall::OP_BP_HTTP_REQUEST_DISCARD_V1,
            u64::from(id),
            0,
            &[],
            &mut [],
        ) as i32
    } else {
        discard_host(Owner::Kernel, id)
    }
}
