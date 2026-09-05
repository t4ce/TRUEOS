//! Font Rush2 face generations, worker admission, and deferred frame retirement.

use super::{
    ActivePreview, CloudBrushState, DesiredPreview, GpgpuPreviewMetrics, GpgpuPreviewPreset,
    PREVIEW_HEIGHT, PREVIEW_OWNER, PREVIEW_WIDTH, PREVIEW_Z,
    abandon_compute_preview_initialization,
};
use crate::ui4::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FramePoolError, FrameSpec,
    FrameWriteLease, OutputId, PremultipliedRgba8, ScanoutFormat, WindowCreate, WindowPlacement,
    WindowPlane, acquire_frame_buffer, begin_window_session, cancel_frame_buffer, create_frame,
    create_window, destroy_frame, gpgpu_rgba_surface, publish_frame_buffer,
    publish_gpu_font_frame_buffer, publish_window_frame, window_frame_was_presented,
};
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;
use trueos_time::Instant;

const CPP_FONT_RUSH2_STAGE_MS: u64 = 3_000;

struct FontProducerWorker {
    producer: crate::r::services::font_kernel_service::FontGpuProducer,
    producer_index: u8,
    font: crate::intel::gpu_font::GpuFontFace,
    font_pixels: f32,
    rng: crate::tyche::SoftRng,
}

struct PublishedFontPlane {
    rows: Vec<crate::r::services::font_kernel_service::FontProducedRow>,
    publish_serial: u64,
    epoch_started_ns: u64,
    gpu_ready_ns: u64,
    published_ns: u64,
    surflive_ns: Option<u64>,
}

pub(super) fn ensure_application_planes_idle(stage: &'static str) -> Result<(), &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let usage = crate::ui4::ui4_live_resource_usage();
    let application_windows = crate::ui4::live_application_window_count(output);
    if application_windows == 0 {
        crate::log_info!(target: "ui4";
            "ui4 cpp-font-rush2 admission accepted stage={} application_windows=0 interaction_or_service_windows={} active_sessions={} active_frames={} slot4_policy=coexist\n",
            stage,
            usage.live_windows,
            usage.active_sessions,
            usage.active_frames,
        );
        return Ok(());
    }
    crate::log_warn!(target: "ui4";
        "ui4 cpp-font-rush2 admission rejected stage={} reason=application-planes-busy application_windows={} active_frames={} active_sessions={} live_windows={} slot4_policy=ignored\n",
        stage,
        application_windows,
        usage.active_frames,
        usage.active_sessions,
        usage.live_windows,
    );
    Err("ui4-application-planes-busy")
}

const CPP_FONT_RUSH2_PRODUCER_COUNT: usize = 8;

const CPP_FONT_RUSH2_PLANE_COUNT: usize = 4;

const CPP_FONT_RUSH2_COLUMNS: u32 = 4;

const CPP_FONT_RUSH2_ROWS: u32 = 2;

const CPP_FONT_RUSH2_LADDER: [usize; 4] = [1, 2, 4, 8];

const CPP_FONT_RUSH2_FACE_MS: u64 = 30_000;

const CPP_FONT_RUSH2_FACES: [crate::intel::gpu_font::GpuFontFace; 3] = [
    crate::intel::gpu_font::GpuFontFace::Default,
    crate::intel::gpu_font::GpuFontFace::NotoSansSc,
    crate::intel::gpu_font::GpuFontFace::Inconsolata,
];

const fn cpp_font_rush2_next_face_index(index: usize) -> usize {
    (index + 1) % CPP_FONT_RUSH2_FACES.len()
}

/// One producer request deliberately contains several independently placed
/// glyphs.  The Font RCS encoder can keep non-overlapping analytical walkers
/// in flight as one GPGPU wave instead of retiring a one-glyph batch at a
/// time.
const CPP_FONT_RUSH2_GLYPHS_PER_ROW: usize = 8;

const fn cpp_font_rush2_tier(producer: usize) -> u16 {
    (producer % 4 + 1) as u16
}

const _: () = {
    assert!(
        CPP_FONT_RUSH2_PRODUCER_COUNT == (CPP_FONT_RUSH2_COLUMNS * CPP_FONT_RUSH2_ROWS) as usize
    );
    assert!(cpp_font_rush2_tier(0) == 1);
    assert!(cpp_font_rush2_tier(3) == 4);
    assert!(cpp_font_rush2_tier(7) == 4);
    assert!(
        CPP_FONT_RUSH2_GLYPHS_PER_ROW <= crate::intel::gpgpu::FONT_OUTLINE_COVERAGE_BATCH_MAX_RUNS
    );
};

static CPP_FONT_RUSH2_RETIRED: Mutex<Vec<CppFontRush2RetiredFrame>> = Mutex::new(Vec::new());

