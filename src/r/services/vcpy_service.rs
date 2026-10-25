//! Shared BCS0 copy operations and consumer accounting.

use core::sync::atomic::{AtomicU64, Ordering};

pub(crate) fn queue_rgba_copies(
    destination: crate::intel::GucBcs0RgbaSurface,
    copies: &[crate::intel::GucBcs0RgbaCopy],
) -> Result<crate::intel::GucBcs0CopySubmission, crate::intel::GucBcs0CopySubmitError> {
    crate::intel::queue_guc_bcs0_rgba_copies(destination, copies)
}

pub(crate) fn queue_rgba_copies_with_scanout_cache(
    destination: crate::intel::GucBcs0RgbaSurface,
    copies: &[crate::intel::GucBcs0RgbaCopy],
    source_scanout_cache: &[bool],
) -> Result<crate::intel::GucBcs0CopySubmission, crate::intel::GucBcs0CopySubmitError> {
    crate::intel::queue_guc_bcs0_rgba_copies_with_scanout_cache(
        destination,
        copies,
        source_scanout_cache,
    )
}

pub(crate) fn queue_uncached_copies(
    destination: crate::intel::GucBcs0RgbaSurface,
    copies: &[crate::intel::GucBcs0RgbaCopy],
) -> Result<crate::intel::GucBcs0CopySubmission, crate::intel::GucBcs0CopySubmitError> {
    crate::intel::queue_guc_bcs0_uncached_copies(destination, copies)
}

pub(crate) fn poll_rgba_copies(
    submission: crate::intel::GucBcs0CopySubmission,
) -> crate::intel::GucBcs0CopyCompletion {
    crate::intel::poll_guc_bcs0_rgba_copies(submission)
}


#[derive(Clone, Copy)]
pub(crate) enum RgbaCopyConsumer {
    
    ResidentScene,
}

/// Same ownership and fallback contract as `copy_rgba8_complete`.
pub(crate) fn copy_rgba8_complete_for(
    source: crate::intel::gpgpu::GpgpuRgba8Surface,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    consumer: RgbaCopyConsumer,
) -> crate::intel::gpgpu::GpgpuSubmissionOutcome {
    use crate::intel::gpgpu::GpgpuSubmissionOutcome as Outcome;
    use crate::intel::{
        GucBcs0CopyCompletion as Completion, GucBcs0CopySubmitError as SubmitError,
    };

    let result = (|| {
        if source.storage_order != destination.storage_order
            || source.width > destination.width
            || source.height > destination.height
        {
            return Outcome::Unavailable;
        }
        let surface =
            |s: crate::intel::gpgpu::GpgpuRgba8Surface| crate::intel::GucBcs0RgbaSurface {
                phys: s.phys,
                gpu: s.gpu,
                bytes: s.bytes,
                width: s.width,
                height: s.height,
                pitch_bytes: s.pitch_bytes,
            };
        let copy = crate::intel::GucBcs0RgbaCopy {
            source: surface(source),
            source_x: 0,
            source_y: 0,
            destination_x: 0,
            destination_y: 0,
            width: source.width,
            height: source.height,
        };
        let submission = match queue_rgba_copies(surface(destination), &[copy]) {
            Ok(submission) => submission,
            Err(SubmitError::Busy | SubmitError::Unavailable | SubmitError::InvalidRequest) => {
                return Outcome::Unavailable;
            }
            Err(SubmitError::SubmitFailed) => return Outcome::SubmittedIncomplete,
        };
        loop {
            match poll_rgba_copies(submission) {
                Completion::Pending => core::hint::spin_loop(),
                Completion::Complete => return Outcome::Complete,
                Completion::Failed | Completion::InvalidSubmission => {
                    return Outcome::SubmittedIncomplete;
                }
            }
        }
    })();
    let (copies, bytes, fallbacks, failures) = match consumer {
        
        RgbaCopyConsumer::ResidentScene => {
            (&SCENE_COPIES, &SCENE_BYTES, &SCENE_FALLBACKS, &SCENE_FAILURES)
        }
    };
    match result {
        Outcome::Complete => {
            copies.fetch_add(1, Ordering::Relaxed);
            bytes.fetch_add(
                u64::from(source.width) * u64::from(source.height) * 4,
                Ordering::Relaxed,
            );
        }
        Outcome::Unavailable => {
            fallbacks.fetch_add(1, Ordering::Relaxed);
        }
        Outcome::SubmittedIncomplete => {
            failures.fetch_add(1, Ordering::Relaxed);
        }
    }
    result
}

