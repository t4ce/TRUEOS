extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::String as HString;
use spin::Mutex;

const MAX_PENDING_REQUESTS: usize = 32;
const MAX_STATUS_ENTRIES: usize = 64;

static NEXT_BROWSER_NET_OP_ID: AtomicU32 = AtomicU32::new(1);
static BROWSER_NET_QUEUE: Mutex<VecDeque<BrowserNetRequest>> = Mutex::new(VecDeque::new());
static BROWSER_NET_STATUS: Mutex<VecDeque<BrowserNetStatus>> = Mutex::new(VecDeque::new());
static BROWSER_NET_LATEST: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());

#[derive(Clone, Debug)]
struct BrowserNetRequest {
    op_id: u32,
    browser_instance_id: u32,
    url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserNetState {
    Queued,
    Loading,
    Succeeded,
    Failed,
    Superseded,
}

impl BrowserNetState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Loading => "loading",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Superseded => "superseded",
        }
    }
}

#[derive(Clone, Debug)]
pub struct BrowserNetStatus {
    pub op_id: u32,
    pub browser_instance_id: u32,
    pub url: String,
    pub state: BrowserNetState,
    pub delivered: bool,
    pub bytes: usize,
    pub error: Option<String>,
}

impl BrowserNetStatus {
    fn new(op_id: u32, browser_instance_id: u32, url: String) -> Self {
        Self {
            op_id,
            browser_instance_id,
            url,
            state: BrowserNetState::Queued,
            delivered: false,
            bytes: 0,
            error: None,
        }
    }
}

fn normalize_browser_instance_id(instance_id: u32) -> u32 {
    if instance_id == 0 {
        trueos_qjs::browser_task::PRIMARY_BROWSER_INSTANCE_ID
    } else {
        instance_id
    }
}

fn upsert_latest_op(browser_instance_id: u32, op_id: u32) {
    let mut latest = BROWSER_NET_LATEST.lock();
    if let Some(entry) = latest
        .iter_mut()
        .find(|entry| entry.0 == browser_instance_id)
    {
        entry.1 = op_id;
        return;
    }
    latest.push((browser_instance_id, op_id));
}

fn latest_op_for_browser(browser_instance_id: u32) -> u32 {
    BROWSER_NET_LATEST
        .lock()
        .iter()
        .find(|entry| entry.0 == browser_instance_id)
        .map(|entry| entry.1)
        .unwrap_or(0)
}

fn push_status(status: BrowserNetStatus) {
    let mut statuses = BROWSER_NET_STATUS.lock();
    if let Some(existing) = statuses
        .iter_mut()
        .find(|entry| entry.op_id == status.op_id)
    {
        *existing = status;
        return;
    }
    if statuses.len() >= MAX_STATUS_ENTRIES {
        let _ = statuses.pop_front();
    }
    statuses.push_back(status);
}

fn update_status(op_id: u32, update: impl FnOnce(&mut BrowserNetStatus)) {
    let mut statuses = BROWSER_NET_STATUS.lock();
    if let Some(entry) = statuses.iter_mut().find(|entry| entry.op_id == op_id) {
        update(entry);
    }
}

fn pop_request() -> Option<BrowserNetRequest> {
    BROWSER_NET_QUEUE.lock().pop_front()
}

pub fn submit_navigation(browser_instance_id: u32, url: &str) -> u32 {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return 0;
    }

    let browser_instance_id = normalize_browser_instance_id(browser_instance_id);
    let op_id = NEXT_BROWSER_NET_OP_ID.fetch_add(1, Ordering::Relaxed);
    let request = BrowserNetRequest {
        op_id,
        browser_instance_id,
        url: String::from(trimmed),
    };

    {
        let mut queue = BROWSER_NET_QUEUE.lock();
        if queue.len() >= MAX_PENDING_REQUESTS {
            return 0;
        }
        queue.push_back(request.clone());
    }

    upsert_latest_op(browser_instance_id, op_id);
    push_status(BrowserNetStatus::new(
        op_id,
        browser_instance_id,
        request.url.clone(),
    ));
    op_id
}

pub fn status(op_id: u32) -> Option<BrowserNetStatus> {
    BROWSER_NET_STATUS
        .lock()
        .iter()
        .find(|entry| entry.op_id == op_id)
        .cloned()
}

#[embassy_executor::task]
pub async fn browser_net_task() {
    loop {
        let Some(request) = pop_request() else {
            Timer::after(EmbassyDuration::from_millis(25)).await;
            continue;
        };

        update_status(request.op_id, |entry| {
            entry.state = BrowserNetState::Loading;
            entry.error = None;
            entry.delivered = false;
            entry.bytes = 0;
        });

        let mut url: HString<256> = HString::new();
        if url.push_str(request.url.as_str()).is_err() {
            update_status(request.op_id, |entry| {
                entry.state = BrowserNetState::Failed;
                entry.error = Some(String::from("url too long"));
            });
            continue;
        }

        match crate::tst_html::fetch_html_best_effort(url).await {
            Ok(html) => {
                let is_latest = latest_op_for_browser(request.browser_instance_id) == request.op_id;
                let delivered = if is_latest {
                    trueos_qjs::browser_task::queue_set_html_with_url_for_browser(
                        request.browser_instance_id,
                        html.clone(),
                        Some(request.url.clone()),
                    )
                } else {
                    false
                };
                update_status(request.op_id, |entry| {
                    entry.state = if is_latest {
                        BrowserNetState::Succeeded
                    } else {
                        BrowserNetState::Superseded
                    };
                    entry.bytes = html.len();
                    entry.delivered = delivered;
                    entry.error = None;
                });
                let preview_len = core::cmp::min(10, html.len());
                let preview = &html.as_str()[..preview_len];
                crate::log!(
                    "browser-net: op={} browser={} state={} delivered={} bytes={} preview='{}'\n",
                    request.op_id,
                    request.browser_instance_id,
                    if is_latest { "ok" } else { "superseded" },
                    if delivered { 1 } else { 0 },
                    html.len(),
                    preview
                );
            }
            Err(err) => {
                update_status(request.op_id, |entry| {
                    entry.state = BrowserNetState::Failed;
                    entry.error = Some(String::from(err));
                });
                crate::log!(
                    "browser-net: op={} browser={} state=failed err={}\n",
                    request.op_id,
                    request.browser_instance_id,
                    err
                );
            }
        }
    }
}
