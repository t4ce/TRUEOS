//! Ahead-of-time FPGA function calls over the TRUEGA BAR window.
//!
//! The FPGA contains three compiled circuits. This service is the only software worker:
//! it serializes calls through one fixed work package, observes the completion flag, and
//! hands the result to either a Rust `Future` or a registered completion callback. There
//! is no FPGA processor, device-side queue parser, TLB, or runtime HDL toolchain.

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use core::task::{Context, Poll, Waker};

use embassy_sync::signal::Signal;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;
use trueos_fpga_abi::{
    ABI_VERSION, FLAG_INTERRUPT_ON_COMPLETE, FunctionId, INLINE_INPUT_BYTES, INLINE_OUTPUT_BYTES,
    WORK_PACKAGE_MAGIC, WorkPackage, WorkState,
};

const MAX_ACTIVE_CALLS: usize = 32;
const DEVICE_RETRY_MS: u64 = 100;
const COMPLETION_POLL_MS: u64 = 1;
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
    pub output_len: usize,
    pub output: [u8; INLINE_OUTPUT_BYTES],
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
    Device(i32),
    UnknownCall,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Stats {
    pub submitted: u64,
    pub completed: u64,
    pub failed: u64,
    pub queued: usize,
    pub active: usize,
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
}

impl ServiceState {
    const fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            calls: Vec::new(),
            submitted: 0,
            completed: 0,
            failed: 0,
        }
    }
}

/// Awaitable handle for one exact FPGA call.
///
/// Dropping it detaches the waiter but does not cancel hardware already in flight.
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

        if let Delivery::Future { waiter, .. } = &mut service.calls[index].delivery {
            if waiter
                .as_ref()
                .map(|old| !old.will_wake(cx.waker()))
                .unwrap_or(true)
            {
                *waiter = Some(cx.waker().clone());
            }
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
        let already_complete = service.calls[index].result.is_some();
        if let Delivery::Future { waiter, detached } = &mut service.calls[index].delivery {
            *waiter = None;
            *detached = true;
        }
        if already_complete {
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

/// First typed function in the TRUEGA firmware: advance the visible LED state and
/// return the liveness magic through the same work-package completion path.
pub async fn led_step_heartbeat() -> Result<bool, Error> {
    let completion = call(trueos_fpga_abi::builtins::LED_STEP_HEARTBEAT, &[], 4).await?;
    let reply =
        trueos_fpga_abi::builtins::result_u32(completion.output()).ok_or(Error::Protocol)?;
    Ok(reply == trueos_fpga_abi::builtins::HEARTBEAT_REPLY)
}

pub async fn heartbeat() -> Result<bool, Error> {
    led_step_heartbeat().await
}

pub async fn add_u32(a: u32, b: u32) -> Result<u32, Error> {
    let input = trueos_fpga_abi::builtins::binary_u32_args(a, b);
    let completion = call(trueos_fpga_abi::builtins::ADD_U32, &input, 4).await?;
    trueos_fpga_abi::builtins::result_u32(completion.output()).ok_or(Error::Protocol)
}

pub async fn xor_u32(a: u32, b: u32) -> Result<u32, Error> {
    let input = trueos_fpga_abi::builtins::binary_u32_args(a, b);
    let completion = call(trueos_fpga_abi::builtins::XOR_U32, &input, 4).await?;
    trueos_fpga_abi::builtins::result_u32(completion.output()).ok_or(Error::Protocol)
}

pub fn stats() -> Stats {
    let service = SERVICE.lock();
    Stats {
        submitted: service.submitted,
        completed: service.completed,
        failed: service.failed,
        queued: service.queue.len(),
        active: service.calls.len(),
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
    if service.calls.len() >= MAX_ACTIVE_CALLS {
        return Err(Error::QueueFull);
    }
    let call_id = CallId(NEXT_CALL_ID.fetch_add(1, Ordering::Relaxed).max(1));
    let mut inline_input = [0; INLINE_INPUT_BYTES];
    inline_input[..input.len()].copy_from_slice(input);
    let request = Request {
        call_id,
        function,
        input_len: input.len(),
        input: inline_input,
        output_capacity,
    };
    service.calls.push(CallRecord {
        request,
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

#[embassy_executor::task]
pub async fn fpga_offload_service_task() {
    if WORKER_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    crate::log!(
        "fpga-offload: single worker started functions=3 transport=tga-bar package_bytes={}\n",
        core::mem::size_of::<WorkPackage>()
    );

    loop {
        let Some(request) = take_next_request() else {
            WORK_AVAILABLE.wait().await;
            continue;
        };

        while !crate::tga::is_online() {
            Timer::after(EmbassyDuration::from_millis(DEVICE_RETRY_MS)).await;
        }

        let package = package_for(request);
        match crate::tga::submit_offload_work_package(&package) {
            Ok(()) => {}
            Err(crate::tga::OffloadTransportError::Offline) => {
                finish_call(request.call_id, Err(Error::DeviceLost));
                continue;
            }
            Err(crate::tga::OffloadTransportError::InvalidPackage) => {
                finish_call(request.call_id, Err(Error::Protocol));
                continue;
            }
        }

        loop {
            Timer::after(EmbassyDuration::from_millis(COMPLETION_POLL_MS)).await;
            match crate::tga::offload_work_state() {
                Ok(WorkState::Complete | WorkState::Failed) => {
                    let result = crate::tga::read_offload_work_package()
                        .map_err(|_| Error::Protocol)
                        .and_then(|package| decode_completion(request, package));
                    let _ = crate::tga::ack_offload_interrupt();
                    finish_call(request.call_id, result);
                    break;
                }
                Ok(WorkState::Idle | WorkState::HostReady | WorkState::FpgaBusy) => {}
                Err(crate::tga::OffloadTransportError::Offline) => {
                    finish_call(request.call_id, Err(Error::DeviceLost));
                    break;
                }
                Err(crate::tga::OffloadTransportError::InvalidPackage) => {
                    finish_call(request.call_id, Err(Error::Protocol));
                    break;
                }
            }
        }
    }
}

/// Periodic client of slot 0. This is not another hardware worker: all calls still pass
/// through `fpga_offload_service_task`, which permits exactly one in-flight work package.
/// If that end-to-end path wedges, the visible LED sequence stops as intended.
#[embassy_executor::task]
pub async fn fpga_offload_heartbeat_task() {
    crate::log!("fpga-offload: LED function heartbeat client started\n");
    loop {
        if crate::tga::is_online() {
            match led_step_heartbeat().await {
                Ok(true) => {}
                Ok(false) => {
                    crate::log_warn!(
                        "fpga-offload: heartbeat disabled after bad liveness magic; flash matching TRUEGA firmware\n"
                    );
                    return;
                }
                Err(error) => {
                    crate::log_warn!(
                        "fpga-offload: heartbeat disabled after first failure: {:?}; flash matching TRUEGA firmware\n",
                        error
                    );
                    return;
                }
            }
        }
        Timer::after(EmbassyDuration::from_millis(HEARTBEAT_PERIOD_MS)).await;
    }
}
