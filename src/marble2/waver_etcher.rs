use alloc::vec::Vec;

use super::problem::ProblemProgram;
use super::widget::{MarbleWidget, MarbleWidgetKind};
use super::world::{MarbleWorld, TileId};

#[derive(Clone, Copy, Debug)]
pub struct WidgetPlacement {
    pub tile: TileId,
    pub widget: MarbleWidget,
}

#[derive(Clone, Debug)]
pub struct WaverPlan {
    pub placements: Vec<WidgetPlacement>,
}

impl WaverPlan {
    pub fn required_world_size(&self) -> usize {
        self.placements
            .iter()
            .map(|p| p.tile as usize + 1)
            .max()
            .unwrap_or(0)
    }
}

pub struct Waver;

impl Waver {
    pub fn plan(program: &ProblemProgram) -> WaverPlan {
        let mut placements: Vec<WidgetPlacement> = Vec::new();
        let mut tile: TileId = 0;

        placements.push(WidgetPlacement {
            tile,
            widget: MarbleWidget::new(MarbleWidgetKind::WhiteHole),
        });
        tile += 1;

        for _ in 0..program.vars {
            placements.push(WidgetPlacement {
                tile,
                widget: MarbleWidget::new(MarbleWidgetKind::Variable),
            });
            tile += 1;
        }

        for _ in &program.clauses {
            placements.push(WidgetPlacement {
                tile,
                widget: MarbleWidget::new(MarbleWidgetKind::Clause),
            });
            tile += 1;
        }

        placements.push(WidgetPlacement {
            tile,
            widget: MarbleWidget::new(MarbleWidgetKind::SatSink),
        });
        tile += 1;

        placements.push(WidgetPlacement {
            tile,
            widget: MarbleWidget::new(MarbleWidgetKind::UnsatSink),
        });
        tile += 1;

        placements.push(WidgetPlacement {
            tile,
            widget: MarbleWidget::new(MarbleWidgetKind::BlackHole),
        });

        WaverPlan { placements }
    }
}

pub struct Etcher;

impl Etcher {
    pub fn apply(world: &mut MarbleWorld, plan: &WaverPlan) -> bool {
        for p in &plan.placements {
            if !world.place_widget(p.tile, p.widget) {
                return false;
            }
        }
        true
    }
}