struct CppFontRush2RetiredFrame {
    frame: FrameHandle,
    state: CppFontRush2PlaneState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CppFontRush2LeasePhase {
    Running,
    Draining,
}

pub(super) struct CppFontRush2PlaneState {
    plane_index: u8,
    workers: Vec<FontProducerWorker>,
    font_index: u8,
    font_activated_at: Instant,
    lease_phase: CppFontRush2LeasePhase,
    active_workers: usize,
    epoch_workers: usize,
    pending: Option<CppFontRush2PendingRow>,
    building: Vec<crate::r::services::font_kernel_service::FontProducedRow>,
    published: [Option<PublishedFontPlane>; 2],
    epoch_started_ns: u64,
}

struct CppFontRush2PendingRow {
    lease: FrameWriteLease,
    worker: usize,
    pending: crate::r::services::font_kernel_service::PendingFontProducerRow,
}

static CPP_FONT_RUSH2_LIFECYCLE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn initialize_cpp_font_rush2_set(
    desired: DesiredPreview,
) -> Result<Vec<ActivePreview>, &'static str> {
    ensure_application_planes_idle("initialize-rush2")?;
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let (scanout_width, scanout_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((PREVIEW_WIDTH, PREVIEW_HEIGHT));
    let canvas_bytes = u64::from(scanout_width)
        .saturating_mul(u64::from(scanout_height))
        .saturating_mul(4);
    let frame_ring_bytes = canvas_bytes
        .saturating_mul(2)
        .saturating_mul(CPP_FONT_RUSH2_PLANE_COUNT as u64);
    crate::log_info!(target: "ui4";
        "ui4 cpp-font-rush2 layout request={} producers={} plane_canvases={} producer_grid={}x{} scanout={}x{} canvas_extent={}x{} canvas_bytes={} frame_ring_bytes={} mapping=producer-modulo-four/second-wave-nonoverlap-band glyph_alignment=independent-cells glyph_fanout={} font_pixels=32,80,128,176 faces=font,noto-sans-sc,inconsolata face_step_ms={} face_switch=exact-buffer-drain+lease-reregister alpha=176..255 gpgpu=nonoverlap-walker-waves publication=one-per-changed-plane\n",
        desired.serial,
        CPP_FONT_RUSH2_PRODUCER_COUNT,
        CPP_FONT_RUSH2_PLANE_COUNT,
        CPP_FONT_RUSH2_COLUMNS,
        CPP_FONT_RUSH2_ROWS,
        scanout_width,
        scanout_height,
        scanout_width,
        scanout_height,
        canvas_bytes,
        frame_ring_bytes,
        CPP_FONT_RUSH2_GLYPHS_PER_ROW,
        CPP_FONT_RUSH2_FACE_MS,
    );
    let session =
        begin_window_session(PREVIEW_OWNER).map_err(|_| "font-rush2-session-create-failed")?;
    let mut previews = Vec::with_capacity(CPP_FONT_RUSH2_PLANE_COUNT);
    let now = Instant::now();
    // Establish the four ordinary UI4 members first. Broker rebalancing then
    // settles them deterministically as Slot0 plus lease Slots1-3 before the
    // first publication starts the normal open transition.
    for plane_index in 0..CPP_FONT_RUSH2_PLANE_COUNT {
        let frame = match create_frame(FrameSpec {
            output,
            content: FrameContent::FontScene2d,
            cadence: FrameCadence::Dirty,
            buffering: crate::ui4::FrameBuffering::Double,
            format: ScanoutFormat::Rgba8888Premultiplied,
            width: scanout_width,
            height: scanout_height,
            base_color: Some(PremultipliedRgba8::TRANSPARENT),
        }) {
            Ok(frame) => frame,
            Err(error) => {
                let usage = crate::ui4::ui4_live_resource_usage();
                let pmm = crate::phys::pmm_stats();
                crate::log_warn!(target: "ui4";
                    "ui4 cpp-font-rush2 frame creation rejected request={} plane={} extent={}x{} buffering=double error={:?} active_frames={} active_sessions={} live_windows={} pmm_free_bytes={} pmm_largest_free_bytes={} pmm_free_regions={}\n",
                    desired.serial,
                    plane_index,
                    scanout_width,
                    scanout_height,
                    error,
                    usage.active_frames,
                    usage.active_sessions,
                    usage.live_windows,
                    pmm.map_or(0, |stats| stats.free_bytes),
                    pmm.map_or(0, |stats| stats.largest_free_region),
                    pmm.map_or(0, |stats| stats.free_regions),
                );
                abandon_compute_preview_initialization(session, &previews);
                return Err("font-rush2-frame-create-failed");
            }
        };
        let window = match create_window(WindowCreate {
            owner: PREVIEW_OWNER,
            session,
            frame,
            output,
            plane: if plane_index == 0 {
                WindowPlane::Primary
            } else {
                WindowPlane::Universal(plane_index as u8)
            },
            placement: WindowPlacement {
                x: 0,
                y: 0,
                width: scanout_width,
                height: scanout_height,
                z: PREVIEW_Z + plane_index as i32,
                opacity: u8::MAX,
                visible: true,
            },
            interaction: crate::ui4::WindowInteraction {
                movable: false,
                maximizable: false,
                receives_input: false,
                hit_testable: false,
                // The normal UI4 open transition projects the stable full-size
                // source through a plane scaler before settling to 1:1.
                resize_on_maximize: true,
            },
        }) {
            Ok(window) => window,
            Err(_) => {
                let _ = destroy_frame(frame);
                abandon_compute_preview_initialization(session, &previews);
                return Err("font-rush2-window-create-failed");
            }
        };
        let rush2 = CppFontRush2PlaneState {
            plane_index: plane_index as u8,
            workers: Vec::with_capacity(CPP_FONT_RUSH2_ROWS as usize),
            font_index: 0,
            font_activated_at: now,
            lease_phase: CppFontRush2LeasePhase::Running,
            active_workers: 0,
            epoch_workers: 0,
            pending: None,
            building: Vec::with_capacity(CPP_FONT_RUSH2_ROWS as usize),
            published: [None, None],
            epoch_started_ns: 0,
        };
        let mut config = desired.config;
        config.preset = GpgpuPreviewPreset::CppFontRush2;
        previews.push(ActivePreview {
            request_serial: desired.serial,
            config,
            policy: desired.policy,
            cadence_phase: 0,
            session,
            frame,
            window,
            width: scanout_width,
            height: scanout_height,
            resize_retry_width: 0,
            resize_retry_height: 0,
            resize_retry_at: now,
            pending_resize_previous_frame: None,
            pending_resize_logical_extent: None,
            pending_resize_epoch: None,
            committed_logical_extent: (scanout_width, scanout_height),
            started: now,
            next_render: now,
            static_needs_publish: false,
            extra_surfaces: Vec::new(),
            particle_craft: None,
            cloud_brush: CloudBrushState::new(),
            font_stamp: None,
            font_rush2: Some(rush2),
            metrics: GpgpuPreviewMetrics::default(),
        });
    }

    for producer_index in 0..CPP_FONT_RUSH2_PRODUCER_COUNT {
        let font_pixels = 32.0 + (producer_index % CPP_FONT_RUSH2_PLANE_COUNT) as f32 * 48.0;
        let registration = crate::r::services::font_producer_service::FontProducerRegistration {
            face: crate::intel::gpu_font::GpuFontFace::Default.id() as u16,
            tier: cpp_font_rush2_tier(producer_index),
            font_pixels_milli: (font_pixels * 1_000.0) as u32,
            row_width_px: scanout_width,
            row_height_px: scanout_height,
            format:
                crate::r::services::font_producer_service::FontProducerFormat::Rgba8Premultiplied,
            max_chars: CPP_FONT_RUSH2_GLYPHS_PER_ROW,
            row_ring_depth: 2,
        };
        let producer = match crate::r::services::font_kernel_service::register_ui4_gpu_font_producer(
            registration,
        ) {
            Ok(producer) => producer,
            Err(error) => {
                crate::log_warn!(target: "ui4";
                    "ui4 cpp-font-rush2 producer registration rejected request={} producer={} plane={} face={} tier={} font_pixels_milli={} extent={}x{} rows={} error={:?}\n",
                    desired.serial,
                    producer_index,
                    producer_index % CPP_FONT_RUSH2_PLANE_COUNT,
                    registration.face,
                    registration.tier,
                    registration.font_pixels_milli,
                    registration.row_width_px,
                    registration.row_height_px,
                    registration.row_ring_depth,
                    error,
                );
                abandon_compute_preview_initialization(session, &previews);
                return Err("font-rush2-producer-register-failed");
            }
        };
        let worker = FontProducerWorker {
            producer,
            producer_index: producer_index as u8,
            font: CPP_FONT_RUSH2_FACES[0],
            font_pixels,
            rng: crate::tyche::SoftRng::from_seed(
                desired.serial ^ (producer_index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15),
            ),
        };
        previews[producer_index % CPP_FONT_RUSH2_PLANE_COUNT]
            .font_rush2
            .as_mut()
            .expect("rush2 plane state exists")
            .workers
            .push(worker);
    }

    // Publish the initialized transparent fronts only after all four windows
    // exist, so the broker's final Slot0-3 assignment precedes the ordinary
    // open transition and never changes beneath a producer publication.
    let initial_publication = previews.iter_mut().try_for_each(|preview| {
        let lease = acquire_frame_buffer(preview.frame)
            .map_err(|_| "font-rush2-initial-frame-acquire-failed")?;
        if publish_frame_buffer(lease).is_err() {
            let _ = cancel_frame_buffer(lease);
            return Err("font-rush2-initial-frame-publish-failed");
        }
        publish_window_frame(PREVIEW_OWNER, preview.window, DamageRect::FULL)
            .map_err(|_| "font-rush2-initial-window-publish-failed")?;
        Ok(())
    });
    if let Err(reason) = initial_publication {
        abandon_compute_preview_initialization(session, &previews);
        return Err(reason);
    }
    Ok(previews)
}

pub(super) fn grow_cpp_font_rush2(
    previews: &mut [ActivePreview],
    now: Instant,
) -> Result<(), &'static str> {
    let (lease_phase, font_index, font_activated_at) = previews
        .first()
        .and_then(|preview| preview.font_rush2.as_ref())
        .map(|state| (state.lease_phase, state.font_index, state.font_activated_at))
        .ok_or("font-rush2-plane-state-missing")?;
    if lease_phase == CppFontRush2LeasePhase::Running
        && now.saturating_duration_since(font_activated_at).as_millis() >= CPP_FONT_RUSH2_FACE_MS
    {
        let next_font_index = cpp_font_rush2_next_face_index(usize::from(font_index));
        let next_font = CPP_FONT_RUSH2_FACES[next_font_index];
        crate::intel::gpu_font::ensure_font_face_available(next_font)
            .map_err(|_| "font-rush2-next-face-unavailable")?;

        for preview in previews.iter_mut() {
            let state = preview
                .font_rush2
                .as_mut()
                .ok_or("font-rush2-plane-state-missing")?;
            if state.font_index != font_index {
                return Err("font-rush2-face-generation-mismatch");
            }
            state.lease_phase = CppFontRush2LeasePhase::Draining;
            state.active_workers = 0;
            preview.static_needs_publish = true;
        }
        crate::log_info!(target: "ui4";
            "ui4 cpp-font-rush2 face switch draining request={} from_face={} from_font={} planes={} producers={} interval_ms={} action=stop-admission+exact-buffer-reacquire-before-unregister\n",
            previews[0].request_serial,
            CPP_FONT_RUSH2_FACES[usize::from(font_index)].id(),
            CPP_FONT_RUSH2_FACES[usize::from(font_index)].registry_name(),
            previews.len(),
            previews.iter().filter_map(|preview| preview.font_rush2.as_ref()).map(|state| state.workers.len()).sum::<usize>(),
            CPP_FONT_RUSH2_FACE_MS,
        );
        return Ok(());
    }
    if lease_phase == CppFontRush2LeasePhase::Draining {
        let drained = previews.iter().all(|preview| {
            preview.font_rush2.as_ref().is_some_and(|state| {
                state.pending.is_none()
                    && state.building.is_empty()
                    && state.published.iter().all(Option::is_none)
            })
        });
        if !drained {
            for preview in previews.iter_mut() {
                preview.static_needs_publish = true;
            }
            return Ok(());
        }

        let next_font_index = cpp_font_rush2_next_face_index(usize::from(font_index));
        let next_font = CPP_FONT_RUSH2_FACES[next_font_index];
        crate::intel::gpu_font::ensure_font_face_available(next_font)
            .map_err(|_| "font-rush2-next-face-unavailable")?;
        for preview in previews.iter_mut() {
            let state = preview
                .font_rush2
                .as_mut()
                .ok_or("font-rush2-plane-state-missing")?;
            for worker in &state.workers {
                if !worker
                    .producer
                    .request_release()
                    .map_err(|_| "font-rush2-producer-release-failed")?
                {
                    return Err("font-rush2-producer-release-deferred");
                }
            }
            state.workers.clear();
        }

        register_cpp_font_rush2_workers(previews, next_font, next_font_index as u8)?;
        for preview in previews.iter_mut() {
            let state = preview
                .font_rush2
                .as_mut()
                .ok_or("font-rush2-plane-state-missing")?;
            state.font_index = next_font_index as u8;
            state.font_activated_at = now;
            state.lease_phase = CppFontRush2LeasePhase::Running;
            preview.static_needs_publish = true;
        }
        crate::log_info!(target: "ui4";
            "ui4 cpp-font-rush2 face switch complete request={} face={} font={} planes={} producers={} tiers=32,80,128,176 cache=fresh-per-lease action=old-generation-released+new-generation-registered\n",
            previews[0].request_serial,
            next_font.id(),
            next_font.registry_name(),
            previews.len(),
            CPP_FONT_RUSH2_PRODUCER_COUNT,
        );
    }

