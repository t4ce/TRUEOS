use alloc::{string::String, vec, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

const PRESENT_MAX_SCALE: usize = 4;
const PRESENT_BG_XRGB: u32 = 0x00FF_FFFF;

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

    let (present_width, present_height, present_scale) = present_size();
    let present_pitch_bytes = present_width * core::mem::size_of::<u32>();
    let mut argb = vec![0u32; present_width * present_height];
    let mut rgba = vec![0u8; present_width * present_height * core::mem::size_of::<u32>()];
    let mut frame = 0u64;

    let scanout = crate::intel::active_scanout_dimensions()
        .map(|(w, h)| alloc::format!(" scanout={}x{}", w, h))
        .unwrap_or_default();
    crate::shell2::print_matrix_target_line(
        target,
        alloc::format!(
            "gboy: presenting {}x{} scale={}{}",
            present_width,
            present_height,
            present_scale,
            scanout
        )
        .as_str(),
    );

    while current_run_generation() == generation {
        sync_gboy_buttons_from_hid_hut(&mut emulator);
        emulator.tick();
        emulator.render(&mut argb, present_width, present_height);
        argb_to_rgba(&argb, &mut rgba);

        let presented = crate::intel::present_rgba_primary_center_plane_bg(
            &rgba,
            present_width as u32,
            present_height as u32,
            present_pitch_bytes,
            PRESENT_BG_XRGB,
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
                    present_width,
                    present_height
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

fn present_size() -> (usize, usize, usize) {
    let base_w = crate::trueos_gboi::gpu::SCREEN_W;
    let base_h = crate::trueos_gboi::gpu::SCREEN_H;
    let max_scale = crate::intel::active_scanout_dimensions()
        .map(|(w, h)| {
            ((w as usize) / base_w)
                .min((h as usize) / base_h)
                .clamp(1, PRESENT_MAX_SCALE)
        })
        .unwrap_or(PRESENT_MAX_SCALE);
    let scale = if max_scale >= 4 {
        4
    } else if max_scale >= 2 {
        2
    } else {
        1
    };
    (base_w * scale, base_h * scale, scale)
}

#[inline]
fn hid_key_is_down(key_down_bits: &[u32; 8], key_code: u8) -> bool {
    let key_code = key_code as usize;
    (key_down_bits[key_code / 32] & (1u32 << (key_code % 32))) != 0
}

fn button_from_hid_boot_keycode(key_code: u8) -> Option<crate::trueos_gboi::GameBoyButton> {
    match key_code {
        0x04 => Some(crate::trueos_gboi::GameBoyButton::Left),
        0x06 => Some(crate::trueos_gboi::GameBoyButton::Select),
        0x07 => Some(crate::trueos_gboi::GameBoyButton::Right),
        0x16 => Some(crate::trueos_gboi::GameBoyButton::Down),
        0x1A => Some(crate::trueos_gboi::GameBoyButton::Up),
        0x1B | 0x2C => Some(crate::trueos_gboi::GameBoyButton::A),
        0x1D => Some(crate::trueos_gboi::GameBoyButton::B),
        0x28 => Some(crate::trueos_gboi::GameBoyButton::Start),
        0x4F => Some(crate::trueos_gboi::GameBoyButton::Right),
        0x50 => Some(crate::trueos_gboi::GameBoyButton::Left),
        0x51 => Some(crate::trueos_gboi::GameBoyButton::Down),
        0x52 => Some(crate::trueos_gboi::GameBoyButton::Up),
        _ => None,
    }
}

fn sync_gboy_buttons_from_hid_hut(emulator: &mut crate::trueos_gboi::GameBoyEmulator) {
    const KEY_CODES: &[u8] = &[
        0x04, 0x06, 0x07, 0x16, 0x1A, 0x1B, 0x1D, 0x28, 0x2C, 0x4F, 0x50, 0x51, 0x52,
    ];

    let keyboards = crate::usb3::hid::hut::keyboards_snapshot();
    for button in [
        crate::trueos_gboi::GameBoyButton::Right,
        crate::trueos_gboi::GameBoyButton::Left,
        crate::trueos_gboi::GameBoyButton::Up,
        crate::trueos_gboi::GameBoyButton::Down,
        crate::trueos_gboi::GameBoyButton::A,
        crate::trueos_gboi::GameBoyButton::B,
        crate::trueos_gboi::GameBoyButton::Select,
        crate::trueos_gboi::GameBoyButton::Start,
    ] {
        let pressed = KEY_CODES
            .iter()
            .filter(|key_code| button_from_hid_boot_keycode(**key_code) == Some(button))
            .any(|key_code| {
                keyboards
                    .iter()
                    .any(|keyboard| hid_key_is_down(&keyboard.key_down_bits, *key_code))
            });
        emulator.set_button(button, pressed);
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
