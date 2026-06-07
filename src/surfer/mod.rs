extern crate alloc;

pub(crate) mod html_shack;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use embassy_executor::{SpawnError, Spawner};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

pub(crate) type HostedSurfaceState = trueos_qjs::browser_task::HostedBrowserSurfaceState;
pub(crate) type HostedInteractiveState = trueos_qjs::browser_task::HostedBrowserInteractiveState;
pub(crate) type HostedGadgetSnapshot = trueos_qjs::browser_task::HostedBrowserGadgetSnapshot;
pub(crate) type HostedKeyboardEvent = trueos_qjs::browser_task::HostedKeyboardEvent;
pub(crate) type ParseResult = trueos_qjs::browser_task::ParseResult;

pub(crate) const MAX_BROWSER_INSTANCE_ID: u32 = trueos_qjs::browser_task::MAX_BROWSER_INSTANCE_ID;
pub(crate) const HOSTED_KEYBOARD_MOD_SHIFT: u8 =
    trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_SHIFT;
pub(crate) const HOSTED_KEYBOARD_MOD_CTRL: u8 = trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_CTRL;
pub(crate) const HOSTED_KEYBOARD_MOD_ALT: u8 = trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_ALT;
pub(crate) const HOSTED_KEYBOARD_MOD_META: u8 = trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_META;

pub(crate) const HOSTED_BROWSER_DIRTY_CONTENT: u32 = 1 << 0;
pub(crate) const HOSTED_BROWSER_DIRTY_INTERACTIVE: u32 = 1 << 1;

pub(crate) const BROWSER_PARSE_HOST_POOL_SIZE: u32 =
    trueos_qjs::browser_task::TRUESURFER_TASK_POOL_SIZE as u32;
const BROWSER_PARSE_HOST_LIMIT: u32 = if BROWSER_PARSE_HOST_POOL_SIZE < MAX_BROWSER_INSTANCE_ID {
    BROWSER_PARSE_HOST_POOL_SIZE
} else {
    MAX_BROWSER_INSTANCE_ID
};
const BROWSER_PARSE_POOL_BOOT_COUNT: u32 = 0;
const BROWSER_ASSET_FETCH_POLL_MS: u64 = 8;

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct HostedBrowserDirtyMask {
    pub content: u64,
    pub interactive: u64,
}

#[derive(Copy, Clone, Debug, Default)]
struct HostedBrowserParsePoolSignalState {
    latest_mask: u64,
    seq: u32,
    taken_seq: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserParseQueueTicket {
    pub browser_instance_id: u32,
    pub queued: bool,
}

struct BrowserParsePool {
    next_instance_id: u32,
    spawned_mask: u64,
    queued_html_count: u32,
}

impl BrowserParsePool {
    const fn new() -> Self {
        Self {
            next_instance_id: 1,
            spawned_mask: 0,
            queued_html_count: 0,
        }
    }

    fn next_instance_id(&self) -> Option<u32> {
        if self.next_instance_id > BROWSER_PARSE_HOST_LIMIT {
            None
        } else {
            Some(self.next_instance_id)
        }
    }

    fn mark_spawned(&mut self, browser_instance_id: u32) {
        self.next_instance_id = self.next_instance_id.saturating_add(1);
        let bit = 1u64 << browser_instance_id.saturating_sub(1);
        self.spawned_mask |= bit;
    }

    fn mark_html_queued(&mut self) -> u32 {
        self.queued_html_count = self.queued_html_count.saturating_add(1);
        self.queued_html_count
    }

    fn spawned_mask(&self) -> u64 {
        self.spawned_mask
    }
}

static BROWSER_PARSE_POOL: Mutex<BrowserParsePool> = Mutex::new(BrowserParsePool::new());
static HOSTED_BROWSER_PARSE_POOL_SIGNAL: Mutex<HostedBrowserParsePoolSignalState> =
    Mutex::new(HostedBrowserParsePoolSignalState {
        latest_mask: 0,
        seq: 0,
        taken_seq: 0,
    });
static HOSTED_BROWSER_DIRTY_CONTENT_MASK: AtomicU64 = AtomicU64::new(0);
static HOSTED_BROWSER_DIRTY_INTERACTIVE_MASK: AtomicU64 = AtomicU64::new(0);

pub(crate) const fn browser_parse_pool_boot_count() -> u32 {
    if BROWSER_PARSE_POOL_BOOT_COUNT > BROWSER_PARSE_HOST_LIMIT {
        BROWSER_PARSE_HOST_LIMIT
    } else {
        BROWSER_PARSE_POOL_BOOT_COUNT
    }
}

pub(crate) fn boot_browser_instance_ids() -> &'static [u32] {
    &trueos_qjs::browser_task::BOOT_BROWSER_INSTANCE_IDS
}

#[inline]
fn hosted_browser_bit(content_id: u32) -> Option<u64> {
    if !(1..=64).contains(&content_id) {
        return None;
    }
    Some(1u64 << content_id.saturating_sub(1))
}