static CONSUMER_COPIES: AtomicU64 = AtomicU64::new(0);
static CONSUMER_BYTES: AtomicU64 = AtomicU64::new(0);
static CONSUMER_FALLBACKS: AtomicU64 = AtomicU64::new(0);
static CONSUMER_FAILURES: AtomicU64 = AtomicU64::new(0);
static SCENE_COPIES: AtomicU64 = AtomicU64::new(0);
static SCENE_BYTES: AtomicU64 = AtomicU64::new(0);
static SCENE_FALLBACKS: AtomicU64 = AtomicU64::new(0);
static SCENE_FAILURES: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub(crate) enum RgbaFillConsumer {
    Font,
    
    Probe,
    Ui4,
}

static FILL_COUNTS: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];

pub(crate) fn fill_stats(consumer: RgbaFillConsumer) -> [u64; 4] {
    let base = match consumer {
        RgbaFillConsumer::Font => 0,
        
        RgbaFillConsumer::Probe => 8,
        RgbaFillConsumer::Ui4 => 12,
    };
    core::array::from_fn(|index| FILL_COUNTS[base + index].load(Ordering::Relaxed))
}

/// Full-surface packed RGBA8 fill. Only Unavailable permits a compute fallback;
/// SubmittedIncomplete requires retaining the destination until reboot.
pub(crate) fn fill_rgba8_complete(
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
    color: u32,
    consumer: RgbaFillConsumer,
) -> crate::intel::gpgpu::GpgpuSubmissionOutcome {
    use crate::intel::gpgpu::GpgpuSubmissionOutcome as Outcome;
    use crate::intel::{
        GucBcs0CopyCompletion as Completion, GucBcs0CopySubmitError as SubmitError,
    };
    let result = (|| {
        if destination.storage_order != crate::intel::gpgpu::GpgpuRgba8StorageOrder::Rgba {
            return Outcome::Unavailable;
        }
        let surface = crate::intel::GucBcs0RgbaSurface {
            phys: destination.phys,
            gpu: destination.gpu,
            bytes: destination.bytes,
            width: destination.width,
            height: destination.height,
            pitch_bytes: destination.pitch_bytes,
        };
        let submission = match crate::intel::queue_guc_bcs0_rgba_fill(surface, color) {
            Ok(submission) => submission,
            Err(SubmitError::Busy | SubmitError::Unavailable | SubmitError::InvalidRequest) => {
                return Outcome::Unavailable;
            }
            Err(SubmitError::SubmitFailed) => return Outcome::SubmittedIncomplete,
        };
        loop {
            match poll_rgba_copies(submission) {
                Completion::Pending => core::hint::spin_loop(),
                Completion::Complete => return Outcome::Complete,
                Completion::Failed | Completion::InvalidSubmission => {
                    return Outcome::SubmittedIncomplete;
                }
            }
        }
    })();
    let base = match consumer {
        RgbaFillConsumer::Font => 0,
        
        RgbaFillConsumer::Probe => 8,
        RgbaFillConsumer::Ui4 => 12,
    };
    match result {
        Outcome::Complete => {
            FILL_COUNTS[base].fetch_add(1, Ordering::Relaxed);
            FILL_COUNTS[base + 1].fetch_add(
                u64::from(destination.width) * u64::from(destination.height) * 4,
                Ordering::Relaxed,
            );
        }
        Outcome::Unavailable => {
            FILL_COUNTS[base + 2].fetch_add(1, Ordering::Relaxed);
        }
        Outcome::SubmittedIncomplete => {
            FILL_COUNTS[base + 3].fetch_add(1, Ordering::Relaxed);
        }
    }
    result
}

