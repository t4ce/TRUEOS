//! Ahead-of-time FPGA function calls over the TRUEGA BAR window.
//!
//! The FPGA contains fixed compiled circuits. This service is the only software worker:
//! it serializes inline BAR0 packages and fixed BAR2 row operations, sleeps for MSI, and
//! hands each result to either a Rust `Future` or a registered completion callback. There
//! is no FPGA processor, device-side queue parser, TLB, or runtime HDL toolchain.

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use core::task::{Context, Poll, Waker};

use embassy_sync::{mutex::Mutex as AsyncMutex, signal::Signal};
use embassy_time::{Duration as EmbassyDuration, Timer, with_timeout};
use spin::Mutex;
use trueos_fpga_abi::{
    ABI_VERSION, FLAG_INTERRUPT_ON_COMPLETE, FunctionId, INLINE_INPUT_BYTES, INLINE_OUTPUT_BYTES,
    WORK_PACKAGE_MAGIC, WorkPackage, WorkState,
};

// One 4,608-element projection row contains 72 cached-pair calls. Keeping the
// whole row queued lets the single worker submit the next hardware operation
// immediately after MSI retirement instead of waiting for the Lumen caller to
// wake, verify, and enqueue again. There is still exactly one FPGA call in
// flight; this is host-side sequencing, not a hardware worker pool.
const MAX_ACTIVE_CALLS: usize = 128;
const DEVICE_RETRY_MS: u64 = 100;
const COMPLETION_INTERRUPT_TIMEOUT_MS: u64 = 2_000;
const HEARTBEAT_PERIOD_MS: u64 = 250;

static NEXT_CALL_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_LFM25_DECODE_SESSION_EPOCH: AtomicU32 = AtomicU32::new(1);
static WORKER_STARTED: AtomicBool = AtomicBool::new(false);
static WORK_AVAILABLE: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();
static SERVICE: Mutex<ServiceState> = Mutex::new(ServiceState::new());
static LFM25_FFN_STEP_LANE: AsyncMutex<crate::wait::EmbassySpinRawMutex, ()> = AsyncMutex::new(());
// Serializes complete LFM25 stream sessions so one caller owns the BAR2
// activation/weight buffers from the first load through the final row. The
// service worker separately owns TGA_TRANSPORT_LANE for every actual hardware
// operation, including each row in this session.
static LFM25_STREAM_SESSION_LANE: AsyncMutex<crate::wait::EmbassySpinRawMutex, ()> =
    AsyncMutex::new(());
static TGA_TRANSPORT_LANE: AsyncMutex<crate::wait::EmbassySpinRawMutex, ()> = AsyncMutex::new(());

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
    TransportWriteVerification {
        word: u8,
        observed: u32,
        expected: u32,
        rx_captures: u32,
        rx_capture_delta: u32,
        decoded_writes: u32,
        decoded_write_delta: u32,
        word30_writes: u32,
        word30_write_delta: u32,
        word30_last_payload: u32,
        word30_storage: u32,
        rx_fifo_state: u32,
        rx_errors: u32,
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
    pub active: usize,
    pub interrupts: u64,
    pub interrupt_wakes: u64,
    pub timeout_recoveries: u64,
    pub lfm25_ffn_step_completed: u64,
}

#[derive(Copy, Clone)]
struct Request {
    call_id: CallId,
    function: FunctionId,
    input_len: usize,
    input: [u8; INLINE_INPUT_BYTES],
    output_capacity: usize,
}

type Lfm25StreamCallResult = Result<crate::tga::Lfm25StreamResult, Error>;
pub type Lfm25DecodeCallResult =
    Result<trueos_fpga_abi::lfm25_decode_transport::Completion, Error>;
type Lfm25DecodeCompletionCallback = Box<dyn FnOnce(Lfm25DecodeCallResult) + Send + 'static>;
pub type Lfm25FeedCallResult =
    Result<trueos_fpga_abi::lfm25_decode_feed::FeedRetirementStatus, Error>;
type Lfm25FeedCompletionCallback = Box<dyn FnOnce(Lfm25FeedCallResult) + Send + 'static>;

pub const LFM25_FEED_MAX_PAYLOAD_BYTES: usize = 64;

/// One exact validator-issued payload owned by a queued TGF2 job. Keeping the
/// bytes with the descriptor prevents caller mutation while the single worker
/// sleeps behind earlier jobs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lfm25FeedStage {
    descriptor: trueos_fpga_abi::lfm25_decode_feed::StagedPayload,
    payload_len: u8,
    payload: [u8; LFM25_FEED_MAX_PAYLOAD_BYTES],
}

