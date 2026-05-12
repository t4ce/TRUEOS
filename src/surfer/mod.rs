extern crate alloc;

pub(crate) mod html_demo;
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

const TRUESURFER_FACTORY_BOOT_COUNT: u32 = 0;
const BROWSER_ASSET_FETCH_POLL_MS: u64 = 8;

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct HostedBrowserDirtyMask {
    pub content: u64,
    pub interactive: u64,
}

#[derive(Copy, Clone, Debug, Default)]
struct HostedBrowserFactorySignalState {
    latest_mask: u64,
    seq: u32,
    taken_seq: u32,
}

struct TruesurferFactory {
    next_instance_id: u32,
    spawned_mask: u64,
}

impl TruesurferFactory {
    const fn new() -> Self {
        Self {
            next_instance_id: 1,
            spawned_mask: 0,
        }
    }

    fn next_instance_id(&self) -> Option<u32> {
        if self.next_instance_id > MAX_BROWSER_INSTANCE_ID {
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

    fn spawned_mask(&self) -> u64 {
        self.spawned_mask
    }
}

static TRUESURFER_FACTORY: Mutex<TruesurferFactory> = Mutex::new(TruesurferFactory::new());
static HOSTED_BROWSER_FACTORY_SIGNAL: Mutex<HostedBrowserFactorySignalState> =
    Mutex::new(HostedBrowserFactorySignalState {
        latest_mask: 0,
        seq: 0,
        taken_seq: 0,
    });
static HOSTED_BROWSER_DIRTY_CONTENT_MASK: AtomicU64 = AtomicU64::new(0);
static HOSTED_BROWSER_DIRTY_INTERACTIVE_MASK: AtomicU64 = AtomicU64::new(0);

pub(crate) const fn truesurfer_factory_boot_count() -> u32 {
    if TRUESURFER_FACTORY_BOOT_COUNT > MAX_BROWSER_INSTANCE_ID {
        MAX_BROWSER_INSTANCE_ID
    } else {
        TRUESURFER_FACTORY_BOOT_COUNT
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

pub(crate) fn signal_hosted_browser_factory_mask(mask: u64) {
    let mut signal = HOSTED_BROWSER_FACTORY_SIGNAL.lock();
    signal.latest_mask = mask;
    signal.seq = signal.seq.wrapping_add(1).max(1);
}

pub(crate) fn take_hosted_browser_factory_mask() -> Option<u64> {
    let mut signal = HOSTED_BROWSER_FACTORY_SIGNAL.lock();
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

fn spawn_truesurfer_on_worker(browser_instance_id: u32) -> Result<bool, SpawnError> {
    let Some(worker_spawner) = crate::workers::pick_background_spawner() else {
        return Ok(false);
    };
    let token = trueos_qjs::browser_task::truesurfer_task(browser_instance_id)?;
    worker_spawner.spawn(token);
    Ok(true)
}

pub(crate) fn spawn_html_demo(spawner: Spawner) -> Result<bool, SpawnError> {
    let token = html_demo::html_demo_task()?;
    spawner.spawn(token);
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

    let mut factory = TRUESURFER_FACTORY.lock();
    let mut spawned_any = false;

    for _ in 0..requested {
        let Some(browser_instance_id) = factory.next_instance_id() else {
            break;
        };

        match spawn_truesurfer_on_worker(browser_instance_id) {
            Ok(true) => {
                factory.mark_spawned(browser_instance_id);
                signal_hosted_browser_factory_mask(factory.spawned_mask());
                spawned_any = true;
                crate::log!(
                    "truesurfer-factory: spawned browser_instance_id={} mask={:#x} remaining={}\n",
                    browser_instance_id,
                    factory.spawned_mask(),
                    MAX_BROWSER_INSTANCE_ID.saturating_sub(browser_instance_id)
                );
            }
            Ok(false) => break,
            Err(e) => {
                if !spawned_any {
                    return Err(e);
                }
                crate::log!(
                    "truesurfer-factory: spawn failed browser_instance_id={} err={:?}\n",
                    browser_instance_id,
                    e
                );
                break;
            }
        }
    }

    Ok(spawned_any)
}

pub(crate) fn spawn_truesurfer_factory(spawner: Spawner) -> Result<bool, SpawnError> {
    spawn_truesurfer_batch(spawner, truesurfer_factory_boot_count())
}

pub(crate) fn spawn_truesurfer_tab_with_html() -> Option<u32> {
    let mut factory = TRUESURFER_FACTORY.lock();
    let browser_instance_id = factory.next_instance_id()?;

    match spawn_truesurfer_on_worker(browser_instance_id) {
        Ok(true) => {
            factory.mark_spawned(browser_instance_id);
            signal_hosted_browser_factory_mask(factory.spawned_mask());
            crate::log!(
                "truesurfer-factory: handoff-spawned browser_instance_id={} mask={:#x} remaining={}\n",
                browser_instance_id,
                factory.spawned_mask(),
                MAX_BROWSER_INSTANCE_ID.saturating_sub(browser_instance_id)
            );
            Some(browser_instance_id)
        }
        Ok(false) | Err(_) => {
            crate::log!(
                "truesurfer-factory: handoff-spawn skipped browser_instance_id={}\n",
                browser_instance_id
            );
            None
        }
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
