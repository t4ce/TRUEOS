use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use super::{CalculatorGatherPolicy, Marble, MarbleGadget, MarbleTransform, WidgetKind};

// Sidequest only: these collapse, waver, and etcher concepts are intentionally
// separate from the current calculator CCW path.

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
    PinTile { tile: usize, kind: CalculatorTile },
    RequirePortalAt { tile: usize },
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
    pub(crate) tiles: Vec<CalculatorTile>,
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

impl Marble for WorldDescription {
    fn kind(&self) -> &'static str {
        "world-description"
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
        compile_world_description(&WorldDescription::with_all_widgets(
            MarbleUniverseId(0),
            width,
        ))
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
        for widget in &self.widgets {
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

impl Marble for RunnableMarbleWorld {
    fn kind(&self) -> &'static str {
        "runnable-marble-world"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldMarble {
    pub universe: MarbleUniverseId,
    pub world: WorldDescription,
    pub white_hole: MarbleTileLocation,
}

impl Marble for WorldMarble {
    fn kind(&self) -> &'static str {
        "world-marble"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollapsedWorldMarble {
    pub universe: MarbleUniverseId,
    pub collapsed: CollapsedMarbleWorld,
    pub black_hole: MarbleTileLocation,
}

impl Marble for CollapsedWorldMarble {
    fn kind(&self) -> &'static str {
        "collapsed-world-marble"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarblePlacementRecord {
    pub universe: MarbleUniverseId,
    pub ingress: MarbleTileLocation,
    pub egress: MarbleTileLocation,
    pub memory_word_offset: usize,
    pub tile_count: usize,
}

impl MarblePlacementRecord {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "placement-universe={}", self.universe.0);
        let _ = writeln!(out, "ingress-white-hole={}", self.ingress.index);
        let _ = writeln!(out, "egress-black-hole={}", self.egress.index);
        let _ = writeln!(out, "memory-word-offset={}", self.memory_word_offset);
        let _ = writeln!(out, "tile-count={}", self.tile_count);
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarbleCollapseError {
    Contradiction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaverKind {
    Waver1,
    Waver2,
    Waver3,
}

impl WaverKind {
    pub const fn name(self) -> &'static str {
        match self {
            WaverKind::Waver1 => "waver1",
            WaverKind::Waver2 => "waver2",
            WaverKind::Waver3 => "waver3",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtcherKind {
    Etcher1,
    Etcher2,
    Etcher3,
}

impl EtcherKind {
    pub const fn name(self) -> &'static str {
        match self {
            EtcherKind::Etcher1 => "etcher1",
            EtcherKind::Etcher2 => "etcher2",
            EtcherKind::Etcher3 => "etcher3",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaverCollapsedProblem {
    pub waver: WaverKind,
    pub input: WorldMarble,
    pub output: CollapsedWorldMarble,
}

impl WaverCollapsedProblem {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "collapser={}", self.waver.name());
        let _ = writeln!(out, "input-kind={}", self.input.kind());
        let _ = writeln!(out, "problem-universe={}", self.input.universe.0);
        let _ = writeln!(out, "problem-width={}", self.input.world.width);
        let _ = writeln!(out, "input-white-hole={}", self.input.white_hole.index);
        let _ = writeln!(out, "output-kind={}", self.output.kind());
        let _ = writeln!(out, "output-black-hole={}", self.output.black_hole.index);
        let _ = writeln!(out, "{}", self.output.collapsed.render());
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EtchedWorld {
    pub etcher: EtcherKind,
    pub input: CollapsedWorldMarble,
    pub runnable: RunnableMarbleWorld,
    pub placement: MarblePlacementRecord,
}

impl EtchedWorld {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "etcher={}", self.etcher.name());
        let _ = writeln!(out, "input-kind={}", self.input.kind());
        let _ = writeln!(out, "etcher-white-hole={}", self.input.black_hole.index);
        let _ = writeln!(out, "{}", self.placement.render());
        let _ = writeln!(out, "{}", self.runnable.render());
        out
    }
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

#[derive(Debug, Clone, Copy)]
pub struct WorldCollapserWidget {
    kind: WaverKind,
    engine: MarbleCollapseEngine,
}

impl WorldCollapserWidget {
    pub const fn new(kind: WaverKind) -> Self {
        Self {
            kind,
            engine: MarbleCollapseEngine::new(),
        }
    }

    pub fn collapse_world(
        &mut self,
        description: WorldDescription,
    ) -> Result<CollapsedMarbleWorld, MarbleCollapseError> {
        let input = MarbleCollapseInput::from_description(&description);
        let collapsed = collapse_input_to_world(&input);
        if collapsed.contradictions.is_empty() {
            Ok(collapsed)
        } else {
            Err(MarbleCollapseError::Contradiction)
        }
    }

    pub fn run_world(
        &mut self,
        description: WorldDescription,
    ) -> Result<RunnableMarbleWorld, MarbleCollapseError> {
        self.engine.collapse_description(description)
    }

    pub fn place_world(
        &mut self,
        description: WorldDescription,
    ) -> Result<MarblePlacementRecord, MarbleCollapseError> {
        let input = MarbleCollapseInput::from_description(&description);
        let runnable = build_runnable_world(&input);
        if !runnable.contradictions.is_empty() {
            return Err(MarbleCollapseError::Contradiction);
        }
        Ok(plan_world_placement(&input, &runnable))
    }

    pub fn collapse_problem(
        &mut self,
        problem: WorldDescription,
    ) -> Result<WaverCollapsedProblem, MarbleCollapseError> {
        let input = world_marble_from_description(problem.clone());
        let collapsed = self.collapse_world(problem)?;
        let universe = input.universe;
        let tile_count = input
            .world
            .width
            .max(input.world.widgets.len().saturating_add(2));
        Ok(WaverCollapsedProblem {
            waver: self.kind,
            input,
            output: collapsed_world_marble(universe, collapsed, tile_count),
        })
    }
}

impl MarbleGadget for WorldCollapserWidget {
    fn name(&self) -> &'static str {
        self.kind.name()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Waver1 {
    inner: WorldCollapserWidget,
}

impl Waver1 {
    pub const fn new() -> Self {
        Self {
            inner: WorldCollapserWidget::new(WaverKind::Waver1),
        }
    }

    pub fn collapse_world(
        &mut self,
        description: WorldDescription,
    ) -> Result<CollapsedMarbleWorld, MarbleCollapseError> {
        self.inner.collapse_world(description)
    }

    pub fn run_world(
        &mut self,
        description: WorldDescription,
    ) -> Result<RunnableMarbleWorld, MarbleCollapseError> {
        self.inner.run_world(description)
    }

    pub fn place_world(
        &mut self,
        description: WorldDescription,
    ) -> Result<MarblePlacementRecord, MarbleCollapseError> {
        self.inner.place_world(description)
    }
}

impl Default for Waver1 {
    fn default() -> Self {
        Self::new()
    }
}

impl MarbleGadget for Waver1 {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Waver2 {
    inner: WorldCollapserWidget,
}

impl Waver2 {
    pub const fn new() -> Self {
        Self {
            inner: WorldCollapserWidget::new(WaverKind::Waver2),
        }
    }

    pub fn collapse_world(
        &mut self,
        description: WorldDescription,
    ) -> Result<CollapsedMarbleWorld, MarbleCollapseError> {
        self.inner.collapse_world(description)
    }

    pub fn run_world(
        &mut self,
        description: WorldDescription,
    ) -> Result<RunnableMarbleWorld, MarbleCollapseError> {
        self.inner.run_world(description)
    }

    pub fn place_world(
        &mut self,
        description: WorldDescription,
    ) -> Result<MarblePlacementRecord, MarbleCollapseError> {
        self.inner.place_world(description)
    }
}

impl Default for Waver2 {
    fn default() -> Self {
        Self::new()
    }
}

impl MarbleGadget for Waver2 {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Waver3 {
    inner: WorldCollapserWidget,
}

impl Waver3 {
    pub const fn new() -> Self {
        Self {
            inner: WorldCollapserWidget::new(WaverKind::Waver3),
        }
    }

    pub fn collapse_world(
        &mut self,
        description: WorldDescription,
    ) -> Result<CollapsedMarbleWorld, MarbleCollapseError> {
        self.inner.collapse_world(description)
    }

    pub fn run_world(
        &mut self,
        description: WorldDescription,
    ) -> Result<RunnableMarbleWorld, MarbleCollapseError> {
        self.inner.run_world(description)
    }

    pub fn place_world(
        &mut self,
        description: WorldDescription,
    ) -> Result<MarblePlacementRecord, MarbleCollapseError> {
        self.inner.place_world(description)
    }
}

impl Default for Waver3 {
    fn default() -> Self {
        Self::new()
    }
}

impl MarbleGadget for Waver3 {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
}

pub fn initialized_world_collapsers() -> Vec<WorldCollapserWidget> {
    vec![
        WorldCollapserWidget::new(WaverKind::Waver1),
        WorldCollapserWidget::new(WaverKind::Waver2),
        WorldCollapserWidget::new(WaverKind::Waver3),
    ]
}

pub fn collapse_problem_worlds(
    problem_worlds: &[WorldDescription],
) -> Vec<Result<WaverCollapsedProblem, MarbleCollapseError>> {
    let mut wavers = initialized_world_collapsers();
    let mut out = Vec::with_capacity(problem_worlds.len());

    for (index, problem) in problem_worlds.iter().cloned().enumerate() {
        let slot = index % wavers.len();
        out.push(wavers[slot].collapse_problem(problem));
    }

    out
}

#[derive(Debug, Clone, Copy)]
pub struct WorldEtcherWidget {
    kind: EtcherKind,
}

impl WorldEtcherWidget {
    pub const fn new(kind: EtcherKind) -> Self {
        Self { kind }
    }

    pub fn etch(
        &mut self,
        input: CollapsedWorldMarble,
    ) -> Result<EtchedWorld, MarbleCollapseError> {
        if !input.collapsed.contradictions.is_empty() {
            return Err(MarbleCollapseError::Contradiction);
        }

        let collapse_input = MarbleCollapseInput {
            universe: input.universe,
            world: MarbleWorld {
                width: input.collapsed.layout.tiles.len(),
                tile_count: input.collapsed.layout.tiles.len(),
                widgets: input.collapsed.widgets.clone(),
                widget_bindings: input
                    .collapsed
                    .placements
                    .iter()
                    .copied()
                    .map(|(widget, tile)| ImportedWidgetBinding { widget, tile })
                    .collect(),
                influences: linear_world_influences(input.collapsed.layout.tiles.len()),
                conditions: input.collapsed.conditions.clone(),
            },
            singularities: vec![
                MarbleSingularityStub {
                    kind: MarbleSingularityKind::WhiteHole,
                    location: MarbleTileLocation::new(0),
                },
                MarbleSingularityStub {
                    kind: MarbleSingularityKind::BlackHole,
                    location: input.black_hole,
                },
            ],
        };

        let runnable = RunnableMarbleWorld {
            universe: input.universe,
            layout: input.collapsed.layout.clone(),
            widgets: input
                .collapsed
                .placements
                .iter()
                .copied()
                .map(|(widget, index)| RunnableWidgetPlacement {
                    widget,
                    tile: input.collapsed.layout.tiles[index],
                    location: MarbleTileLocation::new(index),
                })
                .collect(),
            singularities: collapse_input.singularities.clone(),
            contradictions: input.collapsed.contradictions.clone(),
            propagation_steps: input.collapsed.propagation_steps,
            gather_policy: gather_policy_from_conditions(&input.collapsed.conditions),
        };

        let placement = plan_world_placement(&collapse_input, &runnable);

        Ok(EtchedWorld {
            etcher: self.kind,
            input,
            runnable,
            placement,
        })
    }
}

impl MarbleGadget for WorldEtcherWidget {
    fn name(&self) -> &'static str {
        self.kind.name()
    }
}

pub fn initialized_world_etchers() -> Vec<WorldEtcherWidget> {
    vec![
        WorldEtcherWidget::new(EtcherKind::Etcher1),
        WorldEtcherWidget::new(EtcherKind::Etcher2),
        WorldEtcherWidget::new(EtcherKind::Etcher3),
    ]
}

pub fn etch_collapsed_worlds(
    collapsed_worlds: &[CollapsedWorldMarble],
) -> Vec<Result<EtchedWorld, MarbleCollapseError>> {
    let mut etchers = initialized_world_etchers();
    let mut out = Vec::with_capacity(collapsed_worlds.len());

    for (index, input) in collapsed_worlds.iter().cloned().enumerate() {
        let slot = index % etchers.len();
        out.push(etchers[slot].etch(input));
    }

    out
}

pub fn collapse_and_etch_problem_worlds(
    problem_worlds: &[WorldDescription],
) -> Vec<Result<EtchedWorld, MarbleCollapseError>> {
    let collapsed = collapse_problem_worlds(problem_worlds);
    let mut collapsed_marbles = Vec::new();
    for result in collapsed {
        match result {
            Ok(problem) => collapsed_marbles.push(problem.output),
            Err(error) => return vec![Err(error)],
        }
    }
    etch_collapsed_worlds(&collapsed_marbles)
}

impl MarbleTransform<WorldDescription, RunnableMarbleWorld> for MarbleCollapseEngine {
    type Error = MarbleCollapseError;

    fn transform(
        &mut self,
        description: WorldDescription,
    ) -> Result<RunnableMarbleWorld, Self::Error> {
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
    let tile_count = description
        .width
        .max(widgets.len().saturating_add(2))
        .max(3);
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

pub fn world_marble_from_description(description: WorldDescription) -> WorldMarble {
    WorldMarble {
        universe: description.universe,
        world: description,
        white_hole: MarbleTileLocation::new(0),
    }
}

pub fn collapsed_world_marble(
    universe: MarbleUniverseId,
    collapsed: CollapsedMarbleWorld,
    tile_count: usize,
) -> CollapsedWorldMarble {
    CollapsedWorldMarble {
        universe,
        collapsed,
        black_hole: MarbleTileLocation::new(tile_count.saturating_sub(1)),
    }
}

pub fn plan_world_placement(
    input: &MarbleCollapseInput,
    runnable: &RunnableMarbleWorld,
) -> MarblePlacementRecord {
    let ingress = input
        .singularities
        .iter()
        .find(|stub| stub.kind == MarbleSingularityKind::WhiteHole)
        .map(|stub| stub.location)
        .unwrap_or(MarbleTileLocation::new(0));
    let egress = input
        .singularities
        .iter()
        .find(|stub| stub.kind == MarbleSingularityKind::BlackHole)
        .map(|stub| stub.location)
        .unwrap_or(MarbleTileLocation::new(
            runnable.layout.tiles.len().saturating_sub(1),
        ));

    MarblePlacementRecord {
        universe: runnable.universe,
        ingress,
        egress,
        memory_word_offset: placement_offset_for_universe(
            runnable.universe,
            runnable.layout.tiles.len(),
        ),
        tile_count: runnable.layout.tiles.len(),
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
            let _ = writeln!(
                out,
                "universe={} width={}",
                description.universe.0, description.width
            );
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

pub fn marble_waver1_visual() -> String {
    let description = WorldDescription::with_all_widgets(MarbleUniverseId(21), 7);
    let mut waver = Waver1::new();
    let collapsed = waver.collapse_world(description.clone());
    let placement = waver.place_world(description);

    let mut out = String::new();
    let _ = writeln!(out, "waver1");
    match collapsed {
        Ok(collapsed) => {
            let _ = writeln!(out, "stage=collapsed-world");
            let _ = writeln!(out, "{}", collapsed.render());
        }
        Err(error) => {
            let _ = writeln!(out, "collapse-error: {:?}", error);
        }
    }
    match placement {
        Ok(placement) => {
            let _ = writeln!(out, "stage=placement");
            let _ = writeln!(out, "{}", placement.render());
        }
        Err(error) => {
            let _ = writeln!(out, "placement-error: {:?}", error);
        }
    }
    out
}

pub fn marble_waver_cluster_visual() -> String {
    let problems = vec![
        WorldDescription::with_all_widgets(MarbleUniverseId(31), 7),
        collapser_world_description_for(&WorldDescription::with_all_widgets(
            MarbleUniverseId(32),
            8,
        )),
        WorldDescription {
            universe: MarbleUniverseId(33),
            width: 9,
            widgets: import_all_widgets(),
            conditions: vec![
                WorldCondition::PreferFastPath,
                WorldCondition::RequirePortalAt { tile: 2 },
                WorldCondition::AllowEmptyFill,
            ],
        },
    ];

    let collapsed = collapse_problem_worlds(&problems);
    let mut out = String::new();
    let _ = writeln!(out, "waver-cluster");
    for result in collapsed {
        match result {
            Ok(problem) => {
                let _ = writeln!(out, "{}", problem.render());
            }
            Err(error) => {
                let _ = writeln!(out, "collapse-error: {:?}", error);
            }
        }
    }
    out
}

pub fn marble_etcher_cluster_visual() -> String {
    let problems = vec![
        WorldDescription::with_all_widgets(MarbleUniverseId(51), 7),
        WorldDescription::with_all_widgets(MarbleUniverseId(52), 8),
        collapser_world_description_for(&WorldDescription::with_all_widgets(
            MarbleUniverseId(53),
            9,
        )),
    ];

    let etched = collapse_and_etch_problem_worlds(&problems);
    let mut out = String::new();
    let _ = writeln!(out, "etcher-cluster");
    for result in etched {
        match result {
            Ok(etched) => {
                let _ = writeln!(out, "{}", etched.render());
            }
            Err(error) => {
                let _ = writeln!(out, "etch-error: {:?}", error);
            }
        }
    }
    out
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

fn condition_influences(tile_count: usize, conditions: &[WorldCondition]) -> Vec<MarbleInfluence> {
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

fn placement_offset_for_universe(universe: MarbleUniverseId, tile_count: usize) -> usize {
    ((universe.0 as usize) << 4).saturating_add(tile_count)
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
    fn wave_collapse_anchors_source_and_sink() {
        let layout = collapse_calculator_layout(6);
        assert_eq!(layout.tiles.first(), Some(&CalculatorTile::Source));
        assert_eq!(layout.tiles.last(), Some(&CalculatorTile::Sink));
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

    #[test]
    fn world_conditions_pin_tiles_and_box_before_sink() {
        let description = WorldDescription {
            universe: MarbleUniverseId(9),
            width: 7,
            widgets: import_all_widgets(),
            conditions: vec![
                WorldCondition::PinTile {
                    tile: 2,
                    kind: CalculatorTile::Portal,
                },
                WorldCondition::RequireBoxBeforeSink,
            ],
        };

        let world = compile_world_description(&description);
        let collapsed = collapse_marble_world(&world);

        assert_eq!(collapsed.layout.tiles[2], CalculatorTile::Portal);
        assert_eq!(
            collapsed.layout.tiles[world.tile_count - 2],
            CalculatorTile::BoxStore
        );
        assert!(collapsed.contradictions.is_empty());
    }

    #[test]
    fn strict_packages_condition_reaches_runnable_world() {
        let description = WorldDescription {
            universe: MarbleUniverseId(12),
            width: 7,
            widgets: import_all_widgets(),
            conditions: vec![WorldCondition::StrictPackages],
        };

        let input = MarbleCollapseInput::from_description(&description);
        let runnable = build_runnable_world(&input);

        assert_eq!(runnable.gather_policy, CalculatorGatherPolicy::Strict);
    }

    #[test]
    fn collapse_engine_turns_description_into_runnable_world() {
        let target = WorldDescription::with_all_widgets(MarbleUniverseId(17), 7);
        let description = collapser_world_description_for(&target);
        let mut engine = MarbleCollapseEngine::new();

        let runnable = engine.collapse_description(description.clone()).unwrap();

        assert_eq!(runnable.universe, description.universe);
        assert!(runnable.contradictions.is_empty());
        assert!(marble_collapse_engine_visual().contains("collapsed-by=marble-collapse-engine"));
    }

    #[test]
    fn waver1_places_black_hole_output_into_memory_record() {
        let description = WorldDescription::with_all_widgets(MarbleUniverseId(23), 7);
        let mut waver = Waver1::new();

        let placement = waver.place_world(description).unwrap();

        assert_eq!(placement.ingress.index, 0);
        assert_eq!(placement.egress.index, 6);
        assert!(placement.memory_word_offset > placement.tile_count);
        assert!(marble_waver1_visual().contains("stage=placement"));
    }

    #[test]
    fn initialized_world_collapsers_expose_waver_names() {
        let collapser_names: Vec<&'static str> = initialized_world_collapsers()
            .iter()
            .map(MarbleGadget::name)
            .collect();

        assert_eq!(collapser_names, vec!["waver1", "waver2", "waver3"]);
    }

    #[test]
    fn multiple_problem_worlds_can_be_collapsed_by_multiple_wavers() {
        let problems = vec![
            WorldDescription::with_all_widgets(MarbleUniverseId(41), 7),
            WorldDescription::with_all_widgets(MarbleUniverseId(42), 8),
            WorldDescription::with_all_widgets(MarbleUniverseId(43), 9),
        ];

        let collapsed = collapse_problem_worlds(&problems);

        assert_eq!(collapsed.len(), 3);
        assert_eq!(collapsed[0].as_ref().unwrap().waver, WaverKind::Waver1);
        assert_eq!(collapsed[1].as_ref().unwrap().waver, WaverKind::Waver2);
        assert_eq!(collapsed[2].as_ref().unwrap().waver, WaverKind::Waver3);
        assert!(marble_waver_cluster_visual().contains("collapser=waver2"));
    }

    #[test]
    fn world_transfer_uses_special_world_marbles() {
        let description = WorldDescription::with_all_widgets(MarbleUniverseId(61), 7);
        let world_marble = world_marble_from_description(description.clone());
        let collapsed = collapse_marble_world(&compile_world_description(&description));
        let collapsed_marble = collapsed_world_marble(description.universe, collapsed, 7);

        assert_eq!(world_marble.kind(), "world-marble");
        assert_eq!(world_marble.white_hole.index, 0);
        assert_eq!(collapsed_marble.kind(), "collapsed-world-marble");
        assert_eq!(collapsed_marble.black_hole.index, 6);
    }

    #[test]
    fn etchers_place_collapsed_worlds_after_wavers() {
        let problems = vec![
            WorldDescription::with_all_widgets(MarbleUniverseId(71), 7),
            WorldDescription::with_all_widgets(MarbleUniverseId(72), 8),
            WorldDescription::with_all_widgets(MarbleUniverseId(73), 9),
        ];

        let etched = collapse_and_etch_problem_worlds(&problems);

        assert_eq!(etched.len(), 3);
        assert_eq!(etched[0].as_ref().unwrap().etcher, EtcherKind::Etcher1);
        assert_eq!(etched[1].as_ref().unwrap().etcher, EtcherKind::Etcher2);
        assert_eq!(etched[2].as_ref().unwrap().etcher, EtcherKind::Etcher3);
        assert!(marble_etcher_cluster_visual().contains("etcher=etcher2"));
    }
}