impl Lfm25FeedStage {
    pub fn new(
        descriptor: trueos_fpga_abi::lfm25_decode_feed::StagedPayload,
        payload: &[u8],
    ) -> Result<Self, Error> {
        use trueos_fpga_abi::lfm25_decode_feed::FeedPayloadFormat;

        let expected_len = descriptor.payload_bytes as usize;
        let shape_is_exact = descriptor.generation != 0
            && descriptor.bar2_offset().is_some()
            && expected_len == payload.len()
            && match descriptor.payload_format {
                FeedPayloadFormat::GgmlQ8_0Block => expected_len == 34,
                FeedPayloadFormat::Bf16 | FeedPayloadFormat::Bf16x3 => expected_len == 64,
                FeedPayloadFormat::None => false,
            };
        if !shape_is_exact || payload.len() > LFM25_FEED_MAX_PAYLOAD_BYTES {
            return Err(Error::Protocol);
        }
        let mut owned = [0u8; LFM25_FEED_MAX_PAYLOAD_BYTES];
        owned[..payload.len()].copy_from_slice(payload);
        Ok(Self {
            descriptor,
            payload_len: payload.len() as u8,
            payload: owned,
        })
    }

    pub const fn descriptor(&self) -> trueos_fpga_abi::lfm25_decode_feed::StagedPayload {
        self.descriptor
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.payload_len as usize]
    }
}

struct Lfm25StreamRequest {
    mode: u32,
    row: u32,
    connection_generation: u32,
    reply: Arc<Signal<crate::wait::EmbassySpinRawMutex, Lfm25StreamCallResult>>,
}

struct Lfm25DecodeRequest {
    command: trueos_fpga_abi::lfm25_decode_transport::Command,
    connection_generation: u32,
    callback: Option<Lfm25DecodeCompletionCallback>,
}

impl Lfm25DecodeRequest {
    fn deliver(mut self, result: Lfm25DecodeCallResult) {
        if let Some(callback) = self.callback.take() {
            callback(result);
        }
    }
}

struct Lfm25FeedRequest {
    record: trueos_fpga_abi::lfm25_decode_feed::FeedCommitRecord,
    stages: Vec<Lfm25FeedStage>,
    connection_generation: u32,
    previous_completion_count: Option<u32>,
    callback: Option<Lfm25FeedCompletionCallback>,
}

impl Lfm25FeedRequest {
    fn deliver(mut self, result: Lfm25FeedCallResult) {
        if let Some(callback) = self.callback.take() {
            callback(result);
        }
    }
}

/// One operation accepted by the sole FPGA transport worker. The variants
/// differ only in how their fixed circuit is started, inspected, and decoded;
/// interrupt arming, MSI sleeping, timeout recovery, acknowledgement, and
/// result delivery are owned by the same worker path.
enum TransportJob {
    Inline(CallId),
    Lfm25Stream(Lfm25StreamRequest),
    Lfm25Decode(Lfm25DecodeRequest),
    Lfm25Feed(Lfm25FeedRequest),
}

enum ActiveTransportJob {
    Inline(Request),
    Lfm25Stream(Lfm25StreamRequest),
    Lfm25Decode(Lfm25DecodeRequest),
    Lfm25Feed(Lfm25FeedRequest),
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
    queue: VecDeque<TransportJob>,
    calls: Vec<CallRecord>,
    submitted: u64,
    completed: u64,
    failed: u64,
    interrupt_wakes: u64,
    timeout_recoveries: u64,
    lfm25_ffn_step_completed: u64,
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
            lfm25_ffn_step_completed: 0,
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
    use trueos_fpga_abi::builtins::led_step_heartbeat as function;

    let input = function::encode();
    let completion = call(function::ID, &input, function::OUTPUT_BYTES).await?;
    let reply = function::decode(completion.output()).ok_or(Error::Protocol)?;
    Ok(reply == trueos_fpga_abi::builtins::HEARTBEAT_REPLY)
}

pub async fn heartbeat() -> Result<bool, Error> {
    led_step_heartbeat().await
}

pub async fn add_u32(a: u32, b: u32) -> Result<u32, Error> {
    use trueos_fpga_abi::builtins::add_u32 as function;

    let input = function::encode(a, b);
    let completion = call(function::ID, &input, function::OUTPUT_BYTES).await?;
    function::decode(completion.output()).ok_or(Error::Protocol)
}