pub(crate) fn signal_hosted_browser_dirty(content_id: u32, flags: u32) {
    let Some(bit) = hosted_browser_bit(content_id) else {
        return;
    };
    if (flags & HOSTED_BROWSER_DIRTY_CONTENT) != 0 {
        HOSTED_BROWSER_DIRTY_CONTENT_MASK.fetch_or(bit, Ordering::Release);
    }
    if (flags & HOSTED_BROWSER_DIRTY_INTERACTIVE) != 0 {
        HOSTED_BROWSER_DIRTY_INTERACTIVE_MASK.fetch_or(bit, Ordering::Release);
    }
}

pub(crate) fn take_hosted_browser_dirty_mask() -> HostedBrowserDirtyMask {
    HostedBrowserDirtyMask {
        content: HOSTED_BROWSER_DIRTY_CONTENT_MASK.swap(0, Ordering::AcqRel),
        interactive: HOSTED_BROWSER_DIRTY_INTERACTIVE_MASK.swap(0, Ordering::AcqRel),
    }
}

pub(crate) fn signal_hosted_browser_parse_pool_mask(mask: u64) {
    let mut signal = HOSTED_BROWSER_PARSE_POOL_SIGNAL.lock();
    signal.latest_mask = mask;
    signal.seq = signal.seq.wrapping_add(1).max(1);
}

pub(crate) fn take_hosted_browser_parse_pool_mask() -> Option<u64> {
    let mut signal = HOSTED_BROWSER_PARSE_POOL_SIGNAL.lock();
    if signal.seq == signal.taken_seq {
        return None;
    }
    signal.taken_seq = signal.seq;
    Some(signal.latest_mask)
}

pub(crate) fn hosted_interactive_seq(browser_instance_id: u32) -> u32 {
    trueos_qjs::browser_task::hosted_interactive_seq_for_browser(browser_instance_id)
}

pub(crate) fn hosted_surface_state(browser_instance_id: u32) -> HostedSurfaceState {
    trueos_qjs::browser_task::hosted_surface_state_for_browser(browser_instance_id)
}

pub(crate) fn hosted_interactive_state(browser_instance_id: u32) -> HostedInteractiveState {
    trueos_qjs::browser_task::hosted_interactive_state_for_browser(browser_instance_id)
}

pub(crate) fn hosted_gadget_snapshot(browser_instance_id: u32) -> HostedGadgetSnapshot {
    trueos_qjs::browser_task::hosted_gadget_snapshot_for_browser(browser_instance_id)
}

pub(crate) fn set_hosted_viewport(
    browser_instance_id: u32,
    viewport_width: u32,
    viewport_height: u32,
    content_x: i32,
    content_y: i32,
    content_width: u32,
    content_height: u32,
) -> bool {
    trueos_qjs::browser_task::set_hosted_viewport_for_browser(
        browser_instance_id,
        viewport_width,
        viewport_height,
        content_x,
        content_y,
        content_width,
        content_height,
    )
}

pub(crate) fn set_hosted_scroll(browser_instance_id: u32, scroll_x: u32, scroll_y: u32) -> bool {
    trueos_qjs::browser_task::set_hosted_scroll_for_browser(browser_instance_id, scroll_x, scroll_y)
}

pub(crate) fn queue_hosted_keyboard_events(
    browser_instance_id: u32,
    events: &[HostedKeyboardEvent],
) -> bool {
    let window_id = browser_window_id_for_instance(browser_instance_id);
    if window_id == 0 {
        return false;
    }
    trueos_qjs::browser_task::queue_hosted_keyboard_events(window_id, events)
}

pub(crate) fn bind_browser_window_to_instance(browser_instance_id: u32, window_id: u32) -> bool {
    trueos_qjs::browser_task::bind_browser_window_to_instance(browser_instance_id, window_id)
}

pub(crate) fn primary_browser_window_id() -> u32 {
    browser_window_id_for_instance(1)
}

pub(crate) fn browser_window_id_for_instance(browser_instance_id: u32) -> u32 {
    trueos_qjs::browser_task::browser_window_id_for_instance(browser_instance_id)
}

pub(crate) fn set_browser_render_target_tex_id(browser_instance_id: u32, tex_id: u32) -> bool {
    trueos_qjs::browser_task::set_browser_render_target_tex_id_for_browser(
        browser_instance_id,
        tex_id,
    )
}

pub(crate) fn render_tex_id_for_browser_instance(browser_instance_id: u32) -> u32 {
    trueos_qjs::browser_task::render_tex_id_for_browser_instance(browser_instance_id)
}

pub(crate) fn latest_parse_result_for_browser(browser_instance_id: u32) -> Option<ParseResult> {
    trueos_qjs::browser_task::latest_parse_result_for_browser(browser_instance_id)
}

pub(crate) async fn queue_html_for_browser(
    browser_instance_id: u32,
    html: String,
    url: Option<String>,
) -> bool {
    trueos_qjs::browser_task::queue_set_html_with_url_for_browser(browser_instance_id, html, url)
        .await
}

