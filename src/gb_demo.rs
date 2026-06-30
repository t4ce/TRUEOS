use alloc::{string::String, vec, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

const PRESENT_MAX_SCALE: usize = 4;
const PRESENT_BG_XRGB: u32 = 0x00FF_FFFF;
const DEBUG_ATLAS_PATH: &str = "gboy_athlas.bmp";
const DEBUG_ATLAS_META_PATH: &str = "gboy_athlas.txt";
const DEBUG_ATLAS_MARKER_PATH: &str = "gboy_athlas.marker.txt";
const DEBUG_ATLAS_WARMUP_FRAMES: u64 = 8;
const DEBUG_ATLAS_FALLBACK_FRAMES: u64 = 240;
const DEBUG_ATLAS_MIN_NONZERO_BYTES: usize = 64;
const GB_TILE_BYTES: usize = 16;
const GB_TILE_PIXELS: usize = 8;
const GB_TILE_COUNT: usize = 384;
const GB_ATLAS_COLS: usize = 16;
const GB_ATLAS_BANK_ROWS: usize = GB_TILE_COUNT / GB_ATLAS_COLS;
const GB_ATLAS_BANKS: usize = 2;
const GB_ATLAS_WIDTH: usize = GB_ATLAS_COLS * GB_TILE_PIXELS;
const GB_ATLAS_HEIGHT: usize = GB_ATLAS_BANK_ROWS * GB_TILE_PIXELS * GB_ATLAS_BANKS;
const FNV1A64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A64_PRIME: u64 = 0x0000_0100_0000_01b3;

static GBOY_RUN_GENERATION: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy)]
struct GboyVramAtlasStats {
    bank_nonzero_bytes: [usize; GB_ATLAS_BANKS],
    bank_nonzero_tiles: [usize; GB_ATLAS_BANKS],
    oam_nonzero_bytes: usize,
}

impl GboyVramAtlasStats {
    fn total_nonzero_bytes(self) -> usize {
        self.bank_nonzero_bytes.iter().sum()
    }

    fn total_nonzero_tiles(self) -> usize {
        self.bank_nonzero_tiles.iter().sum()
    }
}

#[derive(Clone, Copy)]
enum GboyDebugAtlasReason {
    VramSeeded,
    Fallback,
}

impl GboyDebugAtlasReason {
    fn as_str(self) -> &'static str {
        match self {
            GboyDebugAtlasReason::VramSeeded => "vram_seeded",
            GboyDebugAtlasReason::Fallback => "fallback",
        }
    }
}

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
    let mut debug_atlas_written = false;
    let rom_hash = fnv1a64(rom.as_slice());

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
        frame = frame.wrapping_add(1);

        if !debug_atlas_written && frame >= DEBUG_ATLAS_WARMUP_FRAMES {
            let stats = gboy_vram_atlas_stats(&emulator.gpu);
            let reason = if stats.total_nonzero_bytes() >= DEBUG_ATLAS_MIN_NONZERO_BYTES {
                Some(GboyDebugAtlasReason::VramSeeded)
            } else if frame >= DEBUG_ATLAS_FALLBACK_FRAMES {
                Some(GboyDebugAtlasReason::Fallback)
            } else {
                None
            };

            match reason {
                Some(reason) => {
                    debug_atlas_written = true;
                    crate::log!(
                        "gboy: debug atlas attempt frame={} reason={} vram0_nonzero_bytes={} vram1_nonzero_bytes={} nonzero_tiles={} path={} meta={} marker={}\n",
                        frame,
                        reason.as_str(),
                        stats.bank_nonzero_bytes[0],
                        stats.bank_nonzero_bytes[1],
                        stats.total_nonzero_tiles(),
                        DEBUG_ATLAS_PATH,
                        DEBUG_ATLAS_META_PATH,
                        DEBUG_ATLAS_MARKER_PATH
                    );
                    match write_gboy_debug_atlas(path, rom_hash, &emulator, frame, reason, stats)
                        .await
                    {
                        Ok(()) => {
                            crate::log!(
                                "gboy: debug atlas written frame={} reason={} bmp={} meta={}\n",
                                frame,
                                reason.as_str(),
                                DEBUG_ATLAS_PATH,
                                DEBUG_ATLAS_META_PATH
                            );
                            crate::shell2::print_matrix_target_line(
                                target,
                                alloc::format!(
                                    "gboy: wrote /{} and /{} after {} frame(s)",
                                    DEBUG_ATLAS_PATH,
                                    DEBUG_ATLAS_META_PATH,
                                    frame
                                )
                                .as_str(),
                            );
                        }
                        Err(err) => {
                            crate::log!(
                                "gboy: debug atlas write failed frame={} err={}\n",
                                frame,
                                err
                            );
                            crate::shell2::print_matrix_target_line(
                                target,
                                alloc::format!("gboy: debug atlas write failed: {err}").as_str(),
                            );
                        }
                    }
                }
                None if frame == DEBUG_ATLAS_WARMUP_FRAMES || frame.is_multiple_of(60) => {
                    crate::log!(
                        "gboy: debug atlas waiting frame={} vram0_nonzero_bytes={} vram1_nonzero_bytes={} nonzero_tiles={}\n",
                        frame,
                        stats.bank_nonzero_bytes[0],
                        stats.bank_nonzero_bytes[1],
                        stats.total_nonzero_tiles()
                    );
                }
                None => {}
            }
        }

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

