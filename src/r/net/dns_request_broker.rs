//! Blocking-ABI compatibility bridge for the BSP-owned async DNS resolver.
//!
//! Blueprint's current C ABI still contains a synchronous DNS entry point.
//! Its callers execute on dedicated background AP/VM carrier lanes, while the
//! network futures themselves must be created and polled by the BSP executor.
//! The caller therefore submits only owned request data and parks until this
//! BSP task completes it.
//!
//! Never implement this boundary by recursively polling the local executor.
//! That can re-enter the BSP executor from one of its own tasks and deadlock
//! unrelated work sharing that executor.

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use spin::Mutex;

use super::vlayer::DnsResolveError;
use crate::wait::CompletionCell;

const BROKER_QUEUE_CAP: usize = 128;

static BROKER_ONLINE: AtomicBool = AtomicBool::new(false);
static BROKER_REQUESTS: Mutex<VecDeque<Request>> = Mutex::new(VecDeque::new());
static BROKER_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
static BROKER_REQUEST_SEQ: AtomicU64 = AtomicU64::new(1);

type ResolveResult = Result<[u8; 4], DnsResolveError>;
type Completion = Arc<CompletionCell<ResolveResult>>;

struct Request {
    id: u64,
    device_index: usize,
    host: String,
    completion: Completion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockingRequestError {
    BrokerOffline,
    WrongExecutionRealm,
    QueueFull,
}

fn validate_caller() -> Result<(), BlockingRequestError> {
    if !BROKER_ONLINE.load(Ordering::Acquire) {
        return Err(BlockingRequestError::BrokerOffline);
    }

    let cpu_slot = crate::percpu::this_cpu().cpu_index();
    if !crate::workers::is_background_worker_slot(cpu_slot) {
        return Err(BlockingRequestError::WrongExecutionRealm);
    }
    Ok(())
}

/// Submit an owned lookup request to the BSP and park the background carrier
/// lane. This synchronous shape exists only because it is part of the current
/// Blueprint ABI; native kernel users should await `dns` directly.
pub fn resolve_ipv4(
    device_index: usize,
    host: String,
) -> Result<ResolveResult, BlockingRequestError> {
    validate_caller()?;

    let id = BROKER_REQUEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let completion = Arc::new(CompletionCell::new());
    let queue_depth = {
        let mut requests = BROKER_REQUESTS.lock();
        if requests.len() >= BROKER_QUEUE_CAP {
            return Err(BlockingRequestError::QueueFull);
        }
        requests.push_back(Request {
            id,
            device_index,
            host,
            completion: completion.clone(),
        });
        requests.len()
    };

    crate::log_info!(target: "net";
        "dns-request-broker: submitted id={} caller_cpu={} device={} queue_depth={}\n",
        id,
        crate::percpu::this_cpu().cpu_index(),
        device_index,
        queue_depth,
    );

    BROKER_WAIT.notify_one();
    crate::remote_work_wake::wake_cpu_for_remote_work(0);
    Ok(completion.join_blocking_parked())
}

async fn process_request(request: Request) {
    let Request {
        id,
        device_index,
        host,
        completion,
    } = request;
    crate::log_info!(target: "net";
        "dns-request-broker: begin id={} realm=bsp device={}\n",
        id,
        device_index,
    );

    let result = super::dns::resolve_ipv4_for_device(
        device_index,
        host.as_str(),
        super::dns::DnsConfig::for_device(device_index),
    )
    .await
    .map_err(DnsResolveError::from);
    let status = if result.is_ok() { "ok" } else { "error" };

    if completion.complete(result).is_err() {
        crate::log_error!(target: "net";
            "dns-request-broker: duplicate completion id={}\n",
            id,
        );
    } else {
        crate::log_info!(target: "net";
            "dns-request-broker: done id={} status={}\n",
            id,
            status,
        );
    }
}

#[embassy_executor::task]
pub async fn service_task() {
    BROKER_ONLINE.store(true, Ordering::Release);
    crate::log_info!(target: "net";
        "dns-request-broker: online realm=bsp callers=background-carrier-lanes wait=parked-no-executor-poll\n"
    );

    loop {
        // Drop the queue lock before entering the network future so other AP
        // carriers can enqueue while a lookup is in flight.
        let request = BROKER_REQUESTS.lock().pop_front();
        match request {
            Some(request) => process_request(request).await,
            // Bound the lost-notification race between checking the queue and
            // registering the waiter. A submitted request is delayed by at
            // most one tick rather than being stranded indefinitely.
            None => {
                let _ = BROKER_WAIT.wait_for_event_timeout(1).await;
            }
        }
    }
}
