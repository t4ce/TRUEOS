//! Minimal retained DOM-paint seam for the first TRUEOS Picasso bridge.
//!
//! Rows remain high-level CPU-owned scene facts. [`PicassoScene::lower`] is a
//! deliberately bounded compatibility lowering to the ordered solid quads and
//! retained FontKernel canvas already accepted by UI4; it is not Picasso's
//! eventual packed GPU ABI.

#![allow(clippy::too_many_arguments)]

use alloc::{string::String, vec::Vec};
use core::cmp::Ordering;

use crate::scene_db::{EpochExhausted, PublishedScene, SceneHandle, SceneStore};

/// Existing UI4's maximum ordered sprite/solid records in one scene.
pub const MAX_LOWERED_COMMANDS: usize = 8_192;
/// Logical primitive budget before bounded compatibility lowering.
pub const MAX_SCENE_PRIMITIVES: usize = 8_192;
/// Per-row CPU SceneDB payload cap. This owned Unicode payload is never a GPU
/// row or guest wire ABI; lowering emits its stable [`PrimitiveRef`] instead.
pub const MAX_FONT_LOOKUP_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn is_valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width >= 0.0
            && self.height >= 0.0
            && (self.x + self.width).is_finite()
            && (self.y + self.height).is_finite()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerRadii {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

impl CornerRadii {
    pub const ZERO: Self = Self::all(0.0);

    pub const fn all(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    fn is_valid(self) -> bool {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
        .into_iter()
        .all(|radius| radius.is_finite() && radius >= 0.0)
    }

    /// CSS's proportional radius-overlap reduction for circular corner radii.
    fn normalized(self, rect: Rect) -> Self {
        let top = self.top_left + self.top_right;
        let bottom = self.bottom_left + self.bottom_right;
        let left = self.top_left + self.bottom_left;
        let right = self.top_right + self.bottom_right;
        let mut scale = 1.0f32;
        for (extent, sum) in [
            (rect.width, top),
            (rect.width, bottom),
            (rect.height, left),
            (rect.height, right),
        ] {
            if sum > 0.0 {
                scale = scale.min(extent / sum);
            }
        }
        Self {
            top_left: self.top_left * scale,
            top_right: self.top_right * scale,
            bottom_right: self.bottom_right * scale,
            bottom_left: self.bottom_left * scale,
        }
    }

    fn inset(self, width: f32) -> Self {
        Self {
            top_left: (self.top_left - width).max(0.0),
            top_right: (self.top_right - width).max(0.0),
            bottom_right: (self.bottom_right - width).max(0.0),
            bottom_left: (self.bottom_left - width).max(0.0),
        }
    }
}

/// Conventional straight-alpha sRGBA8 channels.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

/// Stable selectors for the one boot-warmed FontKernel face registry.
///
/// Scene producers carry this compact identity plus Unicode. They deliberately
/// do not manufacture TTF glyph IDs or copy warmed outline commands into the
/// scene. FontKernel remains authoritative for both operations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u32)]
pub enum FontFace {
    Default = 1,
    NotoSansSc = 2,
    #[default]
    Inconsolata = 3,
}

impl FontFace {
    pub const fn from_id(id: u32) -> Option<Self> {
        match id {
            1 => Some(Self::Default),
            2 => Some(Self::NotoSansSc),
            3 => Some(Self::Inconsolata),
            _ => None,
        }
    }
}

/// V0 CSS slant selectors. The raster shear remains a FontKernel policy;
/// producers retain semantic style identity rather than baking geometry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u32)]
pub enum FontSlant {
    #[default]
    Normal = 0,
    Italic = 1,
}

impl FontSlant {
    /// Current synthetic shear used when the warmed face has no separate
    /// italic registration. This conversion belongs at the kernel bridge.
    pub const fn kernel_shear(self) -> f32 {
        match self {
            Self::Normal => 0.0,
            Self::Italic => 0.15,
        }
    }
}

/// One compact, logical text resource lookup retained in SceneDB.
///
/// `rect` is the producer's scene-space bounds/clip and `origin` is the text
/// position in the same coordinate system. Only this Unicode/style row is
/// copied on publication. The consumer builds any outline ops per request and
/// the retained scene owns its derived R8 coverage resource.
#[derive(Clone, Debug, PartialEq)]
pub struct FontLookupRun {
    pub rect: Rect,
    pub origin: [f32; 2],
    pub text: String,
    pub face: FontFace,
    pub slant: FontSlant,
    pub font_pixels: f32,
    pub color: Color,
}

impl Color {
    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    pub fn from_rgba_f32(channels: [f32; 4]) -> Result<Self, SceneError> {
        if !channels.into_iter().all(|channel| channel.is_finite()) {
            return Err(SceneError::InvalidColor);
        }
        let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        Ok(Self::rgba(byte(channels[0]), byte(channels[1]), byte(channels[2]), byte(channels[3])))
    }

    /// UI4's conventional RGBA channels packed as a little-endian `u32`.
    pub const fn to_u32_le(self) -> u32 {
        u32::from_le_bytes([self.red, self.green, self.blue, self.alpha])
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontCanvas {
    /// Full retained FontKernel canvas in the root CSS coordinate space.
    pub rect: Rect,
}

/// Stable reference into the producer-owned image-resource SceneStore.
///
/// The primitive table retains only this logical identity. Encoded bytes,
/// decoded RGBA, UI4 sprite ids and eventual GPU leases remain resource-table
/// or renderer facts rather than primitive-row payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageResourceRef(pub SceneHandle);

#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    SolidRect {
        rect: Rect,
        color: Color,
    },
    RoundedRect {
        rect: Rect,
        radii: CornerRadii,
        color: Color,
    },
    RoundedBorder {
        rect: Rect,
        radii: CornerRadii,
        width: f32,
        color: Color,
    },
    Image {
        rect: Rect,
        source: Rect,
        resource: ImageResourceRef,
        opacity: u8,
    },
    /// Ordered marker for the separately retained FontKernel canvas.
    FontCanvas(FontCanvas),
    /// Unicode lookup row resolved through FontKernel's boot-warmed outlines.
    FontLookup(FontLookupRun),
}

impl Primitive {
    pub const fn solid_rect(rect: Rect, color: Color) -> Self {
        Self::SolidRect { rect, color }
    }

    pub const fn rounded_rect(rect: Rect, radii: CornerRadii, color: Color) -> Self {
        Self::RoundedRect { rect, radii, color }
    }

    pub const fn rounded_border(rect: Rect, radii: CornerRadii, width: f32, color: Color) -> Self {
        Self::RoundedBorder {
            rect,
            radii,
            width,
            color,
        }
    }

    pub const fn image(rect: Rect, source: Rect, resource: ImageResourceRef, opacity: u8) -> Self {
        Self::Image {
            rect,
            source,
            resource,
            opacity,
        }
    }

    pub const fn font_canvas(rect: Rect) -> Self {
        Self::FontCanvas(FontCanvas { rect })
    }

    pub fn font_lookup(run: FontLookupRun) -> Self {
        Self::FontLookup(run)
    }

    fn validate(&self) -> Result<(), SceneError> {
        match self {
            Self::SolidRect { rect, .. } => validate_nonempty_rect(*rect),
            Self::RoundedRect { rect, radii, .. } => {
                validate_nonempty_rect(*rect)?;
                if !radii.is_valid() {
                    return Err(SceneError::InvalidRadii);
                }
                Ok(())
            }
            Self::RoundedBorder {
                rect, radii, width, ..
            } => {
                validate_nonempty_rect(*rect)?;
                if !radii.is_valid() {
                    return Err(SceneError::InvalidRadii);
                }
                if !width.is_finite() || *width <= 0.0 {
                    return Err(SceneError::InvalidBorderWidth);
                }
                Ok(())
            }
            Self::Image { rect, source, .. } => {
                validate_nonempty_rect(*rect)?;
                if source.x < 0.0 || source.y < 0.0 {
                    return Err(SceneError::InvalidRect);
                }
                validate_nonempty_rect(*source)
            }
            Self::FontCanvas(canvas) => validate_nonempty_rect(canvas.rect),
            Self::FontLookup(run) => {
                validate_nonempty_rect(run.rect)?;
                if run.text.is_empty()
                    || run.text.len() > MAX_FONT_LOOKUP_BYTES
                    || !run.origin.into_iter().all(f32::is_finite)
                    || !run.font_pixels.is_finite()
                    || run.font_pixels <= 0.0
                {
                    return Err(SceneError::InvalidFontLookup);
                }
                Ok(())
            }
        }
    }
}

fn validate_nonempty_rect(rect: Rect) -> Result<(), SceneError> {
    if !rect.is_valid() || rect.width == 0.0 || rect.height == 0.0 {
        Err(SceneError::InvalidRect)
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrimitiveRow {
    pub order: u32,
    pub primitive: Primitive,
}

impl PrimitiveRow {
    pub const fn new(order: u32, primitive: Primitive) -> Self {
        Self { order, primitive }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimitiveRef(pub SceneHandle);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneError {
    InvalidRect,
    InvalidRadii,
    InvalidBorderWidth,
    InvalidColor,
    InvalidFontLookup,
    DuplicateOrder,
    OrderNotIncreasing,
    GeometryAfterFontCanvas,
    MultipleFontCanvases,
    StalePrimitive,
    PrimitiveLimit { limit: usize },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    pub const fn new(x: f32, y: f32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn is_valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width > 0
            && self.height > 0
            && (self.x + self.width as f32).is_finite()
            && (self.y + self.height as f32).is_finite()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoweredCommand {
    /// One nonempty viewport-local solid quad with integer pixel edges.
    SolidSpan {
        order: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        color: Color,
    },
    /// Viewport-local crop of the retained FontKernel canvas.
    FontCanvas {
        order: u32,
        x: u32,
        y: u32,
        source_x: u32,
        source_y: u32,
        width: u32,
        height: u32,
    },
    /// Viewport-local crop of one revisioned image resource.
    Image {
        order: u32,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        source_x: u16,
        source_y: u16,
        source_width: u16,
        source_height: u16,
        resource: ImageResourceRef,
        opacity: u8,
    },
    /// Viewport-visible logical text row. The renderer resolves this compact
    /// lookup through FontKernel and retains the returned resource leases for
    /// its render ticket; no outline or atlas bytes transit SceneDB.
    FontLookup {
        order: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        lookup: PrimitiveRef,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LowerError {
    InvalidViewport,
    ImageCoordinateOverflow,
    CommandLimit { limit: usize },
}

pub struct PicassoScene {
    rows: SceneStore<PrimitiveRow>,
}

impl PicassoScene {
    pub const fn new() -> Self {
        Self {
            rows: SceneStore::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            rows: SceneStore::with_capacity(capacity.min(MAX_SCENE_PRIMITIVES)),
        }
    }

    pub const fn live_count(&self) -> usize {
        self.rows.live_count()
    }

    pub fn get(&self, primitive_ref: PrimitiveRef) -> Option<&PrimitiveRow> {
        self.rows.get(primitive_ref.0)
    }

    /// Resolve a lowered font handle while the scene publication is retained.
    /// The caller may then copy only the compact Unicode/style lookup into its
    /// asynchronous kernel request; outline and atlas resources never leave
    /// FontKernel ownership.
    pub fn font_lookup(&self, primitive_ref: PrimitiveRef) -> Option<&FontLookupRun> {
        match &self.get(primitive_ref)?.primitive {
            Primitive::FontLookup(run) => Some(run),
            _ => None,
        }
    }

    /// Iterate logical text lookups directly in paint order.
    ///
    /// Font request export must not depend on compatibility geometry lowering:
    /// a rounded-border span budget cannot make unrelated text unavailable.
    pub fn font_lookup_rows(&self) -> impl Iterator<Item = (PrimitiveRef, &FontLookupRun)> + '_ {
        let mut rows = self
            .rows
            .iter_with_handles()
            .filter_map(|(handle, row)| match &row.primitive {
                Primitive::FontLookup(run) => Some((row.order, PrimitiveRef(handle), run)),
                _ => None,
            })
            .collect::<Vec<_>>();
        rows.sort_unstable_by_key(|(order, _, _)| *order);
        rows.into_iter().map(|(_, lookup, run)| (lookup, run))
    }

    pub fn insert(&mut self, row: PrimitiveRow) -> Result<PrimitiveRef, SceneError> {
        if self.rows.live_count() >= MAX_SCENE_PRIMITIVES {
            return Err(SceneError::PrimitiveLimit {
                limit: MAX_SCENE_PRIMITIVES,
            });
        }
        self.validate_candidate(None, &row)?;
        Ok(PrimitiveRef(self.rows.insert(row)))
    }

    pub fn update(
        &mut self,
        primitive_ref: PrimitiveRef,
        row: PrimitiveRow,
    ) -> Result<(), SceneError> {
        if !self.rows.contains(primitive_ref.0) {
            return Err(SceneError::StalePrimitive);
        }
        self.validate_candidate(Some(primitive_ref.0), &row)?;
        self.rows
            .update(primitive_ref.0, row)
            .map_err(|_| SceneError::StalePrimitive)
    }

    pub fn remove(&mut self, primitive_ref: PrimitiveRef) -> bool {
        self.rows.remove(primitive_ref.0)
    }

    /// Atomically validate and replace the logical stream. The input is
    /// required to be in strictly increasing paint order so accidental
    /// cross-kind reordering is caught at the producer boundary.
    pub fn replace_ordered<I>(&mut self, rows: I) -> Result<(), SceneError>
    where
        I: IntoIterator<Item = PrimitiveRow>,
    {
        let mut replacement = Vec::new();
        for row in rows {
            if replacement.len() >= MAX_SCENE_PRIMITIVES {
                return Err(SceneError::PrimitiveLimit {
                    limit: MAX_SCENE_PRIMITIVES,
                });
            }
            replacement.push(row);
        }
        validate_ordered_rows(&replacement)?;
        let retired: Vec<_> = self
            .rows
            .iter_with_handles()
            .map(|(handle, _)| handle)
            .collect();
        for handle in retired {
            let removed = self.rows.remove(handle);
            debug_assert!(removed);
        }
        for row in replacement {
            let _ = self.rows.insert(row);
        }
        Ok(())
    }

    pub fn publish(&mut self) -> PublishedScene<'_, PrimitiveRow> {
        self.rows.publish()
    }

    pub fn try_publish(&mut self) -> Result<PublishedScene<'_, PrimitiveRow>, EpochExhausted> {
        self.rows.try_publish()
    }

    pub fn lower(
        &self,
        viewport: Viewport,
        max_commands: usize,
    ) -> Result<Vec<LoweredCommand>, LowerError> {
        if !viewport.is_valid() {
            return Err(LowerError::InvalidViewport);
        }
        let limit = max_commands.min(MAX_LOWERED_COMMANDS);
        let mut ordered: Vec<_> = self.rows.iter_with_handles().collect();
        ordered.sort_unstable_by_key(|(_, row)| row.order);
        let mut output = Vec::new();
        for (handle, row) in ordered {
            match &row.primitive {
                Primitive::SolidRect { rect, color } => {
                    lower_solid_rect(row.order, *rect, *color, viewport, limit, &mut output)?;
                }
                Primitive::RoundedRect { rect, radii, color } => {
                    lower_fill(row.order, *rect, *radii, *color, viewport, limit, &mut output)?;
                }
                Primitive::RoundedBorder {
                    rect,
                    radii,
                    width,
                    color,
                } => {
                    lower_border(
                        row.order,
                        *rect,
                        *radii,
                        *width,
                        *color,
                        viewport,
                        limit,
                        &mut output,
                    )?;
                }
                Primitive::Image {
                    rect,
                    source,
                    resource,
                    opacity,
                } => {
                    if let Some((
                        x,
                        y,
                        width,
                        height,
                        source_x,
                        source_y,
                        source_width,
                        source_height,
                    )) = image_crop(*rect, *source, viewport)
                    {
                        let [
                            x,
                            y,
                            width,
                            height,
                            source_x,
                            source_y,
                            source_width,
                            source_height,
                        ] = [
                            x,
                            y,
                            width,
                            height,
                            source_x,
                            source_y,
                            source_width,
                            source_height,
                        ]
                        .map(|value| {
                            u16::try_from(value).map_err(|_| LowerError::ImageCoordinateOverflow)
                        });
                        push_bounded(
                            &mut output,
                            LoweredCommand::Image {
                                order: row.order,
                                x: x?,
                                y: y?,
                                width: width?,
                                height: height?,
                                source_x: source_x?,
                                source_y: source_y?,
                                source_width: source_width?,
                                source_height: source_height?,
                                resource: *resource,
                                opacity: *opacity,
                            },
                            limit,
                        )?;
                    }
                }
                Primitive::FontCanvas(canvas) => {
                    if let Some((x, y, source_x, source_y, width, height)) =
                        font_canvas_crop(canvas.rect, viewport)
                    {
                        push_bounded(
                            &mut output,
                            LoweredCommand::FontCanvas {
                                order: row.order,
                                x,
                                y,
                                source_x,
                                source_y,
                                width,
                                height,
                            },
                            limit,
                        )?;
                    }
                }
                Primitive::FontLookup(run) => {
                    if let Some((x, y, width, height)) = clipped_integer_rect(run.rect, viewport) {
                        push_bounded(
                            &mut output,
                            LoweredCommand::FontLookup {
                                order: row.order,
                                x,
                                y,
                                width,
                                height,
                                lookup: PrimitiveRef(handle),
                            },
                            limit,
                        )?;
                    }
                }
            }
        }
        Ok(output)
    }

    fn validate_candidate(
        &self,
        replaced: Option<SceneHandle>,
        candidate: &PrimitiveRow,
    ) -> Result<(), SceneError> {
        candidate.primitive.validate()?;
        let mut rows = Vec::with_capacity(self.rows.live_count() + usize::from(replaced.is_none()));
        for (handle, row) in self.rows.iter_with_handles() {
            if Some(handle) != replaced {
                rows.push(row.clone());
            }
        }
        rows.push(candidate.clone());
        rows.sort_unstable_by_key(|row| row.order);
        validate_ordered_rows(&rows)
    }
}

impl Default for PicassoScene {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_ordered_rows(rows: &[PrimitiveRow]) -> Result<(), SceneError> {
    let mut previous = None;
    let mut saw_font = false;
    for row in rows {
        row.primitive.validate()?;
        if let Some(order) = previous {
            match row.order.cmp(&order) {
                Ordering::Less => return Err(SceneError::OrderNotIncreasing),
                Ordering::Equal => return Err(SceneError::DuplicateOrder),
                Ordering::Greater => {}
            }
        }
        match &row.primitive {
            Primitive::FontCanvas(_) if saw_font => return Err(SceneError::MultipleFontCanvases),
            Primitive::FontCanvas(_) => saw_font = true,
            // FontCanvas is the fallback/shadow representation. Native lookup
            // rows deliberately follow it, while geometry remains forbidden,
            // so a consumer selects one text representation without changing
            // DOM paint order.
            Primitive::FontLookup(_) | Primitive::Image { .. } => {}
            _ if saw_font => return Err(SceneError::GeometryAfterFontCanvas),
            _ => {}
        }
        previous = Some(row.order);
    }
    Ok(())
}

fn push_bounded(
    output: &mut Vec<LoweredCommand>,
    command: LoweredCommand,
    limit: usize,
) -> Result<(), LowerError> {
    if output.len() >= limit {
        return Err(LowerError::CommandLimit { limit });
    }
    output.push(command);
    Ok(())
}

fn lower_fill(
    order: u32,
    rect: Rect,
    radii: CornerRadii,
    color: Color,
    viewport: Viewport,
    limit: usize,
    output: &mut Vec<LoweredCommand>,
) -> Result<(), LowerError> {
    let radii = radii.normalized(rect);
    for y in visible_rows(rect, viewport) {
        if let Some((left, right)) = rounded_interval(rect, radii, viewport.y + y as f32 + 0.5) {
            push_scene_interval(order, y, left, right, color, viewport, limit, output)?;
        }
    }
    Ok(())
}

fn lower_border(
    order: u32,
    rect: Rect,
    radii: CornerRadii,
    width: f32,
    color: Color,
    viewport: Viewport,
    limit: usize,
    output: &mut Vec<LoweredCommand>,
) -> Result<(), LowerError> {
    let outer_radii = radii.normalized(rect);
    let inset = width.min(rect.width * 0.5).min(rect.height * 0.5);
    let inner = Rect::new(
        rect.x + inset,
        rect.y + inset,
        (rect.width - inset * 2.0).max(0.0),
        (rect.height - inset * 2.0).max(0.0),
    );
    let inner_radii = outer_radii.inset(inset).normalized(inner);
    for y in visible_rows(rect, viewport) {
        let sample_y = viewport.y + y as f32 + 0.5;
        let Some((outer_left, outer_right)) = rounded_interval(rect, outer_radii, sample_y) else {
            continue;
        };
        let inner_interval = if inner.width > 0.0 && inner.height > 0.0 {
            rounded_interval(inner, inner_radii, sample_y)
        } else {
            None
        };
        match inner_interval {
            Some((inner_left, inner_right)) => {
                push_scene_interval(
                    order, y, outer_left, inner_left, color, viewport, limit, output,
                )?;
                push_scene_interval(
                    order,
                    y,
                    inner_right,
                    outer_right,
                    color,
                    viewport,
                    limit,
                    output,
                )?;
            }
            None => push_scene_interval(
                order,
                y,
                outer_left,
                outer_right,
                color,
                viewport,
                limit,
                output,
            )?,
        }
    }
    Ok(())
}

fn visible_rows(rect: Rect, viewport: Viewport) -> core::ops::Range<u32> {
    let local_top = rect.y - viewport.y - 0.5;
    let local_bottom = rect.y + rect.height - viewport.y - 0.5;
    let top = libm::ceilf(local_top).max(0.0).min(viewport.height as f32) as u32;
    let bottom = libm::ceilf(local_bottom)
        .max(0.0)
        .min(viewport.height as f32) as u32;
    top..bottom
}

/// Continuous horizontal coverage interval at a pixel-center Y sample.
fn rounded_interval(rect: Rect, radii: CornerRadii, scene_y: f32) -> Option<(f32, f32)> {
    if scene_y < rect.y || scene_y >= rect.y + rect.height {
        return None;
    }
    let local_y = scene_y - rect.y;
    let mut left = rect.x;
    let mut right = rect.x + rect.width;
    let corner_offset = |radius: f32, center_y: f32| {
        if radius <= 0.0 {
            return 0.0;
        }
        let dy = scene_y - center_y;
        radius - libm::sqrtf((radius * radius - dy * dy).max(0.0))
    };
    if local_y < radii.top_left {
        left += corner_offset(radii.top_left, rect.y + radii.top_left);
    } else if local_y >= rect.height - radii.bottom_left {
        left += corner_offset(radii.bottom_left, rect.y + rect.height - radii.bottom_left);
    }
    if local_y < radii.top_right {
        right -= corner_offset(radii.top_right, rect.y + radii.top_right);
    } else if local_y >= rect.height - radii.bottom_right {
        right -= corner_offset(radii.bottom_right, rect.y + rect.height - radii.bottom_right);
    }
    (right > left).then_some((left, right))
}

fn push_scene_interval(
    order: u32,
    local_y: u32,
    scene_left: f32,
    scene_right: f32,
    color: Color,
    viewport: Viewport,
    limit: usize,
    output: &mut Vec<LoweredCommand>,
) -> Result<(), LowerError> {
    let left = libm::ceilf(scene_left - viewport.x - 0.5)
        .max(0.0)
        .min(viewport.width as f32);
    let right = libm::ceilf(scene_right - viewport.x - 0.5)
        .max(0.0)
        .min(viewport.width as f32);
    if right <= left {
        return Ok(());
    }
    let x = left as u32;
    let width = (right - left) as u32;
    for existing in output.iter_mut().rev() {
        match existing {
            LoweredCommand::SolidSpan {
                order: existing_order,
                x: existing_x,
                y: existing_y,
                width: existing_width,
                height: existing_height,
                color: existing_color,
            } if *existing_order == order
                && *existing_x == x
                && *existing_width == width
                && *existing_color == color
                && existing_y.saturating_add(*existing_height) == local_y =>
            {
                *existing_height = existing_height.saturating_add(1);
                return Ok(());
            }
            LoweredCommand::SolidSpan {
                order: existing_order,
                ..
            }
            | LoweredCommand::FontCanvas {
                order: existing_order,
                ..
            } if *existing_order != order => break,
            _ => {}
        }
    }
    push_bounded(
        output,
        LoweredCommand::SolidSpan {
            order,
            x,
            y: local_y,
            width,
            height: 1,
            color,
        },
        limit,
    )
}

fn lower_solid_rect(
    order: u32,
    rect: Rect,
    color: Color,
    viewport: Viewport,
    limit: usize,
    output: &mut Vec<LoweredCommand>,
) -> Result<(), LowerError> {
    let Some((x, y, width, height)) = clipped_integer_rect(rect, viewport) else {
        return Ok(());
    };
    push_bounded(
        output,
        LoweredCommand::SolidSpan {
            order,
            x,
            y,
            width,
            height,
            color,
        },
        limit,
    )
}

fn clipped_integer_rect(rect: Rect, viewport: Viewport) -> Option<(u32, u32, u32, u32)> {
    let left = libm::ceilf(rect.x - viewport.x - 0.5)
        .max(0.0)
        .min(viewport.width as f32);
    let top = libm::ceilf(rect.y - viewport.y - 0.5)
        .max(0.0)
        .min(viewport.height as f32);
    let right = libm::ceilf(rect.x + rect.width - viewport.x - 0.5)
        .max(0.0)
        .min(viewport.width as f32);
    let bottom = libm::ceilf(rect.y + rect.height - viewport.y - 0.5)
        .max(0.0)
        .min(viewport.height as f32);
    if right <= left || bottom <= top {
        return None;
    }
    Some((left as u32, top as u32, (right - left) as u32, (bottom - top) as u32))
}

fn font_canvas_crop(rect: Rect, viewport: Viewport) -> Option<(u32, u32, u32, u32, u32, u32)> {
    let (x, y, width, height) = clipped_integer_rect(rect, viewport)?;
    let scene_x = viewport.x + x as f32;
    let scene_y = viewport.y + y as f32;
    Some((
        x,
        y,
        (scene_x - rect.x).max(0.0) as u32,
        (scene_y - rect.y).max(0.0) as u32,
        width,
        height,
    ))
}

fn image_crop(
    rect: Rect,
    source: Rect,
    viewport: Viewport,
) -> Option<(u32, u32, u32, u32, u32, u32, u32, u32)> {
    let (x, y, width, height) = clipped_integer_rect(rect, viewport)?;
    let scene_left = viewport.x + x as f32;
    let scene_top = viewport.y + y as f32;
    let scene_right = scene_left + width as f32;
    let scene_bottom = scene_top + height as f32;
    let map_x = |scene: f32| source.x + (scene - rect.x) * source.width / rect.width;
    let map_y = |scene: f32| source.y + (scene - rect.y) * source.height / rect.height;
    let source_left = libm::floorf(map_x(scene_left).max(source.x));
    let source_top = libm::floorf(map_y(scene_top).max(source.y));
    let source_right = libm::ceilf(map_x(scene_right).min(source.x + source.width));
    let source_bottom = libm::ceilf(map_y(scene_bottom).min(source.y + source.height));
    if source_right <= source_left || source_bottom <= source_top {
        return None;
    }
    Some((
        x,
        y,
        width,
        height,
        source_left as u32,
        source_top as u32,
        (source_right - source_left) as u32,
        (source_bottom - source_top) as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color::rgba(255, 0, 0, 255);
    const BLUE: Color = Color::rgba(0, 0, 255, 255);

    #[test]
    fn bulk_replace_preserves_cross_kind_order_and_crop() {
        let mut scene = PicassoScene::new();
        scene
            .replace_ordered([
                PrimitiveRow::new(10, Primitive::solid_rect(Rect::new(0.0, 0.0, 8.0, 6.0), RED)),
                PrimitiveRow::new(
                    20,
                    Primitive::rounded_border(
                        Rect::new(1.0, 1.0, 6.0, 4.0),
                        CornerRadii::all(1.0),
                        1.0,
                        BLUE,
                    ),
                ),
                PrimitiveRow::new(30, Primitive::font_canvas(Rect::new(0.0, 0.0, 8.0, 6.0))),
            ])
            .unwrap();
        let commands = scene.lower(Viewport::new(2.0, 1.0, 4, 3), 64).unwrap();
        assert!(!commands.is_empty());
        let orders: Vec<_> = commands
            .iter()
            .map(|command| match command {
                LoweredCommand::SolidSpan { order, .. }
                | LoweredCommand::FontCanvas { order, .. }
                | LoweredCommand::Image { order, .. }
                | LoweredCommand::FontLookup { order, .. } => *order,
            })
            .collect();
        assert!(orders.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(matches!(
            commands.last(),
            Some(LoweredCommand::FontCanvas {
                source_x: 2,
                source_y: 1,
                width: 4,
                height: 3,
                ..
            })
        ));
    }

    #[test]
    fn image_resource_lowers_after_font_canvas_with_exact_crop() {
        let resource = ImageResourceRef(SceneHandle {
            slot: 3,
            generation: 2,
        });
        let mut scene = PicassoScene::new();
        scene
            .replace_ordered([
                PrimitiveRow::new(10, Primitive::font_canvas(Rect::new(0.0, 0.0, 200.0, 100.0))),
                PrimitiveRow::new(
                    20,
                    Primitive::image(
                        Rect::new(10.0, 20.0, 100.0, 50.0),
                        Rect::new(0.0, 0.0, 200.0, 100.0),
                        resource,
                        192,
                    ),
                ),
            ])
            .unwrap();

        let commands = scene.lower(Viewport::new(35.0, 30.0, 40, 20), 2).unwrap();
        assert_eq!(commands.len(), 2);
        assert_eq!(
            commands[1],
            LoweredCommand::Image {
                order: 20,
                x: 0,
                y: 0,
                width: 40,
                height: 20,
                source_x: 50,
                source_y: 20,
                source_width: 80,
                source_height: 40,
                resource,
                opacity: 192,
            }
        );
    }

    #[test]
    fn failed_replace_is_atomic_and_font_canvas_must_be_last() {
        let mut scene = PicassoScene::new();
        scene
            .replace_ordered([PrimitiveRow::new(
                10,
                Primitive::solid_rect(Rect::new(0.0, 0.0, 4.0, 4.0), RED),
            )])
            .unwrap();
        assert_eq!(
            scene.replace_ordered([
                PrimitiveRow::new(10, Primitive::font_canvas(Rect::new(0.0, 0.0, 4.0, 4.0))),
                PrimitiveRow::new(20, Primitive::solid_rect(Rect::new(0.0, 0.0, 1.0, 1.0), RED)),
            ]),
            Err(SceneError::GeometryAfterFontCanvas)
        );
        assert_eq!(scene.live_count(), 1);
        assert_eq!(scene.publish().rows[0].order, 10);
    }

    #[test]
    fn stable_ref_update_reports_dirty_base_epoch() {
        let mut scene = PicassoScene::new();
        let primitive_ref = scene
            .insert(PrimitiveRow::new(
                10,
                Primitive::solid_rect(Rect::new(0.0, 0.0, 2.0, 2.0), RED),
            ))
            .unwrap();
        let first = scene.publish();
        assert_eq!((first.dirty_base_epoch, first.epoch), (0, 1));
        scene
            .update(
                primitive_ref,
                PrimitiveRow::new(10, Primitive::solid_rect(Rect::new(0.0, 0.0, 3.0, 2.0), BLUE)),
            )
            .unwrap();
        assert_eq!(scene.get(primitive_ref).unwrap().order, 10);
        let second = scene.publish();
        assert_eq!((second.dirty_base_epoch, second.epoch), (1, 2));
        assert_eq!(second.dirty.count, 1);
    }

    #[test]
    fn rounded_fill_and_border_emit_bounded_nonempty_spans() {
        let mut scene = PicassoScene::new();
        scene
            .replace_ordered([
                PrimitiveRow::new(
                    1,
                    Primitive::rounded_rect(
                        Rect::new(0.0, 0.0, 8.0, 6.0),
                        CornerRadii::all(2.0),
                        RED,
                    ),
                ),
                PrimitiveRow::new(
                    2,
                    Primitive::rounded_border(
                        Rect::new(0.0, 0.0, 8.0, 6.0),
                        CornerRadii::all(2.0),
                        1.0,
                        BLUE,
                    ),
                ),
            ])
            .unwrap();
        let commands = scene.lower(Viewport::new(0.0, 0.0, 8, 6), 64).unwrap();
        assert!(commands.iter().all(|command| match command {
            LoweredCommand::SolidSpan { width, .. } => *width > 0,
            LoweredCommand::FontCanvas { .. }
            | LoweredCommand::Image { .. }
            | LoweredCommand::FontLookup { .. } => false,
        }));
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, LoweredCommand::SolidSpan { order: 2, .. }))
        );
    }

    #[test]
    fn rounded_sampling_uses_scene_space_at_nonzero_crop_origin() {
        let mut scene = PicassoScene::new();
        scene
            .insert(PrimitiveRow::new(
                1,
                Primitive::rounded_rect(
                    Rect::new(100.0, 200.0, 8.0, 6.0),
                    CornerRadii::all(2.0),
                    RED,
                ),
            ))
            .unwrap();
        let commands = scene.lower(Viewport::new(100.0, 200.0, 8, 6), 64).unwrap();
        assert!(!commands.is_empty());
        assert!(commands.iter().all(|command| match command {
            LoweredCommand::SolidSpan {
                x,
                y,
                width,
                height,
                ..
            } => *x < 8 && *y < 6 && x + width <= 8 && y + height <= 6,
            LoweredCommand::FontCanvas { .. }
            | LoweredCommand::Image { .. }
            | LoweredCommand::FontLookup { .. } => false,
        }));
    }

    #[test]
    fn validation_and_command_cap_are_explicit() {
        let mut scene = PicassoScene::new();
        assert_eq!(
            scene.insert(PrimitiveRow::new(
                1,
                Primitive::solid_rect(Rect::new(f32::NAN, 0.0, 1.0, 1.0), RED),
            )),
            Err(SceneError::InvalidRect)
        );
        scene
            .insert(PrimitiveRow::new(
                1,
                Primitive::rounded_rect(
                    Rect::new(0.0, 0.0, 10.0, 10.0),
                    CornerRadii::all(1.0),
                    RED,
                ),
            ))
            .unwrap();
        assert_eq!(
            scene.lower(Viewport::new(0.0, 0.0, 10, 10), 0),
            Err(LowerError::CommandLimit { limit: 0 })
        );
        assert!(
            !scene
                .lower(Viewport::new(0.0, 0.0, 10, 10), 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn solid_rect_is_one_command_and_hard_cap_is_exact() {
        let mut solid = PicassoScene::new();
        solid
            .insert(PrimitiveRow::new(
                1,
                Primitive::solid_rect(Rect::new(0.0, 0.0, 960.0, 2_000.0), RED),
            ))
            .unwrap();
        assert_eq!(
            solid.lower(Viewport::new(0.0, 0.0, 960, 720), 1).unwrap(),
            [LoweredCommand::SolidSpan {
                order: 1,
                x: 0,
                y: 0,
                width: 960,
                height: 720,
                color: RED,
            }]
        );

        let rows = (0..MAX_LOWERED_COMMANDS as u32).map(|order| {
            PrimitiveRow::new(
                order,
                Primitive::solid_rect(Rect::new(order as f32, 0.0, 1.0, 1.0), RED),
            )
        });
        let mut capped = PicassoScene::new();
        capped.replace_ordered(rows).unwrap();
        assert_eq!(
            capped
                .lower(
                    Viewport::new(0.0, 0.0, MAX_LOWERED_COMMANDS as u32, 1),
                    MAX_LOWERED_COMMANDS,
                )
                .unwrap()
                .len(),
            MAX_LOWERED_COMMANDS
        );
        let old = PrimitiveRef(SceneHandle {
            slot: 0,
            generation: 1,
        });
        assert!(capped.remove(old));
        capped
            .insert(PrimitiveRow::new(
                MAX_LOWERED_COMMANDS as u32,
                Primitive::rounded_border(
                    Rect::new(0.0, 2.0, 4.0, 4.0),
                    CornerRadii::all(1.0),
                    1.0,
                    RED,
                ),
            ))
            .unwrap();
        assert_eq!(
            capped.lower(
                Viewport::new(0.0, 0.0, MAX_LOWERED_COMMANDS as u32, 8),
                MAX_LOWERED_COMMANDS,
            ),
            Err(LowerError::CommandLimit {
                limit: MAX_LOWERED_COMMANDS
            })
        );
    }

    #[test]
    fn bulk_replace_retires_previous_refs_without_resetting_epoch() {
        let mut scene = PicassoScene::new();
        let old = scene
            .insert(PrimitiveRow::new(1, Primitive::solid_rect(Rect::new(0.0, 0.0, 1.0, 1.0), RED)))
            .unwrap();
        let first = scene.publish();
        assert_eq!(first.epoch, 1);
        scene
            .replace_ordered([PrimitiveRow::new(
                2,
                Primitive::solid_rect(Rect::new(1.0, 0.0, 1.0, 1.0), BLUE),
            )])
            .unwrap();
        assert_eq!(scene.get(old), None);
        let second = scene.publish();
        assert_eq!((second.dirty_base_epoch, second.epoch), (1, 2));
    }

    #[test]
    fn duplicate_and_unsorted_orders_are_rejected() {
        let mut scene = PicassoScene::new();
        assert_eq!(
            scene.replace_ordered([
                PrimitiveRow::new(2, Primitive::solid_rect(Rect::new(0.0, 0.0, 1.0, 1.0), RED)),
                PrimitiveRow::new(1, Primitive::solid_rect(Rect::new(1.0, 0.0, 1.0, 1.0), RED)),
            ]),
            Err(SceneError::OrderNotIncreasing)
        );
        assert_eq!(
            scene.replace_ordered([
                PrimitiveRow::new(1, Primitive::solid_rect(Rect::new(0.0, 0.0, 1.0, 1.0), RED)),
                PrimitiveRow::new(1, Primitive::solid_rect(Rect::new(1.0, 0.0, 1.0, 1.0), RED)),
            ]),
            Err(SceneError::DuplicateOrder)
        );
    }

    #[test]
    fn logical_primitive_input_is_bounded_before_install() {
        // An arbitrarily large producer hint is clamped before allocation; the
        // logical insertion limit remains the same independently of capacity.
        let mut scene = PicassoScene::with_capacity(usize::MAX);
        let rows = (0..=MAX_SCENE_PRIMITIVES as u32).map(|order| {
            PrimitiveRow::new(
                order,
                Primitive::solid_rect(Rect::new(order as f32, 0.0, 1.0, 1.0), RED),
            )
        });
        assert_eq!(
            scene.replace_ordered(rows),
            Err(SceneError::PrimitiveLimit {
                limit: MAX_SCENE_PRIMITIVES
            })
        );
        assert_eq!(scene.live_count(), 0);
    }

    #[test]
    fn font_lookup_lowers_to_stable_handle_without_copying_unicode() {
        let mut scene = PicassoScene::new();
        let lookup = scene
            .insert(PrimitiveRow::new(
                1,
                Primitive::font_lookup(FontLookupRun {
                    rect: Rect::new(100.0, 200.0, 80.0, 24.0),
                    origin: [100.0, 218.0],
                    text: String::from("SceneDB text"),
                    face: FontFace::Inconsolata,
                    slant: FontSlant::Italic,
                    font_pixels: 18.0,
                    color: BLUE,
                }),
            ))
            .unwrap();
        let commands = scene.lower(Viewport::new(110.0, 200.0, 40, 24), 8).unwrap();
        assert_eq!(
            commands,
            alloc::vec![LoweredCommand::FontLookup {
                order: 1,
                x: 0,
                y: 0,
                width: 40,
                height: 24,
                lookup,
            }]
        );
        let run = scene.font_lookup(lookup).unwrap();
        assert_eq!(run.text, "SceneDB text");
        assert_eq!(run.slant.kernel_shear(), 0.15);
    }

    #[test]
    fn font_lookup_payload_is_bounded_and_not_a_gpu_row() {
        let mut scene = PicassoScene::new();
        let result = scene.insert(PrimitiveRow::new(
            1,
            Primitive::font_lookup(FontLookupRun {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                origin: [0.0, 8.0],
                text: "x".repeat(MAX_FONT_LOOKUP_BYTES + 1),
                face: FontFace::Default,
                slant: FontSlant::Normal,
                font_pixels: 8.0,
                color: RED,
            }),
        ));
        assert_eq!(result, Err(SceneError::InvalidFontLookup));
        assert_eq!(core::mem::size_of::<LoweredCommand>(), 32);
    }

    #[test]
    fn font_lookup_export_is_paint_ordered_and_independent_of_geometry_lowering() {
        let mut scene = PicassoScene::new();
        for order in 0..4 {
            scene
                .insert(PrimitiveRow::new(
                    order,
                    Primitive::rounded_border(
                        Rect::new(0.0, 0.0, 64.0, 64.0),
                        CornerRadii::all(8.0),
                        1.0,
                        RED,
                    ),
                ))
                .unwrap();
        }
        for (order, text) in [(20, "later"), (10, "earlier")] {
            scene
                .insert(PrimitiveRow::new(
                    order,
                    Primitive::font_lookup(FontLookupRun {
                        rect: Rect::new(0.0, 0.0, 64.0, 20.0),
                        origin: [0.0, 16.0],
                        text: String::from(text),
                        face: FontFace::Default,
                        slant: FontSlant::Normal,
                        font_pixels: 16.0,
                        color: BLUE,
                    }),
                ))
                .unwrap();
        }
        assert_eq!(
            scene.lower(Viewport::new(0.0, 0.0, 64, 64), 1),
            Err(LowerError::CommandLimit { limit: 1 })
        );
        assert_eq!(
            scene
                .font_lookup_rows()
                .map(|(_, row)| row.text.as_str())
                .collect::<Vec<_>>(),
            alloc::vec!["earlier", "later"]
        );
    }
}