/// Exercise monochrome expansion with two colors, background writes, source
/// slot changes and guards. The probe never falls back to a CPU renderer.
pub(crate) fn mono_check() -> Result<(), &'static str> {
    use crate::intel::{GucBcs0CopyCompletion as Completion, GucBcs0CopySubmitError as Error};
    let poison = 0xA55A_C33Cu32;
    let owned = crate::intel::gpgpu::allocate_font_instance_rgba8_surface_cleared(32, 16, poison)
        .ok_or("allocation-unavailable")?;
    let dst = owned.surface();
    let mut glyphs = [crate::intel::GucBcs0MonoGlyph {
        x: 3,
        y: 2,
        width: 6,
        height: 11,
        mask: [0; 64],
        foreground: 0xFF34_A853,
        background: 0xFF80_4020,
    }; 2];
    glyphs[1].x = 15;
    glyphs[1].foreground = 0xFFB4_69FF;
    glyphs[1].background = 0xFF20_4080;
    for (glyph, character) in glyphs.iter_mut().zip(['§', 'A']) {
        let atlas = super::microfont_mask_service::ready_atlas().ok_or("microfont-not-ready")?;
        for y in 0..11 { for x in 0..6 {
            if atlas.coverage(character, x, y, 6, 11) >= 128 {
                glyph.mask[y as usize * 2] |= 0x80 >> x;
            }
        }}
    }
    let submission = match crate::intel::queue_guc_bcs0_mono_glyphs(
        crate::intel::GucBcs0RgbaSurface {
            phys: dst.phys,
            gpu: dst.gpu,
            bytes: dst.bytes,
            width: dst.width,
            height: dst.height,
            pitch_bytes: dst.pitch_bytes,
        },
        &glyphs,
    ) {
        Ok(submission) => submission,
        Err(Error::SubmitFailed) => {
            owned.quarantine_backing();
            return Err("mono-submit-uncertain-backing-retained");
        }
        Err(_) => return Err("mono-not-admitted"),
    };
    loop {
        match poll_rgba_copies(submission) {
            Completion::Pending => core::hint::spin_loop(),
            Completion::Complete => break,
            _ => {
                owned.quarantine_backing();
                return Err("mono-retirement-uncertain-backing-retained");
            }
        }
    }
    let bytes = owned.readback_tight_rgba().ok_or("readback-unavailable")?;
    for (index, pixel) in bytes.chunks_exact(4).enumerate() {
        let x = index as u32 % 32;
        let y = index as u32 / 32;
        let mut expected = poison;
        for glyph in &glyphs {
            if (glyph.x..glyph.x + glyph.width).contains(&x)
                && (glyph.y..glyph.y + glyph.height).contains(&y)
            {
                expected =
                    if glyph.mask[((y - glyph.y) * 2) as usize] & (0x80 >> (x - glyph.x)) != 0 {
                        glyph.foreground
                    } else {
                        glyph.background
                    };
            }
        }
        if pixel != expected.to_le_bytes() {
            return Err("mono-color-or-guard-mismatch");
        }
    }
    Ok(())
}

/// Explicit hardware check: a nonzero fill followed by zero, with padded
/// rows and untouched right/bottom guards. No compute fallback can pass it.
pub(crate) fn fill_check() -> Result<(), &'static str> {
    use crate::intel::gpgpu::GpgpuSubmissionOutcome as Outcome;
    let poison = 0xA55A_C33Cu32;
    let owned = crate::intel::gpgpu::allocate_font_instance_rgba8_surface_cleared(32, 4, poison)
        .ok_or("allocation-unavailable")?;
    let mut destination = owned.surface();
    destination.width = 17;
    destination.height = 3;
    for color in [0x8040_2010u32, 0] {
        match fill_rgba8_complete(destination, color, RgbaFillConsumer::Probe) {
            Outcome::Complete => {}
            Outcome::Unavailable => return Err("fill-not-admitted"),
            Outcome::SubmittedIncomplete => {
                owned.quarantine_backing();
                return Err("fill-retirement-uncertain-backing-retained");
            }
        }
        let bytes = owned.readback_tight_rgba().ok_or("readback-unavailable")?;
        for (index, pixel) in bytes.chunks_exact(4).enumerate() {
            let expected = if index / 32 < 3 && index % 32 < 17 {
                color
            } else {
                poison
            };
            if pixel != expected.to_le_bytes() {
                return Err("pixel-or-guard-mismatch");
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Status {
    pub consumer_copies: u64,
    pub consumer_bytes: u64,
    pub consumer_fallbacks: u64,
    pub consumer_failures: u64,
    pub scene_copies: u64,
    pub scene_bytes: u64,
    pub scene_fallbacks: u64,
    pub scene_failures: u64,
}

pub fn status() -> Status {
    Status {
        consumer_copies: CONSUMER_COPIES.load(Ordering::Relaxed),
        consumer_bytes: CONSUMER_BYTES.load(Ordering::Relaxed),
        consumer_fallbacks: CONSUMER_FALLBACKS.load(Ordering::Relaxed),
        consumer_failures: CONSUMER_FAILURES.load(Ordering::Relaxed),
        scene_copies: SCENE_COPIES.load(Ordering::Relaxed),
        scene_bytes: SCENE_BYTES.load(Ordering::Relaxed),
        scene_fallbacks: SCENE_FALLBACKS.load(Ordering::Relaxed),
        scene_failures: SCENE_FAILURES.load(Ordering::Relaxed),
    }
}