    let elapsed = now
        .saturating_duration_since(previews.first().map_or(now, |p| p.started))
        .as_millis();
    let rung = (elapsed / CPP_FONT_RUSH2_STAGE_MS) as usize;
    let active = CPP_FONT_RUSH2_LADDER[rung.min(CPP_FONT_RUSH2_LADDER.len() - 1)];
    for preview in previews.iter_mut() {
        let state = preview
            .font_rush2
            .as_mut()
            .ok_or("font-rush2-plane-state-missing")?;
        let active_workers = state
            .workers
            .iter()
            .filter(|worker| usize::from(worker.producer_index) < active)
            .count();
        state.active_workers = active_workers;
        preview.static_needs_publish = active_workers != 0;
    }
    Ok(())
}

fn register_cpp_font_rush2_workers(
    previews: &mut [ActivePreview],
    font: crate::intel::gpu_font::GpuFontFace,
    font_generation: u8,
) -> Result<(), &'static str> {
    if previews.len() != CPP_FONT_RUSH2_PLANE_COUNT {
        return Err("font-rush2-plane-count-mismatch");
    }
    let request_serial = previews[0].request_serial;
    let width = previews[0].width;
    let height = previews[0].height;
    for producer_index in 0..CPP_FONT_RUSH2_PRODUCER_COUNT {
        let font_pixels = 32.0 + (producer_index % CPP_FONT_RUSH2_PLANE_COUNT) as f32 * 48.0;
        let registration = crate::r::services::font_producer_service::FontProducerRegistration {
            face: font.id() as u16,
            tier: cpp_font_rush2_tier(producer_index),
            font_pixels_milli: (font_pixels * 1_000.0) as u32,
            row_width_px: width,
            row_height_px: height,
            format:
                crate::r::services::font_producer_service::FontProducerFormat::Rgba8Premultiplied,
            max_chars: CPP_FONT_RUSH2_GLYPHS_PER_ROW,
            row_ring_depth: 2,
        };
        let producer =
            crate::r::services::font_kernel_service::register_ui4_gpu_font_producer(registration)
                .map_err(|_| "font-rush2-producer-reregister-failed")?;
        previews[producer_index % CPP_FONT_RUSH2_PLANE_COUNT]
            .font_rush2
            .as_mut()
            .ok_or("font-rush2-plane-state-missing")?
            .workers
            .push(FontProducerWorker {
                producer,
                producer_index: producer_index as u8,
                font,
                font_pixels,
                rng: crate::tyche::SoftRng::from_seed(
                    request_serial
                        ^ (u64::from(font_generation) << 56)
                        ^ (producer_index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15),
                ),
            });
    }
    Ok(())
}

