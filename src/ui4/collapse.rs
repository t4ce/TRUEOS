//! Kernel-owned compact presentation. Producer frames and their logical extent
//! survive unchanged; only the broker snapshot switches to the owned icon.
use super::*;

pub(super) const MENU_ACTION: u32 = u32::MAX;
pub(crate) const MAX_COLLAPSE_IMAGE_SIZE: u32 = 256;
const COLLAPSED_SIZE: u32 = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CollapseError {
    InvalidImage,
    NotFound,
    Busy,
    Ui4,
}
impl From<FramePoolError> for CollapseError {
    fn from(error: FramePoolError) -> Self {
        match error {
            FramePoolError::Busy => Self::Busy,
            _ => Self::Ui4,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Collapse {
    pub frame: FrameHandle,
    pub width: u32,
    pub height: u32,
    pub tile: Option<WindowPlacement>,
}

impl Collapse {
    pub fn toggle(&mut self, placement: WindowPlacement) {
        self.tile = if self.tile.is_some() {
            None
        } else {
            Some(WindowPlacement {
                y: placement
                    .y
                    .saturating_add(placement.height.saturating_sub(self.height) as i32),
                width: self.width,
                height: self.height,
                ..placement
            })
        };
    }

    pub fn project(self, mut window: WindowSnapshot) -> WindowSnapshot {
        if let Some(tile) = self.tile {
            let placement = WindowPlacement {
                visible: window.placement.visible,
                opacity: window.placement.opacity,
                z: window.placement.z,
                ..tile
            };
            window.frame = self.frame;
            window.arc = 0;
            window.background = None;
            window.buffering = FrameBuffering::Double;
            window.placement = placement;
            window.presentation_placement = placement;
            window.maximized = false;
            window.dock_target = None;
            // The first click selects the tile; only a subsequent click restores it.
            window.interaction.primary_activation = false;
            window.interaction.maximizable = false;
            window.interaction.resize_on_maximize = false;
        }
        window
    }
}

/// Per-cursor click tracking; leaving and returning never revives a drag.
#[derive(Clone, Copy)]
pub(super) struct RestorePress {
    start: (u32, u32),
    moved: bool,
}
impl RestorePress {
    pub fn new(x: u32, y: u32) -> Self {
        Self {
            start: (x, y),
            moved: false,
        }
    }
    pub fn update(&mut self, x: u32, y: u32, secondary_down: bool) {
        self.moved |=
            x.abs_diff(self.start.0) > 4 || y.abs_diff(self.start.1) > 4 || secondary_down;
    }
    pub fn clicked(self, inside: bool) -> bool {
        inside && !self.moved
    }
}

/// Copies a pitched straight-alpha source into an owned premultiplied frame.
/// All registered images are resampled to the standard 64×64 compact tile.
pub(crate) fn enable_window_collapse(
    owner: WindowOwner,
    window: WindowId,
    width: u32,
    height: u32,
    pitch: usize,
    rgba: &[u8],
) -> Result<(), CollapseError> {
    if !(1..=MAX_COLLAPSE_IMAGE_SIZE).contains(&width)
        || !(1..=MAX_COLLAPSE_IMAGE_SIZE).contains(&height)
        || pitch < width as usize * 4
        || pitch
            .checked_mul(height as usize)
            .is_none_or(|bytes| rgba.len() < bytes)
    {
        return Err(CollapseError::InvalidImage);
    }
    let output = window_broker::window_snapshot(owner, window)
        .ok_or(CollapseError::NotFound)?
        .output;
    let frame = create_frame(FrameSpec {
        output,
        content: FrameContent::Image,
        cadence: FrameCadence::Dirty,
        buffering: FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width: COLLAPSED_SIZE,
        height: COLLAPSED_SIZE,
        base_color: Some(PremultipliedRgba8::TRANSPARENT),
    })
    .map_err(CollapseError::from)?;
    let result = (|| {
        let lease = acquire_frame_buffer(frame).map_err(CollapseError::from)?;
        let view = match writable_rgba_view(lease) {
            Ok(view) => view,
            Err(error) => {
                let _ = cancel_frame_buffer(lease);
                return Err(CollapseError::from(error));
            }
        };
        let pixels = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
        pixels.fill(0);
        for y in 0..COLLAPSED_SIZE as usize {
            for x in 0..COLLAPSED_SIZE as usize {
                let (source_x, source_y) = collapse_source_pixel(x, y, width, height);
                let src = &rgba[source_y * pitch + source_x * 4..][..4];
                let dst = &mut pixels[y * view.pitch as usize + x * 4..][..4];
                premultiply(src, dst);
            }
        }
        crate::intel::dma_flush(view.virt, view.byte_len);
        if let Err(error) = publish_frame_buffer(lease) {
            let _ = cancel_frame_buffer(lease);
            return Err(CollapseError::from(error));
        }
        window_broker::install_collapse(
            owner,
            window,
            Collapse {
                frame,
                width: COLLAPSED_SIZE,
                height: COLLAPSED_SIZE,
                tile: None,
            },
        )
        .map_err(|_| CollapseError::Ui4)
    })();
    if result.is_err() {
        window_broker::retire_frame_when_released(frame);
    }
    result
}

fn premultiply(src: &[u8], dst: &mut [u8]) {
    for channel in 0..3 {
        dst[channel] = ((u16::from(src[channel]) * u16::from(src[3]) + 127) / 255) as u8;
    }
    dst[3] = src[3];
}

pub(super) fn menu_entry(owner: WindowOwner, window: WindowId) -> Option<ContextMenuEntry> {
    window_broker::collapse_state(owner, window).map(|collapsed| {
        ContextMenuEntry::action(if collapsed { "restore" } else { "collapse" }, MENU_ACTION)
    })
}

pub(super) fn ignore_menu(_: ContextMenuResult) {}

// Sample at destination pixel centers; source extents are validated before use.
fn collapse_source_pixel(x: usize, y: usize, width: u32, height: u32) -> (usize, usize) {
    let size = COLLAPSED_SIZE as usize;
    (
        ((2 * x + 1) * width as usize / (2 * size)).min(width as usize - 1),
        ((2 * y + 1) * height as usize / (2 * size)).min(height as usize - 1),
    )
}
