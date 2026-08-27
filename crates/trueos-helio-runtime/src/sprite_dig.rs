//! Artifact-described, interactive `sprite_dig_demo` gameplay for TRUEOS.
//!
//! Hosted Helio renders the scene with an atlas-backed GPU sprite batch,
//! culling, sorting, and 2D radiance cascades. TRUEOS retains its complete
//! textured scene contract: stable atlas identities, deterministic zone
//! placement, layered trees, animated critters, player, background, mining,
//! arbitrary-sprite hotbar and placement. It exposes both the compatibility
//! colored-quad lowering and a compact atlas/tilemap frame for the
//! Bakery-produced C++ iGPU sprite kernel. Radiance remains a separate pass.

use alloc::vec;
use alloc::vec::Vec;

use crate::churn::Batch;
use crate::scene_db::{DirtyRows, PublishedScene, SceneStore};
use crate::{Error, linear_rgba_to_srgba8};
use trueos_helio_artifact::SectionKind;

pub use crate::scene_db::SceneHandle as SpriteHandle;

pub const SECTION_NAME: &str = "scene/sprite-dig-v1.bin";
pub const ATLAS_MAGIC: &[u8; 8] = b"HDIGATL\0";
pub const ATLAS_VERSION: u16 = 2;
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
pub const SPRITE_BOAR_WALK_BASE: u16 = 38;
pub const SPRITE_BOAR_IDLE_BASE: u16 = 44;
pub const SPRITE_BEE_FLY_BASE: u16 = 48;
pub const SPRITE_SNAIL_WALK_BASE: u16 = 52;
pub const SPRITE_BUSH_BASE: u16 = 60;
pub const SPRITE_CABIN: u16 = 64;
pub const SPRITE_TREE_GREEN_TALL_BASE: u16 = 65;
pub const SPRITE_TREE_GREEN_MED_BASE: u16 = 68;
pub const SPRITE_TREE_DARK_TALL_BASE: u16 = 71;
pub const SPRITE_TREE_DARK_MED_BASE: u16 = 74;
pub const SPRITE_TREE_RED_TALL_BASE: u16 = 77;
pub const SPRITE_TREE_GOLDEN_TALL_BASE: u16 = 80;
pub const SPRITE_TREE_GOLDEN_MED_BASE: u16 = 83;
pub const SPRITE_TREE_YELLOW_TALL_BASE: u16 = 86;
pub const SPRITE_TREE_YELLOW_MED_BASE: u16 = 89;
pub const SPRITE_BACKGROUND: u16 = 92;
pub const SPRITE_COUNT: u16 = 93;
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
            || count != usize::from(SPRITE_COUNT)
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
    /// Hosted Helio's sprite batch sorts low depth first.
    pub depth: f32,
    pub rotation: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextureFrame {
    pub surface_heights: Vec<f32>,
    /// Column-major dense tile IDs, including the top surface row.
    pub cells: Vec<u8>,
    pub sprites: Vec<TexturedSprite>,
    /// Publication metadata for the stable retained-prop rows from which the
    /// compatibility overlay list was lowered.  A native backend uses this
    /// to delta-upload the parallel row/liveness/generation buffers instead.
    pub scene_epoch: u64,
    pub scene_rows: usize,
    pub scene_live_rows: usize,
    pub scene_dirty: DirtyRows,
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

#[derive(Clone, Copy, Debug, PartialEq)]
struct PlacedBlock {
    active: bool,
    position: [f32; 2],
    sprite_id: u16,
    size: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct InventorySlot {
    sprite_id: u16,
    size: [f32; 2],
    count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RetainedSprite {
    pub sprite_id: u16,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub depth: f32,
    pub rotation: f32,
    pub tint: [u8; 4],
    pub atlas_layer: u32,
    pub flip_x: bool,
    breakable: bool,
    /// The compatibility tilemap kernel reconstructs retained terrain itself;
    /// only sparse rows must also be lowered into ordered overlay descriptors.
    compatibility_overlay: bool,
}

/// Byte-exact twin of Helio's `SpriteInstance` / `sprite.wgsl` storage row.
/// The explicit padding preserves the two 16-byte-aligned `vec4` fields.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuSpriteInstance {
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub rotation: f32,
    pub depth: f32,
    pad_uv: [f32; 2],
    pub uv_rect: [f32; 4],
    pub color: [f32; 4],
    pub atlas_layer: u32,
    pad_tail: [u32; 3],
}

impl GpuSpriteInstance {
    pub const BYTE_LEN: usize = 80;

    pub fn to_le_bytes(self) -> [u8; Self::BYTE_LEN] {
        let float_fields = [
            self.position[0],
            self.position[1],
            self.size[0],
            self.size[1],
            self.rotation,
            self.depth,
            self.pad_uv[0],
            self.pad_uv[1],
            self.uv_rect[0],
            self.uv_rect[1],
            self.uv_rect[2],
            self.uv_rect[3],
            self.color[0],
            self.color[1],
            self.color[2],
            self.color[3],
        ];
        let mut bytes = [0u8; Self::BYTE_LEN];
        for (index, value) in float_fields.into_iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_bits().to_le_bytes());
        }
        bytes[64..68].copy_from_slice(&self.atlas_layer.to_le_bytes());
        for (index, value) in self.pad_tail.into_iter().enumerate() {
            let offset = 68 + index * 4;
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

const _: () = assert!(core::mem::size_of::<GpuSpriteInstance>() == GpuSpriteInstance::BYTE_LEN);

impl RetainedSprite {
    /// Resolve the stable artifact sprite identity into Helio's native GPU
    /// row. Atlas pixels stay independently resident; only normalized UVs
    /// and the layer identity enter the instance buffer.
    pub fn gpu_instance(&self, atlas: &Atlas<'_>) -> Result<GpuSpriteInstance, Error> {
        let sprite = atlas
            .sprite(self.sprite_id)
            .ok_or(Error::InvalidSpriteDigAtlas)?;
        let inv_width = 1.0 / atlas.width as f32;
        let inv_height = 1.0 / atlas.height as f32;
        let mut u0 = f32::from(sprite.x) * inv_width;
        let mut u1 = f32::from(sprite.x + sprite.width) * inv_width;
        if self.flip_x {
            core::mem::swap(&mut u0, &mut u1);
        }
        Ok(GpuSpriteInstance {
            position: self.position,
            size: self.size,
            rotation: self.rotation,
            depth: self.depth,
            pad_uv: [0.0; 2],
            uv_rect: [
                u0,
                f32::from(sprite.y) * inv_height,
                u1,
                f32::from(sprite.y + sprite.height) * inv_height,
            ],
            color: self.tint.map(|component| f32::from(component) / 255.0),
            atlas_layer: self.atlas_layer,
            pad_tail: [0; 3],
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Critter {
    handle: SpriteHandle,
    base_position: [f32; 2],
    phase: f32,
    frame_base: u16,
    frame_count: u16,
    fps: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MineTarget {
    Terrain { col: usize, row: usize },
    Placed { slot: usize },
    Scene(SpriteHandle),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Mining {
    target: MineTarget,
    elapsed: f32,
}

pub struct Engine {
    spec: Spec,
    broken: Vec<bool>,
    terrain_handles: Vec<SpriteHandle>,
    player_handle: SpriteHandle,
    placed: Vec<PlacedBlock>,
    inventory: Vec<InventorySlot>,
    selected_hotbar: usize,
    mining: Option<Mining>,
    cursor_px: [f32; 2],
    player_position: [f32; 2],
    player_velocity: [f32; 2],
    player_on_ground: bool,
    player_animation: PlayerAnimation,
    player_animation_time: f32,
    player_facing_right: bool,
    player_native_size: [f32; 2],
    camera_center: [f32; 2],
    elapsed: f32,
    scene_sprites: SceneStore<RetainedSprite>,
    critters: Vec<Critter>,
    scene_installed: bool,
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
        let mut scene_sprites = SceneStore::with_capacity(spec.pool_capacity);
        let mut terrain_handles = Vec::with_capacity(terrain_count);
        for col in 0..spec.world_cols {
            let top = surface_top_world_y(spec.tile, col);
            for row in 0..=terrain_rows {
                let sprite_id = if row == 0 {
                    if (spec.lake_start..spec.lake_end).contains(&col) {
                        SPRITE_WATER
                    } else {
                        SPRITE_GRASS
                    }
                } else if row <= DIRT_LAYERS {
                    SPRITE_DIRT
                } else {
                    SPRITE_STONE
                };
                terrain_handles.push(scene_sprites.insert(RetainedSprite {
                    sprite_id,
                    position: [
                        col as f32 * spec.tile,
                        top - spec.tile * 0.5 - row as f32 * spec.tile,
                    ],
                    size: [spec.tile + 1.0; 2],
                    depth: 0.0,
                    rotation: 0.0,
                    tint: [255; 4],
                    atlas_layer: 0,
                    flip_x: false,
                    breakable: true,
                    compatibility_overlay: false,
                }));
            }
        }
        let player_handle = scene_sprites.insert(RetainedSprite {
            sprite_id: SPRITE_PLAYER_IDLE_BASE,
            position: player_position,
            size: [20.0 * spec.player_scale, 40.0 * spec.player_scale],
            depth: 0.5,
            rotation: 0.0,
            tint: [255; 4],
            atlas_layer: 0,
            flip_x: false,
            breakable: false,
            compatibility_overlay: true,
        });
        Ok(Self {
            spec: spec.clone(),
            broken: vec![false; terrain_count],
            terrain_handles,
            player_handle,
            placed: vec![
                PlacedBlock {
                    active: false,
                    position: [0.0; 2],
                    sprite_id: SPRITE_GRASS,
                    size: [spec.tile; 2],
                };
                spec.max_placed
            ],
            inventory: Vec::new(),
            selected_hotbar: 0,
            mining: None,
            cursor_px: [0.0; 2],
            player_position,
            player_velocity: [0.0; 2],
            player_on_ground: false,
            player_animation: PlayerAnimation::Idle,
            player_animation_time: 0.0,
            player_facing_right: true,
            player_native_size: [20.0, 40.0],
            camera_center: player_position,
            elapsed: 0.0,
            scene_sprites,
            critters: Vec::new(),
            scene_installed: false,
            batches,
        })
    }

    /// Install the complete retained Sprite Dig scene from Bakery atlas IDs.
    /// This is deliberately separate from `new`: the gameplay spec remains a
    /// small artifact, while native sprite extents come from the immutable
    /// atlas artifact exactly once, before the first submitted frame.
    pub fn install_atlas_scene(&mut self, atlas: &Atlas<'_>) -> Result<(), Error> {
        if self.scene_installed {
            return Ok(());
        }
        self.player_native_size = [
            f32::from(atlas.player_size[0]),
            f32::from(atlas.player_size[1]),
        ];
        self.player_position = [
            self.spec.tile * 4.0,
            surface_top_world_y(self.spec.tile, 4)
                + self.player_native_size[1] * self.spec.player_scale * 0.5,
        ];
        self.camera_center = self.player_position;
        let player = self
            .scene_sprites
            .get_mut(self.player_handle)
            .ok_or(Error::InvalidSpriteDigScene)?;
        player.position = self.player_position;
        player.size = [
            self.player_native_size[0] * self.spec.player_scale,
            self.player_native_size[1] * self.spec.player_scale,
        ];
        let mut rng = SceneRng::new(0xC0FF_EE12_3456_7890);

        // Spawn ground cover.
        self.scatter_static(atlas, &mut rng, 2, 14, (4, 7), &[60, 61, 62, 63], 0.2)?;

        // Forest A.
        self.scatter_trees(
            atlas,
            &mut rng,
            14,
            40,
            (4, 8),
            &[SPRITE_TREE_GREEN_TALL_BASE, SPRITE_TREE_GREEN_MED_BASE],
            0.15,
        )?;
        self.scatter_static(atlas, &mut rng, 14, 40, (2, 4), &[60, 61, 62, 63], 0.2)?;
        self.scatter_critters(
            atlas,
            &mut rng,
            14,
            40,
            (10, 16),
            &[
                (SPRITE_SNAIL_WALK_BASE, 8, 5.0),
                (SPRITE_BEE_FLY_BASE, 4, 16.0),
            ],
            0.3,
        )?;

        // Village: preserve the source demo's cabin and left-to-right bush row.
        let village_x = 58.0 * self.spec.tile;
        let cabin = atlas
            .sprite(SPRITE_CABIN)
            .ok_or(Error::InvalidSpriteDigAtlas)?;
        self.place_prop(
            atlas,
            SPRITE_CABIN,
            village_x + f32::from(cabin.width) * 1.5,
            0.15,
            false,
            -self.spec.tile,
        )?;
        let mut cursor = village_x - f32::from(cabin.width) * 0.5 - 30.0;
        for sprite_id in SPRITE_BUSH_BASE..SPRITE_BUSH_BASE + 3 {
            let sprite = atlas
                .sprite(sprite_id)
                .ok_or(Error::InvalidSpriteDigAtlas)?;
            cursor += f32::from(sprite.width) * 0.5;
            self.place_prop(atlas, sprite_id, cursor, 0.2, false, 0.0)?;
            cursor += f32::from(sprite.width) * 0.5 + 14.0;
        }

        // Mining zone and monster den.
        const DEN_TREES: &[u16] = &[
            SPRITE_TREE_DARK_TALL_BASE,
            SPRITE_TREE_DARK_MED_BASE,
            SPRITE_TREE_RED_TALL_BASE,
        ];
        self.scatter_trees(atlas, &mut rng, 84, 110, (4, 8), DEN_TREES, 0.15)?;
        self.scatter_static(atlas, &mut rng, 84, 110, (2, 4), &[60, 61, 62, 63], 0.2)?;
        self.scatter_trees(atlas, &mut rng, 110, 134, (4, 7), DEN_TREES, 0.1)?;
        self.scatter_critters(
            atlas,
            &mut rng,
            110,
            134,
            (5, 8),
            &[
                (SPRITE_BOAR_WALK_BASE, 6, 9.0),
                (SPRITE_BEE_FLY_BASE, 4, 16.0),
                (SPRITE_BOAR_IDLE_BASE, 4, 9.0),
            ],
            0.3,
        )?;

        // Forest B and its landmark tree.
        self.scatter_trees(
            atlas,
            &mut rng,
            134,
            166,
            (4, 8),
            &[SPRITE_TREE_GREEN_TALL_BASE, SPRITE_TREE_GREEN_MED_BASE],
            0.15,
        )?;
        self.scatter_static(atlas, &mut rng, 134, 166, (2, 4), &[60, 61, 62, 63], 0.2)?;
        self.scatter_critters(
            atlas,
            &mut rng,
            134,
            166,
            (12, 18),
            &[
                (SPRITE_SNAIL_WALK_BASE, 8, 5.0),
                (SPRITE_BEE_FLY_BASE, 4, 16.0),
            ],
            0.3,
        )?;
        self.place_tree(atlas, SPRITE_TREE_GREEN_TALL_BASE, 148.0 * self.spec.tile, 0.12, false)?;

        // Autumn market/tail.
        self.scatter_trees(
            atlas,
            &mut rng,
            168,
            188,
            (3, 6),
            &[
                SPRITE_TREE_GOLDEN_TALL_BASE,
                SPRITE_TREE_GOLDEN_MED_BASE,
                SPRITE_TREE_YELLOW_TALL_BASE,
                SPRITE_TREE_YELLOW_MED_BASE,
            ],
            0.15,
        )?;
        self.scatter_static(atlas, &mut rng, 168, 236, (2, 4), &[62, 63], 0.2)?;
        self.scene_installed = true;
        Ok(())
    }

    fn insert_scene_sprite(
        &mut self,
        sprite_id: u16,
        position: [f32; 2],
        size: [f32; 2],
        depth: f32,
        flip_x: bool,
        breakable: bool,
    ) -> Result<SpriteHandle, Error> {
        if self.scene_sprites.live_count() >= self.spec.pool_capacity {
            return Err(Error::InvalidSpriteDigScene);
        }
        Ok(self.scene_sprites.insert(RetainedSprite {
            sprite_id,
            position,
            size,
            depth,
            rotation: 0.0,
            tint: [255; 4],
            atlas_layer: 0,
            flip_x,
            breakable,
            compatibility_overlay: true,
        }))
    }

    fn place_prop(
        &mut self,
        atlas: &Atlas<'_>,
        sprite_id: u16,
        x: f32,
        depth: f32,
        flip_x: bool,
        y_offset: f32,
    ) -> Result<SpriteHandle, Error> {
        let sprite = atlas
            .sprite(sprite_id)
            .ok_or(Error::InvalidSpriteDigAtlas)?;
        let col = libm::roundf(x / self.spec.tile).max(0.0) as usize;
        let position = [
            x,
            surface_top_world_y(self.spec.tile, col) + f32::from(sprite.height) * 0.5 + y_offset,
        ];
        self.insert_scene_sprite(
            sprite_id,
            position,
            [f32::from(sprite.width), f32::from(sprite.height)],
            depth,
            flip_x,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn scatter_static(
        &mut self,
        atlas: &Atlas<'_>,
        rng: &mut SceneRng,
        start: i32,
        end: i32,
        step: (i32, i32),
        sprite_ids: &[u16],
        depth: f32,
    ) -> Result<(), Error> {
        let mut col = start;
        loop {
            col += rng.range_i32(step.0, step.1);
            if col >= end {
                return Ok(());
            }
            let x = col as f32 * self.spec.tile + rng.range_i32(-6, 6) as f32;
            let sprite_id = sprite_ids[rng.range_usize(sprite_ids.len())];
            self.place_prop(atlas, sprite_id, x, depth, rng.bool(), 0.0)?;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn scatter_critters(
        &mut self,
        atlas: &Atlas<'_>,
        rng: &mut SceneRng,
        start: i32,
        end: i32,
        step: (i32, i32),
        animations: &[(u16, u16, f32)],
        depth: f32,
    ) -> Result<(), Error> {
        let mut col = start;
        loop {
            col += rng.range_i32(step.0, step.1);
            if col >= end {
                return Ok(());
            }
            let x = col as f32 * self.spec.tile + rng.range_i32(-6, 6) as f32;
            let (frame_base, frame_count, fps) = animations[rng.range_usize(animations.len())];
            let sprite = atlas
                .sprite(frame_base)
                .ok_or(Error::InvalidSpriteDigAtlas)?;
            let rounded_col = libm::roundf(x / self.spec.tile).max(0.0) as usize;
            let base_position = [
                x,
                surface_top_world_y(self.spec.tile, rounded_col) + f32::from(sprite.height) * 0.5,
            ];
            let handle = self.insert_scene_sprite(
                frame_base,
                base_position,
                [f32::from(sprite.width), f32::from(sprite.height)],
                depth,
                false,
                true,
            )?;
            self.critters.push(Critter {
                handle,
                base_position,
                phase: rng.next_f32() * core::f32::consts::TAU,
                frame_base,
                frame_count,
                fps,
            });
        }
    }

    fn place_tree(
        &mut self,
        atlas: &Atlas<'_>,
        base: u16,
        x: f32,
        depth: f32,
        flip_x: bool,
    ) -> Result<(), Error> {
        let col = libm::roundf(x / self.spec.tile).max(0.0) as usize;
        let top = surface_top_world_y(self.spec.tile, col);
        for layer in 0..3u16 {
            let sprite_id = base + layer;
            let sprite = atlas
                .sprite(sprite_id)
                .ok_or(Error::InvalidSpriteDigAtlas)?;
            self.insert_scene_sprite(
                sprite_id,
                [x, top + f32::from(sprite.height) * 0.5],
                [f32::from(sprite.width), f32::from(sprite.height)],
                depth + f32::from(2 - layer) * 0.01,
                flip_x,
                layer == 0,
            )?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn scatter_trees(
        &mut self,
        atlas: &Atlas<'_>,
        rng: &mut SceneRng,
        start: i32,
        end: i32,
        step: (i32, i32),
        bases: &[u16],
        depth: f32,
    ) -> Result<(), Error> {
        let mut col = start;
        loop {
            col += rng.range_i32(step.0, step.1);
            if col >= end {
                return Ok(());
            }
            let x = col as f32 * self.spec.tile + rng.range_i32(-6, 6) as f32;
            let base = bases[rng.range_usize(bases.len())];
            self.place_tree(atlas, base, x, depth, rng.bool())?;
        }
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
        self.placed.iter().filter(|placed| placed.active).count() + self.scene_sprites.live_count()
    }

    pub fn broken_count(&self) -> usize {
        self.broken.iter().filter(|broken| **broken).count()
    }

    pub fn placed_count(&self) -> usize {
        self.placed.iter().filter(|placed| placed.active).count()
    }

    pub fn inventory_count(&self) -> u32 {
        self.inventory.iter().map(|slot| slot.count).sum()
    }

    pub fn selected_material(&self) -> usize {
        self.selected_hotbar
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
        self.elapsed += dt_seconds;
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
        let player_height = self.player_native_size[1] * self.spec.player_scale;
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
        let mut scene_hit = None;
        let mut scene_depth = f32::NEG_INFINITY;
        for (handle, sprite) in self.scene_sprites.iter_with_handles() {
            if sprite.compatibility_overlay
                && sprite.breakable
                && (point[0] - sprite.position[0]).abs() <= sprite.size[0] * 0.5
                && (point[1] - sprite.position[1]).abs() <= sprite.size[1] * 0.5
                && sprite.depth > scene_depth
            {
                scene_hit = Some(MineTarget::Scene(handle));
                scene_depth = sprite.depth;
            }
        }
        if scene_hit.is_some() {
            return scene_hit;
        }
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
        let (sprite_id, size) = match target {
            MineTarget::Terrain { col, row } => {
                let index = self.terrain_index(col, row);
                if self.broken[index] {
                    return;
                }
                self.broken[index] = true;
                let _ = self.scene_sprites.remove(self.terrain_handles[index]);
                let sprite_id = if row == 0 {
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
                (sprite_id, [self.spec.tile; 2])
            }
            MineTarget::Placed { slot } => {
                let Some(placed) = self.placed.get_mut(slot) else {
                    return;
                };
                if !placed.active {
                    return;
                }
                placed.active = false;
                (placed.sprite_id, placed.size)
            }
            MineTarget::Scene(handle) => {
                let Some(sprite) = self.scene_sprites.get(handle).copied() else {
                    return;
                };
                if !sprite.breakable {
                    return;
                }
                if !self.scene_sprites.remove(handle) {
                    return;
                }
                self.critters.retain(|critter| critter.handle != handle);
                (sprite.sprite_id, sprite.size)
            }
        };
        if let Some(slot) = self
            .inventory
            .iter_mut()
            .find(|slot| slot.sprite_id == sprite_id)
        {
            slot.count = slot.count.saturating_add(1);
        } else {
            self.inventory.push(InventorySlot {
                sprite_id,
                size,
                count: 1,
            });
            if self.inventory.len() == 1 {
                self.selected_hotbar = 0;
            }
        }
    }

    fn cycle_hotbar(&mut self, direction: i32) {
        if self.inventory.is_empty() {
            return;
        }
        self.selected_hotbar = (self.selected_hotbar as i32 + direction)
            .rem_euclid(self.inventory.len() as i32) as usize;
    }

    fn place_selected(&mut self, world: [f32; 2]) {
        let Some(selected) = self.inventory.get(self.selected_hotbar).copied() else {
            return;
        };
        let Some(slot) = self.placed.iter_mut().find(|placed| !placed.active) else {
            return;
        };
        slot.active = true;
        slot.position = [
            libm::roundf(world[0] / self.spec.tile) * self.spec.tile,
            world[1],
        ];
        slot.sprite_id = selected.sprite_id;
        slot.size = selected.size;
        self.inventory[self.selected_hotbar].count -= 1;
        if self.inventory[self.selected_hotbar].count == 0 {
            self.inventory.remove(self.selected_hotbar);
            if self.selected_hotbar >= self.inventory.len() {
                self.selected_hotbar = self.inventory.len().saturating_sub(1);
            }
        }
    }

    fn sync_retained_animations(&mut self, player_native_size: [u16; 2]) -> Result<(), Error> {
        if player_native_size.contains(&0) {
            return Err(Error::InvalidSpriteDigScene);
        }
        // Animation is an authoritative scene write. Stable rows change in
        // place; publication exposes only their enclosing dirty span.
        for critter in &self.critters {
            let frame = (libm::floorf(self.elapsed * critter.fps) as u16) % critter.frame_count;
            if let Some(retained) = self.scene_sprites.get_mut(critter.handle) {
                retained.sprite_id = critter.frame_base + frame;
                retained.position = [
                    critter.base_position[0],
                    critter.base_position[1] + libm::sinf(self.elapsed * 2.0 + critter.phase) * 6.0,
                ];
            }
        }
        let (animation_base, animation_frames, fps) = match self.player_animation {
            PlayerAnimation::Idle => (SPRITE_PLAYER_IDLE_BASE, 4u16, 7.0),
            PlayerAnimation::Run => (SPRITE_PLAYER_RUN_BASE, 8u16, 12.0),
            PlayerAnimation::Jump => (SPRITE_PLAYER_JUMP_BASE, 15u16, 14.0),
            PlayerAnimation::Fall => (SPRITE_PLAYER_FALL_BASE, 3u16, 10.0),
        };
        let animation_index =
            (libm::floorf(self.player_animation_time * fps) as u16) % animation_frames;
        let player = self
            .scene_sprites
            .get_mut(self.player_handle)
            .ok_or(Error::InvalidSpriteDigScene)?;
        player.sprite_id = animation_base + animation_index;
        player.position = self.player_position;
        player.size = [
            f32::from(player_native_size[0]) * self.spec.player_scale,
            f32::from(player_native_size[1]) * self.spec.player_scale,
        ];
        player.flip_x = !self.player_facing_right;
        Ok(())
    }

    /// Publish the authoritative retained rows directly to a renderer.
    ///
    /// This is the native-backend seam: rows/liveness/generations are owned
    /// here, while visibility lists, sorting and indirect commands remain
    /// renderer-derived data exactly as in Helio's SceneDB integration.
    pub fn publish_retained_scene(
        &mut self,
        player_native_size: [u16; 2],
    ) -> Result<PublishedScene<'_, RetainedSprite>, Error> {
        self.sync_retained_animations(player_native_size)?;
        Ok(self.scene_sprites.publish())
    }

    /// Build the compact state consumed by the Bakery C++ tilemap kernel.
    /// Terrain stays column-major so the GPU can locate a tile directly;
    /// sparse and animated objects remain a small ordered sprite overlay.
    pub fn texture_frame(
        &mut self,
        width: u32,
        height: u32,
        player_native_size: [u16; 2],
    ) -> Result<TextureFrame, Error> {
        if width == 0 || height == 0 || player_native_size.contains(&0) {
            return Err(Error::InvalidSpriteDigScene);
        }
        self.sync_retained_animations(player_native_size)?;
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

        let mut sprites = Vec::with_capacity(
            self.scene_sprites.len() + self.spec.max_placed + MATERIAL_KINDS + 4,
        );

        let background_sprite = self.scene_installed.then_some(SPRITE_BACKGROUND);
        if let Some(sprite_id) = background_sprite {
            // The packed native background is 480x272 in Helio. The artifact
            // preserves that trimmed size, so this is the hosted cover/drift
            // rule expressed through the same retained-frame lowering.
            let native = [480.0, 272.0];
            let scale = (height as f32 / native[1]).max(2.0) * 1.1;
            let size = [native[0] * scale, native[1] * scale];
            let drift = self.camera_center[0] * 0.05;
            let wrapped_drift = drift - libm::floorf(drift / size[0]) * size[0];
            let position = [
                self.camera_center[0] + wrapped_drift - size[0] * 0.5,
                self.camera_center[1] + height as f32 * 0.35,
            ];
            push_world_sprite(
                &mut sprites,
                position,
                size,
                sprite_id,
                [255; 4],
                false,
                -10.0,
                0.0,
                self.camera_center,
                self.spec.zoom,
                width,
                height,
            );
        }

        for retained in self
            .scene_sprites
            .iter()
            .filter(|sprite| sprite.compatibility_overlay)
        {
            push_world_sprite(
                &mut sprites,
                retained.position,
                retained.size,
                retained.sprite_id,
                retained.tint,
                retained.flip_x,
                retained.depth,
                retained.rotation,
                self.camera_center,
                self.spec.zoom,
                width,
                height,
            );
        }
        for placed in self.placed.iter().filter(|placed| placed.active) {
            push_world_sprite(
                &mut sprites,
                placed.position,
                placed.size,
                placed.sprite_id,
                [255; 4],
                false,
                0.2,
                0.0,
                self.camera_center,
                self.spec.zoom,
                width,
                height,
            );
        }

        if let Some(mining) = self.mining
            && self.mining_stage() != 0
        {
            let (center, target_depth) = match mining.target {
                MineTarget::Terrain { col, row } => (
                    [
                        col as f32 * self.spec.tile,
                        surface_top_world_y(self.spec.tile, col)
                            - self.spec.tile * 0.5
                            - row as f32 * self.spec.tile,
                    ],
                    0.0,
                ),
                MineTarget::Placed { slot } => (self.placed[slot].position, 0.2),
                MineTarget::Scene(handle) => self
                    .scene_sprites
                    .get(handle)
                    .map(|sprite| (sprite.position, sprite.depth))
                    .unwrap_or(([0.0; 2], 0.0)),
            };
            let stage = self.mining_stage().min(self.spec.break_stages).max(1);
            push_world_sprite(
                &mut sprites,
                center,
                [self.spec.tile; 2],
                SPRITE_CRACK_BASE + stage as u16 - 1,
                [255; 4],
                false,
                target_depth + 0.01,
                0.0,
                self.camera_center,
                self.spec.zoom,
                width,
                height,
            );
        }

        for (visible_index, slot) in self.inventory.iter().enumerate() {
            let center = hotbar_world_position(
                self.camera_center,
                width,
                height,
                visible_index,
                self.inventory.len(),
                &self.spec,
            );
            if visible_index == self.selected_hotbar {
                push_world_sprite(
                    &mut sprites,
                    center,
                    [self.spec.hotbar_icon_size + 10.0; 2],
                    SPRITE_WHITE,
                    [255, 220, 72, 210],
                    false,
                    0.89,
                    0.0,
                    self.camera_center,
                    self.spec.zoom,
                    width,
                    height,
                );
            }
            push_world_sprite(
                &mut sprites,
                center,
                [self.spec.hotbar_icon_size; 2],
                slot.sprite_id,
                [255; 4],
                false,
                0.9,
                0.0,
                self.camera_center,
                self.spec.zoom,
                width,
                height,
            );
        }
        sprites.sort_by(|left, right| left.depth.total_cmp(&right.depth));
        let publication = self.scene_sprites.publish();
        Ok(TextureFrame {
            surface_heights,
            cells,
            sprites,
            scene_epoch: publication.epoch,
            scene_rows: publication.rows.len(),
            scene_live_rows: publication.live_count,
            scene_dirty: publication.dirty,
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
            [
                self.player_native_size[0] * self.spec.player_scale,
                self.player_native_size[1] * self.spec.player_scale,
            ],
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
                MineTarget::Scene(handle) => self
                    .scene_sprites
                    .get(handle)
                    .map_or([0.0; 2], |sprite| sprite.position),
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
        // The colored-quad compatibility view has three legacy icon batches;
        // the atlas path below carries the complete arbitrary-sprite hotbar.
        for visible_index in 0..self.inventory.len().min(MATERIAL_KINDS) {
            let center = hotbar_world_position(
                self.camera_center,
                width,
                height,
                visible_index,
                self.inventory.len(),
                &self.spec,
            );
            let batch = HOTBAR_GRASS_BATCH + visible_index;
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
            if visible_index == self.selected_hotbar {
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
    depth: f32,
    rotation: f32,
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
        depth,
        rotation,
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

/// Byte-for-byte equivalent deterministic random sequence to the upstream
/// demo's small PCG-style LCG. Keeping this here makes scene placement part
/// of the portable contract rather than an offline screenshot convention.
struct SceneRng(u64);

impl SceneRng {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 32) as u32
    }

    fn next_f32(&mut self) -> f32 {
        self.next_u32() as f32 / u32::MAX as f32
    }

    fn range_i32(&mut self, low: i32, high: i32) -> i32 {
        low + (self.next_f32() * (high - low) as f32) as i32
    }

    fn range_usize(&mut self, length: usize) -> usize {
        ((self.next_f32() * length as f32) as usize).min(length - 1)
    }

    fn bool(&mut self) -> bool {
        self.next_f32() < 0.5
    }
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

    const ARTIFACT: &[u8] = include_bytes!("../../../picasso/simple-cube.trueos.intel.helio");
    const ATLAS: &[u8] = include_bytes!("../../../picasso/sprite-dig-atlas.trueos.rgba");

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
    fn atlas_contract_decodes_complete_upstream_scene_identity_table() {
        let atlas = Atlas::decode(ATLAS).unwrap();
        assert_eq!((atlas.width, atlas.height), (1536, 1248));
        assert_eq!(atlas.sprites().len(), 93);
        assert_eq!(atlas.player_size, [61, 65]);
        assert!(atlas.sprite(super::SPRITE_BACKGROUND).is_some());
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
    fn native_publication_exposes_retained_terrain_and_player_mirrors() {
        let atlas = Atlas::decode(ATLAS).unwrap();
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let expected_rows = spec.world_cols * (spec.dirt_rows + spec.stone_rows + 1) + 1;
        let mut engine = Engine::new(spec).unwrap();
        let publication = engine.publish_retained_scene(atlas.player_size).unwrap();
        assert_eq!(publication.rows.len(), expected_rows);
        assert_eq!(publication.alive.len(), expected_rows);
        assert_eq!(publication.generations.len(), expected_rows);
        assert_eq!(publication.live_count, expected_rows);
        assert!(publication.alive.iter().all(|alive| *alive == 1));
        assert!(
            publication
                .generations
                .iter()
                .all(|generation| *generation == 1)
        );
        assert_eq!(publication.dirty.first, 0);
        assert_eq!(publication.dirty.count as usize, expected_rows);
        let gpu = publication.rows[0].gpu_instance(&atlas).unwrap();
        assert_eq!(core::mem::size_of_val(&gpu), super::GpuSpriteInstance::BYTE_LEN);
        assert_eq!(gpu.position, publication.rows[0].position);
        assert_eq!(gpu.size, publication.rows[0].size);
        assert_eq!(gpu.atlas_layer, 0);
        assert!(
            gpu.uv_rect
                .iter()
                .all(|component| (0.0..=1.0).contains(component))
        );
        assert_eq!(&gpu.to_le_bytes()[64..80], &[0; 16]);
    }

    #[test]
    fn installed_scene_retains_upstream_props_trees_critters_and_background() {
        let atlas = Atlas::decode(ATLAS).unwrap();
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut engine = Engine::new(spec).unwrap();
        engine.install_atlas_scene(&atlas).unwrap();
        engine
            .step(InputFrame::default(), 1280, 720, 1.0 / 30.0)
            .unwrap();
        assert!(engine.scene_sprites.len() > 100);
        assert!(!engine.critters.is_empty());
        assert!(
            engine
                .scene_sprites
                .iter()
                .any(|sprite| sprite.sprite_id == super::SPRITE_CABIN)
        );
        let frame = engine.texture_frame(1280, 720, atlas.player_size).unwrap();
        assert!(
            frame
                .sprites
                .iter()
                .any(|sprite| sprite.sprite_id == super::SPRITE_BACKGROUND)
        );
        assert!(
            frame
                .sprites
                .windows(2)
                .all(|pair| pair[0].depth <= pair[1].depth)
        );
        assert_eq!(frame.scene_epoch, 1);
        assert_eq!(frame.scene_live_rows, engine.scene_sprites.live_count());
        assert_eq!(frame.scene_rows, engine.scene_sprites.len());
        assert_eq!(frame.scene_dirty.first, 0);
        assert_eq!(frame.scene_dirty.count as usize, frame.scene_rows);
    }

    #[test]
    fn retained_prop_round_trips_through_identity_hotbar_and_placement() {
        let atlas = Atlas::decode(ATLAS).unwrap();
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut engine = Engine::new(spec).unwrap();
        engine.install_atlas_scene(&atlas).unwrap();
        let cabin = engine
            .scene_sprites
            .iter_with_handles()
            .find(|(_, sprite)| sprite.sprite_id == super::SPRITE_CABIN)
            .map(|(handle, sprite)| (handle, *sprite))
            .unwrap();
        engine.finish_mining(super::MineTarget::Scene(cabin.0));
        assert_eq!(engine.inventory.len(), 1);
        assert_eq!(engine.inventory[0].sprite_id, super::SPRITE_CABIN);
        assert_eq!(engine.inventory[0].size, cabin.1.size);
        engine.place_selected([123.0, 45.0]);
        let placed = engine.placed.iter().find(|placed| placed.active).unwrap();
        assert_eq!(placed.sprite_id, super::SPRITE_CABIN);
        assert_eq!(placed.size, cabin.1.size);
        assert!(engine.inventory.is_empty());
    }

    #[test]
    fn held_ui4_style_input_moves_and_jumps_the_player() {
        let spec = Spec::decode_artifact(ARTIFACT).unwrap();
        let mut engine = Engine::new(spec).unwrap();
        engine
            .step(InputFrame::default(), 1280, 720, 1.0 / 30.0)
            .unwrap();
        let before = engine.player_position();
        let player_handle = engine.player_handle;
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
        let atlas = Atlas::decode(ATLAS).unwrap();
        let _ = engine.texture_frame(1280, 720, atlas.player_size).unwrap();
        assert_eq!(
            engine.scene_sprites.get(player_handle).unwrap().position,
            engine.player_position()
        );
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
        let terrain_handle = engine.terrain_handles[engine.terrain_index(col, 0)];
        assert!(engine.scene_sprites.contains(terrain_handle));
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
        assert!(!engine.scene_sprites.contains(terrain_handle));
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
