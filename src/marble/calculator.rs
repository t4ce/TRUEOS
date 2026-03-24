//! CPUs are good at solving a general set of problems and can be reprogrammed fast.
//! The marble wave function collapse operates on a constructed input marble world.
//! It tiles a virtual area that we do not restrict in space.
//! Afterwards a universe is collapsed and operates on the meta it resembles.
//! Universe contains worlds.
//! World gets collapsed.
//! Collapsed world becomes runnable.
//! Runnable world can be hosted or enacted as a park.
//! Park is where marbles actually flow.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use crate::tst_widget_tree::WidgetKind;

use super::{Marble, MarbleEmpty, MarbleGadget, MarbleGather, MarblePackage, MarbleTraceField};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalculatorMarble {
    pub source_lane: usize,
    pub value: u32,
    pub empty: bool,
}

impl CalculatorMarble {
    pub const fn value(source_lane: usize, value: u32) -> Self {
        Self {
            source_lane,
            value,
            empty: false,
        }
    }

    pub const fn empty(source_lane: usize) -> Self {
        Self {
            source_lane,
            value: 0,
            empty: true,
        }
    }

    fn render(&self) -> String {
        if self.empty {
            "__".into()
        } else {
            let mut out = String::new();
            let _ = write!(out, "{:02}", self.value);
            out
        }
    }
}

impl Marble for CalculatorMarble {
    fn kind(&self) -> &'static str {
        if self.empty {
            "calculator-empty-marble"
        } else {
            "calculator-marble"
        }
    }
}

