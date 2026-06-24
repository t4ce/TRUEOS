use alloc::{string::String, vec, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

const PRESENT_SCALE: usize = 2;
const PRESENT_WIDTH: usize = crate::trueos_gboi::gpu::SCREEN_W * PRESENT_SCALE;
const PRESENT_HEIGHT: usize = crate::trueos_gboi::gpu::SCREEN_H * PRESENT_SCALE;
const PRESENT_PITCH_BYTES: usize = PRESENT_WIDTH * core::mem::size_of::<u32>();

static GBOY_RUN_GENERATION: AtomicU32 = AtomicU32::new(0);

pub(crate) fn next_run_generation() -> u32 {
    GBOY_RUN_GENERATION
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1)
}

fn current_run_generation() -> u32 {
    GBOY_RUN_GENERATION.load(Ordering::Acquire)
}

#[embassy_executor::task(pool_size = 2)]
pub(crate) async fn gboy_task(path: String, target: crate::shell2::MatrixTarget, generation: u32) {
    let result = run_gboy(path.as_str(), &target, generation).await;
    if let Err(err) = result {
        crate::shell2::print_matrix_target_line(&target, alloc::format!("gboy: {err}").as_str());
    }
    crate::shell2::set_matrix_target_active(&target, false);
}

async fn run_gboy(
    path: &str,
    target: &crate::shell2::MatrixTarget,
    generation: u32,
) -> Result<(), String> {
    if !crate::intel::has_claimed_device() {
        return Err(String::from("Intel display backend is not ready"));
    }

    let rom = read_rom(path, target).await?;
    crate::shell2::print_matrix_target_line(
        target,
        alloc::format!("gboy: loaded {} bytes from {}", rom.len(), path).as_str(),
    );

    let mut emulator = crate::trueos_gboi::GameBoyEmulator::new();
    if !emulator.load_rom(rom.as_slice()) {
        return Err(String::from("ROM parser rejected file"));
    }

    let mut argb = vec![0u32; PRESENT_WIDTH * PRESENT_HEIGHT];
    let mut rgba = vec![0u8; PRESENT_WIDTH * PRESENT_HEIGHT * core::mem::size_of::<u32>()];
    let mut frame = 0u64;

    let scanout = crate::intel::active_scanout_dimensions()
        .map(|(w, h)| alloc::format!(" scanout={}x{}", w, h))
        .unwrap_or_default();
    crate::shell2::print_matrix_target_line(
        target,
        alloc::format!("gboy: presenting {}x{}{}", PRESENT_WIDTH, PRESENT_HEIGHT, scanout).as_str(),
    );

    while current_run_generation() == generation {
        emulator.tick();
        emulator.render(&mut argb, PRESENT_WIDTH, PRESENT_HEIGHT);
        argb_to_rgba(&argb, &mut rgba);

        let presented = crate::intel::present_rgba_primary_center_unscaled(
            &rgba,
            PRESENT_WIDTH as u32,
            PRESENT_HEIGHT as u32,
            PRESENT_PITCH_BYTES,
            "gboy",
        );
        frame = frame.wrapping_add(1);
        if frame <= 8 || frame.is_multiple_of(120) || !presented {
            crate::shell2::print_matrix_target_line(
                target,
                alloc::format!(
                    "gboy: frame={} presented={} size={}x{}",
                    frame,
                    presented as u8,
                    PRESENT_WIDTH,
                    PRESENT_HEIGHT
                )
                .as_str(),
            );
        }

        Timer::after(EmbassyDuration::from_millis(16)).await;
    }

    crate::shell2::print_matrix_target_line(
        target,
        alloc::format!("gboy: stopped after {} frame(s)", frame).as_str(),
    );
    Ok(())
}

async fn read_rom(path: &str, target: &crate::shell2::MatrixTarget) -> Result<Vec<u8>, String> {
    let disk = crate::r::fs::trueosfs::primary_root_handle()
        .ok_or_else(|| String::from("no TRUEOSFS root mounted"))?;
    let rel = crate::r::path::FsPath::parse(path, false)
        .map(|path| path.to_relative_string())
        .map_err(|err| alloc::format!("bad path {:?}: {}", err, path))?;

    crate::shell2::print_matrix_target_line(
        target,
        alloc::format!("gboy: reading /{}", rel).as_str(),
    );

    match crate::r::fs::trueosfs::file_out_async(disk, rel.as_str())
        .await
        .map_err(|err| alloc::format!("read failed: {:?}", err))?
    {
        Some(bytes) => Ok(bytes),
        None => Err(alloc::format!("{path}: not found")),
    }
}

fn argb_to_rgba(src: &[u32], dst: &mut [u8]) {
    let mut out = dst.chunks_exact_mut(4);
    for pixel in src {
        let Some(bytes) = out.next() else {
            break;
        };
        bytes[0] = ((pixel >> 16) & 0xFF) as u8;
        bytes[1] = ((pixel >> 8) & 0xFF) as u8;
        bytes[2] = (pixel & 0xFF) as u8;
        bytes[3] = 0xFF;
    }
}
