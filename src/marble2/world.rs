use alloc::vec::Vec;

use super::widget::MarbleWidget;

pub type TileId = u32;

#[derive(Clone, Debug)]
pub struct MarbleWorld {
    pub slots: Vec<Option<MarbleWidget>>,
}

impl MarbleWorld {
    pub fn new_empty(size: usize) -> Self {
        Self {
            slots: vec![None; size],
        }
    }

    pub fn size(&self) -> usize {
        self.slots.len()
    }

    pub fn place_widget(&mut self, tile: TileId, widget: MarbleWidget) -> bool {
        let idx = tile as usize;
        if idx >= self.slots.len() {
            return false;
        }
        self.slots[idx] = Some(widget);
        true
    }

    pub fn widget_at(&self, tile: TileId) -> Option<&MarbleWidget> {
        self.slots.get(tile as usize).and_then(|s| s.as_ref())
    }
}

#[derive(Clone, Debug)]
pub struct MarbleUniverse {
    pub worlds: Vec<MarbleWorld>,
}

impl MarbleUniverse {
    pub fn new() -> Self {
        Self { worlds: Vec::new() }
    }

    pub fn push_world(&mut self, world: MarbleWorld) -> usize {
        self.worlds.push(world);
        self.worlds.len() - 1
    }

    pub fn world(&self, index: usize) -> Option<&MarbleWorld> {
        self.worlds.get(index)
    }

    pub fn world_mut(&mut self, index: usize) -> Option<&mut MarbleWorld> {
        self.worlds.get_mut(index)
    }
}
