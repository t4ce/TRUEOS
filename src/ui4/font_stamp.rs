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
    GpuFontFace, GpuFontRgba, GpuFontTextRequest, GpuFontUi4Document,
    prepare_ui4_font_document, release_ui4_font_document, render_ui4_font_document_view,
};

pub(crate) const MAX_FONT_STAMP_SLOTS: usize = 10;
const FONT_OWNER: WindowOwner = WindowOwner::FONT_STAMP;
const FONT_PLANE_SLOT: u8 = 1;
const FONT_Z_BASE: i32 = 70;
const FONT_INPUT_POLL_MS: u64 = 8;
const FONT_CASCADE_PX: i32 = 18;
const FONT_DOCUMENT_WIDTH: u32 = 1920;
const FONT_DOCUMENT_HEIGHT: u32 = 1080;
const FONT_VIEW_WIDTH: u32 = super::DEFAULT_FRAME_WIDTH;
const FONT_VIEW_HEIGHT: u32 = super::DEFAULT_FRAME_HEIGHT;

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

struct FontDocumentView {
    document: GpuFontUi4Document,
    pan_x: u32,
    pan_y: u32,
    active_pan_source: Option<super::Ui4CursorSource>,
    dirty: bool,
}

struct FontStampState {
    slots: [Option<FontSlot>; MAX_FONT_STAMP_SLOTS],
    documents: [Option<FontDocumentView>; MAX_FONT_STAMP_SLOTS],
    next_reuse: usize,
    next_request_serial: u64,
    retired_frames: Vec<FrameHandle>,
    retired_documents: Vec<GpuFontUi4Document>,
    quarantined_frames: Vec<FrameHandle>,
    quarantined_documents: Vec<GpuFontUi4Document>,
}

