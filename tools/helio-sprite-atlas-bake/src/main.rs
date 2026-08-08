use std::env;
use std::f32::consts::TAU;
use std::fs;
use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};

const MAGIC: &[u8; 8] = b"HDIGATL\0";
const VERSION: u16 = 1;
const HEADER_BYTES: usize = 64;
const ENTRY_BYTES: usize = 16;
const ATLAS_WIDTH: u32 = 512;
const GAP: u32 = 2;

const WHITE: u16 = 0;
const GRASS: u16 = 1;
const WATER: u16 = 2;
const DIRT: u16 = 3;
const STONE: u16 = 4;
const CRACK_BASE: u16 = 5;
const PLAYER_IDLE_BASE: u16 = 8;
const PLAYER_RUN_BASE: u16 = 12;
const PLAYER_JUMP_BASE: u16 = 20;
const PLAYER_FALL_BASE: u16 = 35;
const EXPECTED_ENTRY_COUNT: usize = 38;

#[derive(Clone, Copy)]
struct Placement {
    id: u16,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_f32(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 32) as u32 as f32 / u32::MAX as f32
    }
}

fn load_rgba(path: &Path) -> RgbaImage {
    image::open(path)
        .unwrap_or_else(|error| panic!("cannot decode {}: {error}", path.display()))
        .to_rgba8()
}

fn crop(image: &RgbaImage, x: u32, y: u32, width: u32, height: u32) -> RgbaImage {
    image::imageops::crop_imm(image, x, y, width, height).to_image()
}