/// Feed one unchanged native Q8_0 activation/weight pair into the fixed
/// layer-row accumulator. The slot keeps only the signed Q30 accumulator
/// between calls; the single worker remains the transport owner.
pub async fn lfm25_q8_row_block(
    first: bool,
    last: bool,
    block_index: u8,
    activation: &[u8; 34],
    weight: &[u8; 34],
) -> Result<trueos_fpga_abi::builtins::lfm25_q8_row_block::Q8RowBlockResult, Error> {
    lfm25_q8_projection_block(false, first, last, block_index, activation, weight).await
}

/// Execute one block of either a 32-block (1,024 element) or 144-block
/// (4,608 element) projection row. The width bit is part of the fixed slot
/// protocol and is checked by hardware throughout the row.
pub async fn lfm25_q8_projection_block(
    wide: bool,
    first: bool,
    last: bool,
    block_index: u8,
    activation: &[u8; 34],
    weight: &[u8; 34],
) -> Result<trueos_fpga_abi::builtins::lfm25_ffn_step::Q8RowBlockResult, Error> {
    use trueos_fpga_abi::builtins::lfm25_ffn_step as function;

    let input = function::encode_projection(first, last, wide, block_index, activation, weight);
    let completion = lfm25_ffn_step_with_callback(&input).await?;
    function::decode(completion.output()).ok_or(Error::Protocol)
}

/// Populate one entry of slot 2's fixed activation cache. The cache contains at
/// most the 144 native Q8_0 blocks required by the widest layer-0 projection and
/// is circuit state, not a device-side command or model store.
pub async fn lfm25_cache_q8_activation(
    wide: bool,
    block_index: u8,
    activation: &[u8; 34],
) -> Result<(), Error> {
    use trueos_fpga_abi::builtins::lfm25_ffn_step as function;

    let input = function::encode_activation_cache(wide, block_index, activation);
    lfm25_ffn_step_with_callback(&input).await?;
    Ok(())
}

/// Process two consecutive weight blocks against cached activations. One fixed
/// work package and one MSI retirement now cover both exact dot/scale terms.
pub async fn lfm25_q8_cached_pair(
    wide: bool,
    first: bool,
    last: bool,
    block_index: u8,
    weight0: &[u8; 34],
    weight1: &[u8; 34],
) -> Result<trueos_fpga_abi::builtins::lfm25_ffn_step::Q8RowBlockResult, Error> {
    submit_lfm25_q8_cached_pair(wide, first, last, block_index, weight0, weight1)?
        .complete()
        .await
}

/// Typed handle for one already-queued cached-pair operation. Queueing all
/// pairs in a row removes caller round trips while preserving FIFO execution,
/// one work package in flight, one MSI per operation, and exact per-pair
/// verification.
pub struct Lfm25CachedPairCall(FpgaCall);

impl Lfm25CachedPairCall {
    pub async fn complete(
        self,
    ) -> Result<trueos_fpga_abi::builtins::lfm25_ffn_step::Q8RowBlockResult, Error> {
        use trueos_fpga_abi::builtins::lfm25_ffn_step as function;

        let completion = self.0.await?;
        function::decode(completion.output()).ok_or(Error::Protocol)
    }
}

pub fn submit_lfm25_q8_cached_pair(
    wide: bool,
    first: bool,
    last: bool,
    block_index: u8,
    weight0: &[u8; 34],
    weight1: &[u8; 34],
) -> Result<Lfm25CachedPairCall, Error> {
    use trueos_fpga_abi::builtins::lfm25_ffn_step as function;

    let input = function::encode_cached_pair(first, last, wide, block_index, weight0, weight1);
    let call = submit(function::ID, &input, function::OUTPUT_BYTES)?;
    Ok(Lfm25CachedPairCall(call))
}

/// Fixed hardware vector operation used between the gate/up projections and
/// the down projection. Inputs and output are signed Q30.
pub async fn lfm25_silu_mul_q30(gate_q30: i64, up_q30: i64) -> Result<i64, Error> {
    use trueos_fpga_abi::builtins::lfm25_ffn_step as function;

    let input = function::encode_silu(gate_q30, up_q30);
    let completion = lfm25_ffn_step_with_callback(&input).await?;
    let result = function::decode(completion.output()).ok_or(Error::Protocol)?;
    Ok(result.row_q30)
}

