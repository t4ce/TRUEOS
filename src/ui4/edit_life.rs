//! Edit's retained page input and independent, bounded Conway animation task.
//! Native compute consumes immutable glyph input; the bounded CPU reference
//! keeps the same experiment available on outputs without that native backend.
use super::{DamageRect, FrameHandle, WindowId, WindowOwner};
use alloc::{sync::Arc, vec::Vec};
use core::{future::Future, task::Poll, sync::atomic::{AtomicU64, Ordering}};
use embassy_sync::signal::Signal;
use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};

const OWNER: WindowOwner = WindowOwner::SHELL3_SERVICE;
const MAX_PAGES: usize = 8;
pub(crate) const MAX_PAGE_PIXELS: usize = 4096 * 2160;
const GENERATION_MS: u64 = 100; // Demo speed 1: ten generations per second.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl PixelRect {
    pub(crate) fn contains(self, x: u32, y: u32) -> bool {
        x >= self.x && y >= self.y && x - self.x < self.width && y - self.y < self.height
    }
}

pub(crate) struct Page {
    pub width: u32,
    pub height: u32,
    /// Viewport-local terminal cells: glyph index, straight foreground RGBA,
    /// background RGBA, flags (bit0 opaque background, bit1 underline).
    pub cells: Vec<[u32; 4]>,
    pub columns: u32,
    pub rows: u32,
    pub cell_width: u32,
    pub cell_height: u32,
    pub viewport: PixelRect,
    pub scroll_revision: u64,
    pub emission_revision: u64,
}
impl Page {
    fn valid(&self) -> bool {
        let pixels = self.width as usize * self.height as usize;
        pixels != 0 && pixels <= MAX_PAGE_PIXELS
            && self.cell_width != 0 && self.cell_height != 0
            && self.cell_width <= 256 && self.cell_height <= 256
            && self.columns == self.viewport.width.div_ceil(self.cell_width)
            && self.rows == self.viewport.height.div_ceil(self.cell_height)
            && self.cells.len() == self.columns as usize * self.rows as usize
            && self.viewport.width != 0 && self.viewport.height != 0
            && self.viewport.x.checked_add(self.viewport.width).is_some_and(|x| x <= self.width)
            && self.viewport.y.checked_add(self.viewport.height).is_some_and(|y| y <= self.height)
    }
}

#[derive(Clone)]
struct Registration {
    window: WindowId,
    source: FrameHandle,
    page: Arc<Page>,
    serial: u64,
    displayed: Option<FrameHandle>,
    paused: bool,
    failed: bool,
    pause_revision: u64,
}
static PAGES: Mutex<Vec<Registration>> = Mutex::new(Vec::new());
static WAKE: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();
static NEXT_SERIAL: AtomicU64 = AtomicU64::new(1);

/// One newest immutable input per window. Keep the last completed effect
/// visible until its successor is ready, avoiding opaque flashes while typing.
pub(crate) fn publish_page(window: WindowId, source: FrameHandle, page: Arc<Page>) {
    if !page.valid() { remove(window); return; }
    let mut pages = PAGES.lock();
    let serial = NEXT_SERIAL.fetch_add(1, Ordering::Relaxed);
    if let Some(old) = pages.iter_mut().find(|old| old.window == window) {
        // A newer document frame is not permission to restart a failed GPU
        // producer. Only disabling/removing the registration clears this latch.
        if old.failed { return; }
        let geometry_changed = (old.page.width, old.page.height, old.page.viewport,
            old.page.cell_width, old.page.cell_height)
            != (page.width, page.height, page.viewport, page.cell_width, page.cell_height);
        if geometry_changed && old.displayed.is_some() {
            // Geometry changes replace the effect frame. Detach its predecessor
            // before the worker retires it; ordinary edits keep their alpha.
            if show_source(window, source).is_ok() { old.displayed = None; }
        }
        old.source = source;
        old.page = page;
        old.serial = serial;
        old.paused = false;
    } else if pages.len() < MAX_PAGES {
        pages.push(Registration { window, source, page, serial, displayed: None,
            paused: false, failed: false, pause_revision: 0 });
    }
    drop(pages);
    WAKE.signal(());
}

