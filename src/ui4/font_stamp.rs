//! UI4 ownership for the shell-visible kernel font stamper.
//!
//! Ten logical slots may remain visible, but font production is deliberately
//! serialized through the kernel's existing Render/GPGPU GuC lanes. Every
//! slot owns one broker session so focused Escape teardown cannot affect a
//! sibling stamp.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration, Timer};
use spin::Mutex;

use super::{
    DamageRect, FrameBuffering, FrameCadence, FrameContent, FrameHandle, FramePoolError, FrameSpec,
    OutputId, PremultipliedRgba8, ScanoutFormat, Ui4InputEvent, WindowCreate, WindowId,
    WindowInteraction, WindowOwner, WindowPlacement, WindowPlane, WindowSessionId,
    acquire_frame_buffer, begin_window_session, cancel_frame_buffer, create_frame, create_window,
    destroy_frame, finish_window_session, gpgpu_rgba_surface, publish_gpu_font_frame_buffer,
    publish_window_frame, replace_window_frame, set_window_placement, take_owner_input_events,
};
use crate::intel::gpu_font::{
    GpuFontFace, GpuFontRgba, GpuFontTextRequest, GpuFontUi4Stamp, prepare_text_stamp_for_ui4,
    render_prepared_text_stamp_to_ui4,
};

pub(crate) const MAX_FONT_STAMP_SLOTS: usize = 10;
const FONT_OWNER: WindowOwner = WindowOwner::FONT_STAMP;
const FONT_PLANE_SLOT: u8 = 1;
const FONT_Z_BASE: i32 = 70;
const FONT_INPUT_POLL_MS: u64 = 8;
const FONT_CASCADE_PX: i32 = 18;

#[derive(Copy, Clone)]
struct FontSlot {
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
    width: u32,
    height: u32,
    placement: WindowPlacement,
    request_serial: u64,
}

struct FontStampState {
    slots: [Option<FontSlot>; MAX_FONT_STAMP_SLOTS],
    next_reuse: usize,
    next_request_serial: u64,
    retired_frames: Vec<FrameHandle>,
    quarantined_frames: Vec<FrameHandle>,
}

impl FontStampState {
    const fn new() -> Self {
        Self {
            slots: [None; MAX_FONT_STAMP_SLOTS],
            next_reuse: 0,
            next_request_serial: 0,
            retired_frames: Vec::new(),
            quarantined_frames: Vec::new(),
        }
    }

    fn reserve_logical_slot(&mut self) -> (usize, Option<FontSlot>, u64) {
        let index = self
            .slots
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.next_reuse);
        self.next_reuse = (index + 1) % MAX_FONT_STAMP_SLOTS;
        self.next_request_serial = self.next_request_serial.wrapping_add(1).max(1);
        (index, self.slots[index], self.next_request_serial)
    }

    fn queue_retirement(&mut self, frame: FrameHandle) {
        if !self.retired_frames.contains(&frame) {
            self.retired_frames.push(frame);
        }
    }

    fn quarantine(&mut self, frame: FrameHandle) {
        if !self.quarantined_frames.contains(&frame) {
            self.quarantined_frames.push(frame);
        }
    }
}

static FONT_STAMPS: Mutex<FontStampState> = Mutex::new(FontStampState::new());
static FONT_PRESENT_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

struct PresentGuard;

impl PresentGuard {
    fn acquire() -> Result<Self, &'static str> {
        FONT_PRESENT_IN_FLIGHT
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| "font-ui4-producer-busy")
    }
}

impl Drop for PresentGuard {
    fn drop(&mut self) {
        FONT_PRESENT_IN_FLIGHT.store(false, Ordering::Release);
    }
}

pub(crate) struct FontStampPresentation {
    pub(crate) slot: usize,
    pub(crate) frame: FrameHandle,
    pub(crate) window: WindowId,
    pub(crate) request_serial: u64,
    pub(crate) reused_slot: bool,
    pub(crate) reused_frame: bool,
    pub(crate) stamp: GpuFontUi4Stamp,
}