/// Exclusively owns the stateful slot-2 row accumulator across a complete
/// multi-call projection/FFN transaction. Heartbeat and add use other slots.
pub async fn acquire_lfm25_ffn_step_lane()
-> embassy_sync::mutex::MutexGuard<'static, crate::wait::EmbassySpinRawMutex, ()> {
    LFM25_FFN_STEP_LANE.lock().await
}

/// Exclusively owns BAR2's stateful activation and weight buffers across a
/// complete streamed FFN transaction. Every row is still submitted to the
/// generic worker, which owns the shared MSI bridge only while that hardware
/// operation is in flight.
pub async fn acquire_lfm25_stream_transport()
-> embassy_sync::mutex::MutexGuard<'static, crate::wait::EmbassySpinRawMutex, ()> {
    LFM25_STREAM_SESSION_LANE.lock().await
}

/// Exclusive ownership of the BAR2 resident-tensor domain for one fixed decode session.
/// The epoch is carried through every device request and becomes part of every resident
/// handle minted by the higher-level decode backend.
pub struct Lfm25DecodeTransportSession {
    _bar2_lane:
        embassy_sync::mutex::MutexGuard<'static, crate::wait::EmbassySpinRawMutex, ()>,
    connection_generation: u32,
    session_epoch: u32,
    epoch_installed: bool,
}

impl Lfm25DecodeTransportSession {
    pub const fn connection_generation(&self) -> u32 {
        self.connection_generation
    }

    pub const fn session_epoch(&self) -> u32 {
        self.session_epoch
    }

    /// Submit one fixed AOT operation. The returned future is signalled only by the
    /// callback which the single worker invokes after MSI retirement.
    pub async fn execute(
        &mut self,
        command: trueos_fpga_abi::lfm25_decode_transport::Command,
    ) -> Lfm25DecodeCallResult {
        use trueos_fpga_abi::lfm25_decode::DecodeOpKind;

        if command.session_epoch != self.session_epoch {
            return Err(Error::Protocol);
        }
        if !crate::tga::is_online()
            || crate::tga::connection_generation() != self.connection_generation
        {
            return Err(Error::DeviceLost);
        }
        command.validate().map_err(|_| Error::Protocol)?;
        let installs_epoch = !self.epoch_installed;
        if installs_epoch
            && (command.operation != DecodeOpKind::TokenEmbedding || command.position != 0)
        {
            return Err(Error::Protocol);
        }
        if self.epoch_installed
            && command.operation == DecodeOpKind::TokenEmbedding
            && command.position == 0
        {
            return Err(Error::Protocol);
        }

        let reply = Arc::new(Signal::<crate::wait::EmbassySpinRawMutex, Lfm25DecodeCallResult>::new());
        let callback_reply = Arc::clone(&reply);
        let request = Lfm25DecodeRequest {
            command,
            connection_generation: self.connection_generation,
            callback: Some(Box::new(move |result| callback_reply.signal(result))),
        };
        {
            let mut service = SERVICE.lock();
            if service.queue.len() >= MAX_ACTIVE_CALLS {
                return Err(Error::QueueFull);
            }
            service.queue.push_back(TransportJob::Lfm25Decode(request));
        }
        WORK_AVAILABLE.signal(());
        let result = reply.wait().await;
        if installs_epoch && result.is_ok() {
            self.epoch_installed = true;
        }
        result
    }
}

pub fn lfm25_decode_transport_available() -> bool {
    crate::tga::lfm25_decode_transport_available()
}

/// Completion/status registers still absent from the finalized TGF2 feed ABI.
/// Their names are intentionally exposed without invented offsets so callers
/// can distinguish an exact staging capability from a usable async job path.
pub const LFM25_FEED_MISSING_COMPLETION_REGISTERS: [&str; 8] =
    crate::tga::LFM25_FEED_MISSING_COMPLETION_REGISTERS;

/// Read the exact five-dword TGF2 publication at BAR0 0x280..0x293. This is a
/// diagnostic read; callers must use `capability_is_exact` before interpreting
/// the endpoint as a matching staging implementation.
pub fn lfm25_feed_capability() -> Result<trueos_fpga_abi::lfm25_decode_feed::FeedCapability, Error>
{
    crate::tga::lfm25_feed_capability().map_err(map_transport_error)
}

/// True when BAR2 exists and all five TGF2 capability fields match exactly.
/// Staging availability alone does not authorize a worker job.
pub fn lfm25_feed_staging_available() -> bool {
    crate::tga::lfm25_feed_staging_available()
}