/// Pause before any capture or upload. Ordinary terminal pixels, including
/// menus and pickers, are displayed while the retained world stays untouched.
pub(crate) fn pause(window: WindowId, source: FrameHandle) {
    let mut pages = PAGES.lock();
    if let Some(old) = pages.iter_mut().find(|old| old.window == window) {
        let mut changed = !old.paused || old.source != source;
        // Retain the displayed handle on a transient restore failure so the
        // next ordinary terminal redraw retries detaching the effect frame.
        if old.displayed.is_some() && show_source(window, source).is_ok() {
            old.displayed = None;
            changed = true;
        }
        old.source = source;
        if !old.paused {
            old.pause_revision = old.pause_revision.wrapping_add(1);
        }
        old.paused = true;
        if changed { old.serial = NEXT_SERIAL.fetch_add(1, Ordering::Relaxed); }
        drop(pages);
        if changed { WAKE.signal(()); }
    }
}

fn input_is_active(input: &Registration) -> bool {
    PAGES.lock().iter().any(|current| current.window == input.window
        && current.serial == input.serial && current.source == input.source
        && !current.paused && !current.failed)
}

pub(crate) fn is_failed(window: WindowId) -> bool {
    PAGES.lock().iter().any(|current| current.window == window && current.failed)
}

/// Fail closed on the opaque source. Never use a new page, a timer tick or a
/// menu round trip to restart an effect whose native initialization/render failed.
fn stop(window: WindowId, reason: &'static str) {
    let mut pages = PAGES.lock();
    let Some(old) = pages.iter_mut().find(|old| old.window == window) else { return; };
    // A render may finish after a newer source was registered. Restore that
    // current source, never the stale frame borrowed by the failed render.
    if old.displayed.is_some() && show_source(window, old.source).is_ok() { old.displayed = None; }
    let first = !old.failed;
    old.failed = true;
    old.paused = true;
    if first { old.serial = NEXT_SERIAL.fetch_add(1, Ordering::Relaxed); }
    drop(pages);
    if first {
        crate::log_error!(target: "ui4/edit-life";
            "edit-life: stopped window={} reason={} action=opaque-source-and-latch no-auto-retry=1 peer-contexts=untouched\n",
            window.raw(), reason);
        WAKE.signal(());
    }
}

/// Called before source retirement or turning the effect off. Only this frame
/// is restored; other windows, leases, plane opacity and input routing are intact.
pub(crate) fn remove(window: WindowId) {
    let mut pages = PAGES.lock();
    if let Some(index) = pages.iter().position(|old| old.window == window) {
        let old = pages.remove(index);
        if old.displayed.is_some() { let _ = show_source(window, old.source); }
    }
    drop(pages);
    WAKE.signal(());
}

fn show_source(window: WindowId, source: FrameHandle) -> Result<(), ()> {
    let (placement, epoch) = super::window_resize_state(OWNER, window).map_err(|_| ())?;
    let plan = super::frame_snapshot(source).map_err(|_| ())?.plan;
    if (placement.width, placement.height) != (plan.width, plan.height) { return Err(()); }
    super::commit_window_frame_replacement(OWNER, window, source, placement, epoch, DamageRect::FULL)
        .map(|_| ()).map_err(|_| ())
}