async fn write_gboy_debug_atlas(
    rom_path: &str,
    rom_hash: u64,
    emulator: &crate::trueos_gboi::GameBoyEmulator,
    frame: u64,
    reason: GboyDebugAtlasReason,
    stats: GboyVramAtlasStats,
) -> Result<(), String> {
    let disk = crate::r::fs::trueosfs::primary_root_handle()
        .ok_or_else(|| String::from("no TRUEOSFS root mounted"))?;
    let bmp = encode_gboy_vram_atlas_bmp(&emulator.gpu);
    let meta = gboy_debug_atlas_metadata(rom_path, rom_hash, emulator, frame, reason, stats);

    let ok = crate::r::fs::trueosfs::file_in_async(
        disk,
        DEBUG_ATLAS_MARKER_PATH,
        b"gboy debug atlas hook reached\n",
    )
    .await
    .map_err(|err| alloc::format!("marker write failed: {:?}", err))?;
    if !ok {
        return Err(String::from("marker write rejected by TRUEOSFS"));
    }

    let ok = crate::r::fs::trueosfs::file_in_async(disk, DEBUG_ATLAS_PATH, bmp.as_slice())
        .await
        .map_err(|err| alloc::format!("bmp write failed: {:?}", err))?;
    if !ok {
        return Err(String::from("bmp write rejected by TRUEOSFS"));
    }

    let ok = crate::r::fs::trueosfs::file_in_async(disk, DEBUG_ATLAS_META_PATH, meta.as_bytes())
        .await
        .map_err(|err| alloc::format!("metadata write failed: {:?}", err))?;
    if !ok {
        return Err(String::from("metadata write rejected by TRUEOSFS"));
    }

    Ok(())
}

fn gboy_debug_atlas_metadata(
    rom_path: &str,
    rom_hash: u64,
    emulator: &crate::trueos_gboi::GameBoyEmulator,
    frame: u64,
    reason: GboyDebugAtlasReason,
    stats: GboyVramAtlasStats,
) -> String {
    alloc::format!(
        "rom_path={}\nrom_title={}\nrom_hash_fnv1a64={:016x}\ncgb={}\nwarmup_frames={}\nfallback_frames={}\nmin_nonzero_bytes={}\ndump_frame={}\ndump_reason={}\nvram0_nonzero_bytes={}\nvram1_nonzero_bytes={}\nvram0_nonzero_tiles={}\nvram1_nonzero_tiles={}\noam_nonzero_bytes={}\natlas_file={}\nformat=bmp-bgra32\nwidth={}\nheight={}\nlayout=bank0 tiles 0..383 top, bank1 tiles 0..383 bottom, 16 columns, 8x8 pixels per tile\n",
        rom_path,
        cartridge_title(&emulator.cart.title),
        rom_hash,
        emulator.cgb_mode as u8,
        DEBUG_ATLAS_WARMUP_FRAMES,
        DEBUG_ATLAS_FALLBACK_FRAMES,
        DEBUG_ATLAS_MIN_NONZERO_BYTES,
        frame,
        reason.as_str(),
        stats.bank_nonzero_bytes[0],
        stats.bank_nonzero_bytes[1],
        stats.bank_nonzero_tiles[0],
        stats.bank_nonzero_tiles[1],
        stats.oam_nonzero_bytes,
        DEBUG_ATLAS_PATH,
        GB_ATLAS_WIDTH,
        GB_ATLAS_HEIGHT,
    )
}