fn trim(image: &RgbaImage) -> RgbaImage {
    let (mut min_x, mut min_y) = (image.width(), image.height());
    let (mut max_x, mut max_y) = (0, 0);
    for y in 0..image.height() {
        for x in 0..image.width() {
            if image.get_pixel(x, y)[3] != 0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    assert!(max_x >= min_x && max_y >= min_y, "selected sprite is empty");
    crop(image, min_x, min_y, max_x - min_x + 1, max_y - min_y + 1)
}

fn point_segment_distance(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let length_squared = dx * dx + dy * dy;
    let t = if length_squared > 0.0 {
        ((px - x0) * dx + (py - y0) * dy) / length_squared
    } else {
        0.0
    }
    .clamp(0.0, 1.0);
    let (cx, cy) = (x0 + dx * t, y0 + dy * t);
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

fn crack(stage: u32) -> RgbaImage {
    const SIZE: u32 = 64;
    let mut image = RgbaImage::new(SIZE, SIZE);
    let mut rng = Rng::new(0x0C7A_CC00 + u64::from(stage));
    let lines = 2 + stage * 2;
    let thickness = 1.6 + stage as f32 * 0.5;
    let center = SIZE as f32 * 0.5;
    for _ in 0..lines {
        let angle = rng.next_f32() * TAU;
        let length = SIZE as f32 * (0.25 + rng.next_f32() * 0.35);
        let (x1, y1) = (center + angle.cos() * length, center + angle.sin() * length);
        for y in 0..SIZE {
            for x in 0..SIZE {
                let distance =
                    point_segment_distance(x as f32 + 0.5, y as f32 + 0.5, center, center, x1, y1);
                if distance < thickness {
                    let alpha = ((1.0 - distance / thickness) * 210.0) as u8;
                    if alpha > image.get_pixel(x, y)[3] {
                        image.put_pixel(x, y, Rgba([15, 12, 10, alpha]));
                    }
                }
            }
        }
    }
    image
}

fn player_frames(sprites: &Path) -> Vec<(u16, RgbaImage)> {
    let sheets = [
        (PLAYER_IDLE_BASE, sprites.join("Character/Idle/Idle-Sheet.png"), 64, 80, 4),
        (PLAYER_RUN_BASE, sprites.join("Character/Run/Run-Sheet.png"), 80, 80, 8),
        (PLAYER_JUMP_BASE, sprites.join("Character/Jumlp-All/Jump-All-Sheet.png"), 64, 64, 15),
        (PLAYER_FALL_BASE, sprites.join("Character/Jump-End/Jump-End-Sheet.png"), 64, 64, 3),
    ];
    let mut raw = Vec::new();
    for (base, path, cell_width, cell_height, count) in sheets {
        let sheet = load_rgba(&path);
        for frame in 0..count {
            raw.push((
                base + frame as u16,
                trim(&crop(&sheet, frame * cell_width, 0, cell_width, cell_height)),
            ));
        }
    }
    let box_width = raw.iter().map(|(_, image)| image.width()).max().unwrap();
    let box_height = raw.iter().map(|(_, image)| image.height()).max().unwrap();
    raw.into_iter()
        .map(|(id, image)| {
            let mut canvas = RgbaImage::new(box_width, box_height);
            let x = (box_width - image.width()) / 2;
            let y = box_height - image.height();
            image::imageops::overlay(&mut canvas, &image, i64::from(x), i64::from(y));
            (id, canvas)
        })
        .collect()
}

fn selected_sprites(helio: &Path) -> Vec<(u16, RgbaImage)> {
    let sprites = helio.join("assets/sprites");
    let tiles = load_rgba(&sprites.join("Assets/Tiles.png"));
    let mut selected = vec![
        (WHITE, RgbaImage::from_pixel(4, 4, Rgba([255, 255, 255, 255]))),
        (GRASS, crop(&tiles, 16, 16, 16, 16)),
        (WATER, crop(&tiles, 3 * 16, 17 * 16, 16, 16)),
        (DIRT, crop(&tiles, 16, 3 * 16, 16, 16)),
        (STONE, crop(&tiles, 16, 6 * 16, 16, 16)),
    ];
    for stage in 1..=3 {
        selected.push((CRACK_BASE + stage as u16 - 1, crack(stage)));
    }
    selected.extend(player_frames(&sprites));
    selected.sort_by_key(|(id, _)| *id);
    assert_eq!(selected.len(), EXPECTED_ENTRY_COUNT);
    for (expected, (actual, _)) in selected.iter().enumerate() {
        assert_eq!(*actual as usize, expected, "sprite IDs must be dense");
    }
    selected
}

fn align_up(value: usize, alignment: usize) -> usize {
    value.div_ceil(alignment) * alignment
}

fn put_u16(output: &mut [u8], offset: usize, value: u16) {
    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn bake(helio: &Path, output_path: &Path) {
    let sprites = selected_sprites(helio);
    let mut placements = Vec::with_capacity(sprites.len());
    let (mut x, mut y, mut shelf_height) = (GAP, GAP, 0);
    for (id, image) in &sprites {
        if x + image.width() + GAP > ATLAS_WIDTH {
            x = GAP;
            y += shelf_height + GAP;
            shelf_height = 0;
        }
        placements.push(Placement {
            id: *id,
            x: x.try_into().unwrap(),
            y: y.try_into().unwrap(),
            width: image.width().try_into().unwrap(),
            height: image.height().try_into().unwrap(),
        });
        x += image.width() + GAP;
        shelf_height = shelf_height.max(image.height());
    }
    let atlas_height = (y + shelf_height + GAP).next_multiple_of(16);
    let mut atlas = RgbaImage::new(ATLAS_WIDTH, atlas_height);
    for ((_, image), placement) in sprites.iter().zip(&placements) {
        image::imageops::overlay(&mut atlas, image, i64::from(placement.x), i64::from(placement.y));
    }

    let entries_offset = HEADER_BYTES;
    let pixels_offset = align_up(entries_offset + placements.len() * ENTRY_BYTES, 64);
    let pixels = atlas.as_raw();
    let total_bytes = pixels_offset + pixels.len();
    let mut output = vec![0u8; total_bytes];
    output[..8].copy_from_slice(MAGIC);
    put_u16(&mut output, 8, VERSION);
    put_u16(&mut output, 10, HEADER_BYTES as u16);
    put_u32(&mut output, 12, total_bytes.try_into().unwrap());
    put_u16(&mut output, 16, ATLAS_WIDTH as u16);
    put_u16(&mut output, 18, atlas_height.try_into().unwrap());
    put_u32(&mut output, 20, ATLAS_WIDTH * 4);
    put_u16(&mut output, 24, placements.len().try_into().unwrap());
    put_u16(&mut output, 26, ENTRY_BYTES as u16);
    put_u32(&mut output, 28, entries_offset as u32);
    put_u32(&mut output, 32, pixels_offset as u32);
    put_u32(&mut output, 36, pixels.len().try_into().unwrap());
    put_u32(&mut output, 40, crc32fast::hash(pixels));
    put_u16(&mut output, 44, placements[PLAYER_IDLE_BASE as usize].width);
    put_u16(&mut output, 46, placements[PLAYER_IDLE_BASE as usize].height);
    for (index, placement) in placements.iter().enumerate() {
        let offset = entries_offset + index * ENTRY_BYTES;
        put_u16(&mut output, offset, placement.id);
        put_u16(&mut output, offset + 2, placement.x);
        put_u16(&mut output, offset + 4, placement.y);
        put_u16(&mut output, offset + 6, placement.width);
        put_u16(&mut output, offset + 8, placement.height);
    }
    output[pixels_offset..].copy_from_slice(pixels);
    fs::write(output_path, output)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", output_path.display()));
    println!(
        "baked Sprite Dig atlas {}x{} entries={} bytes={}",
        ATLAS_WIDTH,
        atlas_height,
        placements.len(),
        total_bytes
    );
}

fn main() {
    let mut args = env::args_os().skip(1).map(PathBuf::from);
    let helio = args
        .next()
        .expect("usage: helio-sprite-atlas-bake HELIO_REPO OUTPUT");
    let output = args
        .next()
        .expect("usage: helio-sprite-atlas-bake HELIO_REPO OUTPUT");
    assert!(args.next().is_none(), "usage: helio-sprite-atlas-bake HELIO_REPO OUTPUT");
    bake(&helio, &output);
}