pub(crate) async fn queue_html_parse(
    html: String,
    url: Option<String>,
) -> Option<BrowserParseQueueTicket> {
    let browser_instance_id = spawn_parse_host_for_html_queue()?;
    let queued = queue_html_for_browser(browser_instance_id, html, url).await;
    if queued {
        let queued_total = BROWSER_PARSE_POOL.lock().mark_html_queued();
        crate::log!(
            "truesurfer-parse-queue: queued browser_instance_id={} queued_total={}\n",
            browser_instance_id,
            queued_total
        );
    } else {
        crate::log!(
            "truesurfer-parse-queue: enqueue failed browser_instance_id={}\n",
            browser_instance_id
        );
    }
    Some(BrowserParseQueueTicket {
        browser_instance_id,
        queued,
    })
}

fn spawn_truesurfer_on_worker(browser_instance_id: u32) -> Result<bool, SpawnError> {
    let Some(worker_spawner) = crate::workers::pick_background_spawner() else {
        return Ok(false);
    };
    let token = trueos_qjs::browser_task::truesurfer_task(browser_instance_id)?;
    worker_spawner.spawn(token);
    Ok(true)
}

pub(crate) fn spawn_html_fetch_service(spawner: Spawner) -> Result<bool, SpawnError> {
    let token = html_shack::html_fetch_service()?;
    spawner.spawn(token);
    Ok(true)
}

pub(crate) fn spawn_truesurfer_batch(
    _spawner: Spawner,
    requested: u32,
) -> Result<bool, SpawnError> {
    if requested == 0 {
        return Ok(false);
    }

    let mut parse_pool = BROWSER_PARSE_POOL.lock();
    let mut spawned_any = false;

    for _ in 0..requested {
        match spawn_next_parse_host_locked(&mut parse_pool, "batch-spawned") {
            Ok(Some(_browser_instance_id)) => spawned_any = true,
            Ok(None) => break,
            Err(_) if spawned_any => break,
            Err(e) => return Err(e),
        }
    }

    Ok(spawned_any)
}

pub(crate) fn spawn_truesurfer_parse_pool(spawner: Spawner) -> Result<bool, SpawnError> {
    spawn_truesurfer_batch(spawner, browser_parse_pool_boot_count())
}

fn spawn_next_parse_host_locked(
    parse_pool: &mut BrowserParsePool,
    reason: &str,
) -> Result<Option<u32>, SpawnError> {
    let Some(browser_instance_id) = parse_pool.next_instance_id() else {
        return Ok(None);
    };

    match spawn_truesurfer_on_worker(browser_instance_id) {
        Ok(true) => {
            parse_pool.mark_spawned(browser_instance_id);
            signal_hosted_browser_parse_pool_mask(parse_pool.spawned_mask());
            crate::log!(
                "truesurfer-parse-pool: {} browser_instance_id={} mask={:#x} remaining={}\n",
                reason,
                browser_instance_id,
                parse_pool.spawned_mask(),
                BROWSER_PARSE_HOST_LIMIT.saturating_sub(browser_instance_id)
            );
            Ok(Some(browser_instance_id))
        }
        Ok(false) => {
            crate::log!(
                "truesurfer-parse-pool: {} skipped browser_instance_id={}\n",
                reason,
                browser_instance_id,
            );
            Ok(None)
        }
        Err(e) => {
            crate::log!(
                "truesurfer-parse-pool: {} failed browser_instance_id={} err={:?}\n",
                reason,
                browser_instance_id,
                e
            );
            Err(e)
        }
    }
}

fn spawn_parse_host_for_html_queue() -> Option<u32> {
    let mut parse_pool = BROWSER_PARSE_POOL.lock();
    match spawn_next_parse_host_locked(&mut parse_pool, "html-queue-spawned") {
        Ok(Some(browser_instance_id)) => Some(browser_instance_id),
        Ok(None) | Err(_) => None,
    }
}

fn browser_asset_fetch_start(url: &str) -> Result<u32, i32> {
    if url.starts_with('/') {
        trueos_qjs::async_fs::start_read_file(url.as_bytes())
    } else {
        trueos_qjs::async_fs::start_net_fetch_bytes(url.as_bytes())
    }
}

pub(crate) async fn fetch_browser_asset_bytes(url: &str) -> Result<Vec<u8>, i32> {
    let op_id = browser_asset_fetch_start(url)?;
    loop {
        let rc_or_done = trueos_qjs::async_fs::result_len(op_id);
        if rc_or_done == trueos_qjs::async_fs::FS_ERR_NOT_FOUND as isize {
            Timer::after(EmbassyDuration::from_millis(BROWSER_ASSET_FETCH_POLL_MS)).await;
            continue;
        }
        if rc_or_done < 0 {
            let _ = trueos_qjs::async_fs::discard(op_id);
            return Err(rc_or_done as i32);
        }
        let mut bytes = vec![0u8; rc_or_done as usize];
        let got = trueos_qjs::async_fs::read_result(op_id, bytes.as_mut_ptr(), bytes.len());
        if got < 0 {
            let _ = trueos_qjs::async_fs::discard(op_id);
            return Err(got as i32);
        }
        bytes.truncate(got as usize);
        return Ok(bytes);
    }
}