pub(crate) fn present_font_stamp(
    request: GpuFontTextRequest<'_>,
    font: GpuFontFace,
    size_percent: u32,
    rgba: GpuFontRgba,
) -> Result<FontStampPresentation, &'static str> {
    let _guard = PresentGuard::acquire()?;
    let prepared = prepare_text_stamp_for_ui4(request, font, size_percent, rgba)?;
    let width = prepared.width();
    let height = prepared.height();
    let (slot_index, existing, request_serial) = FONT_STAMPS.lock().reserve_logical_slot();

    let mut candidate_is_new =
        existing.is_none_or(|slot| slot.width != width || slot.height != height);
    let mut frame = if candidate_is_new {
        create_font_frame(width, height)?
    } else {
        existing.expect("matching font slot checked").frame
    };
    let lease = match acquire_frame_buffer(frame) {
        Ok(lease) => lease,
        Err(FramePoolError::Busy) if !candidate_is_new => {
            // The existing double buffer is still wholly owned by display.
            // Preserve it and render the replacement before atomically
            // switching the broker window to the new frame.
            candidate_is_new = true;
            frame = create_font_frame(width, height)?;
            acquire_frame_buffer(frame).map_err(|_| "font-ui4-frame-acquire")?
        }
        Err(_) => {
            if candidate_is_new {
                let _ = destroy_frame(frame);
            }
            return Err("font-ui4-frame-acquire");
        }
    };
    let destination = match gpgpu_rgba_surface(lease) {
        Ok(destination) => destination,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            if candidate_is_new {
                let _ = destroy_frame(frame);
            }
            return Err("font-ui4-gpu-surface");
        }
    };

    let stamp = match render_prepared_text_stamp_to_ui4(prepared, destination) {
        Ok(stamp) => stamp,
        Err(error) => {
            quarantine_failed_destination(slot_index, existing, frame, candidate_is_new, error);
            return Err(error);
        }
    };
    if let Err(error) = publish_gpu_font_frame_buffer(lease, stamp.release) {
        // The exact producer fence retired, so cancelling is safe even though
        // publication validation rejected the lease.
        let _ = cancel_frame_buffer(lease);
        if candidate_is_new {
            let _ = destroy_frame(frame);
        }
        crate::log_error!(
            target: "ui4/font-stamp";
            "font stamp publish rejected slot={} frame={} error={:?}\n",
            slot_index,
            frame.raw(),
            error,
        );
        return Err("font-ui4-frame-publish");
    }

    let placement = font_placement(slot_index, &stamp);
    let (window, reused_frame) = match existing {
        None => {
            let session = match begin_window_session(FONT_OWNER) {
                Ok(session) => session,
                Err(_) => {
                    let _ = destroy_frame(frame);
                    return Err("font-ui4-session-create");
                }
            };
            let output = OutputId::from_slot(0).ok_or("font-ui4-output")?;
            let window = match create_window(WindowCreate {
                owner: FONT_OWNER,
                session,
                frame,
                output,
                plane: WindowPlane::Universal(FONT_PLANE_SLOT),
                placement,
                interaction: WindowInteraction::APPLICATION_FIXED_FRAME,
            }) {
                Ok(window) => window,
                Err(_) => {
                    let _ = finish_window_session(FONT_OWNER, session);
                    let _ = destroy_frame(frame);
                    return Err("font-ui4-window-create");
                }
            };
            if publish_window_frame(FONT_OWNER, window, DamageRect::FULL).is_err() {
                let _ = finish_window_session(FONT_OWNER, session);
                let _ = destroy_frame(frame);
                return Err("font-ui4-window-publish");
            }
            FONT_STAMPS.lock().slots[slot_index] = Some(FontSlot {
                session,
                frame,
                window,
                width,
                height,
                placement,
                request_serial,
            });
            (window, false)
        }
        Some(previous) if frame == previous.frame => {
            if publish_window_frame(FONT_OWNER, previous.window, DamageRect::FULL).is_err() {
                return Err("font-ui4-window-publish");
            }
            FONT_STAMPS.lock().slots[slot_index] = Some(FontSlot {
                placement,
                request_serial,
                ..previous
            });
            (previous.window, true)
        }
        Some(previous) => {
            if replace_window_frame(FONT_OWNER, previous.window, frame).is_err() {
                let _ = destroy_frame(frame);
                return Err("font-ui4-window-replace");
            }
            if set_window_placement(FONT_OWNER, previous.window, placement).is_err()
                || publish_window_frame(FONT_OWNER, previous.window, DamageRect::FULL).is_err()
            {
                let _ = replace_window_frame(FONT_OWNER, previous.window, previous.frame);
                let _ = set_window_placement(FONT_OWNER, previous.window, previous.placement);
                let _ = destroy_frame(frame);
                return Err("font-ui4-window-republish");
            }
            {
                let mut state = FONT_STAMPS.lock();
                state.slots[slot_index] = Some(FontSlot {
                    session: previous.session,
                    frame,
                    window: previous.window,
                    width,
                    height,
                    placement,
                    request_serial,
                });
                state.queue_retirement(previous.frame);
            }
            (previous.window, false)
        }
    };

    crate::log_info!(
        target: "ui4/font-stamp";
        "font stamp presented request={} slot={} frame={} window={} extent={}x{} logical_slots={} reused_slot={} reused_frame={} plane=slot1-alpha buffering=double producer={} producer_release={} compositor=isolated-guc-ui4 surflive_release=1 cpu_readback=0 cpu_frame_copy=0\n",
        request_serial,
        slot_index,
        frame.raw(),
        window.raw(),
        width,
        height,
        MAX_FONT_STAMP_SLOTS,
        existing.is_some() as u8,
        reused_frame as u8,
        stamp.producer_path,
        stamp.release.sequence(),
    );
    Ok(FontStampPresentation {
        slot: slot_index,
        frame,
        window,
        request_serial,
        reused_slot: existing.is_some(),
        reused_frame,
        stamp,
    })
}