pub(super) fn render_cpp_font_rush2_frame(preview: &mut ActivePreview) -> Result<(), &'static str> {
    use crate::r::services::font_kernel_service::{FontGpuProducerError, FontKernelError};
    use crate::r::services::font_producer_service::FontProducerError;

    preview.metrics.attempted = preview.metrics.attempted.saturating_add(1);

    // UI4 records the exact publish serial only after the compositor's batched
    // plane transaction crosses SURFLIVE. Keep producer control state honest:
    // publication alone is not a display completion.
    {
        let state = preview
            .font_rush2
            .as_mut()
            .ok_or("font-rush2-plane-state-missing")?;
        for published in state.published.iter_mut().flatten() {
            if published.surflive_ns.is_none()
                && window_frame_was_presented(
                    PREVIEW_OWNER,
                    preview.window,
                    published.publish_serial,
                )
            {
                for row in &mut published.rows {
                    row.mark_surflive()
                        .map_err(|_| "font-rush2-surflive-state-failed")?;
                }
                published.surflive_ns = Some(crate::chronos::monotonic_nanos());
            }
        }
    }

    let completion = preview
        .font_rush2
        .as_mut()
        .ok_or("font-rush2-producer-state-missing")?
        .pending
        .as_mut()
        .and_then(|pending| pending.pending.try_take());
    if let Some(completion) = completion {
        let pending = preview
            .font_rush2
            .as_mut()
            .and_then(|state| state.pending.take())
            .ok_or("font-rush2-pending-state-missing")?;
        let produced = match completion {
            Ok(produced) => produced,
            Err(FontKernelError::SubmittedIncomplete(_)) => {
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                // Keep the exact write lease acquired: the service has
                // quarantined an ambiguous GPU write to this allocation.
                return Err("font-rush2-submit-incomplete");
            }
            Err(_) => {
                let _ = cancel_frame_buffer(pending.lease);
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("font-rush2-stamp-failed");
            }
        };
        let index = usize::from(pending.lease.buffer_index);
        let exact_surface =
            gpgpu_rgba_surface(pending.lease).map_err(|_| "font-rush2-surface-lost")?;
        if produced.token().row_index() as usize != index
            || !produced
                .stamp()
                .release()
                .matches(exact_surface.phys, exact_surface.bytes)
        {
            let _ = cancel_frame_buffer(pending.lease);
            return Err("font-rush2-row-buffer-mismatch");
        }
        preview.metrics.completed = preview.metrics.completed.saturating_add(1);
        let state = preview
            .font_rush2
            .as_mut()
            .ok_or("font-rush2-plane-state-missing")?;
        state.building.push(produced);
        let next_worker = pending.worker.saturating_add(1);
        if next_worker < state.epoch_workers {
            let worker = state
                .workers
                .get_mut(next_worker)
                .ok_or("font-rush2-worker-missing")?;
            let request = cpp_font_rush2_request(worker, preview.width, preview.height);
            let destination =
                gpgpu_rgba_surface(pending.lease).map_err(|_| "font-rush2-row-surface-failed")?;
            let next = worker.producer.submit_ui4_row_over(
                request,
                destination,
                pending.lease.buffer_index,
            );
            let next = match next {
                Ok(next) => next,
                Err(error) => {
                    let _ = cancel_frame_buffer(pending.lease);
                    return Err(match error {
                        FontGpuProducerError::Kernel(FontKernelError::QueueFull) => {
                            "font-rush2-continuation-queue-full"
                        }
                        FontGpuProducerError::Control(FontProducerError::NoCredits) => {
                            "font-rush2-continuation-no-credit"
                        }
                        _ => "font-rush2-continuation-submit-failed",
                    });
                }
            };
            state.pending = Some(CppFontRush2PendingRow {
                lease: pending.lease,
                worker: next_worker,
                pending: next,
            });
            preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
            return Ok(());
        }

        let release = state
            .building
            .last()
            .ok_or("font-rush2-plane-build-empty")?
            .stamp()
            .release();
        let gpu_ready_ns = crate::chronos::monotonic_nanos();
        let frame_publish = publish_gpu_font_frame_buffer(pending.lease, release)
            .map_err(|_| "font-rush2-frame-publish-failed")?;
        let window_publish_serial =
            publish_window_frame(PREVIEW_OWNER, preview.window, DamageRect::FULL)
                .map_err(|_| "font-rush2-window-publish-failed")?;
        let published_ns = crate::chronos::monotonic_nanos();
        if state.published[index]
            .replace(PublishedFontPlane {
                rows: core::mem::take(&mut state.building),
                publish_serial: window_publish_serial,
                epoch_started_ns: state.epoch_started_ns,
                gpu_ready_ns,
                published_ns,
                surflive_ns: None,
            })
            .is_some()
        {
            return Err("font-rush2-buffer-capability-overwrite");
        }
        state.epoch_workers = 0;
        preview.metrics.published = preview.metrics.published.saturating_add(1);
        preview.metrics.last_marker = frame_publish.publish_serial as u32;
        preview.static_needs_publish = false;
    }

    if preview
        .font_rush2
        .as_ref()
        .is_some_and(|state| state.pending.is_some())
    {
        return Ok(());
    }
    let lease = match acquire_frame_buffer(preview.frame) {
        Ok(lease) => lease,
        Err(FramePoolError::Busy) => {
            preview.metrics.dropped_busy += 1;
            return Ok(());
        }
        Err(_) => {
            preview.metrics.failed += 1;
            return Err("font-rush2-frame-acquire-failed");
        }
    };
    let state = preview
        .font_rush2
        .as_mut()
        .ok_or("font-rush2-plane-state-missing")?;
    let index = usize::from(lease.buffer_index);
    if let Some(mut displayed) = state.published[index].take() {
        let reacquired_ns = crate::chronos::monotonic_nanos();
        let crossed_surflive = displayed.surflive_ns.is_some();
        let worker_count = displayed.rows.len();
        for row in displayed.rows.drain(..) {
            let acknowledged = if crossed_surflive {
                row.acknowledge_display_release()
            } else {
                row.acknowledge_unpresented_reacquire()
            };
            if acknowledged.is_err() {
                let _ = cancel_frame_buffer(lease);
                return Err("font-rush2-display-ack-failed");
            }
        }
        let lifecycle_sequence = CPP_FONT_RUSH2_LIFECYCLE_SEQUENCE
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        if lifecycle_sequence <= 8 || lifecycle_sequence.is_multiple_of(120) {
            let surflive_ns = displayed.surflive_ns.unwrap_or(reacquired_ns);
            crate::log_info!(target: "render";
                "ui4 cpp-font-rush2 lifecycle sample={} request={} plane={} frame={} buffer={} publish_serial={} workers={} epoch_to_gpu_ready_us={} gpu_ready_to_publish_us={} publish_to_surflive_us={} surflive_to_reacquire_us={} total_epoch_to_reacquire_us={} presentation={} ownership=exact-buffer-reacquire telemetry=first8+every120\n",
                lifecycle_sequence,
                preview.request_serial,
                state.plane_index,
                preview.frame.raw(),
                lease.buffer_index,
                displayed.publish_serial,
                worker_count,
                displayed.gpu_ready_ns.saturating_sub(displayed.epoch_started_ns) / 1_000,
                displayed.published_ns.saturating_sub(displayed.gpu_ready_ns) / 1_000,
                surflive_ns.saturating_sub(displayed.published_ns) / 1_000,
                reacquired_ns.saturating_sub(surflive_ns) / 1_000,
                reacquired_ns.saturating_sub(displayed.epoch_started_ns) / 1_000,
                if crossed_surflive { "surflive" } else { "coalesced-before-surflive" },
            );
        }
    }
    if state.lease_phase == CppFontRush2LeasePhase::Draining {
        if state.published.iter().any(Option::is_some) {
            // This buffer's producer token was just ACKed. Re-publish its
            // already-complete pixels without attaching a new token so UI4
            // can displace and return the final live producer generation.
            publish_frame_buffer(lease).map_err(|_| "font-rush2-drain-frame-publish-failed")?;
            publish_window_frame(PREVIEW_OWNER, preview.window, DamageRect::FULL)
                .map_err(|_| "font-rush2-drain-window-publish-failed")?;
        } else {
            let _ = cancel_frame_buffer(lease);
            preview.static_needs_publish = false;
        }
        return Ok(());
    }
    if state.active_workers == 0 {
        let _ = cancel_frame_buffer(lease);
        return Ok(());
    }
    let destination = match gpgpu_rgba_surface(lease) {
        Ok(surface) => surface,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed += 1;
            return Err("font-rush2-row-surface-failed");
        }
    };
    state.epoch_workers = state.active_workers.min(state.workers.len());
    state.epoch_started_ns = crate::chronos::monotonic_nanos();
    let worker = state
        .workers
        .first_mut()
        .ok_or("font-rush2-worker-missing")?;
    let request = cpp_font_rush2_request(worker, preview.width, preview.height);
    let result = worker.producer.submit_ui4_row(
        request,
        destination,
        lease.buffer_index,
        u32::from_le_bytes(PremultipliedRgba8::TRANSPARENT.to_native_bytes()),
    );
    let pending = match result {
        Ok(pending) => pending,
        Err(FontGpuProducerError::Kernel(FontKernelError::QueueFull))
        | Err(FontGpuProducerError::Control(FontProducerError::NoCredits)) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.dropped_queue_full =
                preview.metrics.dropped_queue_full.saturating_add(1);
            return Ok(());
        }
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-rush2-submit-failed");
        }
    };
    state.pending = Some(CppFontRush2PendingRow {
        lease,
        worker: 0,
        pending,
    });
    preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
    Ok(())
}

