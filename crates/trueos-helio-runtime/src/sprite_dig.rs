//! Artifact-described, interactive `sprite_dig_demo` gameplay for TRUEOS.
//!
//! Hosted Helio renders the scene with an atlas-backed GPU sprite batch,
//! culling, sorting, and 2D radiance cascades. TRUEOS retains the original
//! terrain dimensions and player/dig/hotbar/place loop and exposes both the
//! compatibility colored-quad lowering and a compact atlas/tilemap frame for
//! the Bakery-produced C++ iGPU sprite kernel.

use alloc::vec;
use alloc::vec::Vec;

use crate::churn::Batch;
use crate::{Error, linear_rgba_to_srgba8};
use trueos_helio_artifact::SectionKind;

pub const SECTION_NAME: &str = "scene/sprite-dig-v1.bin";
pub const ATLAS_MAGIC: &[u8; 8] = b"HDIGATL\0";
pub const ATLAS_VERSION: u16 = 1;
pub const SPRITE_WHITE: u16 = 0;
pub const SPRITE_GRASS: u16 = 1;
pub const SPRITE_WATER: u16 = 2;
pub const SPRITE_DIRT: u16 = 3;
pub const SPRITE_STONE: u16 = 4;
pub const SPRITE_CRACK_BASE: u16 = 5;
pub const SPRITE_PLAYER_IDLE_BASE: u16 = 8;
pub const SPRITE_PLAYER_RUN_BASE: u16 = 12;
pub const SPRITE_PLAYER_JUMP_BASE: u16 = 20;
pub const SPRITE_PLAYER_FALL_BASE: u16 = 35;
const ATLAS_HEADER_BYTES: usize = 64;
const ATLAS_ENTRY_BYTES: usize = 16;
const MAGIC: &[u8; 8] = b"HDIG2D\0\0";
const VERSION: u16 = 1;
const ENCODED_LEN: usize = 256;
const DIRT_LAYERS: usize = 3;
const MATERIAL_KINDS: usize = 3;
const HIDDEN: [f32; 3] = [2.0, 2.0, 0.999];

const GRASS_BATCH: usize = 0;
const WATER_BATCH: usize = 1;
const DIRT_BATCH: usize = 2;
const STONE_BATCH: usize = 3;
const PLACED_BATCH: usize = 4;
const PLAYER_BATCH: usize = 5;
const CRACK_BATCH: usize = 6;
const HOTBAR_GRASS_BATCH: usize = 7;
const HOTBAR_DIRT_BATCH: usize = 8;
const HOTBAR_STONE_BATCH: usize = 9;
const HOTBAR_SELECTION_BATCH: usize = 10;
pub const DRAW_BATCH_COUNT: usize = 11;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AtlasSprite {
    pub id: u16,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

pub struct Atlas<'a> {
    pub width: u32,
    pub height: u32,
    pub pitch_bytes: u32,
    pub player_size: [u16; 2],
    entries: Vec<AtlasSprite>,
    pub pixels: &'a [u8],
}