impl MarbleEmpty for CalculatorMarble {
    fn is_empty(&self) -> bool {
        self.empty
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalculatorGatherPolicy {
    Strict,
    EmptyFill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalculatorPackage {
    marbles: Vec<CalculatorMarble>,
    pub sum: u32,
    pub empty_lanes: usize,
}

impl CalculatorPackage {
    fn new(marbles: Vec<CalculatorMarble>) -> Self {
        let mut sum = 0u32;
        let mut empty_lanes = 0usize;
        for marble in marbles.iter() {
            if marble.empty {
                empty_lanes += 1;
            } else {
                sum = sum.saturating_add(marble.value);
            }
        }

        Self {
            marbles,
            sum,
            empty_lanes,
        }
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = write!(out, "sum={} empty={} lanes=[", self.sum, self.empty_lanes);
        for (index, marble) in self.marbles.iter().enumerate() {
            if index != 0 {
                out.push(' ');
            }
            out.push_str(&marble.render());
        }
        out.push(']');
        out
    }
}

impl Marble for CalculatorPackage {
    fn kind(&self) -> &'static str {
        "calculator-package"
    }
}

impl MarblePackage<CalculatorMarble> for CalculatorPackage {
    fn width(&self) -> usize {
        self.marbles.len()
    }

    fn lane(&self, index: usize) -> Option<&CalculatorMarble> {
        self.marbles.get(index)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalculatorHarnessError {
    InvalidLane,
    Full,
}

#[derive(Debug, Clone)]
pub struct MarbleCalculatorHarness {
    lanes: Vec<VecDeque<CalculatorMarble>>,
    capacity_per_lane: usize,
    next_value: u32,
    policy: CalculatorGatherPolicy,
    released: Vec<CalculatorPackage>,
}

impl MarbleCalculatorHarness {
    pub fn new(
        lane_count: usize,
        capacity_per_lane: usize,
        policy: CalculatorGatherPolicy,
    ) -> Self {
        let mut lanes = Vec::with_capacity(lane_count);
        for _ in 0..lane_count {
            lanes.push(VecDeque::with_capacity(capacity_per_lane));
        }

        Self {
            lanes,
            capacity_per_lane,
            next_value: 1,
            policy,
            released: Vec::new(),
        }
    }

    pub fn source_push_generated(
        &mut self,
        lane: usize,
        count: usize,
    ) -> Result<usize, CalculatorHarnessError> {
        let Some(queue) = self.lanes.get_mut(lane) else {
            return Err(CalculatorHarnessError::InvalidLane);
        };

        let mut pushed = 0usize;
        for _ in 0..count {
            if queue.len() >= self.capacity_per_lane {
                return Err(CalculatorHarnessError::Full);
            }

            let marble = CalculatorMarble::value(lane, self.next_value);
            self.next_value = self.next_value.saturating_add(1);
            queue.push_back(marble);
            pushed += 1;
        }

        Ok(pushed)
    }

    pub fn released(&self) -> &[CalculatorPackage] {
        &self.released
    }

    pub fn render_lanes(&self) -> String {
        let mut out = String::new();
        for (lane_index, lane) in self.lanes.iter().enumerate() {
            let _ = write!(out, "in{:02}: ", lane_index);
            if lane.is_empty() {
                out.push_str(".");
            } else {
                for (index, marble) in lane.iter().enumerate() {
                    if index != 0 {
                        out.push(' ');
                    }
                    out.push_str(&marble.render());
                }
            }
            if lane_index + 1 != self.lanes.len() {
                out.push('\n');
            }
        }
        out
    }

    pub fn released_visual(&self) -> String {
        let mut out = String::new();
        for (index, package) in self.released.iter().enumerate() {
            let _ = writeln!(out, "out{:02}: {}", index, package.render());
        }
        out
    }

    pub fn gather_once(&mut self) -> Result<Option<CalculatorPackage>, CalculatorHarnessError> {
        let mut package = Vec::with_capacity(self.lanes.len());

        for (lane_index, lane) in self.lanes.iter_mut().enumerate() {
            if let Some(marble) = lane.pop_front() {
                package.push(marble);
                continue;
            }

            match self.policy {
                CalculatorGatherPolicy::Strict => return Ok(None),
                CalculatorGatherPolicy::EmptyFill => {
                    package.push(CalculatorMarble::empty(lane_index));
                }
            }
        }

        let package = CalculatorPackage::new(package);
        self.released.push(package.clone());
        Ok(Some(package))
    }
}

impl MarbleGadget for MarbleCalculatorHarness {
    fn name(&self) -> &'static str {
        "marble-calculator-harness"
    }
}

impl MarbleTraceField<CalculatorMarble> for MarbleCalculatorHarness {
    type Error = CalculatorHarnessError;

    fn lanes(&self) -> usize {
        self.lanes.len()
    }

    fn try_put_lane(&mut self, lane: usize, marble: CalculatorMarble) -> Result<(), Self::Error> {
        let Some(queue) = self.lanes.get_mut(lane) else {
            return Err(CalculatorHarnessError::InvalidLane);
        };

        if queue.len() >= self.capacity_per_lane {
            return Err(CalculatorHarnessError::Full);
        }

        queue.push_back(marble);
        Ok(())
    }

    fn lane_ready(&self, lane: usize) -> bool {
        self.lanes
            .get(lane)
            .map(|queue| !queue.is_empty())
            .unwrap_or(false)
    }
}

impl MarbleGather<CalculatorMarble, CalculatorPackage> for MarbleCalculatorHarness {
    type Error = CalculatorHarnessError;

    fn gather(&mut self) -> Result<Option<CalculatorPackage>, Self::Error> {
        self.gather_once()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalculatorTile {
    Source,
    Race,
    Gather,
    Portal,
    BoxStore,
    Sink,
}

impl CalculatorTile {
    fn glyph(self) -> char {
        match self {
            CalculatorTile::Source => 'S',
            CalculatorTile::Race => 'R',
            CalculatorTile::Gather => 'G',
            CalculatorTile::Portal => 'P',
            CalculatorTile::BoxStore => 'B',
            CalculatorTile::Sink => 'K',
        }
    }

    fn name(self) -> &'static str {
        match self {
            CalculatorTile::Source => "source",
            CalculatorTile::Race => "race",
            CalculatorTile::Gather => "gather",
            CalculatorTile::Portal => "portal",
            CalculatorTile::BoxStore => "box-store",
            CalculatorTile::Sink => "sink",
        }
    }

    fn allows_next(self, next: Self) -> bool {
        match self {
            CalculatorTile::Source => matches!(next, CalculatorTile::Race | CalculatorTile::Gather),
            CalculatorTile::Race => matches!(next, CalculatorTile::Race | CalculatorTile::Gather),
            CalculatorTile::Gather => matches!(
                next,
                CalculatorTile::Portal | CalculatorTile::BoxStore | CalculatorTile::Sink
            ),
            CalculatorTile::Portal => matches!(
                next,
                CalculatorTile::Race | CalculatorTile::BoxStore | CalculatorTile::Sink
            ),
            CalculatorTile::BoxStore => {
                matches!(next, CalculatorTile::Portal | CalculatorTile::Sink)
            }
            CalculatorTile::Sink => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldCondition {
    PinTile {
        tile: usize,
        kind: CalculatorTile,
    },
    RequirePortalAt {
        tile: usize,
    },
    RequireBoxBeforeSink,
    PreferFastPath,
    StrictPackages,
    AllowEmptyFill,
}

const COLLAPSE_PRIORITY: &[CalculatorTile] = &[
    CalculatorTile::Race,
    CalculatorTile::Gather,
    CalculatorTile::Portal,
    CalculatorTile::BoxStore,
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct WaveCell {
    options: Vec<CalculatorTile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalculatorLayout {
    tiles: Vec<CalculatorTile>,
}

impl CalculatorLayout {
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("glyphs: ");
        for (index, tile) in self.tiles.iter().copied().enumerate() {
            if index != 0 {
                out.push_str(" -> ");
            }
            out.push(tile.glyph());
        }
        out.push('\n');
        out.push_str("names : ");
        for (index, tile) in self.tiles.iter().copied().enumerate() {
            if index != 0 {
                out.push_str(" -> ");
            }
            out.push_str(tile.name());
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportedWidget {
    pub kind: WidgetKind,
    pub preferred_tile: CalculatorTile,
}

impl ImportedWidget {
    pub const fn name(self) -> &'static str {
        match self.kind {
            WidgetKind::Root => "root",
            WidgetKind::Div => "div",
            WidgetKind::Details => "details",
            WidgetKind::Button => "button",
            WidgetKind::TextInput => "text-input",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldDescription {
    pub universe: MarbleUniverseId,
    pub width: usize,
    pub widgets: Vec<ImportedWidget>,
    pub conditions: Vec<WorldCondition>,
}

impl WorldDescription {
    pub fn with_all_widgets(universe: MarbleUniverseId, width: usize) -> Self {
        Self {
            universe,
            width: width.max(3),
            widgets: import_all_widgets(),
            conditions: vec![
                WorldCondition::AllowEmptyFill,
                WorldCondition::PreferFastPath,
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarbleUniverseId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarbleTileLocation {
    pub index: usize,
}

impl MarbleTileLocation {
    pub const fn new(index: usize) -> Self {
        Self { index }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarbleSingularityKind {
    BlackHole,
    WhiteHole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarbleSingularityStub {
    pub kind: MarbleSingularityKind,
    pub location: MarbleTileLocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarbleCollapseInput {
    pub universe: MarbleUniverseId,
    pub world: MarbleWorld,
    pub singularities: Vec<MarbleSingularityStub>,
}

impl MarbleCollapseInput {
    pub fn with_all_widgets(universe: MarbleUniverseId, width: usize) -> Self {
        let world = compile_world_description(&WorldDescription::with_all_widgets(universe, width));
        let singularities = singularity_stubs_for_world(&world);
        Self {
            universe,
            world,
            singularities,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarbleWorld {
    pub width: usize,
    pub tile_count: usize,
    pub widgets: Vec<ImportedWidget>,
    pub widget_bindings: Vec<ImportedWidgetBinding>,
    pub influences: Vec<MarbleInfluence>,
    pub conditions: Vec<WorldCondition>,
}

impl MarbleWorld {
    pub fn with_all_widgets(width: usize) -> Self {
        compile_world_description(&WorldDescription::with_all_widgets(MarbleUniverseId(0), width))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportedWidgetBinding {
    pub widget: ImportedWidget,
    pub tile: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarbleInfluence {
    pub from: usize,
    pub to: usize,
    pub allowed_pairs: Vec<(CalculatorTile, CalculatorTile)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MarbleUnaryConstraint {
    tile: usize,
    allowed: Vec<CalculatorTile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GraphWaveCell {
    options: Vec<CalculatorTile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MarbleConstraintState {
    domains: Vec<GraphWaveCell>,
    contradictions: Vec<usize>,
    propagation_steps: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollapsedMarbleWorld {
    pub layout: CalculatorLayout,
    pub widgets: Vec<ImportedWidget>,
    pub placements: Vec<(ImportedWidget, usize)>,
    pub contradictions: Vec<usize>,
    pub propagation_steps: usize,
    pub conditions: Vec<WorldCondition>,
}

impl CollapsedMarbleWorld {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "collapsed-world");
        let _ = writeln!(out, "{}", self.layout.render());
        let _ = writeln!(out);
        let _ = writeln!(out, "widget-imports");
        for widget in self.widgets.iter().copied() {
            let _ = writeln!(
                out,
                "{} -> prefers {}",
                widget.name(),
                widget.preferred_tile.name()
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "placements");
        for (widget, index) in self.placements.iter().copied() {
            let _ = writeln!(
                out,
                "{} @ cell {} [{}]",
                widget.name(),
                index,
                self.layout.tiles[index].name()
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "propagation-steps: {}", self.propagation_steps);
        let _ = writeln!(out, "contradictions: {:?}", self.contradictions);
        let _ = writeln!(out, "conditions: {:?}", self.conditions);
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnableWidgetPlacement {
    pub widget: ImportedWidget,
    pub tile: CalculatorTile,
    pub location: MarbleTileLocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnableMarbleWorld {
    pub universe: MarbleUniverseId,
    pub layout: CalculatorLayout,
    pub widgets: Vec<RunnableWidgetPlacement>,
    pub singularities: Vec<MarbleSingularityStub>,
    pub contradictions: Vec<usize>,
    pub propagation_steps: usize,
    pub gather_policy: CalculatorGatherPolicy,
}

impl RunnableMarbleWorld {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "runnable-universe={}", self.universe.0);
        let _ = writeln!(out, "{}", self.layout.render());
        let _ = writeln!(out);
        let _ = writeln!(out, "runnable-widgets");
        for widget in self.widgets.iter() {
            let _ = writeln!(
                out,
                "{} @ {} [{}]",
                widget.widget.name(),
                widget.location.index,
                widget.tile.name()
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "singularities");
        for singularity in self.singularities.iter().copied() {
            let _ = writeln!(
                out,
                "{} @ {}",
                singularity_name(singularity.kind),
                singularity.location.index
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "gather-policy: {:?}", self.gather_policy);
        let _ = writeln!(out, "propagation-steps: {}", self.propagation_steps);
        let _ = writeln!(out, "contradictions: {:?}", self.contradictions);
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarbleCollapseError {
    Contradiction,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MarbleCollapseEngine;

impl MarbleCollapseEngine {
    pub const fn new() -> Self {
        Self
    }

    pub fn collapse_description(
        &mut self,
        description: WorldDescription,
    ) -> Result<RunnableMarbleWorld, MarbleCollapseError> {
        self.transform(description)
    }
}

impl MarbleGadget for MarbleCollapseEngine {
    fn name(&self) -> &'static str {
        "marble-collapse-engine"
    }
}

impl super::MarbleTransform<WorldDescription, RunnableMarbleWorld> for MarbleCollapseEngine {
    type Error = MarbleCollapseError;

    fn transform(&mut self, description: WorldDescription) -> Result<RunnableMarbleWorld, Self::Error> {
        let input = MarbleCollapseInput::from_description(&description);
        let runtime = build_runnable_world(&input);
        if runtime.contradictions.is_empty() {
            Ok(runtime)
        } else {
            Err(MarbleCollapseError::Contradiction)
        }
    }
}

pub fn import_all_widgets() -> Vec<ImportedWidget> {
    vec![
        ImportedWidget {
            kind: WidgetKind::Root,
            preferred_tile: CalculatorTile::Source,
        },
        ImportedWidget {
            kind: WidgetKind::Div,
            preferred_tile: CalculatorTile::BoxStore,
        },
        ImportedWidget {
            kind: WidgetKind::Details,
            preferred_tile: CalculatorTile::Portal,
        },
        ImportedWidget {
            kind: WidgetKind::Button,
            preferred_tile: CalculatorTile::Race,
        },
        ImportedWidget {
            kind: WidgetKind::TextInput,
            preferred_tile: CalculatorTile::Gather,
        },
    ]
}

pub fn compile_world_description(description: &WorldDescription) -> MarbleWorld {
    let widgets = description.widgets.clone();
    let tile_count = description.width.max(widgets.len().saturating_add(2)).max(3);
    let mut influences = linear_world_influences(tile_count);
    influences.extend(condition_influences(tile_count, &description.conditions));

    MarbleWorld {
        width: description.width.max(3),
        tile_count,
        widget_bindings: bind_widgets_to_tiles(tile_count, &widgets),
        influences,
        widgets,
        conditions: description.conditions.clone(),
    }
}

pub fn collapse_marble_world(world: &MarbleWorld) -> CollapsedMarbleWorld {
    let state = solve_marble_world(world);
    let layout = CalculatorLayout {
        tiles: state
            .domains
            .iter()
            .map(|cell| {
                cell.options
                    .first()
                    .copied()
                    .unwrap_or(COLLAPSE_PRIORITY[0])
            })
            .collect(),
    };
    let placements = world
        .widget_bindings
        .iter()
        .copied()
        .map(|binding| (binding.widget, binding.tile))
        .collect();

    CollapsedMarbleWorld {
        layout,
        widgets: world.widgets.clone(),
        placements,
        contradictions: state.contradictions,
        propagation_steps: state.propagation_steps,
        conditions: world.conditions.clone(),
    }
}

pub fn collapse_input_to_world(input: &MarbleCollapseInput) -> CollapsedMarbleWorld {
    collapse_marble_world(&input.world)
}

impl MarbleCollapseInput {
    pub fn from_description(description: &WorldDescription) -> Self {
        let world = compile_world_description(description);
        let singularities = singularity_stubs_for_world(&world);
        Self {
            universe: description.universe,
            world,
            singularities,
        }
    }
}

pub fn build_runnable_world(input: &MarbleCollapseInput) -> RunnableMarbleWorld {
    let collapsed = collapse_input_to_world(input);
    let widgets = collapsed
        .placements
        .iter()
        .copied()
        .map(|(widget, index)| RunnableWidgetPlacement {
            widget,
            tile: collapsed.layout.tiles[index],
            location: MarbleTileLocation::new(index),
        })
        .collect();

    RunnableMarbleWorld {
        universe: input.universe,
        layout: collapsed.layout,
        widgets,
        singularities: input.singularities.clone(),
        contradictions: collapsed.contradictions,
        propagation_steps: collapsed.propagation_steps,
        gather_policy: gather_policy_from_conditions(&input.world.conditions),
    }
}

pub fn collapse_marble_world_with_all_widgets(width: usize) -> CollapsedMarbleWorld {
    collapse_marble_world(&MarbleWorld::with_all_widgets(width))
}

pub fn collapse_calculator_layout(width: usize) -> CalculatorLayout {
    let width = width.max(3);
    let mut cells = Vec::with_capacity(width);
    for index in 0..width {
        let options = if index == 0 {
            vec![CalculatorTile::Source]
        } else if index + 1 == width {
            vec![CalculatorTile::Sink]
        } else {
            COLLAPSE_PRIORITY.to_vec()
        };
        cells.push(WaveCell { options });
    }

    propagate_layout(&mut cells);
    while let Some(index) = next_uncollapsed_cell(&cells) {
        let choice = choose_tile_for(&cells, index).unwrap_or(COLLAPSE_PRIORITY[0]);
        cells[index].options.clear();
        cells[index].options.push(choice);
        propagate_layout(&mut cells);
    }

    CalculatorLayout {
        tiles: cells.into_iter().map(|cell| cell.options[0]).collect(),
    }
}

pub fn marble_calculator_example_visual() -> String {
    let layout = collapse_calculator_layout(6);
    let mut harness = MarbleCalculatorHarness::new(5, 8, CalculatorGatherPolicy::EmptyFill);
    let _ = harness.source_push_generated(0, 4);
    let _ = harness.source_push_generated(1, 2);
    let _ = harness.source_push_generated(3, 3);
    let _ = harness.source_push_generated(4, 1);

    let mut out = String::new();
    let _ = writeln!(out, "layout");
    let _ = writeln!(out, "{}", layout.render());
    let _ = writeln!(out);
    let _ = writeln!(out, "wave 0 lanes");
    let _ = writeln!(out, "{}", harness.render_lanes());

    for step in 0..3 {
        match harness.gather_once() {
            Ok(Some(package)) => {
                let _ = writeln!(out);
                let _ = writeln!(out, "collapse {}", step);
                let _ = writeln!(out, "package: {}", package.render());
                let _ = writeln!(out, "remaining");
                let _ = writeln!(out, "{}", harness.render_lanes());
            }
            Ok(None) => {
                let _ = writeln!(out, "halt at collapse {}", step);
                break;
            }
            Err(_) => {
                let _ = writeln!(out, "error at collapse {}", step);
                break;
            }
        }
    }

    out.push('\n');
    out.push_str("released\n");
    out.push_str(&harness.released_visual());
    out
}

pub fn marble_widget_world_visual() -> String {
    collapse_marble_world_with_all_widgets(7).render()
}

pub fn marble_collapse_engine_visual() -> String {
    let description = collapser_world_description_for(&WorldDescription::with_all_widgets(
        MarbleUniverseId(11),
        7,
    ));
    let mut engine = MarbleCollapseEngine::new();
    match engine.collapse_description(description.clone()) {
        Ok(runtime) => {
            let mut out = String::new();
            let _ = writeln!(out, "world-description");
            let _ = writeln!(out, "universe={} width={}", description.universe.0, description.width);
            let _ = writeln!(out, "conditions={:?}", description.conditions);
            let _ = writeln!(out);
            let _ = writeln!(out, "collapsed-by={}", engine.name());
            let _ = writeln!(out, "{}", runtime.render());
            out
        }
        Err(error) => {
            let mut out = String::new();
            let _ = writeln!(out, "collapse-error: {:?}", error);
            out
        }
    }
}

pub fn marble_universe_flow_visual() -> String {
    let input = MarbleCollapseInput::with_all_widgets(MarbleUniverseId(1), 7);
    let collapsed = collapse_input_to_world(&input);
    let runnable = build_runnable_world(&input);
    let mut out = String::new();
    let _ = writeln!(out, "create-collapse-input");
    let _ = writeln!(
        out,
        "universe={} width={} tile_count={}",
        input.universe.0, input.world.width, input.world.tile_count
    );
    let _ = writeln!(
        out,
        "widgets={} singularities={}",
        input.world.widgets.len(),
        input.singularities.len()
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "collapse-world");
    let _ = writeln!(out, "{}", collapsed.render());
    let _ = writeln!(out, "runtime-world");
    let _ = writeln!(out, "{}", runnable.render());
    out
}

fn bind_widgets_to_tiles(
    tile_count: usize,
    widgets: &[ImportedWidget],
) -> Vec<ImportedWidgetBinding> {
    let mut bindings = Vec::with_capacity(widgets.len());
    for (index, widget) in widgets.iter().copied().enumerate() {
        let tile = if widget.kind == WidgetKind::Root {
            0
        } else {
            core::cmp::min(index, tile_count.saturating_sub(2))
        };
        bindings.push(ImportedWidgetBinding { widget, tile });
    }
    bindings
}

fn condition_influences(
    tile_count: usize,
    conditions: &[WorldCondition],
) -> Vec<MarbleInfluence> {
    let mut out = Vec::new();
    if tile_count >= 2 && conditions.contains(&WorldCondition::RequireBoxBeforeSink) {
        let before_sink = tile_count - 2;
        let sink = tile_count - 1;
        out.push(MarbleInfluence {
            from: before_sink,
            to: sink,
            allowed_pairs: vec![(CalculatorTile::BoxStore, CalculatorTile::Sink)],
        });
        out.push(MarbleInfluence {
            from: sink,
            to: before_sink,
            allowed_pairs: vec![(CalculatorTile::Sink, CalculatorTile::BoxStore)],
        });
    }
    out
}

fn singularity_stubs_for_world(world: &MarbleWorld) -> Vec<MarbleSingularityStub> {
    if world.tile_count < 2 {
        return Vec::new();
    }

    vec![
        MarbleSingularityStub {
            kind: MarbleSingularityKind::WhiteHole,
            location: MarbleTileLocation::new(0),
        },
        MarbleSingularityStub {
            kind: MarbleSingularityKind::BlackHole,
            location: MarbleTileLocation::new(world.tile_count - 1),
        },
    ]
}

fn singularity_name(kind: MarbleSingularityKind) -> &'static str {
    match kind {
        MarbleSingularityKind::BlackHole => "black-hole-stub",
        MarbleSingularityKind::WhiteHole => "white-hole-stub",
    }
}

fn linear_world_influences(tile_count: usize) -> Vec<MarbleInfluence> {
    let mut influences = Vec::with_capacity(tile_count.saturating_mul(2));
    for index in 0..tile_count.saturating_sub(1) {
        influences.push(MarbleInfluence {
            from: index,
            to: index + 1,
            allowed_pairs: allowed_forward_pairs(),
        });
        influences.push(MarbleInfluence {
            from: index + 1,
            to: index,
            allowed_pairs: allowed_reverse_pairs(),
        });
    }
    influences
}

fn solve_marble_world(world: &MarbleWorld) -> MarbleConstraintState {
    let mut state = MarbleConstraintState {
        domains: (0..world.tile_count)
            .map(|_| GraphWaveCell {
                options: all_calculator_tiles().to_vec(),
            })
            .collect(),
        contradictions: Vec::new(),
        propagation_steps: 0,
    };
    let mut queue = VecDeque::new();

    for constraint in unary_constraints_for_world(world) {
        let tile = constraint.tile;
        if apply_unary_constraint(&mut state, constraint) {
            queue.push_back(tile);
        }
    }

    propagate_constraints(&mut state, &world.influences, &mut queue);

    while state.contradictions.is_empty() {
        let Some(index) = next_uncollapsed_graph_cell(&state.domains) else {
            break;
        };
        let choice = choose_tile_for_graph(&state.domains[index]).unwrap_or(COLLAPSE_PRIORITY[0]);
        state.domains[index].options.clear();
        state.domains[index].options.push(choice);
        queue.push_back(index);
        propagate_constraints(&mut state, &world.influences, &mut queue);
    }

    state
}

fn unary_constraints_for_world(world: &MarbleWorld) -> Vec<MarbleUnaryConstraint> {
    let mut out = Vec::with_capacity(world.widget_bindings.len().saturating_add(2));
    out.push(MarbleUnaryConstraint {
        tile: 0,
        allowed: vec![CalculatorTile::Source],
    });
    out.push(MarbleUnaryConstraint {
        tile: world.tile_count.saturating_sub(1),
        allowed: vec![CalculatorTile::Sink],
    });

    for binding in world.widget_bindings.iter().copied() {
        out.push(MarbleUnaryConstraint {
            tile: binding.tile,
            allowed: allowed_tiles_for_widget(binding.widget),
        });
    }

    for condition in world.conditions.iter().copied() {
        match condition {
            WorldCondition::PinTile { tile, kind } => {
                out.push(MarbleUnaryConstraint {
                    tile,
                    allowed: vec![kind],
                });
            }
            WorldCondition::RequirePortalAt { tile } => {
                out.push(MarbleUnaryConstraint {
                    tile,
                    allowed: vec![CalculatorTile::Portal],
                });
            }
            WorldCondition::RequireBoxBeforeSink => {
                if world.tile_count >= 2 {
                    out.push(MarbleUnaryConstraint {
                        tile: world.tile_count - 2,
                        allowed: vec![CalculatorTile::BoxStore],
                    });
                }
            }
            WorldCondition::PreferFastPath => {
                if world.tile_count >= 3 {
                    out.push(MarbleUnaryConstraint {
                        tile: 1,
                        allowed: vec![CalculatorTile::Race],
                    });
                }
            }
            WorldCondition::StrictPackages | WorldCondition::AllowEmptyFill => {}
        }
    }

    out
}

fn apply_unary_constraint(
    state: &mut MarbleConstraintState,
    constraint: MarbleUnaryConstraint,
) -> bool {
    let Some(cell) = state.domains.get_mut(constraint.tile) else {
        return false;
    };

    let before = cell.options.len();
    cell.options
        .retain(|candidate| constraint.allowed.contains(candidate));
    if cell.options.is_empty() && !state.contradictions.contains(&constraint.tile) {
        state.contradictions.push(constraint.tile);
    }
    cell.options.len() != before
}

fn propagate_constraints(
    state: &mut MarbleConstraintState,
    influences: &[MarbleInfluence],
    queue: &mut VecDeque<usize>,
) {
    while let Some(tile) = queue.pop_front() {
        state.propagation_steps = state.propagation_steps.saturating_add(1);
        for influence in influences.iter().filter(|influence| influence.from == tile) {
            if revise_edge(state, influence) {
                queue.push_back(influence.to);
            }
        }
    }
}

fn revise_edge(state: &mut MarbleConstraintState, influence: &MarbleInfluence) -> bool {
    let from_domain = state.domains[influence.from].options.clone();
    let Some(target) = state.domains.get_mut(influence.to) else {
        return false;
    };

    let before = target.options.len();
    target.options.retain(|candidate_to| {
        from_domain.iter().copied().any(|candidate_from| {
            influence
                .allowed_pairs
                .iter()
                .copied()
                .any(|(from, to)| from == candidate_from && to == *candidate_to)
        })
    });

    if target.options.is_empty() && !state.contradictions.contains(&influence.to) {
        state.contradictions.push(influence.to);
    }

    target.options.len() != before
}

fn next_uncollapsed_graph_cell(cells: &[GraphWaveCell]) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for (index, cell) in cells.iter().enumerate() {
        let len = cell.options.len();
        if len <= 1 {
            continue;
        }
        if let Some((_, best_len)) = best {
            if len < best_len {
                best = Some((index, len));
            }
        } else {
            best = Some((index, len));
        }
    }
    best.map(|(index, _)| index)
}

fn choose_tile_for_graph(cell: &GraphWaveCell) -> Option<CalculatorTile> {
    COLLAPSE_PRIORITY
        .iter()
        .copied()
        .find(|candidate| cell.options.contains(candidate))
        .or_else(|| cell.options.first().copied())
}

fn gather_policy_from_conditions(conditions: &[WorldCondition]) -> CalculatorGatherPolicy {
    if conditions.contains(&WorldCondition::StrictPackages) {
        CalculatorGatherPolicy::Strict
    } else {
        CalculatorGatherPolicy::EmptyFill
    }
}

fn all_calculator_tiles() -> &'static [CalculatorTile] {
    &[
        CalculatorTile::Source,
        CalculatorTile::Race,
        CalculatorTile::Gather,
        CalculatorTile::Portal,
        CalculatorTile::BoxStore,
        CalculatorTile::Sink,
    ]
}

fn allowed_forward_pairs() -> Vec<(CalculatorTile, CalculatorTile)> {
    let mut pairs = Vec::new();
    for left in all_calculator_tiles().iter().copied() {
        for right in all_calculator_tiles().iter().copied() {
            if left.allows_next(right) {
                pairs.push((left, right));
            }
        }
    }
    pairs
}

fn allowed_reverse_pairs() -> Vec<(CalculatorTile, CalculatorTile)> {
    let mut pairs = Vec::new();
    for left in all_calculator_tiles().iter().copied() {
        for right in all_calculator_tiles().iter().copied() {
            if right.allows_next(left) {
                pairs.push((left, right));
            }
        }
    }
    pairs
}

fn allowed_tiles_for_widget(widget: ImportedWidget) -> Vec<CalculatorTile> {
    match widget.kind {
        WidgetKind::Root => vec![CalculatorTile::Source],
        WidgetKind::Div => vec![CalculatorTile::BoxStore, CalculatorTile::Gather],
        WidgetKind::Details => vec![CalculatorTile::Portal, CalculatorTile::BoxStore],
        WidgetKind::Button => vec![CalculatorTile::Race, CalculatorTile::Gather],
        WidgetKind::TextInput => vec![CalculatorTile::Gather, CalculatorTile::BoxStore],
    }
}

pub fn collapser_world_description_for(target: &WorldDescription) -> WorldDescription {
    let mut conditions = vec![
        WorldCondition::PreferFastPath,
        WorldCondition::RequireBoxBeforeSink,
        WorldCondition::AllowEmptyFill,
    ];
    if target.width >= 5 {
        conditions.push(WorldCondition::RequirePortalAt { tile: 2 });
    }

    WorldDescription {
        universe: target.universe,
        width: target.width.max(target.widgets.len().saturating_add(2)),
        widgets: import_all_widgets(),
        conditions,
    }
}

fn next_uncollapsed_cell(cells: &[WaveCell]) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for (index, cell) in cells.iter().enumerate() {
        let len = cell.options.len();
        if len <= 1 {
            continue;
        }
        if let Some((_, best_len)) = best {
            if len < best_len {
                best = Some((index, len));
            }
        } else {
            best = Some((index, len));
        }
    }
    best.map(|(index, _)| index)
}

fn choose_tile_for(cells: &[WaveCell], index: usize) -> Option<CalculatorTile> {
    COLLAPSE_PRIORITY.iter().copied().find(|candidate| {
        if !cells[index].options.contains(candidate) {
            return false;
        }

        let left_ok = if index == 0 {
            true
        } else {
            cells[index - 1]
                .options
                .iter()
                .copied()
                .any(|left| left.allows_next(*candidate))
        };
        let right_ok = if index + 1 == cells.len() {
            true
        } else {
            cells[index + 1]
                .options
                .iter()
                .copied()
                .any(|right| candidate.allows_next(right))
        };

        left_ok && right_ok
    })
}

fn propagate_layout(cells: &mut [WaveCell]) {
    loop {
        let mut changed = false;

        for index in 0..cells.len() {
            let left = if index == 0 {
                None
            } else {
                Some(cells[index - 1].options.clone())
            };
            let right = if index + 1 == cells.len() {
                None
            } else {
                Some(cells[index + 1].options.clone())
            };

            let before = cells[index].options.len();
            cells[index].options.retain(|candidate| {
                let left_ok = left.as_ref().is_none_or(|options| {
                    options
                        .iter()
                        .copied()
                        .any(|tile| tile.allows_next(*candidate))
                });
                let right_ok = right.as_ref().is_none_or(|options| {
                    options
                        .iter()
                        .copied()
                        .any(|tile| candidate.allows_next(tile))
                });
                left_ok && right_ok
            });

            if cells[index].options.is_empty() {
                cells[index].options.push(if index == 0 {
                    CalculatorTile::Source
                } else if index + 1 == cells.len() {
                    CalculatorTile::Sink
                } else {
                    COLLAPSE_PRIORITY[0]
                });
            }

            if cells[index].options.len() != before {
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_policy_halts_on_missing_lane() {
        let mut harness = MarbleCalculatorHarness::new(3, 4, CalculatorGatherPolicy::Strict);
        harness.source_push_generated(0, 1).unwrap();
        harness.source_push_generated(1, 1).unwrap();

        let package = harness.gather_once().unwrap();
        assert!(package.is_none());
    }

    #[test]
    fn empty_fill_policy_keeps_flowing() {
        let mut harness = MarbleCalculatorHarness::new(3, 4, CalculatorGatherPolicy::EmptyFill);
        harness.source_push_generated(0, 1).unwrap();
        harness.source_push_generated(2, 1).unwrap();

        let package = harness.gather_once().unwrap().unwrap();
        assert_eq!(package.width(), 3);
        assert_eq!(package.empty_lanes, 1);
        assert_eq!(package.sum, 3);
    }

    #[test]
    fn wave_collapse_anchors_source_and_sink() {
        let layout = collapse_calculator_layout(6);
        assert_eq!(layout.tiles.first(), Some(&CalculatorTile::Source));
        assert_eq!(layout.tiles.last(), Some(&CalculatorTile::Sink));
    }

    #[test]
    fn example_visual_mentions_layout_and_release() {
        let visual = marble_calculator_example_visual();
        assert!(visual.contains("layout"));
        assert!(visual.contains("collapse 0"));
        assert!(visual.contains("released"));
    }

    #[test]
    fn widget_world_imports_all_widget_kinds() {
        let world = MarbleWorld::with_all_widgets(7);
        assert_eq!(world.widgets.len(), 5);
        assert_eq!(world.tile_count, 7);
        assert_eq!(world.influences.len(), 12);
        assert!(
            world
                .widgets
                .iter()
                .any(|widget| widget.kind == WidgetKind::Button)
        );
        assert!(
            world
                .widgets
                .iter()
                .any(|widget| widget.kind == WidgetKind::TextInput)
        );
    }

    #[test]
    fn collapsed_widget_world_renders_imports() {
        let visual = marble_widget_world_visual();
        assert!(visual.contains("widget-imports"));
        assert!(visual.contains("button"));
        assert!(visual.contains("placements"));
        assert!(visual.contains("propagation-steps"));
    }

    #[test]
    fn collapsed_widget_world_tracks_graph_solver_state() {
        let collapsed = collapse_marble_world_with_all_widgets(7);
        assert!(collapsed.propagation_steps > 0);
        assert!(collapsed.contradictions.is_empty());
        assert_eq!(collapsed.placements.len(), 5);
    }

    #[test]
    fn collapse_input_builds_universe_and_singularities() {
        let input = MarbleCollapseInput::with_all_widgets(MarbleUniverseId(7), 7);
        assert_eq!(input.universe, MarbleUniverseId(7));
        assert_eq!(input.world.widgets.len(), 5);
        assert_eq!(input.singularities.len(), 2);
    }

    #[test]
    fn runnable_world_has_widget_locations() {
        let input = MarbleCollapseInput::with_all_widgets(MarbleUniverseId(3), 7);
        let runtime = build_runnable_world(&input);
        assert_eq!(runtime.universe, MarbleUniverseId(3));
        assert_eq!(runtime.widgets.len(), 5);
        assert!(
            runtime
                .widgets
                .iter()
                .any(|widget| widget.location.index == 0)
        );
        assert_eq!(runtime.singularities.len(), 2);
    }

    #[test]
    fn universe_flow_visual_shows_pipeline() {
        let visual = marble_universe_flow_visual();
        assert!(visual.contains("create-collapse-input"));
        assert!(visual.contains("collapse-world"));
        assert!(visual.contains("runtime-world"));
        assert!(visual.contains("black-hole-stub"));
        assert!(visual.contains("white-hole-stub"));
    }
}
