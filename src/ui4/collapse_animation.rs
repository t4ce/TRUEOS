//! A collapse owns one captured generation. Producer extent and resize epochs
//! never change; exact-sized immutable backings also work on the BCS stack.
#[path = "collapse_animation/raster.rs"]
mod raster;

use alloc::vec::Vec;
use spin::Mutex;

use super::{
    CollapseError, FrameBuffering, FrameCadence, FrameContent, FrameHandle, FrameReadLease,
    FrameRgbaView, FrameSpec, FrameWriteLease, OutputId, ScanoutFormat, WindowId, WindowOwner,
    WindowPlacement, WindowSnapshot, acquire_frame_buffer, acquire_published_frame,
    cancel_frame_buffer, create_frame, publish_frame_buffer, published_rgba_view,
    release_published_frame, writable_rgba_view,
};
use crate::intel::{
    GucBcs0CopyCompletion, GucBcs0CopySubmission, GucBcs0CopySubmitError, GucBcs0RgbaCopy,
    GucBcs0RgbaSurface,
};
use raster::{Geometry, Image};

const _: () = assert!(raster::ICON_SIZE == super::collapse::COLLAPSED_SIZE);

const CAPTURE_WAIT_MS: u64 = 500;
static ANIMATIONS: Mutex<Vec<Animation>> = Mutex::new(Vec::new());
// Ambiguous submitted work can still access both sides. Retaining the leases
// pins their storage until reboot, matching the BCS lane's quarantine policy.
static QUARANTINED: Mutex<Vec<Capture>> = Mutex::new(Vec::new());

struct Pixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Pixels {
    fn image(&self) -> Image<'_> {
        Image {
            width: self.width,
            height: self.height,
            pitch: self.width as usize * 4,
            pixels: &self.rgba,
        }
    }
}

struct CapturedSource {
    lease: FrameReadLease,
    view: FrameRgbaView,
    scanout_cache: bool,
}

impl CapturedSource {
    fn acquire(mut window: WindowSnapshot, frame: FrameHandle) -> Result<Self, CollapseError> {
        let lease = acquire_published_frame(frame)?;
        let result = (|| {
            let view = published_rgba_view(lease)?;
            if view.gpu_authored
                && !view
                    .gpu_release
                    .is_some_and(|release| release.matches(view.phys, view.byte_len))
            {
                return Err(CollapseError::Ui4);
            }
            window.frame = frame;
            let scanout_cache =
                super::compositor_service::composition_source_uses_scanout_cache(&window, view);
            Ok(Self {
                lease,
                view,
                scanout_cache,
            })
        })();
        if result.is_err() {
            let _ = release_published_frame(lease);
        }
        result
    }
}

struct Capture {
    frame: FrameHandle,
    write: FrameWriteLease,
    destination: FrameRgbaView,
    foreground: CapturedSource,
    background: Option<CapturedSource>,
    background_opacity: u8,
    arc: u16,
    submission: Option<GucBcs0CopySubmission>,
    requested_ms: u64,
}

enum CapturePoll {
    Pending,
    Ready,
    Aborted,
    Quarantined,
}

impl Capture {
    fn new(window: WindowSnapshot) -> Result<Self, CollapseError> {
        let foreground = CapturedSource::acquire(window, window.frame)?;
        let background = match window.background {
            Some(background) => match CapturedSource::acquire(window, background.frame) {
                Ok(source) => Some(source),
                Err(error) => {
                    let _ = release_published_frame(foreground.lease);
                    return Err(error);
                }
            },
            None => None,
        };
        let result = (|| {
            let width = background
                .as_ref()
                .map_or(foreground.view.width, |background| {
                    foreground.view.width.max(background.view.width)
                });
            let height = foreground
                .view
                .height
                .checked_add(
                    background
                        .as_ref()
                        .map_or(0, |background| background.view.height),
                )
                .ok_or(CollapseError::Ui4)?;
            let frame = create_image(window.output, width, height)?;
            let write = match acquire_frame_buffer(frame) {
                Ok(write) => write,
                Err(error) => {
                    retire(frame);
                    return Err(error.into());
                }
            };
            match writable_rgba_view(write) {
                Ok(destination) => {
                    // Evict CPU initialization before BCS may overwrite it.
                    crate::intel::dma_flush(destination.virt, destination.byte_len);
                    Ok((frame, write, destination))
                }
                Err(error) => {
                    let _ = cancel_frame_buffer(write);
                    retire(frame);
                    Err(error.into())
                }
            }
        })();
        let (frame, write, destination) = match result {
            Ok(result) => result,
            Err(error) => {
                let _ = release_published_frame(foreground.lease);
                if let Some(background) = background {
                    let _ = release_published_frame(background.lease);
                }
                return Err(error);
            }
        };
        Ok(Self {
            frame,
            write,
            destination,
            foreground,
            background,
            background_opacity: window
                .background
                .map_or(255, |background| background.opacity),
            arc: window.arc,
            submission: None,
            requested_ms: trueos_time::Instant::now().as_millis(),
        })
    }