struct Effect {
    input: Registration,
    backend: Backend,
    frame: FrameHandle,
    next_generation_ms: u64,
    dirty: bool,
    reset: bool,
    emit: bool,
    steps: u32,
    quarantined: bool,
    disabled: bool,
}
enum Backend {
    Native(crate::intel::gpgpu::EditLifeState),
    Reference(super::edit_life_model::Life),
}
impl Drop for Effect {
    fn drop(&mut self) {
        if self.quarantined { return; }
        // FramePool reader counts do not represent broker attachment. A failed
        // source restore must never leave the window holding a retired handle.
        // Keep the completed frame allocated if detachment cannot be proven.
        if super::window_broker::window_snapshot(OWNER, self.input.window)
            .is_some_and(|window| window.frame == self.frame) { return; }
        super::retire_frame_when_released(self.frame);
    }
}
impl Effect {
    fn new(input: Registration) -> Option<Self> {
        let started_ms = Instant::now().as_millis();
        let page = &input.page;
        let rect = page.viewport;
        let backend = if crate::intel::gpgpu::edit_life_native_supported() {
            let native = crate::intel::gpgpu::EditLifeState::new(
            page.width, page.height,
            crate::intel::gpgpu::GpgpuRect::new(rect.x as i32, rect.y as i32, rect.width, rect.height),
            page.cell_width, page.cell_height,
            )?;
            Backend::Native(native)
        } else {
            Backend::Reference(super::edit_life_model::Life::new(rect.width, rect.height)?)
        };
        let backend_ready_ms = Instant::now().as_millis();
        let frame = super::create_frame(super::FrameSpec {
            output: super::OutputId::from_slot(0)?,
            content: super::FrameContent::Image,
            cadence: super::FrameCadence::Dirty,
            buffering: super::FrameBuffering::Double,
            format: super::ScanoutFormat::Rgba8888Premultiplied,
            width: page.width, height: page.height,
            base_color: Some(super::PremultipliedRgba8::TRANSPARENT),
        }).ok()?;
        let frame_ready_ms = Instant::now().as_millis();
        let mut effect = Self { input, backend, frame,
            next_generation_ms: Instant::now().as_millis() + GENERATION_MS, dirty: true,
            reset: true, emit: true, steps: 0, quarantined: false, disabled: false };
        effect.capture(true, true);
        crate::log_info!(target: "ui4/edit-life";
            "edit-life: enabled backend={} speed=1 generations_hz=10 trail_persistence=0.75 background_alpha=153 font=microfont-hex compact_glyph_cells=1 backend_ms={} frame_ms={} upload_ms={} total_ms={}\n",
            if matches!(effect.backend, Backend::Native(_)) { "native-cpp-adls" } else { "cpu-reference" },
            backend_ready_ms.saturating_sub(started_ms),
            frame_ready_ms.saturating_sub(backend_ready_ms),
            Instant::now().as_millis().saturating_sub(frame_ready_ms),
            Instant::now().as_millis().saturating_sub(started_ms));
        Some(effect)
    }

    fn set_viewport(&mut self, viewport: PixelRect) -> bool {
        match &mut self.backend {
            Backend::Native(native) => native.set_viewport(crate::intel::gpgpu::GpgpuRect::new(
                viewport.x as i32, viewport.y as i32, viewport.width, viewport.height)),
            Backend::Reference(life) => {
                let Some(next) = super::edit_life_model::Life::new(viewport.width, viewport.height)
                    else { return false; };
                *life = next;
                true
            }
        }
    }

    fn capture(&mut self, reset: bool, emit: bool) {
        let page = &self.input.page;
        match &mut self.backend {
            Backend::Reference(life) => {
                if !life.snapshot_hex(&page.cells, page.columns,
                    page.cell_width, page.cell_height, reset, emit) {
                    self.disabled = true;
                }
            }
            Backend::Native(native) => {
                if !native.upload(&page.cells) {
                    self.disabled = true;
                }
                self.reset |= reset;
                self.emit |= emit;
            }
        }
        if reset {
            self.next_generation_ms = Instant::now().as_millis() + GENERATION_MS;
            self.steps = 0;
        }
        self.dirty = true;
    }