fn create_font_frame(width: u32, height: u32) -> Result<FrameHandle, &'static str> {
    let output = OutputId::from_slot(0).ok_or("font-ui4-output")?;
    create_frame(FrameSpec {
        output,
        content: FrameContent::FontScene2d,
        cadence: FrameCadence::Dirty,
        buffering: FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::TRANSPARENT),
    })
    .map_err(|_| "font-ui4-frame-create")
}

fn font_placement(slot: usize, stamp: &GpuFontUi4Stamp) -> WindowPlacement {
    let max_x = stamp.scanout_width.saturating_sub(stamp.stamp_width) as i32;
    let max_y = stamp.scanout_height.saturating_sub(stamp.stamp_height) as i32;
    let centered_x = max_x / 2;
    let centered_y = max_y / 2;
    let column = (slot % 5) as i32 - 2;
    let row = (slot / 5) as i32;
    WindowPlacement {
        x: centered_x
            .saturating_add(column.saturating_mul(FONT_CASCADE_PX))
            .clamp(0, max_x),
        y: centered_y
            .saturating_add(row.saturating_mul(FONT_CASCADE_PX))
            .clamp(0, max_y),
        width: stamp.stamp_width,
        height: stamp.stamp_height,
        z: FONT_Z_BASE.saturating_add(slot as i32),
        opacity: u8::MAX,
        visible: true,
    }
}

fn quarantine_failed_destination(
    slot_index: usize,
    existing: Option<FontSlot>,
    frame: FrameHandle,
    candidate_is_new: bool,
    reason: &'static str,
) {
    if !candidate_is_new && existing.is_some_and(|slot| slot.frame == frame) {
        if let Some(slot) = existing {
            let _ = finish_window_session(FONT_OWNER, slot.session);
        }
        FONT_STAMPS.lock().slots[slot_index] = None;
    }
    let quarantined = {
        let mut state = FONT_STAMPS.lock();
        state.quarantine(frame);
        state.quarantined_frames.len()
    };
    crate::log_error!(
        target: "ui4/font-stamp";
        "font stamp destination quarantined slot={} frame={} reason={} candidate_new={} quarantined={} action=no-cancel-no-reuse\n",
        slot_index,
        frame.raw(),
        reason,
        candidate_is_new as u8,
        quarantined,
    );
}

fn close_slot_for_escape(window: WindowId) {
    let found = {
        let state = FONT_STAMPS.lock();
        state.slots.iter().enumerate().find_map(|(index, slot)| {
            slot.filter(|slot| slot.window == window)
                .map(|slot| (index, slot))
        })
    };
    let Some((index, slot)) = found else {
        return;
    };
    match finish_window_session(FONT_OWNER, slot.session) {
        Ok(_) => {
            let mut state = FONT_STAMPS.lock();
            if state.slots[index].is_some_and(|current| current.window == window) {
                state.slots[index] = None;
                state.queue_retirement(slot.frame);
            }
            crate::log_info!(
                target: "ui4/font-stamp";
                "font stamp slot released trigger=escape slot={} request={} frame={} window={} logical_reusable=1 physical_retire=after-surflive\n",
                index,
                slot.request_serial,
                slot.frame.raw(),
                slot.window.raw(),
            );
        }
        Err(error) => crate::log_warn!(
            target: "ui4/font-stamp";
            "font stamp escape close deferred slot={} frame={} window={} error={:?}\n",
            index,
            slot.frame.raw(),
            slot.window.raw(),
            error,
        ),
    }
}

fn drain_escape_events() {
    for event in take_owner_input_events(FONT_OWNER) {
        let Ui4InputEvent::Keyboard(event) = event else {
            continue;
        };
        if event.event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
            && event.event.key_code == crate::r::keyboard::KEYBOARD_KEY_ESCAPE
        {
            close_slot_for_escape(event.window);
        }
    }
}

fn reap_retired_frames() {
    let mut state = FONT_STAMPS.lock();
    let mut index = 0;
    while index < state.retired_frames.len() {
        match destroy_frame(state.retired_frames[index]) {
            Ok(()) | Err(FramePoolError::InvalidHandle) => {
                state.retired_frames.swap_remove(index);
            }
            Err(FramePoolError::Busy) => index += 1,
            Err(error) => {
                let frame = state.retired_frames.swap_remove(index);
                crate::log_warn!(
                    target: "ui4/font-stamp";
                    "font stamp frame retirement abandoned frame={} error={:?}\n",
                    frame.raw(),
                    error,
                );
            }
        }
    }
}

#[embassy_executor::task]
pub(crate) async fn ui4_font_stamp_service_task() {
    crate::log_info!(
        target: "ui4/font-stamp";
        "font stamp service online slots={} plane=slot1-alpha sessions=one-per-window producer_serialization=one guc=render+gpgpu compositor_guc=isolated escape=focused-window-close physical_release=surflive\n",
        MAX_FONT_STAMP_SLOTS,
    );
    loop {
        if !FONT_PRESENT_IN_FLIGHT.load(Ordering::Acquire) {
            drain_escape_events();
            reap_retired_frames();
        }
        Timer::after(Duration::from_millis(FONT_INPUT_POLL_MS)).await;
    }
}
