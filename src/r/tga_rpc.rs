//! Tiny asynchronous RPC-style service for the dormant TGA card.
//!
//! One kernel task serializes the fixed BAR0 work package, sleeps for MSI, and
//! delivers the result to a Rust future or callback. Only heartbeat and
//! `add_u32` are exposed.

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use core::task::{Context, Poll, Waker};

use embassy_sync::signal::Signal;
use embassy_time::{Duration as EmbassyDuration, Timer, with_timeout};
use spin::Mutex;

pub use crate::tga::protocol::FunctionId;
use crate::tga::protocol::{
    self, ABI_VERSION, FLAG_INTERRUPT_ON_COMPLETE, INLINE_INPUT_BYTES, INLINE_OUTPUT_BYTES,
    WORK_PACKAGE_MAGIC, WorkPackage, WorkState,
};

const MAX_QUEUED_CALLS: usize = 32;
const DEVICE_RETRY_MS: u64 = 100;
const COMPLETION_TIMEOUT_MS: u64 = 2_000;
const HEARTBEAT_PERIOD_MS: u64 = 250;

static NEXT_CALL_ID: AtomicU64 = AtomicU64::new(1);
static WORKER_STARTED: AtomicBool = AtomicBool::new(false);
static WORK_AVAILABLE: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();
static SERVICE: Mutex<ServiceState> = Mutex::new(ServiceState::new());

pub type CompletionCallback = Box<dyn FnOnce(CallResult) + Send + 'static>;
pub type CallResult = Result<Completion, Error>;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CallId(u64);

impl CallId {
    pub const fn raw(self) -> u64 {
        self.0
    }
}
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Completion {
    pub call_id: CallId,
    pub function: FunctionId,
    output_len: usize,
    output: [u8; INLINE_OUTPUT_BYTES],
}

impl Completion {
    pub fn output(&self) -> &[u8] {
        &self.output[..self.output_len]
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    QueueFull,
    InputTooLarge,
    OutputTooLarge,
    DeviceLost,
    Protocol,
    TransportWriteVerification {
        word: u8,
        observed: u32,
        expected: u32,
    },
    Device(i32),
    UnknownCall,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Stats {
    pub submitted: u64,
    pub completed: u64,
    pub failed: u64,
    pub queued: usize,
    pub inflight: usize,
    pub interrupts: u64,
    pub interrupt_wakes: u64,
    pub timeout_recoveries: u64,
}

#[derive(Copy, Clone)]
struct Request {
    call_id: CallId,
    function: FunctionId,
    input_len: usize,
    input: [u8; INLINE_INPUT_BYTES],
    output_capacity: usize,
}

enum Delivery {
    Future {
        waiter: Option<Waker>,
        detached: bool,
    },
    Callback(Option<CompletionCallback>),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum CallState {
    Queued,
    Inflight,
}

struct CallRecord {
    request: Request,
    state: CallState,
    delivery: Delivery,
    result: Option<CallResult>,
}

struct ServiceState {
    queue: VecDeque<CallId>,
    calls: Vec<CallRecord>,
    submitted: u64,
    completed: u64,
    failed: u64,
    interrupt_wakes: u64,
    timeout_recoveries: u64,
}

impl ServiceState {
    const fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            calls: Vec::new(),
            submitted: 0,
            completed: 0,
            failed: 0,
            interrupt_wakes: 0,
            timeout_recoveries: 0,
        }
    }
}

/// Awaitable handle for one serialized hardware call.
///
/// Dropping the handle detaches the waiter; it does not cancel a package that
/// may already be owned by the card.
pub struct FpgaCall {
    call_id: CallId,
    resolved: bool,
}

impl FpgaCall {
    pub const fn id(&self) -> CallId {
        self.call_id
    }
}

impl Future for FpgaCall {
    type Output = CallResult;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut service = SERVICE.lock();
        let Some(index) = service
            .calls
            .iter()
            .position(|record| record.request.call_id == this.call_id)
        else {
            this.resolved = true;
            return Poll::Ready(Err(Error::UnknownCall));
        };

        if let Some(result) = service.calls[index].result.take() {
            service.calls.remove(index);
            this.resolved = true;
            return Poll::Ready(result);
        }
        if let Delivery::Future { waiter, .. } = &mut service.calls[index].delivery
            && waiter.as_ref().is_none_or(|old| !old.will_wake(cx.waker()))
        {
            *waiter = Some(cx.waker().clone());
        }
        Poll::Pending
    }
}

impl Drop for FpgaCall {
    fn drop(&mut self) {
        if self.resolved {
            return;
        }
        let mut service = SERVICE.lock();
        let Some(index) = service
            .calls
            .iter()
            .position(|record| record.request.call_id == self.call_id)
        else {
            return;
        };
        let complete = service.calls[index].result.is_some();
        if let Delivery::Future { waiter, detached } = &mut service.calls[index].delivery {
            *waiter = None;
            *detached = true;
        }
        if complete {
            service.calls.remove(index);
        }
    }
}