impl FontStampState {
    const fn new() -> Self {
        Self {
            slots: [None; MAX_FONT_STAMP_SLOTS],
            documents: [const { None }; MAX_FONT_STAMP_SLOTS],
            next_reuse: 0,
            next_request_serial: 0,
            retired_frames: Vec::new(),
            retired_documents: Vec::new(),
            quarantined_frames: Vec::new(),
            quarantined_documents: Vec::new(),
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

    fn queue_document_retirement(&mut self, document: GpuFontUi4Document) {
        self.retired_documents.push(document);
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
    pub(crate) font_name: &'static str,
    pub(crate) text_chars: usize,
    pub(crate) rows: usize,
    pub(crate) glyphs: usize,
    pub(crate) size_percent: u32,
    pub(crate) font_pixels: f32,
    pub(crate) document_width: u32,
    pub(crate) document_height: u32,
    pub(crate) viewport_width: u32,
    pub(crate) viewport_height: u32,
    pub(crate) render_completed: bool,
    pub(crate) producer_path: &'static str,
    pub(crate) release_sequence: u64,
}

pub(crate) fn present_font_stamp(
    request: GpuFontTextRequest<'_>,
    font: GpuFontFace,
    size_percent: u32,
    rgba: GpuFontRgba,
) -> Result<FontStampPresentation, &'static str> {
    let _guard = PresentGuard::acquire()?;
    let document = prepare_ui4_font_document(
        request,
        font,
        size_percent,
        rgba,
        FONT_VIEW_WIDTH,
        FONT_VIEW_HEIGHT,
        FONT_DOCUMENT_WIDTH,
        FONT_DOCUMENT_HEIGHT,
    )?;
    let width = FONT_VIEW_WIDTH;
    let height = FONT_VIEW_HEIGHT;
    let (slot_index, existing, request_serial) = FONT_STAMPS.lock().reserve_logical_slot();

    let mut candidate_is_new =
        existing.is_none_or(|slot| slot.width != width || slot.height != height);
    let mut frame = if candidate_is_new {
        match create_font_frame(width, height) {
            Ok(frame) => frame,
            Err(error) => {
                retire_font_document(document);
                return Err(error);
            }
        }
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
            frame = match create_font_frame(width, height) {
                Ok(frame) => frame,
                Err(error) => {
                    retire_font_document(document);
                    return Err(error);
                }
            };
            match acquire_frame_buffer(frame) {
                Ok(lease) => lease,
                Err(_) => {
                    let _ = destroy_frame(frame);
                    retire_font_document(document);
                    return Err("font-ui4-frame-acquire");
                }
            }
        }
        Err(_) => {
            if candidate_is_new {
                let _ = destroy_frame(frame);
            }
            retire_font_document(document);
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
            retire_font_document(document);
            return Err("font-ui4-gpu-surface");
        }
    };

    let rendered = match render_ui4_font_document_view(&document, 0, 0, destination) {
        Ok(rendered) => rendered,
        Err(error) => {
            quarantine_failed_destination(
                slot_index,
                existing,
                frame,
                candidate_is_new,
                document,
                error,
            );
            return Err(error);
        }
    };
    if let Err(error) = publish_gpu_font_frame_buffer(lease, rendered.release) {
        // The exact producer fence retired, so cancelling is safe even though
        // publication validation rejected the lease.
        let _ = cancel_frame_buffer(lease);
        if candidate_is_new {
            let _ = destroy_frame(frame);
        }
        retire_font_document(document);
        crate::log_error!(
            target: "ui4/font-stamp";
            "font stamp publish rejected slot={} frame={} error={:?}\n",
            slot_index,
            frame.raw(),
            error,
        );
        return Err("font-ui4-frame-publish");
    }

    let placement = font_placement(slot_index, width, height);
    let (new_slot, reused_frame) = match existing {
        None => {
            let session = match begin_window_session(FONT_OWNER) {
                Ok(session) => session,
                Err(_) => {
                    let _ = destroy_frame(frame);
                    retire_font_document(document);
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
                    retire_font_document(document);
                    return Err("font-ui4-window-create");
                }
            };
            if publish_window_frame(FONT_OWNER, window, DamageRect::FULL).is_err() {
                let _ = finish_window_session(FONT_OWNER, session);
                let _ = destroy_frame(frame);
                retire_font_document(document);
                return Err("font-ui4-window-publish");
            }
            (
                FontSlot {
                    session,
                    frame,
                    window,
                    width,
                    height,
                    placement,
                    request_serial,
                },
                false,
            )
        }
        Some(previous) if frame == previous.frame => {
            if publish_window_frame(FONT_OWNER, previous.window, DamageRect::FULL).is_err() {
                retire_font_document(document);
                return Err("font-ui4-window-publish");
            }
            (
                FontSlot {
                    placement,
                    request_serial,
                    ..previous
                },
                true,
            )
        }
        Some(previous) => {
            if replace_window_frame(FONT_OWNER, previous.window, frame).is_err() {
                let _ = destroy_frame(frame);
                retire_font_document(document);
                return Err("font-ui4-window-replace");
            }
            if set_window_placement(FONT_OWNER, previous.window, placement).is_err()
                || publish_window_frame(FONT_OWNER, previous.window, DamageRect::FULL).is_err()
            {
                let _ = replace_window_frame(FONT_OWNER, previous.window, previous.frame);
                let _ = set_window_placement(FONT_OWNER, previous.window, previous.placement);
                let _ = destroy_frame(frame);
                retire_font_document(document);
                return Err("font-ui4-window-republish");
            }
            (
                FontSlot {
                    session: previous.session,
                    frame,
                    window: previous.window,
                    width,
                    height,
                    placement,
                    request_serial,
                },
                false,
            )
        }
    };

    let font_name = document.font_name;
    let text_chars = document.text_chars;
    let rows = document.rows;
    let glyphs = document.glyphs;
    let font_pixels = document.font_pixels;
    let document_width = document.document_width;
    let document_height = document.document_height;
    let old_document = {
        let mut state = FONT_STAMPS.lock();
        state.slots[slot_index] = Some(new_slot);
        if let Some(previous) = existing.filter(|previous| previous.frame != frame) {
            state.queue_retirement(previous.frame);
        }
        state.documents[slot_index].replace(FontDocumentView {
            document,
            pan_x: 0,
            pan_y: 0,
            active_pan_source: None,
            dirty: false,
        })
    };
    if let Some(previous) = old_document {
        retire_font_document(previous.document);
    }

    crate::log_info!(
        target: "ui4/font-stamp";
        "font document presented request={} slot={} frame={} window={} document={}x{} viewport={}x{} font_px={:.2} rows={} logical_slots={} reused_slot={} reused_frame={} plane=slot1-alpha buffering=double producer={} producer_release={} pan=middle-drag-retained-viewport compositor=isolated-guc-ui4 surflive_release=1 cpu_readback=0 cpu_frame_copy=0\n",
        request_serial,
        slot_index,
        frame.raw(),
        new_slot.window.raw(),
        document_width,
        document_height,
        width,
        height,
        font_pixels,
        rows,
        MAX_FONT_STAMP_SLOTS,
        existing.is_some() as u8,
        reused_frame as u8,
        rendered.producer_path,
        rendered.release.sequence(),
    );
    Ok(FontStampPresentation {
        slot: slot_index,
        frame,
        window: new_slot.window,
        request_serial,
        reused_slot: existing.is_some(),
        reused_frame,
        font_name,
        text_chars,
        rows,
        glyphs,
        size_percent,
        font_pixels,
        document_width,
        document_height,
        viewport_width: width,
        viewport_height: height,
        render_completed: rendered.render.completed,
        producer_path: rendered.producer_path,
        release_sequence: rendered.release.sequence(),
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

fn font_placement(slot: usize, width: u32, height: u32) -> WindowPlacement {
    let (scanout_width, scanout_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((width, height));
    let max_x = scanout_width.saturating_sub(width) as i32;
    let max_y = scanout_height.saturating_sub(height) as i32;
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
        width,
        height,
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
    document: GpuFontUi4Document,
    reason: &'static str,
) {
    let mut retired_document = None;
    if !candidate_is_new && existing.is_some_and(|slot| slot.frame == frame) {
        if let Some(slot) = existing {
            let _ = finish_window_session(FONT_OWNER, slot.session);
        }
        let mut state = FONT_STAMPS.lock();
        state.slots[slot_index] = None;
        retired_document = state.documents[slot_index].take();
    }
    if let Some(previous) = retired_document {
        retire_font_document(previous.document);
    }
    let quarantined = {
        let mut state = FONT_STAMPS.lock();
        state.quarantine(frame);
        state.quarantined_documents.push(document);
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

fn retire_font_document(document: GpuFontUi4Document) {
    if !release_ui4_font_document(&document) {
        FONT_STAMPS.lock().queue_document_retirement(document);
    }
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
            let document = {
                let mut state = FONT_STAMPS.lock();
                if state.slots[index].is_some_and(|current| current.window == window) {
                    state.slots[index] = None;
                    state.queue_retirement(slot.frame);
                    state.documents[index].take()
                } else {
                    None
                }
            };
            if let Some(document) = document {
                retire_font_document(document.document);
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

fn drain_font_events() {
    for event in take_owner_input_events(FONT_OWNER) {
        match event {
            Ui4InputEvent::Keyboard(event)
                if event.event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
                    && event.event.key_code == crate::r::keyboard::KEYBOARD_KEY_ESCAPE =>
            {
                close_slot_for_escape(event.window);
            }
            Ui4InputEvent::Pan(event) => pan_font_document(event),
            _ => {}
        }
    }
}

fn pan_font_document(event: super::Ui4PanEvent) {
    let mut state = FONT_STAMPS.lock();
    let Some(index) = state
        .slots
        .iter()
        .position(|slot| slot.is_some_and(|slot| slot.window == event.window))
    else {
        return;
    };
    let Some(view) = state.documents[index].as_mut() else {
        return;
    };
    match event.phase {
        super::Ui4PanPhase::Begin => view.active_pan_source = Some(event.source),
        super::Ui4PanPhase::Update if view.active_pan_source == Some(event.source) => {
            let next_x = move_document_origin(
                view.pan_x,
                event.dx,
                view.document.document_width.saturating_sub(FONT_VIEW_WIDTH),
            );
            let next_y = move_document_origin(
                view.pan_y,
                event.dy,
                view.document
                    .document_height
                    .saturating_sub(FONT_VIEW_HEIGHT),
            );
            if next_x != view.pan_x || next_y != view.pan_y {
                view.pan_x = next_x;
                view.pan_y = next_y;
                view.dirty = true;
            }
        }
        super::Ui4PanPhase::End if view.active_pan_source == Some(event.source) => {
            view.active_pan_source = None;
            crate::log_info!(
                target: "ui4/font-stamp";
                "font document pan ended slot={} window={} document={}x{} viewport={}x{} crop_origin={},{} retessellate=0 geometry_upload=0\n",
                index,
                event.window.raw(),
                view.document.document_width,
                view.document.document_height,
                FONT_VIEW_WIDTH,
                FONT_VIEW_HEIGHT,
                view.pan_x,
                view.pan_y,
            );
        }
        _ => {}
    }
}

fn move_document_origin(origin: u32, drag_delta: i32, maximum: u32) -> u32 {
    (i64::from(origin) - i64::from(drag_delta)).clamp(0, i64::from(maximum)) as u32
}

fn render_one_dirty_document() {
    let mut state = FONT_STAMPS.lock();
    let Some(index) = state
        .documents
        .iter()
        .position(|view| view.as_ref().is_some_and(|view| view.dirty))
    else {
        return;
    };
    let Some(slot) = state.slots[index] else {
        let document = state.documents[index].take();
        drop(state);
        if let Some(document) = document {
            retire_font_document(document.document);
        }
        return;
    };
    let lease = match acquire_frame_buffer(slot.frame) {
        Ok(lease) => lease,
        Err(FramePoolError::Busy) => return,
        Err(error) => {
            crate::log_warn!(
                target: "ui4/font-stamp";
                "font document pan acquire deferred slot={} frame={} error={:?}\n",
                index,
                slot.frame.raw(),
                error,
            );
            return;
        }
    };
    let destination = match gpgpu_rgba_surface(lease) {
        Ok(destination) => destination,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            return;
        }
    };
    let (pan_x, pan_y, rendered) = {
        let view = state.documents[index]
            .as_ref()
            .expect("dirty font document selected");
        (
            view.pan_x,
            view.pan_y,
            render_ui4_font_document_view(&view.document, view.pan_x, view.pan_y, destination),
        )
    };
    let rendered = match rendered {
        Ok(rendered) => rendered,
        Err(reason) => {
            // A failed direct-RCS call may have crossed the submission
            // boundary. Keep its exact destination and resident mesh mapped.
            let document = state.documents[index].take();
            state.slots[index] = None;
            state.quarantine(slot.frame);
            if let Some(document) = document {
                state.quarantined_documents.push(document.document);
            }
            drop(state);
            let _ = finish_window_session(FONT_OWNER, slot.session);
            crate::log_error!(
                target: "ui4/font-stamp";
                "font document pan render quarantined slot={} frame={} window={} crop_origin={},{} reason={} action=no-cancel-no-unmap\n",
                index,
                slot.frame.raw(),
                slot.window.raw(),
                pan_x,
                pan_y,
                reason,
            );
            return;
        }
    };
    if let Err(error) = publish_gpu_font_frame_buffer(lease, rendered.release) {
        let _ = cancel_frame_buffer(lease);
        crate::log_warn!(
            target: "ui4/font-stamp";
            "font document pan frame publish deferred slot={} frame={} crop_origin={},{} error={:?}\n",
            index,
            slot.frame.raw(),
            pan_x,
            pan_y,
            error,
        );
        return;
    }
    if let Err(error) = publish_window_frame(FONT_OWNER, slot.window, DamageRect::FULL) {
        // The GPU release is exact and retired, so the mesh is safe to retire.
        // The now-ready frame remains broker-owned until its ordinary destroy
        // retry succeeds.
        let document = state.documents[index].take();
        state.slots[index] = None;
        state.queue_retirement(slot.frame);
        drop(state);
        let _ = finish_window_session(FONT_OWNER, slot.session);
        if let Some(document) = document {
            retire_font_document(document.document);
        }
        crate::log_warn!(
            target: "ui4/font-stamp";
            "font document pan window publish closed slot={} frame={} window={} error={:?}\n",
            index,
            slot.frame.raw(),
            slot.window.raw(),
            error,
        );
        return;
    }
    if let Some(view) = state.documents[index].as_mut() {
        // Do not erase a newer input update that arrived before publication.
        if view.pan_x == pan_x && view.pan_y == pan_y {
            view.dirty = false;
        }
    }
    crate::log_info!(
        target: "ui4/font-stamp";
        "font document pan presented slot={} frame={} window={} crop_origin={},{} producer={} release={} geometry_upload=0 cpu_frame_copy=0 surflive_release=1\n",
        index,
        slot.frame.raw(),
        slot.window.raw(),
        pan_x,
        pan_y,
        rendered.producer_path,
        rendered.release.sequence(),
    );
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
    let mut document_index = 0;
    while document_index < state.retired_documents.len() {
        if release_ui4_font_document(&state.retired_documents[document_index]) {
            state.retired_documents.swap_remove(document_index);
        } else {
            document_index += 1;
        }
    }
}

#[embassy_executor::task]
pub(crate) async fn ui4_font_stamp_service_task() {
    crate::log_info!(
        target: "ui4/font-stamp";
        "font stamp service online slots={} plane=slot1-alpha sessions=one-per-window document=1920x1080 viewport={}x{} buffering=double producer_serialization=one guc=render compositor_guc=isolated pan=middle-drag-retained-viewport escape=focused-window-close physical_release=surflive\n",
        MAX_FONT_STAMP_SLOTS,
        FONT_VIEW_WIDTH,
        FONT_VIEW_HEIGHT,
    );
    loop {
        if let Ok(_guard) = PresentGuard::acquire() {
            drain_font_events();
            render_one_dirty_document();
            reap_retired_frames();
        }
        Timer::after(Duration::from_millis(FONT_INPUT_POLL_MS)).await;
    }
}