/// Deliberately false until TGF2 defines completion state, retired
/// mode/session/sequence/item identity, an error code, and IRQ pending/acknowledgement.
/// Consequently no TGF2 `TransportJob` variant can be queued yet.
pub fn lfm25_feed_transport_available() -> bool {
    crate::tga::lfm25_feed_transport_available()
}

/// Acquire the same BAR2 session lane used by the proven FFN row streamer. Firmware
/// without the exact decode-v1 magic and complete capability bits is rejected before an
/// epoch is minted or any request register is written.
pub async fn acquire_lfm25_decode_transport() -> Result<Lfm25DecodeTransportSession, Error> {
    let lane = LFM25_STREAM_SESSION_LANE.lock().await;
    if !crate::tga::is_online() {
        return Err(Error::DeviceLost);
    }
    if !crate::tga::lfm25_decode_transport_available() {
        return Err(Error::Protocol);
    }
    let connection_generation = crate::tga::connection_generation();
    let session_epoch = loop {
        let candidate = NEXT_LFM25_DECODE_SESSION_EPOCH.fetch_add(1, Ordering::Relaxed);
        if candidate != 0 {
            break candidate;
        }
    };
    Ok(Lfm25DecodeTransportSession {
        _bar2_lane: lane,
        connection_generation,
        session_epoch,
        epoch_installed: false,
    })
}

pub fn lfm25_row_stream_available() -> bool {
    crate::tga::lfm25_stream_available()
}

pub fn lfm25_stream_completion_count() -> Result<u32, Error> {
    crate::tga::lfm25_stream_completion_count().map_err(map_transport_error)
}

pub fn lfm25_stream_load_activation(
    blocks: &[[u8; trueos_fpga_abi::lfm25::Q8_0_BLOCK_BYTES]],
) -> Result<(), Error> {
    if !matches!(blocks.len(), 32 | 144) {
        return Err(Error::Protocol);
    }
    crate::tga::lfm25_stream_write_blocks(
        trueos_fpga_abi::BAR2_LFM25_STREAM_ACTIVATION_OFFSET,
        blocks,
    )
    .map_err(map_transport_error)
}

pub async fn lfm25_stream_gate_up_row(
    row: u32,
    gate_weights: &[u8],
    up_weights: &[u8],
) -> Result<crate::tga::Lfm25StreamResult, Error> {
    const ROW_BYTES: usize = 32 * trueos_fpga_abi::lfm25::Q8_0_BLOCK_BYTES;
    if gate_weights.len() != ROW_BYTES || up_weights.len() != ROW_BYTES {
        return Err(Error::Protocol);
    }
    crate::tga::lfm25_stream_write_block_bytes(
        trueos_fpga_abi::BAR2_LFM25_STREAM_WEIGHT0_OFFSET,
        gate_weights,
    )
    .map_err(map_transport_error)?;
    crate::tga::lfm25_stream_write_block_bytes(
        trueos_fpga_abi::BAR2_LFM25_STREAM_WEIGHT1_OFFSET,
        up_weights,
    )
    .map_err(map_transport_error)?;
    execute_lfm25_stream_row(trueos_fpga_abi::LFM25_STREAM_MODE_GATE_UP_SILU, row).await
}

pub async fn lfm25_stream_down_row(row: u32, weights: &[u8]) -> Result<i64, Error> {
    if weights.len() != 144 * trueos_fpga_abi::lfm25::Q8_0_BLOCK_BYTES {
        return Err(Error::Protocol);
    }
    crate::tga::lfm25_stream_write_block_bytes(
        trueos_fpga_abi::BAR2_LFM25_STREAM_WEIGHT0_OFFSET,
        weights,
    )
    .map_err(map_transport_error)?;
    let result = execute_lfm25_stream_row(trueos_fpga_abi::LFM25_STREAM_MODE_DOWN, row).await?;
    Ok(result.result_q30)
}

async fn execute_lfm25_stream_row(
    mode: u32,
    row: u32,
) -> Result<crate::tga::Lfm25StreamResult, Error> {
    if !matches!(
        mode,
        trueos_fpga_abi::LFM25_STREAM_MODE_GATE_UP_SILU | trueos_fpga_abi::LFM25_STREAM_MODE_DOWN
    ) {
        return Err(Error::Protocol);
    }

    let reply = Arc::new(Signal::<crate::wait::EmbassySpinRawMutex, Lfm25StreamCallResult>::new());
    let request = Lfm25StreamRequest {
        mode,
        row,
        // BAR2 contents belong to this exact endpoint generation. Never carry
        // a queued row across SRAM reprogramming/reconnect into a fresh device.
        connection_generation: crate::tga::connection_generation(),
        reply: Arc::clone(&reply),
    };
    {
        let mut service = SERVICE.lock();
        if service.queue.len() >= MAX_ACTIVE_CALLS {
            return Err(Error::QueueFull);
        }
        service.queue.push_back(TransportJob::Lfm25Stream(request));
    }
    WORK_AVAILABLE.signal(());
    reply.wait().await
}