pub fn submit(
    function: FunctionId,
    input: &[u8],
    output_capacity: usize,
) -> Result<FpgaCall, Error> {
    let call_id = enqueue(
        function,
        input,
        output_capacity,
        Delivery::Future {
            waiter: None,
            detached: false,
        },
    )?;
    Ok(FpgaCall {
        call_id,
        resolved: false,
    })
}

pub fn submit_with_callback<F>(
    function: FunctionId,
    input: &[u8],
    output_capacity: usize,
    callback: F,
) -> Result<CallId, Error>
where
    F: FnOnce(CallResult) + Send + 'static,
{
    enqueue(function, input, output_capacity, Delivery::Callback(Some(Box::new(callback))))
}

pub async fn call(function: FunctionId, input: &[u8], output_capacity: usize) -> CallResult {
    submit(function, input, output_capacity)?.await
}

pub async fn heartbeat() -> Result<bool, Error> {
    let completion = call(FunctionId::HEARTBEAT, &[], 4).await?;
    let reply = protocol::decode_u32(completion.output()).ok_or(Error::Protocol)?;
    Ok(reply == protocol::HEARTBEAT_REPLY)
}

pub async fn add_u32(a: u32, b: u32) -> Result<u32, Error> {
    let input = protocol::encode_add_u32(a, b);
    let completion = call(FunctionId::ADD_U32, &input, 4).await?;
    protocol::decode_u32(completion.output()).ok_or(Error::Protocol)
}

pub fn stats() -> Stats {
    let service = SERVICE.lock();
    Stats {
        submitted: service.submitted,
        completed: service.completed,
        failed: service.failed,
        queued: service
            .calls
            .iter()
            .filter(|record| record.state == CallState::Queued)
            .count(),
        inflight: service
            .calls
            .iter()
            .filter(|record| record.state == CallState::Inflight)
            .count(),
        interrupts: crate::tga::completion_interrupt_count(),
        interrupt_wakes: service.interrupt_wakes,
        timeout_recoveries: service.timeout_recoveries,
    }
}

fn enqueue(
    function: FunctionId,
    input: &[u8],
    output_capacity: usize,
    delivery: Delivery,
) -> Result<CallId, Error> {
    if input.len() > INLINE_INPUT_BYTES {
        return Err(Error::InputTooLarge);
    }
    if output_capacity > INLINE_OUTPUT_BYTES {
        return Err(Error::OutputTooLarge);
    }
    let mut service = SERVICE.lock();
    if service.calls.len() >= MAX_QUEUED_CALLS {
        return Err(Error::QueueFull);
    }
    let call_id = CallId(NEXT_CALL_ID.fetch_add(1, Ordering::Relaxed).max(1));
    let mut owned_input = [0; INLINE_INPUT_BYTES];
    owned_input[..input.len()].copy_from_slice(input);
    service.calls.push(CallRecord {
        request: Request {
            call_id,
            function,
            input_len: input.len(),
            input: owned_input,
            output_capacity,
        },
        state: CallState::Queued,
        delivery,
        result: None,
    });
    service.queue.push_back(call_id);
    service.submitted = service.submitted.saturating_add(1);
    drop(service);
    WORK_AVAILABLE.signal(());
    Ok(call_id)
}

fn take_next_request() -> Option<Request> {
    let mut service = SERVICE.lock();
    while let Some(call_id) = service.queue.pop_front() {
        if let Some(record) = service
            .calls
            .iter_mut()
            .find(|record| record.request.call_id == call_id)
        {
            record.state = CallState::Inflight;
            return Some(record.request);
        }
    }
    None
}

fn finish_call(call_id: CallId, result: CallResult) {
    let (waker, callback) = {
        let mut service = SERVICE.lock();
        let Some(index) = service
            .calls
            .iter()
            .position(|record| record.request.call_id == call_id)
        else {
            return;
        };
        if result.is_ok() {
            service.completed = service.completed.saturating_add(1);
        } else {
            service.failed = service.failed.saturating_add(1);
        }
        match &mut service.calls[index].delivery {
            Delivery::Future { waiter, detached } => {
                let waker = waiter.take();
                if *detached {
                    service.calls.remove(index);
                } else {
                    service.calls[index].result = Some(result);
                }
                (waker, None)
            }
            Delivery::Callback(callback) => {
                let callback = callback.take();
                service.calls.remove(index);
                (None, callback)
            }
        }
    };
    if let Some(waker) = waker {
        waker.wake();
    }
    if let Some(callback) = callback {
        callback(result);
    }
}

fn package_for(request: Request) -> WorkPackage {
    let mut package = WorkPackage::ZEROED;
    package.function = request.function.raw();
    package.call_id = request.call_id.raw();
    package.flags = FLAG_INTERRUPT_ON_COMPLETE;
    package.input_len = request.input_len as u32;
    package.output_capacity = request.output_capacity as u32;
    package.input = request.input;
    package.state = WorkState::HostReady as u32;
    package
}

