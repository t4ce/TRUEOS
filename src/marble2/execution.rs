use super::widget::MarbleWidgetKind;
use super::world::{MarbleWorld, TileId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecOutcome {
    Advanced,
    LostOnEmpty,
    Blocked,
    ReachedBlackHole,
}

#[derive(Clone, Copy, Debug)]
pub struct MarbleState {
    pub tile: TileId,
    pub alive: bool,
    // 1-bit variable values plus assignment history up to 63 vars.
    pub value_mask: u64,
    pub assigned_mask: u64,
}

impl MarbleState {
    pub fn new(tile: TileId) -> Self {
        Self {
            tile,
            alive: true,
            value_mask: 0,
            assigned_mask: 0,
        }
    }

    pub fn set_var(&mut self, var_index_1based: u8, value: bool) -> bool {
        if var_index_1based == 0 || var_index_1based > 63 {
            return false;
        }
        let bit = 1u64 << (var_index_1based - 1);
        self.assigned_mask |= bit;
        if value {
            self.value_mask |= bit;
        } else {
            self.value_mask &= !bit;
        }
        true
    }

    pub fn var_value(&self, var_index_1based: u8) -> Option<bool> {
        if var_index_1based == 0 || var_index_1based > 63 {
            return None;
        }
        let bit = 1u64 << (var_index_1based - 1);
        if (self.assigned_mask & bit) == 0 {
            return None;
        }
        Some((self.value_mask & bit) != 0)
    }
}

pub fn step_marble(world: &MarbleWorld, state: &mut MarbleState, link_index: usize) -> ExecOutcome {
    if !state.alive {
        return ExecOutcome::Blocked;
    }

    let Some(current) = world.widget_at(state.tile) else {
        state.alive = false;
        return ExecOutcome::LostOnEmpty;
    };

    if current.kind == MarbleWidgetKind::BlackHole {
        return ExecOutcome::ReachedBlackHole;
    }

    let Some(Some(next_tile)) = current.links.get(link_index) else {
        state.alive = false;
        return ExecOutcome::LostOnEmpty;
    };

    if world.widget_at(*next_tile).is_none() {
        state.alive = false;
        return ExecOutcome::LostOnEmpty;
    }

    state.tile = *next_tile;
    ExecOutcome::Advanced
}