    fn poll(&mut self, now_ms: u64) -> CapturePoll {
        if let Some(submission) = self.submission {
            return match crate::r::services::vcpy_service::poll_rgba_copies(submission) {
                GucBcs0CopyCompletion::Pending => CapturePoll::Pending,
                GucBcs0CopyCompletion::Complete => CapturePoll::Ready,
                GucBcs0CopyCompletion::Failed | GucBcs0CopyCompletion::InvalidSubmission => {
                    CapturePoll::Quarantined
                }
            };
        }
        let mut copies = Vec::with_capacity(2);
        let mut cache = Vec::with_capacity(2);
        for (source, destination_y) in core::iter::once((&self.foreground, 0)).chain(
            self.background
                .as_ref()
                .map(|background| (background, self.foreground.view.height)),
        ) {
            if !source.view.gpu_authored {
                crate::intel::dma_flush(source.view.virt, source.view.byte_len);
            }
            copies.push(GucBcs0RgbaCopy {
                source: surface(source.view),
                source_x: 0,
                source_y: 0,
                destination_x: 0,
                destination_y,
                width: source.view.width,
                height: source.view.height,
            });
            cache.push(source.scanout_cache);
        }
        match crate::r::services::vcpy_service::queue_rgba_copies_with_scanout_cache(
            surface(self.destination),
            &copies,
            &cache,
        ) {
            Ok(submission) => {
                self.submission = Some(submission);
                CapturePoll::Pending
            }
            Err(GucBcs0CopySubmitError::SubmitFailed) => CapturePoll::Quarantined,
            Err(GucBcs0CopySubmitError::InvalidRequest) => CapturePoll::Aborted,
            Err(error @ (GucBcs0CopySubmitError::Busy | GucBcs0CopySubmitError::Unavailable)) => {
                if !self.foreground.view.gpu_authored
                    && self
                        .background
                        .as_ref()
                        .is_none_or(|background| !background.view.gpu_authored)
                {
                    // A CPU-owned generation needs no GPU cache conversion.
                    // Do one snapshot copy when the shared BCS lane is busy.
                    for (source, y) in core::iter::once((&self.foreground, 0)).chain(
                        self.background
                            .as_ref()
                            .map(|background| (background, self.foreground.view.height)),
                    ) {
                        for row in 0..source.view.height as usize {
                            unsafe {
                                core::ptr::copy_nonoverlapping(
                                    source.view.virt.add(row * source.view.pitch as usize),
                                    self.destination
                                        .virt
                                        .add((y as usize + row) * self.destination.pitch as usize),
                                    source.view.width as usize * 4,
                                );
                            }
                        }
                    }
                    CapturePoll::Ready
                } else if matches!(error, GucBcs0CopySubmitError::Unavailable)
                    || now_ms.saturating_sub(self.requested_ms) >= CAPTURE_WAIT_MS
                {
                    CapturePoll::Aborted
                } else {
                    CapturePoll::Pending
                }
            }
        }
    }

    fn pixels(&self) -> Pixels {
        crate::intel::dma_flush(self.destination.virt, self.destination.byte_len);
        // BCS completion pins and retires this exact allocation before any CPU
        // sampling. GPU-authored producer aliases are never read by the CPU.
        let bytes = unsafe {
            core::slice::from_raw_parts(
                self.destination.virt.cast_const(),
                self.destination.byte_len,
            )
        };
        let foreground = Image {
            width: self.foreground.view.width,
            height: self.foreground.view.height,
            pitch: self.destination.pitch as usize,
            pixels: bytes,
        };
        let background = self.background.as_ref().map(|background| Image {
            width: background.view.width,
            height: background.view.height,
            pitch: self.destination.pitch as usize,
            pixels: &bytes
                [self.foreground.view.height as usize * self.destination.pitch as usize..],
        });
        let mut rgba = alloc::vec![0; foreground.width as usize * foreground.height as usize * 4];
        if background.is_none() && self.arc == 0 {
            let row_bytes = foreground.width as usize * 4;
            for row in 0..foreground.height as usize {
                rgba[row * row_bytes..(row + 1) * row_bytes].copy_from_slice(
                    &bytes[row * foreground.pitch..row * foreground.pitch + row_bytes],
                );
            }
            return Pixels {
                width: foreground.width,
                height: foreground.height,
                rgba,
            };
        }
        for y in 0..foreground.height {
            for x in 0..foreground.width {
                if !super::window_arc::contains(foreground.width, foreground.height, self.arc, x, y)
                {
                    continue;
                }
                let mut pixel = foreground.sample(x, y, foreground.width, foreground.height);
                if let Some(background) = background {
                    let underneath = background.sample(x, y, foreground.width, foreground.height);
                    for channel in 0..4 {
                        let alpha = u32::from(255 - pixel[3]);
                        let add = (u32::from(underneath[channel])
                            * u32::from(self.background_opacity)
                            * alpha
                            + 32_512)
                            / 65_025;
                        pixel[channel] = (u32::from(pixel[channel]) + add).min(255) as u8;
                    }
                }
                let offset = (y as usize * foreground.width as usize + x as usize) * 4;
                rgba[offset..offset + 4].copy_from_slice(&pixel);
            }
        }
        Pixels {
            width: foreground.width,
            height: foreground.height,
            rgba,
        }
    }