impl<'a> Atlas<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < ATLAS_HEADER_BYTES
            || bytes.get(..8) != Some(ATLAS_MAGIC.as_slice())
            || read_u16_atlas(bytes, 8)? != ATLAS_VERSION
            || usize::from(read_u16_atlas(bytes, 10)?) != ATLAS_HEADER_BYTES
            || usize::try_from(read_u32_atlas(bytes, 12)?)
                .ok()
                .filter(|length| *length == bytes.len())
                .is_none()
        {
            return Err(Error::InvalidSpriteDigAtlas);
        }
        let width = u32::from(read_u16_atlas(bytes, 16)?);
        let height = u32::from(read_u16_atlas(bytes, 18)?);
        let pitch_bytes = read_u32_atlas(bytes, 20)?;
        let count = usize::from(read_u16_atlas(bytes, 24)?);
        let entry_bytes = usize::from(read_u16_atlas(bytes, 26)?);
        let entries_offset = usize::try_from(read_u32_atlas(bytes, 28)?)
            .map_err(|_| Error::InvalidSpriteDigAtlas)?;
        let pixels_offset = usize::try_from(read_u32_atlas(bytes, 32)?)
            .map_err(|_| Error::InvalidSpriteDigAtlas)?;
        let pixels_bytes = usize::try_from(read_u32_atlas(bytes, 36)?)
            .map_err(|_| Error::InvalidSpriteDigAtlas)?;
        let expected_crc = read_u32_atlas(bytes, 40)?;
        let player_size = [read_u16_atlas(bytes, 44)?, read_u16_atlas(bytes, 46)?];
        let entries_end = entries_offset
            .checked_add(
                count
                    .checked_mul(entry_bytes)
                    .ok_or(Error::InvalidSpriteDigAtlas)?,
            )
            .ok_or(Error::InvalidSpriteDigAtlas)?;
        let pixels_end = pixels_offset
            .checked_add(pixels_bytes)
            .ok_or(Error::InvalidSpriteDigAtlas)?;
        if width == 0
            || height == 0
            || pitch_bytes != width.saturating_mul(4)
            || count < usize::from(SPRITE_PLAYER_FALL_BASE + 3)
            || entry_bytes != ATLAS_ENTRY_BYTES
            || entries_offset != ATLAS_HEADER_BYTES
            || entries_end > pixels_offset
            || pixels_offset % 64 != 0
            || pixels_bytes != pitch_bytes as usize * height as usize
            || pixels_end != bytes.len()
            || player_size.contains(&0)
            || bytes[48..ATLAS_HEADER_BYTES].iter().any(|byte| *byte != 0)
        {
            return Err(Error::InvalidSpriteDigAtlas);
        }
        let pixels = &bytes[pixels_offset..pixels_end];
        if crc32(pixels) != expected_crc {
            return Err(Error::InvalidSpriteDigAtlas);
        }
        let mut entries = Vec::with_capacity(count);
        for id in 0..count {
            let offset = entries_offset + id * entry_bytes;
            let sprite = AtlasSprite {
                id: read_u16_atlas(bytes, offset)?,
                x: read_u16_atlas(bytes, offset + 2)?,
                y: read_u16_atlas(bytes, offset + 4)?,
                width: read_u16_atlas(bytes, offset + 6)?,
                height: read_u16_atlas(bytes, offset + 8)?,
            };
            let flags = read_u16_atlas(bytes, offset + 10)?;
            let reserved = read_u32_atlas(bytes, offset + 12)?;
            if usize::from(sprite.id) != id
                || sprite.width == 0
                || sprite.height == 0
                || u32::from(sprite.x) + u32::from(sprite.width) > width
                || u32::from(sprite.y) + u32::from(sprite.height) > height
                || flags != 0
                || reserved != 0
            {
                return Err(Error::InvalidSpriteDigAtlas);
            }
            entries.push(sprite);
        }
        Ok(Self {
            width,
            height,
            pitch_bytes,
            player_size,
            entries,
            pixels,
        })
    }

    pub fn sprites(&self) -> &[AtlasSprite] {
        &self.entries
    }

    pub fn sprite(&self, id: u16) -> Option<AtlasSprite> {
        self.entries.get(usize::from(id)).copied()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TexturedSprite {
    pub rect_px: [f32; 4],
    pub sprite_id: u16,
    pub tint: [u8; 4],
    pub flip_x: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextureFrame {
    pub surface_heights: Vec<f32>,
    /// Column-major dense tile IDs, including the top surface row.
    pub cells: Vec<u8>,
    pub sprites: Vec<TexturedSprite>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlayerAnimation {
    Idle,
    Run,
    Jump,
    Fall,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub world_cols: usize,
    pub dirt_rows: usize,
    pub stone_rows: usize,
    pub pool_capacity: usize,
    pub max_hotbar_slots: usize,
    pub max_placed: usize,
    pub lake_start: usize,
    pub lake_end: usize,
    pub tile: f32,
    pub zoom: f32,
    pub player_scale: f32,
    pub gravity: f32,
    pub move_speed: f32,
    pub jump_velocity: f32,
    pub break_stage_duration: f32,
    pub hotbar_slot_spacing: f32,
    pub hotbar_icon_size: f32,
    pub hotbar_margin_top: f32,
    pub camera_vertical_bias: f32,
    pub break_stages: u32,
    pub colors: [[u8; 4]; DRAW_BATCH_COUNT],
}

impl Spec {
    pub fn decode_artifact(bytes: &[u8]) -> Result<Self, Error> {
        let artifact =
            trueos_helio_artifact::Artifact::parse(bytes).map_err(|_| Error::Artifact)?;
        let section = artifact
            .section(SECTION_NAME)
            .ok_or(Error::MissingSpriteDigScene)?;
        if section.kind != SectionKind::Unknown(u16::MAX) {
            return Err(Error::InvalidSpriteDigScene);
        }
        let bytes = section.data;
        if bytes.len() != ENCODED_LEN
            || bytes.get(..8) != Some(MAGIC.as_slice())
            || read_u16(bytes, 8)? != VERSION
            || usize::from(read_u16(bytes, 10)?) != ENCODED_LEN
            || usize::try_from(read_u32(bytes, 12)?).map_err(|_| Error::InvalidSpriteDigScene)?
                != ENCODED_LEN
        {
            return Err(Error::InvalidSpriteDigScene);
        }

        let mut colors = [[0; 4]; DRAW_BATCH_COUNT];
        for (index, color) in colors.iter_mut().enumerate() {
            *color = linear_rgba_to_srgba8(read_f32s(bytes, 80 + index * 16)?)?;
        }
        let spec = Self {
            world_cols: usize::from(read_u16(bytes, 16)?),
            dirt_rows: usize::from(read_u16(bytes, 18)?),
            stone_rows: usize::from(read_u16(bytes, 20)?),
            pool_capacity: usize::from(read_u16(bytes, 22)?),
            max_hotbar_slots: usize::from(read_u16(bytes, 24)?),
            max_placed: usize::from(read_u16(bytes, 26)?),
            lake_start: usize::from(read_u16(bytes, 28)?),
            lake_end: usize::from(read_u16(bytes, 30)?),
            tile: read_f32(bytes, 32)?,
            zoom: read_f32(bytes, 36)?,
            player_scale: read_f32(bytes, 40)?,
            gravity: read_f32(bytes, 44)?,
            move_speed: read_f32(bytes, 48)?,
            jump_velocity: read_f32(bytes, 52)?,
            break_stage_duration: read_f32(bytes, 56)?,
            hotbar_slot_spacing: read_f32(bytes, 60)?,
            hotbar_icon_size: read_f32(bytes, 64)?,
            hotbar_margin_top: read_f32(bytes, 68)?,
            camera_vertical_bias: read_f32(bytes, 72)?,
            break_stages: read_u32(bytes, 76)?,
            colors,
        };
        let terrain_rows = spec
            .dirt_rows
            .checked_add(spec.stone_rows)
            .ok_or(Error::InvalidSpriteDigScene)?;
        let terrain_objects = spec
            .world_cols
            .checked_mul(terrain_rows.saturating_add(1))
            .ok_or(Error::InvalidSpriteDigScene)?;
        let retained_objects = terrain_objects
            .checked_add(spec.world_cols)
            .and_then(|count| count.checked_add(spec.max_placed))
            .and_then(|count| count.checked_add(1 + MATERIAL_KINDS + 2))
            .ok_or(Error::InvalidSpriteDigScene)?;
        if spec.world_cols == 0
            || terrain_rows < DIRT_LAYERS
            || retained_objects > spec.pool_capacity
            || spec.max_hotbar_slots < MATERIAL_KINDS
            || spec.max_placed == 0
            || spec.lake_start >= spec.lake_end
            || spec.lake_end > spec.world_cols
            || spec.tile <= 0.0
            || spec.zoom <= 0.0
            || spec.player_scale <= 0.0
            || spec.gravity >= 0.0
            || spec.move_speed <= 0.0
            || spec.jump_velocity <= 0.0
            || spec.break_stage_duration <= 0.0
            || spec.hotbar_slot_spacing <= 0.0
            || spec.hotbar_icon_size <= 0.0
            || spec.hotbar_margin_top <= 0.0
            || spec.break_stages == 0
            || spec.break_stages > 8
            || spec
                .colors
                .iter()
                .take(PLAYER_BATCH + 1)
                .any(|color| color[3] == 0)
        {
            return Err(Error::InvalidSpriteDigScene);
        }
        Ok(spec)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InputFrame {
    pub move_left: bool,
    pub move_right: bool,
    pub jump: bool,
    pub cursor_px: Option<[f32; 2]>,
    /// Last left-button transition in this frame (`true` is pressed).
    pub mine_button: Option<bool>,
    pub place_pressed: bool,
    pub wheel_lines: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MaterialKind {
    Grass = 0,
    Dirt = 1,
    Stone = 2,
}

impl MaterialKind {
    const fn from_row(row: usize) -> Self {
        if row == 0 {
            Self::Grass
        } else if row <= DIRT_LAYERS {
            Self::Dirt
        } else {
            Self::Stone
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PlacedBlock {
    active: bool,
    position: [f32; 2],
    material: MaterialKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MineTarget {
    Terrain { col: usize, row: usize },
    Placed { slot: usize },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Mining {
    target: MineTarget,
    elapsed: f32,
}

pub struct Engine {
    spec: Spec,
    broken: Vec<bool>,
    placed: Vec<PlacedBlock>,
    inventory: [u32; MATERIAL_KINDS],
    selected_material: usize,
    mining: Option<Mining>,
    cursor_px: [f32; 2],
    player_position: [f32; 2],
    player_velocity: [f32; 2],
    player_on_ground: bool,
    player_animation: PlayerAnimation,
    player_animation_time: f32,
    player_facing_right: bool,
    camera_center: [f32; 2],
    batches: Vec<Batch>,
}

impl Engine {
    pub fn new(spec: Spec) -> Result<Self, Error> {
        let terrain_rows = spec.dirt_rows + spec.stone_rows;
        let terrain_count = spec
            .world_cols
            .checked_mul(terrain_rows + 1)
            .ok_or(Error::InvalidSpriteDigScene)?;
        let mut batches = Vec::with_capacity(DRAW_BATCH_COUNT);
        batches.push(quad_batch(spec.world_cols, spec.colors[GRASS_BATCH])?);
        batches.push(quad_batch(spec.world_cols, spec.colors[WATER_BATCH])?);
        batches.push(quad_batch(spec.world_cols * DIRT_LAYERS, spec.colors[DIRT_BATCH])?);
        batches.push(quad_batch(
            spec.world_cols * (terrain_rows - DIRT_LAYERS),
            spec.colors[STONE_BATCH],
        )?);
        batches.push(quad_batch(spec.max_placed, spec.colors[PLACED_BATCH])?);
        batches.push(quad_batch(1, spec.colors[PLAYER_BATCH])?);
        batches.push(quad_batch(1, spec.colors[CRACK_BATCH])?);
        batches.push(quad_batch(1, spec.colors[HOTBAR_GRASS_BATCH])?);
        batches.push(quad_batch(1, spec.colors[HOTBAR_DIRT_BATCH])?);
        batches.push(quad_batch(1, spec.colors[HOTBAR_STONE_BATCH])?);
        batches.push(quad_batch(1, spec.colors[HOTBAR_SELECTION_BATCH])?);

        let player_height = 40.0 * spec.player_scale;
        let player_position = [
            spec.tile * 4.0,
            surface_top_world_y(spec.tile, 4) + player_height * 0.5,
        ];
        Ok(Self {
            spec: spec.clone(),
            broken: vec![false; terrain_count],
            placed: vec![
                PlacedBlock {
                    active: false,
                    position: [0.0; 2],
                    material: MaterialKind::Grass,
                };
                spec.max_placed
            ],
            inventory: [0; MATERIAL_KINDS],
            selected_material: 0,
            mining: None,
            cursor_px: [0.0; 2],
            player_position,
            player_velocity: [0.0; 2],
            player_on_ground: false,
            player_animation: PlayerAnimation::Idle,
            player_animation_time: 0.0,
            player_facing_right: true,
            camera_center: player_position,
            batches,
        })
    }

    pub fn name(&self) -> &'static str {
        "sprite-dig-demo"
    }

    pub fn controls(&self) -> &'static str {
        "A/D-or-arrows,Space,left-hold-mine,right-place,wheel-hotbar"
    }

    pub fn batches(&self) -> &[Batch] {
        &self.batches
    }

    pub fn object_count(&self) -> usize {
        self.broken.iter().filter(|broken| !**broken).count()
            + self.placed.iter().filter(|placed| placed.active).count()
            + 1
    }

    pub fn broken_count(&self) -> usize {
        self.broken.iter().filter(|broken| **broken).count()
    }

    pub fn placed_count(&self) -> usize {
        self.placed.iter().filter(|placed| placed.active).count()
    }

    pub fn inventory_count(&self) -> u32 {
        self.inventory.iter().copied().sum()
    }

    pub fn selected_material(&self) -> usize {
        self.selected_material
    }

    pub fn mining_stage(&self) -> u32 {
        self.mining.map_or(0, |mining| {
            libm::floorf(mining.elapsed / self.spec.break_stage_duration) as u32
        })
    }

    pub fn player_position(&self) -> [f32; 2] {
        self.player_position
    }

    pub fn camera_center(&self) -> [f32; 2] {
        self.camera_center
    }

    pub fn step(
        &mut self,
        input: InputFrame,
        width: u32,
        height: u32,
        dt_seconds: f32,
    ) -> Result<&[Batch], Error> {
        if width == 0
            || height == 0
            || !dt_seconds.is_finite()
            || !(0.0..=0.25).contains(&dt_seconds)
        {
            return Err(Error::InvalidSpriteDigScene);
        }
        if let Some(cursor) = input.cursor_px
            && cursor.iter().all(|value| value.is_finite())
        {
            self.cursor_px = cursor;
        }
        if input.wheel_lines.abs() > 0.01 {
            self.cycle_hotbar(if input.wheel_lines > 0.0 { -1 } else { 1 });
        }
        if let Some(pressed) = input.mine_button {
            self.mining = if pressed {
                self.hit_test(self.world_from_screen(width, height))
                    .map(|target| Mining {
                        target,
                        elapsed: 0.0,
                    })
            } else {
                None
            };
        }
        if input.place_pressed {
            self.place_selected(self.world_from_screen(width, height));
        }

        self.update_player(input, dt_seconds);
        self.update_player_animation(dt_seconds);
        if let Some(mining) = self.mining.as_mut() {
            mining.elapsed += dt_seconds;
            if mining.elapsed >= self.spec.break_stage_duration * self.spec.break_stages as f32 {
                let target = mining.target;
                self.mining = None;
                self.finish_mining(target);
            }
        }

        let target = [
            self.player_position[0],
            self.player_position[1] + self.spec.camera_vertical_bias,
        ];
        let smoothing = (dt_seconds * 5.0).min(1.0);
        self.camera_center[0] += (target[0] - self.camera_center[0]) * smoothing;
        self.camera_center[1] += (target[1] - self.camera_center[1]) * smoothing;
        self.rebuild_batches(width, height)?;
        Ok(&self.batches)
    }

    fn terrain_rows(&self) -> usize {
        self.spec.dirt_rows + self.spec.stone_rows
    }

    fn terrain_index(&self, col: usize, row: usize) -> usize {
        col * (self.terrain_rows() + 1) + row
    }

    fn terrain_is_broken(&self, col: usize, row: usize) -> bool {
        self.broken[self.terrain_index(col, row)]
    }

    fn ground_y_at(&self, col: usize) -> f32 {
        let mut row = 0usize;
        while row <= self.terrain_rows() && self.terrain_is_broken(col, row) {
            row += 1;
        }
        surface_top_world_y(self.spec.tile, col) - row as f32 * self.spec.tile
    }

    fn update_player(&mut self, input: InputFrame, dt: f32) {
        let direction = f32::from(input.move_right) - f32::from(input.move_left);
        if direction != 0.0 {
            self.player_facing_right = direction > 0.0;
        }
        self.player_velocity[0] = direction * self.spec.move_speed;
        self.player_velocity[1] += self.spec.gravity * dt;
        if input.jump && self.player_on_ground {
            self.player_velocity[1] = self.spec.jump_velocity;
            self.player_on_ground = false;
        }
        self.player_position[0] += self.player_velocity[0] * dt;
        self.player_position[1] += self.player_velocity[1] * dt;
        self.player_position[0] = self.player_position[0]
            .clamp(self.spec.tile, (self.spec.world_cols as f32 - 2.0) * self.spec.tile);
        let col = libm::roundf(self.player_position[0] / self.spec.tile) as usize;
        let player_height = 40.0 * self.spec.player_scale;
        let ground = self.ground_y_at(col) + player_height * 0.5;
        if self.player_position[1] <= ground {
            self.player_position[1] = ground;
            self.player_velocity[1] = 0.0;
            self.player_on_ground = true;
        } else {
            self.player_on_ground = false;
        }
    }

    fn update_player_animation(&mut self, dt: f32) {
        let animation = if !self.player_on_ground {
            if self.player_velocity[1] > 0.0 {
                PlayerAnimation::Jump
            } else {
                PlayerAnimation::Fall
            }
        } else if self.player_velocity[0].abs() > 1.0 {
            PlayerAnimation::Run
        } else {
            PlayerAnimation::Idle
        };
        if animation == self.player_animation {
            self.player_animation_time += dt;
        } else {
            self.player_animation = animation;
            self.player_animation_time = 0.0;
        }
    }

    fn world_from_screen(&self, width: u32, height: u32) -> [f32; 2] {
        [
            self.camera_center[0] + (self.cursor_px[0] - width as f32 * 0.5) / self.spec.zoom,
            self.camera_center[1] - (self.cursor_px[1] - height as f32 * 0.5) / self.spec.zoom,
        ]
    }

    fn hit_test(&self, point: [f32; 2]) -> Option<MineTarget> {
        for (slot, placed) in self.placed.iter().enumerate().rev() {
            if placed.active
                && (point[0] - placed.position[0]).abs() <= self.spec.tile * 0.5
                && (point[1] - placed.position[1]).abs() <= self.spec.tile * 0.5
            {
                return Some(MineTarget::Placed { slot });
            }
        }
        let col = libm::roundf(point[0] / self.spec.tile) as i32;
        if col < 0 || col >= self.spec.world_cols as i32 {
            return None;
        }
        let top = surface_top_world_y(self.spec.tile, col as usize);
        let row = libm::roundf((top - self.spec.tile * 0.5 - point[1]) / self.spec.tile) as i32;
        if row < 0 || row > self.terrain_rows() as i32 {
            return None;
        }
        let (col, row) = (col as usize, row as usize);
        (!self.terrain_is_broken(col, row)).then_some(MineTarget::Terrain { col, row })
    }

    fn finish_mining(&mut self, target: MineTarget) {
        let material = match target {
            MineTarget::Terrain { col, row } => {
                let index = self.terrain_index(col, row);
                if self.broken[index] {
                    return;
                }
                self.broken[index] = true;
                MaterialKind::from_row(row)
            }
            MineTarget::Placed { slot } => {
                let Some(placed) = self.placed.get_mut(slot) else {
                    return;
                };
                if !placed.active {
                    return;
                }
                placed.active = false;
                placed.material
            }
        };
        self.inventory[material.index()] = self.inventory[material.index()].saturating_add(1);
        if self.inventory[self.selected_material] == 0 {
            self.selected_material = material.index();
        }
    }

    fn cycle_hotbar(&mut self, direction: i32) {
        if self.inventory.iter().all(|count| *count == 0) {
            return;
        }
        let mut candidate = self.selected_material as i32;
        for _ in 0..MATERIAL_KINDS {
            candidate = (candidate + direction).rem_euclid(MATERIAL_KINDS as i32);
            if self.inventory[candidate as usize] != 0 {
                self.selected_material = candidate as usize;
                break;
            }
        }
    }

    fn place_selected(&mut self, world: [f32; 2]) {
        if self.inventory[self.selected_material] == 0 {
            return;
        }
        let Some(slot) = self.placed.iter_mut().find(|placed| !placed.active) else {
            return;
        };
        slot.active = true;
        slot.position = [
            libm::roundf(world[0] / self.spec.tile) * self.spec.tile,
            world[1],
        ];
        slot.material = match self.selected_material {
            0 => MaterialKind::Grass,
            1 => MaterialKind::Dirt,
            _ => MaterialKind::Stone,
        };
        self.inventory[self.selected_material] -= 1;
        if self.inventory[self.selected_material] == 0 {
            self.cycle_hotbar(1);
        }
    }

    /// Build the compact state consumed by the Bakery C++ tilemap kernel.
    /// Terrain stays column-major so the GPU can locate a tile directly;
    /// sparse and animated objects remain a small ordered sprite overlay.
    pub fn texture_frame(
        &self,
        width: u32,
        height: u32,
        player_native_size: [u16; 2],
    ) -> Result<TextureFrame, Error> {
        if width == 0 || height == 0 || player_native_size.contains(&0) {
            return Err(Error::InvalidSpriteDigScene);
        }
        let rows = self.terrain_rows() + 1;
        let mut surface_heights = Vec::with_capacity(self.spec.world_cols);
        let mut cells = Vec::with_capacity(self.spec.world_cols * rows);
        for col in 0..self.spec.world_cols {
            surface_heights.push(surface_top_world_y(self.spec.tile, col));
            for row in 0..rows {
                let sprite = if self.terrain_is_broken(col, row) {
                    SPRITE_WHITE
                } else if row == 0 {
                    if (self.spec.lake_start..self.spec.lake_end).contains(&col) {
                        SPRITE_WATER
                    } else {
                        SPRITE_GRASS
                    }
                } else if row <= DIRT_LAYERS {
                    SPRITE_DIRT
                } else {
                    SPRITE_STONE
                };
                cells.push(u8::try_from(sprite).map_err(|_| Error::InvalidSpriteDigScene)?);
            }
        }

        let mut sprites = Vec::with_capacity(self.spec.max_placed + MATERIAL_KINDS + 3);
        for placed in self.placed.iter().filter(|placed| placed.active) {
            let sprite_id = match placed.material {
                MaterialKind::Grass => SPRITE_GRASS,
                MaterialKind::Dirt => SPRITE_DIRT,
                MaterialKind::Stone => SPRITE_STONE,
            };
            push_world_sprite(
                &mut sprites,
                placed.position,
                [self.spec.tile; 2],
                sprite_id,
                [255; 4],
                false,
                self.camera_center,
                self.spec.zoom,
                width,
                height,
            );
        }

        let (animation_base, animation_frames, fps) = match self.player_animation {
            PlayerAnimation::Idle => (SPRITE_PLAYER_IDLE_BASE, 4u16, 6.0),
            PlayerAnimation::Run => (SPRITE_PLAYER_RUN_BASE, 8u16, 12.0),
            PlayerAnimation::Jump => (SPRITE_PLAYER_JUMP_BASE, 15u16, 15.0),
            PlayerAnimation::Fall => (SPRITE_PLAYER_FALL_BASE, 3u16, 9.0),
        };
        let animation_index =
            (libm::floorf(self.player_animation_time * fps) as u16) % animation_frames;
        push_world_sprite(
            &mut sprites,
            self.player_position,
            [
                40.0 * f32::from(player_native_size[0]) / f32::from(player_native_size[1])
                    * self.spec.player_scale,
                40.0 * self.spec.player_scale,
            ],
            animation_base + animation_index,
            [255; 4],
            !self.player_facing_right,
            self.camera_center,
            self.spec.zoom,
            width,
            height,
        );

        if let Some(mining) = self.mining
            && self.mining_stage() != 0
        {
            let center = match mining.target {
                MineTarget::Terrain { col, row } => [
                    col as f32 * self.spec.tile,
                    surface_top_world_y(self.spec.tile, col)
                        - self.spec.tile * 0.5
                        - row as f32 * self.spec.tile,
                ],
                MineTarget::Placed { slot } => self.placed[slot].position,
            };
            let stage = self.mining_stage().min(self.spec.break_stages).max(1);
            push_world_sprite(
                &mut sprites,
                center,
                [self.spec.tile; 2],
                SPRITE_CRACK_BASE + stage as u16 - 1,
                [255; 4],
                false,
                self.camera_center,
                self.spec.zoom,
                width,
                height,
            );
        }

        let active = self
            .inventory
            .iter()
            .enumerate()
            .filter(|(_, count)| **count != 0)
            .map(|(material, _)| material)
            .collect::<Vec<_>>();
        for (visible_index, material) in active.iter().copied().enumerate() {
            let center = hotbar_world_position(
                self.camera_center,
                width,
                height,
                visible_index,
                active.len(),
                &self.spec,
            );
            if material == self.selected_material {
                push_world_sprite(
                    &mut sprites,
                    center,
                    [self.spec.hotbar_icon_size + 10.0; 2],
                    SPRITE_WHITE,
                    [255, 220, 72, 210],
                    false,
                    self.camera_center,
                    self.spec.zoom,
                    width,
                    height,
                );
            }
            let sprite_id = [SPRITE_GRASS, SPRITE_DIRT, SPRITE_STONE][material];
            push_world_sprite(
                &mut sprites,
                center,
                [self.spec.hotbar_icon_size; 2],
                sprite_id,
                [255; 4],
                false,
                self.camera_center,
                self.spec.zoom,
                width,
                height,
            );
        }
        Ok(TextureFrame {
            surface_heights,
            cells,
            sprites,
        })
    }

    pub fn tilemap_spec(&self) -> (usize, usize, f32, f32, [f32; 2]) {
        (
            self.spec.world_cols,
            self.terrain_rows() + 1,
            self.spec.tile,
            self.spec.zoom,
            self.camera_center,
        )
    }

    fn rebuild_batches(&mut self, width: u32, height: u32) -> Result<(), Error> {
        for batch in &mut self.batches {
            batch.vertices.fill(HIDDEN);
        }
        let terrain_rows = self.terrain_rows();
        let mut dirt_slot = 0usize;
        let mut stone_slot = 0usize;
        for col in 0..self.spec.world_cols {
            let top = surface_top_world_y(self.spec.tile, col);
            if !self.terrain_is_broken(col, 0) {
                let center = [col as f32 * self.spec.tile, top - self.spec.tile * 0.5];
                let (batch, slot) = if (self.spec.lake_start..self.spec.lake_end).contains(&col) {
                    (WATER_BATCH, col)
                } else {
                    (GRASS_BATCH, col)
                };
                write_world_quad(
                    &mut self.batches[batch],
                    slot,
                    center,
                    [self.spec.tile + 1.0; 2],
                    0.70,
                    self.camera_center,
                    self.spec.zoom,
                    width,
                    height,
                )?;
            }
            for row in 1..=terrain_rows {
                let center = [
                    col as f32 * self.spec.tile,
                    top - self.spec.tile * 0.5 - row as f32 * self.spec.tile,
                ];
                if row <= DIRT_LAYERS {
                    if !self.terrain_is_broken(col, row) {
                        write_world_quad(
                            &mut self.batches[DIRT_BATCH],
                            dirt_slot,
                            center,
                            [self.spec.tile + 1.0; 2],
                            0.72,
                            self.camera_center,
                            self.spec.zoom,
                            width,
                            height,
                        )?;
                    }
                    dirt_slot += 1;
                } else {
                    if !self.terrain_is_broken(col, row) {
                        write_world_quad(
                            &mut self.batches[STONE_BATCH],
                            stone_slot,
                            center,
                            [self.spec.tile + 1.0; 2],
                            0.74,
                            self.camera_center,
                            self.spec.zoom,
                            width,
                            height,
                        )?;
                    }
                    stone_slot += 1;
                }
            }
        }
        for (slot, placed) in self.placed.iter().enumerate() {
            if placed.active {
                write_world_quad(
                    &mut self.batches[PLACED_BATCH],
                    slot,
                    placed.position,
                    [self.spec.tile; 2],
                    0.55,
                    self.camera_center,
                    self.spec.zoom,
                    width,
                    height,
                )?;
            }
        }
        write_world_quad(
            &mut self.batches[PLAYER_BATCH],
            0,
            self.player_position,
            [24.0 * self.spec.player_scale, 40.0 * self.spec.player_scale],
            0.30,
            self.camera_center,
            self.spec.zoom,
            width,
            height,
        )?;
        if let Some(mining) = self.mining
            && self.mining_stage() != 0
        {
            let center = match mining.target {
                MineTarget::Terrain { col, row } => [
                    col as f32 * self.spec.tile,
                    surface_top_world_y(self.spec.tile, col)
                        - self.spec.tile * 0.5
                        - row as f32 * self.spec.tile,
                ],
                MineTarget::Placed { slot } => self.placed[slot].position,
            };
            let stage = self.mining_stage().min(self.spec.break_stages);
            let scale = 0.30 + 0.18 * stage as f32;
            write_world_quad(
                &mut self.batches[CRACK_BATCH],
                0,
                center,
                [self.spec.tile * scale; 2],
                0.18,
                self.camera_center,
                self.spec.zoom,
                width,
                height,
            )?;
        }
        let active = self
            .inventory
            .iter()
            .enumerate()
            .filter(|(_, count)| **count != 0)
            .map(|(material, _)| material)
            .collect::<Vec<_>>();
        for (visible_index, material) in active.iter().copied().enumerate() {
            let center = hotbar_world_position(
                self.camera_center,
                width,
                height,
                visible_index,
                active.len(),
                &self.spec,
            );
            let batch = HOTBAR_GRASS_BATCH + material;
            write_world_quad(
                &mut self.batches[batch],
                0,
                center,
                [self.spec.hotbar_icon_size; 2],
                0.08,
                self.camera_center,
                self.spec.zoom,
                width,
                height,
            )?;
            if material == self.selected_material {
                write_world_quad(
                    &mut self.batches[HOTBAR_SELECTION_BATCH],
                    0,
                    center,
                    [self.spec.hotbar_icon_size + 10.0; 2],
                    0.10,
                    self.camera_center,
                    self.spec.zoom,
                    width,
                    height,
                )?;
            }
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn push_world_sprite(
    sprites: &mut Vec<TexturedSprite>,
    center: [f32; 2],
    size: [f32; 2],
    sprite_id: u16,
    tint: [u8; 4],
    flip_x: bool,
    camera: [f32; 2],
    zoom: f32,
    width: u32,
    height: u32,
) {
    let center_px = [
        width as f32 * 0.5 + (center[0] - camera[0]) * zoom,
        height as f32 * 0.5 - (center[1] - camera[1]) * zoom,
    ];
    let half_px = [size[0] * zoom * 0.5, size[1] * zoom * 0.5];
    let rect_px = [
        center_px[0] - half_px[0],
        center_px[1] - half_px[1],
        center_px[0] + half_px[0],
        center_px[1] + half_px[1],
    ];
    if rect_px[2] <= 0.0
        || rect_px[3] <= 0.0
        || rect_px[0] >= width as f32
        || rect_px[1] >= height as f32
    {
        return;
    }
    sprites.push(TexturedSprite {
        rect_px,
        sprite_id,
        tint,
        flip_x,
    });
}

fn quad_batch(slots: usize, rgba: [u8; 4]) -> Result<Batch, Error> {
    let vertices = vec![HIDDEN; slots.checked_mul(4).ok_or(Error::InvalidSpriteDigScene)?];
    let mut indices = Vec::with_capacity(slots.checked_mul(6).ok_or(Error::InvalidSpriteDigScene)?);
    for slot in 0..slots {
        let base = u32::try_from(slot.checked_mul(4).ok_or(Error::InvalidSpriteDigScene)?)
            .map_err(|_| Error::InvalidSpriteDigScene)?;
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Ok(Batch {
        vertices,
        indices,
        rgba,
    })
}

#[allow(clippy::too_many_arguments)]
fn write_world_quad(
    batch: &mut Batch,
    slot: usize,
    center: [f32; 2],
    size: [f32; 2],
    depth: f32,
    camera: [f32; 2],
    zoom: f32,
    width: u32,
    height: u32,
) -> Result<(), Error> {
    let offset = slot.checked_mul(4).ok_or(Error::InvalidSpriteDigScene)?;
    let vertices = batch
        .vertices
        .get_mut(offset..offset + 4)
        .ok_or(Error::InvalidSpriteDigScene)?;
    let half = [size[0] * 0.5, size[1] * 0.5];
    let project = |world: [f32; 2]| {
        [
            (world[0] - camera[0]) * zoom / (width as f32 * 0.5),
            (world[1] - camera[1]) * zoom / (height as f32 * 0.5),
            depth,
        ]
    };
    vertices.copy_from_slice(&[
        project([center[0] - half[0], center[1] - half[1]]),
        project([center[0] + half[0], center[1] - half[1]]),
        project([center[0] + half[0], center[1] + half[1]]),
        project([center[0] - half[0], center[1] + half[1]]),
    ]);
    Ok(())
}

fn hotbar_world_position(
    camera: [f32; 2],
    width: u32,
    height: u32,
    index: usize,
    total: usize,
    spec: &Spec,
) -> [f32; 2] {
    let count = total.max(1) as f32;
    let x_offset = (index as f32 - (count - 1.0) * 0.5) * spec.hotbar_slot_spacing / spec.zoom;
    let y_offset = height as f32 * 0.5 / spec.zoom - spec.hotbar_margin_top;
    let _ = width;
    [camera[0] + x_offset, camera[1] + y_offset]
}

fn surface_top_world_y(tile: f32, col: usize) -> f32 {
    let value = col as f32;
    let height = libm::sinf(value * 0.09) * 3.5
        + libm::sinf(value * 0.03) * 6.0
        + libm::sinf(value * 0.23) * 1.2;
    libm::roundf(height) * tile
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    let raw = bytes
        .get(offset..offset + 2)
        .ok_or(Error::InvalidSpriteDigScene)?;
    Ok(u16::from_le_bytes(raw.try_into().unwrap()))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    let raw = bytes
        .get(offset..offset + 4)
        .ok_or(Error::InvalidSpriteDigScene)?;
    Ok(u32::from_le_bytes(raw.try_into().unwrap()))
}

fn read_f32(bytes: &[u8], offset: usize) -> Result<f32, Error> {
    let value = f32::from_bits(read_u32(bytes, offset)?);
    value
        .is_finite()
        .then_some(value)
        .ok_or(Error::InvalidSpriteDigScene)
}

fn read_f32s<const N: usize>(bytes: &[u8], offset: usize) -> Result<[f32; N], Error> {
    let mut values = [0.0; N];
    for (index, value) in values.iter_mut().enumerate() {
        *value = read_f32(bytes, offset + index * 4)?;
    }
    Ok(values)
}

fn read_u16_atlas(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    let raw = bytes
        .get(offset..offset + 2)
        .ok_or(Error::InvalidSpriteDigAtlas)?;
    Ok(u16::from_le_bytes(raw.try_into().unwrap()))
}

fn read_u32_atlas(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    let raw = bytes
        .get(offset..offset + 4)
        .ok_or(Error::InvalidSpriteDigAtlas)?;
    Ok(u32::from_le_bytes(raw.try_into().unwrap()))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::{Atlas, Engine, InputFrame, Spec};

    const ARTIFACT: &[u8] = include_bytes!("../../../assets/helio/simple-cube.trueos.intel.helio");
    const ATLAS: &[u8] = include_bytes!("../../../assets/helio/sprite-dig-atlas.trueos.rgba");

    fn screen_for_world(engine: &Engine, width: u32, height: u32, world: [f32; 2]) -> [f32; 2] {
        [
            width as f32 * 0.5 + (world[0] - engine.camera_center()[0]) * engine.spec.zoom,
            height as f32 * 0.5 - (world[1] - engine.camera_center()[1]) * engine.spec.zoom,
        ]
    }

    #[test]
    fn artifact_contract_builds_fixed_topology_batches() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut engine = Engine::new(spec).unwrap();
        let batches = engine
            .step(InputFrame::default(), 1280, 720, 1.0 / 30.0)
            .unwrap();
        assert_eq!(batches.len(), super::DRAW_BATCH_COUNT);
        assert!(batches.iter().all(|batch| !batch.indices.is_empty()));
        assert_eq!(engine.object_count(), 240 * 23 + 1);
    }

    #[test]
    fn atlas_contract_decodes_original_terrain_and_animation_frames() {
        let atlas = Atlas::decode(ATLAS).unwrap();
        assert_eq!((atlas.width, atlas.height), (512, 192));
        assert_eq!(atlas.sprites().len(), 38);
        assert_eq!(atlas.player_size, [35, 59]);
        assert_eq!(atlas.pixels.len(), atlas.pitch_bytes as usize * atlas.height as usize);
    }

    #[test]
    fn textured_frame_is_dense_terrain_plus_bounded_sprite_overlays() {
        let atlas = Atlas::decode(ATLAS).unwrap();
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut engine = Engine::new(spec).unwrap();
        engine
            .step(InputFrame::default(), 1280, 720, 1.0 / 30.0)
            .unwrap();
        let frame = engine.texture_frame(1280, 720, atlas.player_size).unwrap();
        assert_eq!(frame.surface_heights.len(), 240);
        assert_eq!(frame.cells.len(), 240 * 23);
        assert_eq!(frame.sprites.len(), 1);
        assert!(frame.sprites[0].sprite_id >= super::SPRITE_PLAYER_IDLE_BASE);
    }

    #[test]
    fn held_ui4_style_input_moves_and_jumps_the_player() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut engine = Engine::new(spec).unwrap();
        engine
            .step(InputFrame::default(), 1280, 720, 1.0 / 30.0)
            .unwrap();
        let before = engine.player_position();
        engine
            .step(
                InputFrame {
                    move_right: true,
                    jump: true,
                    ..InputFrame::default()
                },
                1280,
                720,
                1.0 / 30.0,
            )
            .unwrap();
        assert!(engine.player_position()[0] > before[0]);
        assert!(engine.player_position()[1] >= before[1]);
    }

    #[test]
    fn hold_to_mine_then_right_click_to_place_round_trips_inventory() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let tile = spec.tile;
        let mut engine = Engine::new(spec).unwrap();
        engine
            .step(InputFrame::default(), 1280, 720, 1.0 / 30.0)
            .unwrap();
        let col = 4usize;
        let target = [
            col as f32 * tile,
            super::surface_top_world_y(tile, col) - tile * 0.5,
        ];
        let cursor = screen_for_world(&engine, 1280, 720, target);
        engine
            .step(
                InputFrame {
                    cursor_px: Some(cursor),
                    mine_button: Some(true),
                    ..InputFrame::default()
                },
                1280,
                720,
                1.0 / 30.0,
            )
            .unwrap();
        for _ in 0..24 {
            engine
                .step(InputFrame::default(), 1280, 720, 1.0 / 30.0)
                .unwrap();
        }
        assert_eq!(engine.broken_count(), 1);
        assert_eq!(engine.inventory_count(), 1);
        engine
            .step(
                InputFrame {
                    cursor_px: Some(cursor),
                    place_pressed: true,
                    ..InputFrame::default()
                },
                1280,
                720,
                1.0 / 30.0,
            )
            .unwrap();
        assert_eq!(engine.placed_count(), 1);
        assert_eq!(engine.inventory_count(), 0);
    }

    #[test]
    fn release_and_focus_loss_style_transition_cancel_mining() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let tile = spec.tile;
        let mut engine = Engine::new(spec).unwrap();
        engine
            .step(InputFrame::default(), 1280, 720, 1.0 / 30.0)
            .unwrap();
        let col = 4usize;
        let target = [
            col as f32 * tile,
            super::surface_top_world_y(tile, col) - tile * 0.5,
        ];
        let cursor = screen_for_world(&engine, 1280, 720, target);
        engine
            .step(
                InputFrame {
                    cursor_px: Some(cursor),
                    mine_button: Some(true),
                    ..InputFrame::default()
                },
                1280,
                720,
                1.0 / 30.0,
            )
            .unwrap();
        assert_eq!(engine.mining_stage(), 0);
        assert!(
            engine.batches[super::CRACK_BATCH]
                .vertices
                .iter()
                .all(|vertex| *vertex == super::HIDDEN)
        );
        engine
            .step(
                InputFrame {
                    mine_button: Some(false),
                    ..InputFrame::default()
                },
                1280,
                720,
                1.0 / 30.0,
            )
            .unwrap();
        for _ in 0..24 {
            engine
                .step(InputFrame::default(), 1280, 720, 1.0 / 30.0)
                .unwrap();
        }
        assert_eq!(engine.broken_count(), 0);
        assert_eq!(engine.inventory_count(), 0);
    }
}