    fn render(&mut self) -> Result<(), ()> {
        if !input_is_active(&self.input) { return Err(()); }
        // Borrow the source front only for this synchronous render. Retaining
        // it between frames would consume Shell3's double-buffer capacity.
        let source_lease = super::acquire_published_frame(self.input.source).map_err(|_| ())?;
        let lease = match super::acquire_frame_buffer(self.frame) {
            Ok(lease) => lease,
            Err(_) => { let _ = super::release_published_frame(source_lease); return Err(()); }
        };
        let mut may_be_live = false;
        let result = (|| {
            let page = &self.input.page;
            let source_view = super::published_rgba_view(source_lease).map_err(|_| ())?;
            if (source_view.width, source_view.height) != (page.width, page.height) { return Err(()); }
            match &mut self.backend {
                Backend::Reference(life) => {
                    let view = super::writable_rgba_view(lease).map_err(|_| ())?;
                    let out = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
                    for y in 0..page.height {
                        let source = unsafe { core::slice::from_raw_parts(
                            source_view.virt.add(y as usize * source_view.pitch as usize), page.width as usize * 4) };
                        let row = &mut out[y as usize * view.pitch as usize..][..page.width as usize * 4];
                        row.copy_from_slice(source);
                        for x in 0..page.width {
                            let p = &mut row[x as usize * 4..][..4];
                            if page.viewport.contains(x, y) {
                                p.copy_from_slice(&life.hex_pixel(x - page.viewport.x, y - page.viewport.y));
                            } else {
                                p[3] = 255;
                            }
                        }
                    }
                    super::mark_frame_buffer_cpu_authored(lease).map_err(|_| ())?;
                    if crate::virtio_gpu_logo::output_dimensions().is_some() { super::emulator_paint::begin(lease); }
                    crate::intel::dma_flush(view.virt, view.byte_len);
                    super::publish_frame_buffer(lease).map_err(|_| ())?;
                }
                Backend::Native(native) => {
                    let dst = super::gpgpu_rgba_surface(lease).map_err(|_| ())?;
                    let source = crate::intel::gpgpu::GpgpuRgba8Surface::new(
                        source_view.phys, source_view.gpu, source_view.byte_len,
                        source_view.width, source_view.height, source_view.pitch).ok_or(())?;
                    let result = native.render(dst, source, self.reset, self.emit, self.steps);
                    if !result.ok || result.release.is_none() {
                        may_be_live = result.submitted;
                        self.quarantined = result.submitted;
                        self.disabled = true;
                        return Err(());
                    }
                    self.reset = false;
                    self.emit = false;
                    self.steps = 0;
                    if super::publish_gpgpu_frame_buffer(lease, result.release.ok_or(())?).is_err() {
                        self.disabled = true;
                        return Err(());
                    }
                }
            }
            // No suspension between generation validation and window commit.
            // A newer capture can never be overwritten by an old effect frame.
            let mut pages = PAGES.lock();
            let current = pages.iter_mut().find(|r| r.window == self.input.window
                && r.serial == self.input.serial && r.source == self.input.source
                && !r.paused && !r.failed).ok_or(())?;
            if current.displayed == Some(self.frame) {
                super::publish_window_frame(OWNER, current.window, DamageRect::FULL).map_err(|_| ())?;
            } else {
                let (placement, epoch) = super::window_resize_state(OWNER, current.window).map_err(|_| ())?;
                if (placement.width, placement.height) != (page.width, page.height) { return Err(()); }
                super::commit_window_frame_replacement(OWNER, current.window, self.frame,
                    placement, epoch, DamageRect::FULL).map_err(|_| ())?;
                current.displayed = Some(self.frame);
            }
            Ok(())
        })();
        // An uncertain GPU read must keep this exact source allocation pinned,
        // just as its destination remains quarantined. Never hand it to a writer.
        if !may_be_live { let _ = super::release_published_frame(source_lease); }
        if result.is_err() && !may_be_live { let _ = super::cancel_frame_buffer(lease); }
        result
    }
}