    // Only call before submission, or after exact successful completion.
    fn release(self) {
        let _ = release_published_frame(self.foreground.lease);
        if let Some(background) = self.background {
            let _ = release_published_frame(background.lease);
        }
        let _ = cancel_frame_buffer(self.write);
        retire(self.frame);
    }
}

struct Visual {
    frame: FrameHandle,
    geometry: Geometry,
}

struct Animation {
    owner: WindowOwner,
    window: WindowId,
    source: WindowSnapshot,
    icon_frame: FrameHandle,
    icon: Pixels,
    output: (u32, u32),
    capture: Option<Capture>,
    snapshot: Option<Pixels>,
    visual: Option<Visual>,
    started_ms: Option<u64>,
    cancelled: bool,
}

pub(super) fn begin(source: WindowSnapshot, icon: WindowSnapshot) -> Result<(), CollapseError> {
    cancel(source.owner, source.id);
    let lease = acquire_published_frame(icon.frame)?;
    let pixels = (|| {
        let view = published_rgba_view(lease)?;
        if view.gpu_authored {
            return Err(CollapseError::Ui4);
        }
        crate::intel::dma_flush(view.virt, view.byte_len);
        let mut rgba = alloc::vec![0; view.width as usize * view.height as usize * 4];
        for row in 0..view.height as usize {
            let start = row * view.width as usize * 4;
            let source = unsafe {
                core::slice::from_raw_parts(
                    view.virt.add(row * view.pitch as usize),
                    view.width as usize * 4,
                )
            };
            rgba[start..start + source.len()].copy_from_slice(source);
        }
        Ok(Pixels {
            width: view.width,
            height: view.height,
            rgba,
        })
    })();
    let _ = release_published_frame(lease);
    let icon_pixels = pixels?;
    let capture = Capture::new(source)?;
    let output = super::output_dimensions().unwrap_or((
        source.presentation_placement.width,
        source.presentation_placement.height,
    ));
    ANIMATIONS.lock().push(Animation {
        owner: source.owner,
        window: source.id,
        source,
        icon_frame: icon.frame,
        icon: icon_pixels,
        output,
        capture: Some(capture),
        snapshot: None,
        visual: None,
        started_ms: None,
        cancelled: false,
    });
    // Capture submission belongs to advance(), where the compositor has
    // already retired its preceding BCS transaction. It must never spin.
    Ok(())
}

pub(super) fn project(mut collapsed: WindowSnapshot) -> WindowSnapshot {
    let animations = ANIMATIONS.lock();
    let Some(animation) = animations.iter().find(|animation| {
        !animation.cancelled
            && animation.owner == collapsed.owner
            && animation.window == collapsed.id
            && animation.icon_frame == collapsed.frame
    }) else {
        return collapsed;
    };
    let initial = animation.source.presentation_placement;
    let target = collapsed.placement;
    let Some(visual) = &animation.visual else {
        // Keep the source's exact presentation while capture is in flight,
        // including an opening reveal or a retained resize presentation.
        collapsed.presentation_placement = WindowPlacement {
            visible: target.visible,
            opacity: target.opacity,
            z: target.z,
            ..initial
        };
        collapsed.frame = animation.source.frame;
        collapsed.background = animation.source.background;
        collapsed.buffering = animation.source.buffering;
        collapsed.arc = animation.source.arc;
        collapsed.interaction.resize_on_maximize = animation.source.interaction.resize_on_maximize;
        return collapsed;
    };
    let geometry = visual.geometry;
    let (x, y) = geometry.position(
        (initial.x, initial.y, initial.width, initial.height),
        (
            i64::from(target.x) * 2 + i64::from(target.width),
            i64::from(target.y) * 2 + i64::from(target.height),
        ),
        animation.output,
    );
    collapsed.presentation_placement = WindowPlacement {
        x,
        y,
        width: geometry.width,
        height: geometry.height,
        ..target
    };
    collapsed.frame = visual.frame;
    collapsed.buffering = FrameBuffering::Single;
    collapsed.arc = 0;
    collapsed
}