fn map_transport_error(error: crate::tga::OffloadTransportError) -> Error {
    match error {
        crate::tga::OffloadTransportError::Offline => Error::DeviceLost,
        crate::tga::OffloadTransportError::InvalidPackage => Error::Protocol,
        crate::tga::OffloadTransportError::Device(code) => Error::Device(code),
        crate::tga::OffloadTransportError::WriteVerification {
            word,
            observed,
            expected,
            rx_captures,
            rx_capture_delta,
            decoded_writes,
            decoded_write_delta,
            word30_writes,
            word30_write_delta,
            word30_last_payload,
            word30_storage,
            rx_fifo_state,
            rx_errors,
        } => Error::TransportWriteVerification {
            word,
            observed,
            expected,
            rx_captures,
            rx_capture_delta,
            decoded_writes,
            decoded_write_delta,
            word30_writes,
            word30_write_delta,
            word30_last_payload,
            word30_storage,
            rx_fifo_state,
            rx_errors,
        },
    }
}

async fn lfm25_ffn_step_with_callback(input: &[u8]) -> Result<Completion, Error> {
    use trueos_fpga_abi::builtins::lfm25_ffn_step as function;

    let reply = Arc::new(Signal::<crate::wait::EmbassySpinRawMutex, CallResult>::new());
    let callback_reply = Arc::clone(&reply);
    submit_with_callback(function::ID, input, function::OUTPUT_BYTES, move |result| {
        callback_reply.signal(result);
    })?;
    reply.wait().await
}

/// Compatibility probe: execute one block as a one-block row.
pub async fn lfm25_q8_block(
    activation: &[u8; 34],
    weight: &[u8; 34],
) -> Result<trueos_fpga_abi::builtins::lfm25_q8_row_block::Q8RowBlockResult, Error> {
    let _lane = acquire_lfm25_ffn_step_lane().await;
    lfm25_q8_row_block(true, true, 0, activation, weight).await
}