fn cpp_font_rush2_request(
    state: &mut FontProducerWorker,
    width: u32,
    height: u32,
) -> crate::r::services::font_kernel_service::FontStampRequest {
    use crate::r::services::font_kernel_service::{
        FontStampFit, FontStampLayer, FontStampRequest, RetainSceneRequest,
        RetainedFontPositioning, RetainedFontRun,
    };

    let roll = state.rng.next_u64();
    let cell_width = width as f32 / CPP_FONT_RUSH2_GLYPHS_PER_ROW as f32;
    let plane_index = u32::from(state.producer_index) % CPP_FONT_RUSH2_COLUMNS;
    let producer_row = u32::from(state.producer_index) / CPP_FONT_RUSH2_COLUMNS;
    let band_top = height.saturating_mul(producer_row) / CPP_FONT_RUSH2_ROWS;
    let band_bottom = height.saturating_mul(producer_row + 1) / CPP_FONT_RUSH2_ROWS;
    let band_height = band_bottom.saturating_sub(band_top);
    // Preserve the shared full-screen plane canvases and blend contract, but
    // keep their samples from landing exactly atop one another. The stagger
    // is small relative to both the glyph cells and each producer band.
    let stagger_x = plane_index as f32 * state.font_pixels * 0.45;
    let stagger_y = plane_index as f32 * state.font_pixels * 0.35;
    let baseline_y = (band_top as f32 + band_height as f32 * 0.5 - state.font_pixels * 0.65
        + stagger_y)
        .max(band_top as f32);
    let mut runs = Vec::with_capacity(CPP_FONT_RUSH2_GLYPHS_PER_ROW);
    for cell in 0..CPP_FONT_RUSH2_GLYPHS_PER_ROW {
        let glyph_roll = state.rng.next_u64();
        let scalar = char::from_u32(33 + (glyph_roll % 94) as u32).unwrap_or('?');
        let mut text = String::new();
        text.push(scalar);
        runs.push(RetainedFontRun {
            text,
            // Each glyph starts in its own cell. Exact visual bounds are
            // checked later by the RCS encoder; only genuinely disjoint
            // rectangles are admitted to the same dependency-free wave.
            position: [
                cell as f32 * cell_width + cell_width * 0.1 + stagger_x,
                baseline_y,
            ],
            font_pixels: state.font_pixels,
            slant: 0.0,
        });
    }
    // Keep every randomized color readable on the ordinary UI4 white scene
    // while retaining per-row RGB and alpha changes over time.
    let (red, green, blue) = match (roll >> 40) as u8 & 7 {
        0 => (210, 32, 72),
        1 => (28, 112, 210),
        2 => (22, 146, 76),
        3 => (126, 46, 210),
        4 => (210, 92, 18),
        5 => (16, 138, 160),
        6 => (178, 32, 166),
        _ => (48, 56, 196),
    };
    let alpha = 176u8.saturating_add((roll >> 32) as u8 % 80);
    FontStampRequest {
        fit: FontStampFit::Canvas,
        layers: alloc::vec![FontStampLayer {
            scene: RetainSceneRequest {
                runs,
                font: state.font,
                viewport_width: width,
                viewport_height: height,
                raster_width: width,
                raster_height: height,
                positioning: RetainedFontPositioning::SceneOrigin,
            },
            foreground: crate::intel::gpu_font::GpuFontRgba::new(red, green, blue, alpha),
        }],
    }
}

