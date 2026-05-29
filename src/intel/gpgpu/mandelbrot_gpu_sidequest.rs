#![allow(dead_code)]

use embassy_executor::{SpawnError, Spawner};

pub(crate) const MANDELBROT_GPU_SIDEQUEST_NAME: &str = "mandelbrot-gpu-sidequest";
pub(crate) const MANDELBROT16_EXAMPLE_COLOR: u32 = 0xFFFF_FFFF;

pub(crate) fn spawn_mandelbrot_gpu_sidequest(spawner: Spawner) -> Result<(), SpawnError> {
    let token = mandelbrot_gpu_sidequest_task()?;
    spawner.spawn(token);
    Ok(())
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn mandelbrot_gpu_sidequest_task() {
    let scanout = crate::intel::active_scanout_dimensions();
    let scanout_w = scanout.map(|dims| dims.0).unwrap_or(0);
    let scanout_h = scanout.map(|dims| dims.1).unwrap_or(0);
    let primary_gpu = crate::intel::primary_surface_gpu_addr().unwrap_or(0);
    let pixels_per_program =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM as u32;
    let x_base = scanout_w.saturating_sub(pixels_per_program) / 2;
    let row_one_based = core::cmp::max(1, scanout_h / 2);
    crate::log!(
        "mandelbrot-gpu-sidequest: start name={} example=walkrow16-white scanout={}x{} primary_gpu=0x{:X} active_artifact={} next=single-submit\n",
        MANDELBROT_GPU_SIDEQUEST_NAME,
        scanout_w,
        scanout_h,
        primary_gpu,
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PROGRAM_NAME,
    );

    let proof = crate::intel::submit_gpgpu_primary_scanout_walkrow16(
        row_one_based,
        x_base,
        MANDELBROT16_EXAMPLE_COLOR,
        true,
    );
    if proof.readback_ok {
        crate::r::readiness::set(crate::r::readiness::MANDELBROT_GPU_SIDEQUEST_READY);
    }
    crate::log!(
        "mandelbrot-gpu-sidequest: walkrow16 submitted={} finished={} readback_ok={} reason={} program_source={} target_gpu=0x{:X} stamp_rect={}x1@{},{} color=0x{:08X} hit_mask=0x{:04X} lane_dispatch_delta={} finish_marker=0x{:08X} next=done\n",
        proof.submitted as u8,
        proof.finished as u8,
        proof.readback_ok as u8,
        proof.reason,
        proof.program_name,
        proof.output_gpu,
        pixels_per_program,
        x_base,
        row_one_based,
        MANDELBROT16_EXAMPLE_COLOR,
        proof.output_hits_lo64 as u32,
        proof.dispatch_delta,
        proof.finish_marker,
    );
}