pub fn stats() -> Stats {
    let service = SERVICE.lock();
    Stats {
        submitted: service.submitted,
        completed: service.completed,
        failed: service.failed,
        queued: service.queue.len(),
        active: service.calls.len(),
        interrupts: crate::tga::completion_interrupt_count(),
        interrupt_wakes: service.interrupt_wakes,
        timeout_recoveries: service.timeout_recoveries,
        lfm25_ffn_step_completed: service.lfm25_ffn_step_completed,
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
    service.queue.push_back(TransportJob::Inline(call_id));
    service.submitted = service.submitted.saturating_add(1);
    drop(service);
    WORK_AVAILABLE.signal(());
    Ok(call_id)
}

fn take_next_transport_job() -> Option<ActiveTransportJob> {
    let mut service = SERVICE.lock();
    while let Some(job) = service.queue.pop_front() {
        match job {
            TransportJob::Inline(call_id) => {
                if let Some(record) = service
                    .calls
                    .iter_mut()
                    .find(|record| record.request.call_id == call_id)
                {
                    record.state = CallState::Inflight;
                    return Some(ActiveTransportJob::Inline(record.request));
                }
            }
            TransportJob::Lfm25Stream(request) => {
                return Some(ActiveTransportJob::Lfm25Stream(request));
            }
            TransportJob::Lfm25Decode(request) => {
                return Some(ActiveTransportJob::Lfm25Decode(request));
            }
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

        let completed_function = service.calls[index].request.function;
        if result.is_ok() {
            service.completed = service.completed.saturating_add(1);
            if completed_function == trueos_fpga_abi::builtins::lfm25_ffn_step::ID {
                service.lfm25_ffn_step_completed =
                    service.lfm25_ffn_step_completed.saturating_add(1);
            }
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

impl ActiveTransportJob {
    fn validate_connection_generation(&self) -> Result<(), Error> {
        match self.connection_generation() {
            Some(generation)
                if !crate::tga::is_online()
                    || crate::tga::connection_generation() != generation =>
            {
                Err(Error::DeviceLost)
            }
            _ => Ok(()),
        }
    }

    fn connection_generation(&self) -> Option<u32> {
        match self {
            Self::Inline(_) => None,
            Self::Lfm25Stream(request) => Some(request.connection_generation),
            Self::Lfm25Decode(request) => Some(request.connection_generation),
        }
    }

    fn start(&self) -> Result<(), Error> {
        self.validate_connection_generation()?;
        match self {
            Self::Inline(request) => {
                let package = package_for(*request);
                crate::tga::submit_offload_work_package(&package).map_err(map_transport_error)
            }
            Self::Lfm25Stream(request) => {
                crate::tga::start_lfm25_stream_row(request.mode, request.row)
                    .map_err(map_transport_error)
            }
            Self::Lfm25Decode(request) => {
                crate::tga::start_lfm25_decode(request.command).map_err(map_transport_error)
            }
        }
    }

    fn is_terminal(&self) -> Result<bool, Error> {
        self.validate_connection_generation()?;
        match self {
            Self::Inline(_) => crate::tga::offload_work_state()
                .map(|state| matches!(state, WorkState::Complete | WorkState::Failed))
                .map_err(map_transport_error),
            Self::Lfm25Stream(_) => crate::tga::lfm25_stream_state()
                .map(|state| {
                    matches!(
                        state,
                        trueos_fpga_abi::Lfm25StreamState::Complete
                            | trueos_fpga_abi::Lfm25StreamState::Failed
                    )
                })
                .map_err(map_transport_error),
            Self::Lfm25Decode(_) => crate::tga::lfm25_decode_state()
                .map(|state| {
                    matches!(
                        state,
                        trueos_fpga_abi::lfm25_decode_transport::State::Complete
                            | trueos_fpga_abi::lfm25_decode_transport::State::Failed
                    )
                })
                .map_err(map_transport_error),
        }
    }

    fn deliver_error(self, error: Error) {
        match self {
            Self::Inline(request) => finish_call(request.call_id, Err(error)),
            Self::Lfm25Stream(request) => request.reply.signal(Err(error)),
            Self::Lfm25Decode(request) => request.deliver(Err(error)),
        }
    }

    fn retire_and_deliver(self, terminal: Result<(), Error>) {
        let terminal_reached = terminal.is_ok();
        match self {
            Self::Inline(request) => {
                let result = terminal.and_then(|()| {
                    crate::tga::read_offload_work_package()
                        .map_err(map_transport_error)
                        .and_then(|package| decode_completion(request, package))
                });
                // A terminal result is acknowledged after all result fields
                // have been consumed, even if decoding rejected their ABI.
                if terminal_reached {
                    let _ = crate::tga::ack_offload_interrupt();
                }
                finish_call(request.call_id, result);
            }
            Self::Lfm25Stream(request) => {
                let result = terminal.and_then(|()| {
                    if !crate::tga::is_online()
                        || crate::tga::connection_generation() != request.connection_generation
                    {
                        return Err(Error::DeviceLost);
                    }
                    let state = crate::tga::lfm25_stream_state().map_err(map_transport_error)?;
                    let result =
                        crate::tga::read_lfm25_stream_result().map_err(map_transport_error)?;
                    match state {
                        trueos_fpga_abi::Lfm25StreamState::Complete if result.error_code == 0 => {
                            Ok(result)
                        }
                        trueos_fpga_abi::Lfm25StreamState::Failed => {
                            Err(Error::Device(result.error_code as i32))
                        }
                        _ => Err(Error::Protocol),
                    }
                });
                if terminal_reached {
                    let _ = crate::tga::ack_offload_interrupt();
                }
                request.reply.signal(result);
            }
            Self::Lfm25Decode(request) => {
                let result = terminal.and_then(|()| {
                    if !crate::tga::is_online()
                        || crate::tga::connection_generation() != request.connection_generation
                    {
                        return Err(Error::DeviceLost);
                    }
                    crate::tga::read_lfm25_decode_completion(request.command)
                        .map_err(map_transport_error)
                });
                if terminal_reached {
                    let _ = crate::tga::ack_offload_interrupt();
                }
                request.deliver(result);
            }
        }
    }
}

#[embassy_executor::task]
pub async fn fpga_offload_service_task() {
    if WORKER_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    crate::log!(
        "fpga-offload: single worker started functions=3 transport=tga-bar0+bar2 completion=msi-worker-wake package_bytes={}\n",
        core::mem::size_of::<WorkPackage>()
    );

    loop {
        let Some(job) = take_next_transport_job() else {
            WORK_AVAILABLE.wait().await;
            continue;
        };

        if let Some(generation) = job.connection_generation() {
            if !crate::tga::is_online() || crate::tga::connection_generation() != generation {
                job.deliver_error(Error::DeviceLost);
                continue;
            }
        } else {
            while !crate::tga::is_online() {
                Timer::after(EmbassyDuration::from_millis(DEVICE_RETRY_MS)).await;
            }
        }

        // Both fixed transports share one physical completion bridge. Only this
        // worker takes the hardware lane, so exactly one operation can be armed,
        // started, retired, acknowledged, and delivered at a time.
        let _transport_lane = TGA_TRANSPORT_LANE.lock().await;

        if let Some(generation) = job.connection_generation()
            && (!crate::tga::is_online() || crate::tga::connection_generation() != generation)
        {
            job.deliver_error(Error::DeviceLost);
            continue;
        }

        let interrupt_sequence = match crate::tga::arm_offload_interrupt() {
            Ok(sequence) => sequence,
            Err(error) => {
                job.deliver_error(map_transport_error(error));
                continue;
            }
        };

        if let Err(error) = job.start() {
            job.deliver_error(error);
            continue;
        }

        let interrupt_wait =
            with_timeout(EmbassyDuration::from_millis(COMPLETION_INTERRUPT_TIMEOUT_MS), async {
                let mut sequence = interrupt_sequence;
                loop {
                    sequence = crate::tga::wait_for_completion_interrupt(sequence).await;
                    record_interrupt_wake();
                    match job.is_terminal() {
                        Ok(true) => return Ok(()),
                        Ok(false) => {
                            // A stale or unrelated edge cannot complete the call. Re-arm
                            // the sequence wait without re-polling the BAR.
                        }
                        Err(error) => return Err(error),
                    }
                }
            })
            .await;

        let terminal = match interrupt_wait {
            Ok(result) => result,
            Err(_) => match job.is_terminal() {
                Ok(true) => {
                    // Timeout recovery is intentionally a single diagnostic read,
                    // never a replacement polling loop.
                    record_timeout_recovery();
                    Ok(())
                }
                Ok(false) => Err(Error::Protocol),
                Err(error) => Err(error),
            },
        };

        job.retire_and_deliver(terminal);
    }
}

async fn wait_for_tga_reconnect(previous_generation: u32) {
    // A live SRAM program invalidates the old endpoint, then the TGA task
    // publishes a new connection generation. Keying the wait to that generation
    // also covers a fast reconnect that completed before this client observed
    // the intermediate offline state.
    while !crate::tga::is_online() || crate::tga::connection_generation() == previous_generation {
        Timer::after(EmbassyDuration::from_millis(DEVICE_RETRY_MS)).await;
    }
}

/// Periodic client of slot 0. This is not another hardware worker: all calls still pass
/// through `fpga_offload_service_task`, which permits exactly one in-flight operation.
/// If that end-to-end path wedges, the visible LED sequence stops as intended.
#[embassy_executor::task]
pub async fn fpga_offload_heartbeat_task() {
    crate::log!("fpga-offload: LED function heartbeat client started\n");
    let mut online_announced = false;
    loop {
        if crate::tga::is_online() {
            let connection_generation = crate::tga::connection_generation();
            match led_step_heartbeat().await {
                Ok(true) => {
                    if !online_announced {
                        crate::log!(
                            "fpga-offload: LED heartbeat online via fused work-package function\n"
                        );
                        online_announced = true;
                    }
                }
                Ok(false) => {
                    crate::log_warn!(
                        "fpga-offload: heartbeat paused after bad liveness magic; waiting for FPGA reconnect\n"
                    );
                    wait_for_tga_reconnect(connection_generation).await;
                    online_announced = false;
                }
                Err(Error::DeviceLost) => {
                    wait_for_tga_reconnect(connection_generation).await;
                    online_announced = false;
                }
                Err(error) => {
                    crate::log_warn!(
                        "fpga-offload: heartbeat paused after first failure: {:?}; waiting for FPGA reconnect\n",
                        error
                    );
                    wait_for_tga_reconnect(connection_generation).await;
                    online_announced = false;
                }
            }
        }
        Timer::after(EmbassyDuration::from_millis(HEARTBEAT_PERIOD_MS)).await;
    }
}