fn gboy_vram_atlas_stats(gpu: &crate::trueos_gboi::gpu::Gpu) -> GboyVramAtlasStats {
    GboyVramAtlasStats {
        bank_nonzero_bytes: [
            count_nonzero_bytes(gpu.vram.as_slice()),
            count_nonzero_bytes(gpu.vram1.as_slice()),
        ],
        bank_nonzero_tiles: [
            count_nonzero_tiles(gpu.vram.as_slice()),
            count_nonzero_tiles(gpu.vram1.as_slice()),
        ],
        oam_nonzero_bytes: count_nonzero_bytes(gpu.oam.as_slice()),
    }
}

fn count_nonzero_bytes(bytes: &[u8]) -> usize {
    bytes.iter().filter(|byte| **byte != 0).count()
}

fn count_nonzero_tiles(vram: &[u8]) -> usize {
    vram.chunks_exact(GB_TILE_BYTES)
        .take(GB_TILE_COUNT)
        .filter(|tile| tile.iter().any(|byte| *byte != 0))
        .count()
}

fn cartridge_title(title: &[u8; 16]) -> String {
    let mut out = String::new();
    for &byte in title.iter() {
        if byte == 0 {
            break;
        }
        if (0x20..=0x7e).contains(&byte) {
            out.push(byte as char);
        }
    }
    if out.is_empty() {
        out.push_str("unknown");
    }
    out
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = FNV1A64_OFFSET;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV1A64_PRIME);
    }
    hash
}

fn encode_gboy_vram_atlas_bmp(gpu: &crate::trueos_gboi::gpu::Gpu) -> Vec<u8> {
    let pixel_bytes = GB_ATLAS_WIDTH * GB_ATLAS_HEIGHT * 4;
    let file_size = 14 + 40 + pixel_bytes;
    let mut out = Vec::with_capacity(file_size);

    out.extend_from_slice(b"BM");
    push_le_u32(&mut out, file_size as u32);
    push_le_u16(&mut out, 0);
    push_le_u16(&mut out, 0);
    push_le_u32(&mut out, 14 + 40);

    push_le_u32(&mut out, 40);
    push_le_i32(&mut out, GB_ATLAS_WIDTH as i32);
    push_le_i32(&mut out, GB_ATLAS_HEIGHT as i32);
    push_le_u16(&mut out, 1);
    push_le_u16(&mut out, 32);
    push_le_u32(&mut out, 0);
    push_le_u32(&mut out, pixel_bytes as u32);
    push_le_i32(&mut out, 2835);
    push_le_i32(&mut out, 2835);
    push_le_u32(&mut out, 0);
    push_le_u32(&mut out, 0);

    for bmp_y in 0..GB_ATLAS_HEIGHT {
        let y = GB_ATLAS_HEIGHT - 1 - bmp_y;
        for x in 0..GB_ATLAS_WIDTH {
            let bank = y / (GB_ATLAS_BANK_ROWS * GB_TILE_PIXELS);
            let bank_y = y % (GB_ATLAS_BANK_ROWS * GB_TILE_PIXELS);
            let tile_col = x / GB_TILE_PIXELS;
            let tile_row = bank_y / GB_TILE_PIXELS;
            let tile_idx = tile_row * GB_ATLAS_COLS + tile_col;
            let px = x % GB_TILE_PIXELS;
            let py = bank_y % GB_TILE_PIXELS;
            let color = gb_tile_color_id(gpu, bank, tile_idx, px, py);
            let shade = match color {
                0 => 0xF8,
                1 => 0xB8,
                2 => 0x68,
                _ => 0x18,
            };
            out.push(shade);
            out.push(shade);
            out.push(shade);
            out.push(0xFF);
        }
    }

    out
}

fn gb_tile_color_id(
    gpu: &crate::trueos_gboi::gpu::Gpu,
    bank: usize,
    tile_idx: usize,
    px: usize,
    py: usize,
) -> u8 {
    let vram = if bank == 1 { &gpu.vram1 } else { &gpu.vram };
    let off = tile_idx * GB_TILE_BYTES + py * 2;
    if off + 1 >= vram.len() {
        return 0;
    }
    let bit = 7 - px;
    let lo = (vram[off] >> bit) & 1;
    let hi = (vram[off + 1] >> bit) & 1;
    lo | (hi << 1)
}

fn push_le_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_le_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_le_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
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