fn decode_completion(request: Request, package: WorkPackage) -> CallResult {
    if package.magic != WORK_PACKAGE_MAGIC
        || package.abi_version != ABI_VERSION
        || package.function != request.function.raw()
        || package.call_id != request.call_id.raw()
    {
        return Err(Error::Protocol);
    }
    match WorkState::from_raw(package.state) {
        Some(WorkState::Failed) => Err(Error::Device(package.error_code)),
        Some(WorkState::Complete) => {
            let output_len = package.output_len as usize;
            if output_len > request.output_capacity || output_len > INLINE_OUTPUT_BYTES {
                return Err(Error::Protocol);
            }
            Ok(Completion {
                call_id: request.call_id,
                function: request.function,
                output_len,
                output: package.output,
            })
        }
        _ => Err(Error::Protocol),
    }
}

fn map_transport_error(error: crate::tga::OffloadTransportError) -> Error {
    match error {
        crate::tga::OffloadTransportError::Offline => Error::DeviceLost,
        crate::tga::OffloadTransportError::InvalidPackage => Error::Protocol,
        crate::tga::OffloadTransportError::WriteVerification {
            word,
            observed,
            expected,
        } => Error::TransportWriteVerification {
            word,
            observed,
            expected,
        },
    }
}

fn record_interrupt_wake() {
    let mut service = SERVICE.lock();
    service.interrupt_wakes = service.interrupt_wakes.saturating_add(1);
}

fn record_timeout_recovery() {
    let mut service = SERVICE.lock();
    service.timeout_recoveries = service.timeout_recoveries.saturating_add(1);
}

async fn execute_request(request: Request) -> CallResult {
    while !crate::tga::is_online() {
        Timer::after(EmbassyDuration::from_millis(DEVICE_RETRY_MS)).await;
    }
    let generation = crate::tga::connection_generation();
    let interrupt_sequence = crate::tga::arm_offload_interrupt().map_err(map_transport_error)?;
    if crate::tga::connection_generation() != generation {
        return Err(Error::DeviceLost);
    }
    crate::tga::submit_offload_work_package(&package_for(request)).map_err(map_transport_error)?;

    let interrupt_wait = with_timeout(EmbassyDuration::from_millis(COMPLETION_TIMEOUT_MS), async {
        let mut sequence = interrupt_sequence;
        loop {
            sequence = crate::tga::wait_for_completion_interrupt(sequence).await;
            record_interrupt_wake();
            if !crate::tga::is_online() || crate::tga::connection_generation() != generation {
                return Err(Error::DeviceLost);
            }
            match crate::tga::offload_work_state().map_err(map_transport_error)? {
                WorkState::Complete | WorkState::Failed => return Ok(()),
                WorkState::Idle | WorkState::HostReady | WorkState::FpgaBusy => {}
            }
        }
    })
    .await;

    match interrupt_wait {
        Ok(result) => result?,
        Err(_) => {
            let terminal = crate::tga::offload_work_state().map_err(map_transport_error)?;
            if !matches!(terminal, WorkState::Complete | WorkState::Failed) {
                return Err(Error::Protocol);
            }
            record_timeout_recovery();
        }
    }
    let package = crate::tga::read_offload_work_package().map_err(map_transport_error)?;
    let result = decode_completion(request, package);
    crate::tga::ack_offload_interrupt().map_err(map_transport_error)?;
    result
}

#[embassy_executor::task]
pub async fn service_task() {
    if WORKER_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    crate::log!(
        "tga-rpc: worker started functions=heartbeat,add_u32 transport=bar0 completion=msi\n"
    );
    loop {
        let Some(request) = take_next_request() else {
            WORK_AVAILABLE.wait().await;
            continue;
        };
        let result = execute_request(request).await;
        finish_call(request.call_id, result);
    }
}

async fn wait_for_reconnect(previous_generation: u32) {
    while !crate::tga::is_online() || crate::tga::connection_generation() == previous_generation {
        Timer::after(EmbassyDuration::from_millis(DEVICE_RETRY_MS)).await;
    }
}

#[embassy_executor::task]
pub async fn heartbeat_task() {
    crate::log!("tga-rpc: heartbeat client started\n");
    let mut announced = false;
    loop {
        if crate::tga::is_online() {
            let generation = crate::tga::connection_generation();
            match heartbeat().await {
                Ok(true) => {
                    if !announced {
                        crate::log!("tga-rpc: heartbeat online\n");
                        announced = true;
                    }
                }
                Ok(false) | Err(_) => {
                    announced = false;
                    wait_for_reconnect(generation).await;
                }
            }
        }
        Timer::after(EmbassyDuration::from_millis(HEARTBEAT_PERIOD_MS)).await;
    }
}
