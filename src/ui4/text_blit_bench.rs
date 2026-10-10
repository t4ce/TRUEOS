//! Opt-in physical comparison using production commands and ready-queue polls.
//! Scratch storage is separate from live frames; it is pinned on cancellation.
use super::text_blit::{self, copy_pixels, mono_pixels, nanos, stamp};
use crate::intel::{GucBcs0MonoGlyph, GucBcs0RgbaCopy, GucBcs0RgbaSurface};
use alloc::{vec, vec::Vec};
use core::sync::atomic::{AtomicU8, Ordering};

static STATE: AtomicU8 = AtomicU8::new(0);
struct Scratch {
    surfaces: Vec<crate::intel::gpgpu::GpgpuOwnedRgba8Surface>,
    in_flight: bool,
}
impl Drop for Scratch {
    fn drop(&mut self) {
        if self.in_flight {
            for s in &self.surfaces {
                s.quarantine_backing();
            }
        }
        if STATE.load(Ordering::Acquire) == 1 {
            STATE.store(3, Ordering::Release);
        }
    }
}
fn surface(s: &crate::intel::gpgpu::GpgpuOwnedRgba8Surface) -> GucBcs0RgbaSurface {
    let s = s.surface();
    GucBcs0RgbaSurface {
        phys: s.phys,
        gpu: s.gpu,
        bytes: s.bytes,
        width: s.width,
        height: s.height,
        pitch_bytes: s.pitch_bytes,
    }
}
async fn ready_yield() {
    let mut yielded = false;
    core::future::poll_fn(|cx| {
        if yielded {
            core::task::Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            core::task::Poll::Pending
        }
    })
    .await
}
async fn submit(
    mut queue: impl FnMut() -> Result<
        crate::intel::GucBcs0CopySubmission,
        crate::intel::GucBcs0CopySubmitError,
    >,
) -> Result<(crate::intel::GucBcs0CopySubmission, u64, u32), crate::intel::GucBcs0CopySubmitError> {
    let started = stamp();
    let mut host = 0;
    let mut retries = 0;
    loop {
        let t = stamp();
        let queued = queue();
        host += stamp().wrapping_sub(t);
        match queued {
            Ok(s) => return Ok((s, host, retries)),
            Err(crate::intel::GucBcs0CopySubmitError::Busy)
                if nanos(stamp().wrapping_sub(started)) < 5_000_000_000 =>
            {
                retries += 1;
                ready_yield().await;
            }
            Err(error) => {
                crate::log_error!(target:"apps";"text-blit-bench: admission error={:?} busy_retries={}\n",error,retries);
                return Err(error);
            }
        }
    }
}
async fn retire(
    queued: Result<crate::intel::GucBcs0CopySubmission, crate::intel::GucBcs0CopySubmitError>,
    scratch: &mut Scratch,
) -> Result<u64, &'static str> {
    scratch.in_flight =
        queued.is_ok() || matches!(queued, Err(crate::intel::GucBcs0CopySubmitError::SubmitFailed));
    let submission = queued.map_err(|_| "text-blit-bench-admission")?;
    let started = stamp();
    let mut host = 0;
    loop {
        let poll = stamp();
        let status = crate::intel::poll_guc_bcs0_rgba_copies(submission);
        host += stamp().wrapping_sub(poll);
        match status {
            crate::intel::GucBcs0CopyCompletion::Complete => {
                scratch.in_flight = false;
                return Ok(host);
            }
            crate::intel::GucBcs0CopyCompletion::Pending => {
                if nanos(stamp().wrapping_sub(started)) > 5_000_000_000 {
                    return Err("text-blit-bench-timeout");
                }
                ready_yield().await;
            }
            _ => return Err("text-blit-bench-retirement"),
        }
    }
}
pub(super) async fn run_once() -> Result<(), &'static str> {
    if !crate::allcaps::text_blit::BENCHMARK
        || STATE
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return Ok(());
    }
    let result = run().await;
    STATE.store(if result.is_ok() { 2 } else { 3 }, Ordering::Release);
    if let Err(error) = result {
        crate::log_error!(target:"apps";"text-blit-bench: failed error={}\n",error);
    }
    result
}
async fn run() -> Result<(), &'static str> {
    let mut scratch = Scratch {
        surfaces: Vec::new(),
        in_flight: false,
    };
    for _ in 0..3 {
        scratch.surfaces.push(
            crate::intel::gpgpu::allocate_font_instance_rgba8_surface(1536, 1408)
                .ok_or("text-blit-bench-allocation")?,
        );
    }
    let cpu = surface(&scratch.surfaces[0]);
    let gpu = surface(&scratch.surfaces[1]);
    let source = surface(&scratch.surfaces[2]);
    let glyphs: Vec<_> = (0..4096)
        .map(|index| {
            let mut mask = [0u8; 64];
            for (i, byte) in mask.iter_mut().enumerate() {
                *byte = (index as u8).wrapping_add(i as u8 * 3) ^ 0xa5;
            }
            GucBcs0MonoGlyph {
                x: (index % 128) * 6,
                y: (index / 128) * 11,
                width: 6,
                height: 11,
                mask,
                foreground: 0xff_91_62_33u32.wrapping_add(index),
                background: 0xa0_00_00_00,
            }
        })
        .collect();
    unsafe { text_blit::mono(source, &glyphs) }
        .then_some(())
        .ok_or("text-blit-bench-source")?;
    crate::log_info!(target:"apps";"text-blit-bench: begin ap={} tsc_hz={} samples=7 warmups=2 glyph=6x11 polling=ready-queue cpu_max_mono={} cpu_max_copy={} scratch_bytes={}\n",
        crate::percpu::current_slot(),crate::time::tsc_hz(),crate::allcaps::text_blit::CPU_MONO_MAX_PIXELS,
        crate::allcaps::text_blit::CPU_COPY_MAX_PIXELS,cpu.bytes+gpu.bytes+source.bytes);
    for kind in ["mono", "copy"] {
        let amounts = if kind == "mono" {
            vec![
                1usize, 2, 4, 8, 16, 32, 50, 64, 128, 256, 512, 1024, 2048, 4096,
            ]
        } else {
            vec![
                1usize, 2, 4, 8, 16, 32, 50, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384,
                32768,
            ]
        };
        for amount in amounts {
            let glyphs = &glyphs[..amount.min(4096)];
            let requested = amount * 66;
            let width = (requested as u32).min(source.width);
            let height = (requested as u32).div_ceil(width);
            let copies = vec![GucBcs0RgbaCopy {
                source,
                source_x: 0,
                source_y: 0,
                destination_x: 0,
                destination_y: 0,
                width,
                height,
            }];
            let pixels = if kind == "mono" {
                mono_pixels(glyphs)
            } else {
                copy_pixels(&copies)
            };
            let mut cpu_ns = Vec::new();
            let mut bcs_ns = Vec::new();
            let mut host_ns = Vec::new();
            let mut busy_retries = Vec::new();
            for iteration in 0..9 {
                // Alternate order to reduce systematic thermal/cache bias.
                let mut c = 0;
                let mut b = 0;
                let mut active = 0;
                let mut retries = 0;
                for cpu_first in if iteration % 2 == 0 {
                    [true, false]
                } else {
                    [false, true]
                } {
                    if cpu_first {
                        let t = stamp();
                        let ok = unsafe {
                            if kind == "mono" {
                                text_blit::mono(cpu, glyphs)
                            } else {
                                text_blit::copy(cpu, &copies)
                            }
                        };
                        c = nanos(stamp().wrapping_sub(t));
                        if !ok {
                            return Err("text-blit-bench-cpu");
                        }
                    } else {
                        let t = stamp();
                        let mut host = 0;
                        if kind == "mono" {
                            for chunk in glyphs.chunks(crate::intel::GUC_BCS0_MONO_MAX_GLYPHS) {
                                let queued =
                                    submit(|| crate::intel::queue_guc_bcs0_mono_glyphs(gpu, chunk))
                                        .await;
                                match queued {
                                    Ok((submission, h, r)) => {
                                        host += h;
                                        retries += r;
                                        host += retire(Ok(submission), &mut scratch).await?;
                                    }
                                    Err(error) => {
                                        retire(Err(error), &mut scratch).await?;
                                    }
                                }
                            }
                        } else {
                            let queued =
                                submit(|| crate::intel::queue_guc_bcs0_rgba_copies(gpu, &copies))
                                    .await;
                            match queued {
                                Ok((submission, h, r)) => {
                                    host += h;
                                    retries += r;
                                    host += retire(Ok(submission), &mut scratch).await?;
                                }
                                Err(error) => {
                                    retire(Err(error), &mut scratch).await?;
                                }
                            }
                        }
                        b = nanos(stamp().wrapping_sub(t));
                        active = nanos(host);
                    }
                }
                if iteration >= 2 {
                    cpu_ns.push(c);
                    bcs_ns.push(b);
                    host_ns.push(active);
                    busy_retries.push(retries);
                }
            }
            let expected = scratch.surfaces[0]
                .readback_tight_rgba()
                .ok_or("text-blit-bench-readback")?;
            let actual = scratch.surfaces[1]
                .readback_tight_rgba()
                .ok_or("text-blit-bench-readback")?;
            if expected != actual {
                return Err("text-blit-bench-pixel-mismatch");
            }
            crate::log_info!(target:"apps";"text-blit-bench: samples kind={} pixels={} cpu_ns={:?} bcs_wall_ns={:?} bcs_host_ns={:?} busy_retries={:?} pixels_equal=1\n",kind,pixels,cpu_ns,bcs_ns,host_ns,busy_retries);
            cpu_ns.sort_unstable();
            bcs_ns.sort_unstable();
            host_ns.sort_unstable();
            crate::log_info!(target:"apps";"text-blit-bench: summary kind={} pixels={} bytes={} cpu_p50_ns={} cpu_p95_ns={} bcs_p50_ns={} bcs_p95_ns={} bcs_host_p50_ns={} batches={} pixels_equal=1\n",
                kind,pixels,pixels*4,cpu_ns[3],cpu_ns[6],bcs_ns[3],bcs_ns[6],host_ns[3],if kind=="mono" {amount.div_ceil(64)} else {1});
        }
    }
    crate::log_info!(target:"apps";"text-blit-bench: complete ap={} verified=31\n",crate::percpu::current_slot());
    Ok(())
}