async fn wait_for_work(active: bool) {
    if !active { WAKE.wait().await; return; }
    let mut wake = core::pin::pin!(WAKE.wait());
    let mut timer = core::pin::pin!(Timer::after(Duration::from_millis(16)));
    core::future::poll_fn(|cx| {
        if wake.as_mut().poll(cx).is_ready() || timer.as_mut().poll(cx).is_ready() {
            Poll::Ready(())
        } else { Poll::Pending }
    }).await;
}

#[trueos_executor::task]
pub(crate) async fn ui4_edit_life_service_task() {
    let mut effects: Vec<Effect> = Vec::new();
    loop {
        let inputs = PAGES.lock().clone();
        effects.retain(|e| inputs.iter().any(|r| r.window == e.input.window
            && (!r.failed || r.displayed == Some(e.frame))));
        for input in inputs {
            if super::window_is_closed(OWNER, input.window) { remove(input.window); continue; }
            // A menu can pause even before the first effect allocation. Never
            // initialize, capture or upload a paused registration.
            if input.paused || input.failed || !input_is_active(&input) { continue; }
            let window = input.window;
            let index = effects.iter().position(|e| e.input.window == input.window);
            if let Some(index) = index {
                let effect = &mut effects[index];
                if effect.input.serial != input.serial || effect.input.source != input.source {
                    let old = &effect.input.page;
                    let new = &input.page;
                    let extent_changed = (old.width, old.height, old.cell_width, old.cell_height)
                        != (new.width, new.height, new.cell_width, new.cell_height);
                    let viewport_changed = old.viewport != new.viewport;
                    let reset = old.scroll_revision != new.scroll_revision || viewport_changed;
                    let emit = old.emission_revision != new.emission_revision;
                    let resumed = effect.input.pause_revision != input.pause_revision;
                    if extent_changed || (viewport_changed && !effect.set_viewport(new.viewport)) {
                        effects.remove(index);
                        if let Some(effect) = Effect::new(input) { effects.push(effect); }
                        else { stop(window, "effect-initialization-failed"); }
                    } else {
                        effect.input = input;
                        effect.capture(reset, emit);
                        if resumed {
                            effect.next_generation_ms = Instant::now().as_millis() + GENERATION_MS;
                            effect.steps = 0;
                        }
                    }
                }
            } else if let Some(effect) = Effect::new(input) { effects.push(effect); }
            else { stop(window, "effect-initialization-failed"); }
        }
        let now = Instant::now().as_millis();
        let mut runnable = false;
        for effect in &mut effects {
            if !input_is_active(&effect.input) { continue; }
            if effect.quarantined || effect.disabled {
                crate::log_warn!(target: "ui4/edit-life";
                    "edit-life: effect stopped window={} uncertain_gpu={} fallback=opaque-editor\n",
                    effect.input.window.raw(), effect.quarantined);
                stop(effect.input.window, "effect-input-or-render-failed");
                continue;
            }
            runnable = true;
            let visible = super::window_placement(OWNER, effect.input.window)
                .is_ok_and(|p| p.visible && p.opacity != 0);
            if !visible { effect.next_generation_ms = now + GENERATION_MS; continue; }
            if now >= effect.next_generation_ms {
                let steps = ((now - effect.next_generation_ms) / GENERATION_MS + 1).min(5);
                match &mut effect.backend {
                    Backend::Reference(life) => for _ in 0..steps { life.step(); },
                    Backend::Native(_) => effect.steps = (effect.steps + steps as u32).min(5),
                }
                effect.next_generation_ms += steps * GENERATION_MS;
                if effect.next_generation_ms <= now { effect.next_generation_ms = now + GENERATION_MS; }
                effect.dirty = true;
            }
            if effect.dirty {
                if effect.render().is_ok() { effect.dirty = false; }
                else if effect.disabled || effect.quarantined {
                    stop(effect.input.window, "native-render-failed");
                }
            }
        }
        // With every page paused, there is no animation timer to service.
        wait_for_work(runnable).await;
    }
}