pub(super) fn retire_cpp_font_rush2_frames() {
    use crate::r::services::font_kernel_service::FontKernelError;

    let mut retired = CPP_FONT_RUSH2_RETIRED.lock();
    let mut index = 0;
    while index < retired.len() {
        if let Some(mut pending) = retired[index].state.pending.take() {
            let ticket = pending.pending.ticket();
            let completion = pending.pending.try_take();
            match completion {
                None => {
                    retired[index].state.pending = Some(pending);
                    index += 1;
                    continue;
                }
                Some(Ok(produced)) => {
                    let _ = cancel_frame_buffer(pending.lease);
                    retired[index].state.building.push(produced);
                    crate::log_info!(target: "ui4";
                        "ui4 cpp-font-rush2 retired producer drained ticket={} frame={} buffer={} action=cancel-unpublished-write+continue-frame-retirement\n",
                        ticket.raw(),
                        pending.lease.frame.raw(),
                        pending.lease.buffer_index,
                    );
                }
                Some(Err(FontKernelError::SubmittedIncomplete(reason))) => {
                    crate::log_error!(target: "ui4";
                        "ui4 cpp-font-rush2 retired producer quarantined ticket={} frame={} buffer={} reason={} action=retain-frame-write-lease+continue-ui4-close\n",
                        ticket.raw(),
                        pending.lease.frame.raw(),
                        pending.lease.buffer_index,
                        reason,
                    );
                }
                Some(Err(error)) => {
                    let _ = cancel_frame_buffer(pending.lease);
                    crate::log_warn!(target: "ui4";
                        "ui4 cpp-font-rush2 retired producer failed ticket={} frame={} buffer={} error={:?} action=cancel-unpublished-write+continue-frame-retirement\n",
                        ticket.raw(),
                        pending.lease.frame.raw(),
                        pending.lease.buffer_index,
                        error,
                    );
                }
            }
        }
        match destroy_frame(retired[index].frame) {
            Ok(()) | Err(FramePoolError::InvalidHandle) => {
                let mut entry = retired.swap_remove(index);
                let mut ack_failed = false;
                for published in &mut entry.state.published {
                    if let Some(mut published) = published.take() {
                        for row in published.rows.drain(..) {
                            ack_failed |= row.acknowledge_ui4_frame_retirement().is_err();
                        }
                    }
                }
                for row in entry.state.building.drain(..) {
                    ack_failed |= row.acknowledge_ui4_frame_retirement().is_err();
                }
                crate::log_info!(target: "ui4";
                    "ui4 cpp-font-rush2 frame retired frame={} plane={} producers={} exact_row_acks={} action=destroy-ui4-ring+release-producer-generations\n",
                    entry.frame.raw(),
                    entry.state.plane_index,
                    entry.state.workers.len(),
                    if ack_failed { "failed-quarantined" } else { "complete" },
                );
                drop(entry);
            }
            Err(FramePoolError::Busy) => index += 1,
            Err(error) => {
                let entry = retired.swap_remove(index);
                crate::log_warn!(target: "ui4";
                    "ui4 cpp-font-rush2 frame retirement abandoned frame={} plane={} producers={} error={:?} action=quarantine-generations\n",
                    entry.frame.raw(),
                    entry.state.plane_index,
                    entry.state.workers.len(),
                    error,
                );
                drop(entry);
            }
        }
    }
}

impl CppFontRush2PlaneState {
    pub(super) fn plane_index(&self) -> u8 {
        self.plane_index
    }
}

pub(super) fn retire(frame: FrameHandle, state: CppFontRush2PlaneState) {
    CPP_FONT_RUSH2_RETIRED
        .lock()
        .push(CppFontRush2RetiredFrame { frame, state });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpp_font_rush2_rotates_all_registered_faces_every_thirty_seconds() {
        use crate::intel::gpu_font::GpuFontFace;

        assert_eq!(CPP_FONT_RUSH2_FACE_MS, 30_000);
        assert_eq!(
            CPP_FONT_RUSH2_FACES,
            [
                GpuFontFace::Default,
                GpuFontFace::NotoSansSc,
                GpuFontFace::Inconsolata,
            ]
        );
        assert_eq!(cpp_font_rush2_next_face_index(0), 1);
        assert_eq!(cpp_font_rush2_next_face_index(1), 2);
        assert_eq!(cpp_font_rush2_next_face_index(2), 0);
    }
}
