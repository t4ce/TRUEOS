//! The original boot typing exercise, using the embedded Paper Blueprint.
//! Keep cursor and keyboard input on Lilly's existing mediated HUT combo.

use alloc::{boxed::Box, string::String};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};

use crate::r::services::keyboard_control_service::{
    self as keyboard, KeyboardControlDevice, KeyboardControlPrincipal,
};
use crate::ui4::{WindowOwner, WindowSnapshot};

const GREETING: &str = "Hello from TrueOS §";
// 19 x 9 whole 10 mm cells, a 19:9 frame. The greeting fills row zero;
// wrapping leaves its caret at row one, ready to erase every scalar.
const SCRIPT: &str = "mode gridpaper\nsize 190x90\nstep 2\nwrap on\n";
const POLL_MS: u64 = 16;
const INPUT_SETTLE_MS: u64 = 48;

static CANCELLED: AtomicBool = AtomicBool::new(false);
static FINISHED: AtomicBool = AtomicBool::new(false);
static INSTANCE: Mutex<Option<(u8, u64)>> = Mutex::new(None);

struct DemoLaunch;
impl crate::hv::launcher::LaunchObserver for DemoLaunch {
    fn is_live(&self) -> bool {
        !CANCELLED.load(Ordering::Acquire)
    }
    fn started(&self, vm: u8) {
        *INSTANCE.lock() = crate::hv::vm_run_generation(vm).map(|run| (vm, run));
    }
    fn finished(
        &self,
        target: &crate::shell3::MatrixTarget,
        _vm: Option<u8>,
        error: Option<&str>,
        _reason: Option<&str>,
    ) {
        if let Some(error) = error {
            crate::log_warn!(target: "gfx"; "trueos-spirit: Paper boot launch failed error={}\n", error);
        }
        *INSTANCE.lock() = None;
        FINISHED.store(true, Ordering::Release);
        crate::hv::matrix::discard(target);
    }
}