pub(super) fn active() -> bool {
    !ANIMATIONS.lock().is_empty()
}

pub(super) fn cancel(owner: WindowOwner, window: WindowId) {
    let mut animations = ANIMATIONS.lock();
    let mut index = 0;
    while index < animations.len() {
        if animations[index].owner != owner || animations[index].window != window {
            index += 1;
            continue;
        }
        let animation = &mut animations[index];
        animation.cancelled = true;
        if let Some(visual) = animation.visual.take() {
            retire(visual.frame);
        }
        if animation
            .capture
            .as_ref()
            .is_some_and(|capture| capture.submission.is_some())
        {
            index += 1; // advance() still owes the submitted copy its retirement.
        } else {
            let animation = animations.swap_remove(index);
            if let Some(capture) = animation.capture {
                capture.release();
            }
        }
    }
}

pub(super) fn advance() {
    let now_ms = trueos_time::Instant::now().as_millis();
    let mut changed = Vec::new();
    {
        let mut animations = ANIMATIONS.lock();
        let mut index = 0;
        while index < animations.len() {
            let animation = &mut animations[index];
            let mut remove = false;
            if let Some(capture) = &mut animation.capture {
                match capture.poll(now_ms) {
                    CapturePoll::Pending => {
                        index += 1;
                        continue;
                    }
                    CapturePoll::Ready => {
                        let capture = animation.capture.take().unwrap();
                        if !animation.cancelled {
                            animation.snapshot = Some(capture.pixels());
                            animation.started_ms = Some(now_ms);
                        }
                        capture.release();
                        remove = animation.cancelled;
                    }
                    CapturePoll::Aborted => {
                        animation.capture.take().unwrap().release();
                        remove = true;
                    }
                    CapturePoll::Quarantined => {
                        QUARANTINED.lock().push(animation.capture.take().unwrap());
                        remove = true;
                    }
                }
            }
            if !remove {
                let elapsed = now_ms.saturating_sub(animation.started_ms.unwrap_or(now_ms));
                if elapsed >= raster::DURATION_MS || animation.cancelled {
                    remove = true;
                } else {
                    let initial = animation.source.presentation_placement;
                    let geometry = Geometry::at(initial.width, initial.height, elapsed);
                    if animation
                        .visual
                        .as_ref()
                        .is_some_and(|visual| visual.geometry == geometry)
                    {
                        index += 1;
                        continue;
                    }
                    match render_frame(animation, geometry) {
                        Ok(frame) => {
                            if let Some(previous) =
                                animation.visual.replace(Visual { frame, geometry })
                            {
                                retire(previous.frame);
                            }
                        }
                        Err(_) => remove = true, // Compact presentation remains usable under pressure.
                    }
                }
            }
            changed.push((animation.owner, animation.window));
            if remove {
                let animation = animations.swap_remove(index);
                if let Some(visual) = animation.visual {
                    retire(visual.frame);
                }
            } else {
                index += 1;
            }
        }
    }
    // Window snapshots take broker -> animation locks. Keep the reverse path
    // outside the animation lock, including completion/cancellation notices.
    for (owner, window) in changed {
        super::window_broker::collapse_animation_changed(owner, window);
    }
}

fn render_frame(animation: &Animation, geometry: Geometry) -> Result<FrameHandle, CollapseError> {
    let frame = create_image(animation.source.output, geometry.width, geometry.height)?;
    let result = (|| {
        let lease = acquire_frame_buffer(frame)?;
        let result = (|| {
            let view = writable_rgba_view(lease)?;
            let destination = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
            raster::render(
                animation
                    .snapshot
                    .as_ref()
                    .ok_or(CollapseError::Ui4)?
                    .image(),
                animation.icon.image(),
                geometry,
                destination,
                view.pitch as usize,
            );
            crate::intel::dma_flush(view.virt, view.byte_len);
            publish_frame_buffer(lease)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = cancel_frame_buffer(lease);
        }
        result
    })();
    if let Err(error) = result {
        retire(frame);
        return Err(error);
    }
    Ok(frame)
}

fn create_image(output: OutputId, width: u32, height: u32) -> Result<FrameHandle, CollapseError> {
    create_frame(FrameSpec {
        output,
        content: FrameContent::Image,
        cadence: FrameCadence::Immutable,
        buffering: FrameBuffering::Single,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: None,
    })
    .map_err(Into::into)
}

fn surface(view: FrameRgbaView) -> GucBcs0RgbaSurface {
    GucBcs0RgbaSurface {
        phys: view.phys,
        gpu: view.gpu,
        bytes: view.byte_len,
        width: view.width,
        height: view.height,
        pitch_bytes: view.pitch,
    }
}

fn retire(frame: FrameHandle) {
    super::window_broker::retire_frame_when_released(frame);
}