/// Run before the normal window selector consumes first-presentation events.
/// This also restores Lilly's keyboard registration for the existing Dobby API.
pub(super) async fn run() {
    let result = trueos_time::with_timeout(Duration::from_secs(30), request_keyboard()).await;
    let Ok(keyboard) = result else {
        crate::log_warn!(target: "gfx"; "trueos-spirit: Paper boot skipped reason=keyboard-unavailable\n");
        return;
    };
    super::dobby_ui::register_lilly_keyboard(keyboard);
    super::dobby_ui::acquire_response_io(keyboard).await;
    let mut window = None;
    let mut target = None;
    let result = trueos_time::with_timeout(
        Duration::from_secs(45),
        exercise(keyboard, &mut window, &mut target),
    )
    .await;
    // Cancel a pending launch as well as closing a live frame. The observer
    // owns and retires the exact Matrix lifetime, never an unrelated Paper VM.
    if !matches!(result, Ok(Ok(()))) {
        if let Some(window) = window {
            let _ = crate::ui4::close_window(window.owner, window.id);
        }
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while target.is_some() && !FINISHED.load(Ordering::Acquire) && Instant::now() < deadline {
        Timer::after(Duration::from_millis(POLL_MS)).await;
    }
    CANCELLED.store(true, Ordering::Release);
    if let Some(target) = target {
        crate::hv::matrix::discard(&target);
    }
    // Clear any unfinished keyboard program before handing the combo to Dobby.
    let _ = keyboard::submit_command(
        KeyboardControlPrincipal::Kernel,
        keyboard.handle,
        keyboard::KeyboardControlCommand {
            opcode: keyboard::KEYBOARD_CONTROL_OPCODE_WAIT,
            flags: keyboard::KEYBOARD_CONTROL_FLAG_CLEAR_QUEUE,
            duration_ms: 48,
            ..Default::default()
        },
    );
    let _ = trueos_time::with_timeout(Duration::from_secs(1), wait_keyboard(keyboard)).await;
    super::dobby_ui::release_response_io(keyboard);
    match result {
        Ok(Ok(())) => crate::log_info!(target: "gfx";
            "trueos-spirit: Paper boot complete scalars={} backspaces={} path=lilly-cursor+paired-vkeyboard->ui4->paper frame=19x9 action=close\n", GREETING.chars().count(), GREETING.chars().count()),
        result => crate::log_warn!(target: "gfx";
            "trueos-spirit: Paper boot failed result={:?} action=close-owned-frame\n", result),
    }
}

async fn request_keyboard() -> KeyboardControlDevice {
    loop {
        if let Ok(device) =
            keyboard::request_keyboard(KeyboardControlPrincipal::Kernel, "Spirit/Lilly chat")
        {
            if super::lilly_cursor::bind_keyboard(device.slot_id).is_ok() {
                return device;
            }
            let _ = keyboard::release_keyboard(KeyboardControlPrincipal::Kernel, device.handle);
        }
        Timer::after(Duration::from_millis(250)).await;
    }
}

async fn exercise(
    keyboard: KeyboardControlDevice,
    owned: &mut Option<WindowSnapshot>,
    target: &mut Option<crate::shell3::MatrixTarget>,
) -> Result<(), &'static str> {
    // AppDB and the ordinary launcher provide the same early admission as other
    // built-ins. There is no catalog/network fetch and no kernel paper renderer.
    let bytes = loop {
        match crate::app_db::get("paper.bp") {
            Ok(Some(bytes)) => break bytes,
            Ok(None) => return Err("embedded-paper-missing"),
            Err(_) => Timer::after(Duration::from_millis(50)).await,
        }
    };
    let mut request = crate::hv::launcher::LaunchRequest::new("paper.bp".into(), bytes);
    request.script = Some(SCRIPT.into());
    let receipt = crate::hv::launcher::enqueue(request, Some(Box::new(DemoLaunch)))
        .map_err(|_| "paper-admission")?;
    *target = Some(crate::shell3::MatrixTarget::from_lease(
        crate::shell3::MatrixSlotLease::from_identity(receipt.slot, receipt.lifetime),
    ));
    crate::log_info!(target: "gfx";
        "trueos-spirit: Paper boot queued size_mm=190x90 cells=19x9 wrap=on source=embedded-appdb\n");
    let output = crate::ui4::OutputId::from_slot(0).ok_or("output")?;
    let window = loop {
        if FINISHED.load(Ordering::Acquire) {
            return Err("paper-exited-before-frame");
        }
        let instance = *INSTANCE.lock();
        if let Some((vm, run)) = instance {
            if crate::hv::vm_run_generation(vm) != Some(run) {
                return Err("paper-instance-retired");
            }
            if let Some(window) = crate::ui4::visible_windows_for_output(output)
                .into_iter()
                .find(|window| {
                    window.owner == WindowOwner::Vm(vm)
                        && crate::ui4::window_frame_was_presented(
                            window.owner,
                            window.id,
                            window.publish_serial,
                        )
                })
            {
                *owned = Some(window);
                break window;
            }
        }
        Timer::after(Duration::from_millis(POLL_MS)).await;
    };
    let (screen_width, screen_height) =
        crate::intel::active_scanout_dimensions().ok_or("scanout")?;
    let p = window.placement;
    super::lilly_cursor::queue_window_outline(
        p.x,
        p.y,
        p.width,
        p.height,
        screen_width,
        screen_height,
    )
    .map_err(|_| "outline")?;
    wait_cursor().await?;
    // Compact Paper has no A4 margins. The first cell is 1/19 by 1/9 of
    // the sheet, and its center follows the same snapped pixel bounds.
    super::lilly_cursor::queue_window_approach(
        p.x + (p.width / 19 / 2) as i32,
        p.y + (p.height / 9 / 2) as i32,
        screen_width,
        screen_height,
    )
    .map_err(|_| "cell-zero-approach")?;
    wait_cursor().await?;
    let source = super::lilly_cursor::selection_source().map_err(|_| "cursor-source")?;
    crate::ui4::select_window_for_cursor(source, window.owner, window.id)
        .map_err(|_| "paper-focus")?;
    super::lilly_cursor::queue_primary_click().map_err(|_| "cell-zero-click")?;
    wait_cursor().await?;
    Timer::after(Duration::from_millis(INPUT_SETTLE_MS)).await;
    if crate::ui4::selected_frame_for_source(source)
        != Some(crate::ui4::CursorFrameKey::new(window.owner, window.id))
    {
        return Err("paper-focus-not-latched");
    }
    type_text(keyboard, GREETING).await?;
    wait_presented_after(window).await?;
    crate::log_info!(target: "gfx";
        "trueos-spirit: Paper greeting dispatched window={} text={:?} path=cell-zero-click+paired-vkeyboard dwell_ms=5000\n", window.id.raw(), GREETING);
    Timer::after(Duration::from_secs(5)).await;
    // Sample the current publication before erasing; require another real
    // SURFLIVE before closing, allowing Paper to consume and draw the edits.
    let before_erase = current_window(window).ok_or("paper-closed")?;
    let backspaces: String = GREETING.chars().map(|_| '\u{0008}').collect();
    type_text(keyboard, &backspaces).await?;
    wait_presented_after(before_erase).await?;
    Timer::after(Duration::from_millis(250)).await;
    // Use UI4's normal selected-frame close action. Closing just the broker
    // window leaves a Blueprint running against a revoked render target.
    type_text(keyboard, "\u{001b}").await?;
    Ok(())
}

fn current_window(window: WindowSnapshot) -> Option<WindowSnapshot> {
    crate::ui4::visible_windows_for_output(window.output)
        .into_iter()
        .find(|current| current.owner == window.owner && current.id == window.id)
}

async fn wait_presented_after(before: WindowSnapshot) -> Result<(), &'static str> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let current = current_window(before).ok_or("paper-closed")?;
        if current.publish_serial != before.publish_serial
            && crate::ui4::window_frame_was_presented(
                current.owner,
                current.id,
                current.publish_serial,
            )
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("paper-present-timeout");
        }
        Timer::after(Duration::from_millis(POLL_MS)).await;
    }
}

async fn type_text(keyboard: KeyboardControlDevice, text: &str) -> Result<(), &'static str> {
    keyboard::submit_text(
        KeyboardControlPrincipal::Kernel,
        keyboard.handle,
        text,
        48,
        true,
    )
    .map_err(|_| "keyboard-submit")?;
    wait_keyboard(keyboard).await?;
    Timer::after(Duration::from_millis(INPUT_SETTLE_MS)).await;
    Ok(())
}
async fn wait_keyboard(device: KeyboardControlDevice) -> Result<(), &'static str> {
    loop {
        if keyboard::keyboard_is_idle(KeyboardControlPrincipal::Kernel, device.handle)
            .map_err(|_| "keyboard-status")?
        {
            return Ok(());
        }
        Timer::after(Duration::from_millis(POLL_MS)).await;
    }
}
async fn wait_cursor() -> Result<(), &'static str> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !super::lilly_cursor::window_approach_complete().map_err(|_| "cursor-status")? {
        if Instant::now() >= deadline {
            return Err("cursor-timeout");
        }
        Timer::after(Duration::from_millis(POLL_MS)).await;
    }
    Timer::after(Duration::from_millis(INPUT_SETTLE_MS)).await;
    Ok(())
}
