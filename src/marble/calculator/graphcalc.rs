use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;

use super::sidequest::{
    CalculatorGatherPolicy, CalculatorLayout, CalculatorTile, CollapsedMarbleWorld,
    CollapsedWorldMarble, EtchedWorld, ImportedWidget, MarblePlacementRecord, MarbleSingularityKind,
    MarbleSingularityStub, MarbleTileLocation, MarbleUniverseId, RunnableMarbleWorld,
    RunnableWidgetPlacement, collapsed_world_marble, initialized_world_etchers,
};
use super::{Marble, MarbleGadget, WidgetKind};

#[inline(always)]
fn raw_port_write_bytes(bytes: &[u8]) {
    for &byte in bytes {
        unsafe { crate::portio::outb(0xE9, byte) };
    }
}

#[inline(always)]
fn raw_port_repeat(byte: u8, count: usize) {
    for _ in 0..count {
        unsafe { crate::portio::outb(0xE9, byte) };
    }
}

pub const MARBLE_COLOR_WORDS_100: [&str; 100] = [
    "red", "green", "blue", "purble", "red", "green", "blue", "purble", "red", "green",
    "blue", "purble", "red", "green", "blue", "purble", "red", "green", "blue", "purble",
    "red", "green", "blue", "purble", "red", "green", "blue", "purble", "red", "green",
    "blue", "purble", "red", "green", "blue", "purble", "red", "green", "blue", "purble",
    "red", "green", "blue", "purble", "red", "green", "blue", "purble", "red", "green",
    "blue", "purble", "red", "green", "blue", "purble", "red", "green", "blue", "purble",
    "red", "green", "blue", "purble", "red", "green", "blue", "purble", "red", "green",
    "blue", "purble", "red", "green", "blue", "purble", "red", "green", "blue", "purble",
    "red", "green", "blue", "purble", "red", "green", "blue", "purble", "red", "green",
    "blue", "purble", "red", "green", "blue", "purble", "red", "green", "blue", "purble",
];

#[inline(always)]
fn marble_color_word(index: usize) -> &'static str {
    MARBLE_COLOR_WORDS_100[index % MARBLE_COLOR_WORDS_100.len()]
}

#[inline(always)]
fn raw_port_log_edge_ordered_colors(instance: &GraphColoringProblemInstance, colors: &[u8]) {
    for &(left, right) in &instance.edges {
        raw_port_write_bytes(marble_color_word(colors[left] as usize).as_bytes());
        raw_port_write_bytes(b", ");
        raw_port_write_bytes(marble_color_word(colors[right] as usize).as_bytes());
        raw_port_write_bytes(b"; ");
    }
    raw_port_write_bytes(b"\n");
}

#[inline(always)]
fn raw_port_log_vertex_marbles(vertex_count: usize) {
    raw_port_repeat(b'O', vertex_count);
    unsafe {
        crate::portio::outb(0xE9, b'\n');
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiverMarble {
    // Control marble announces how many payload marbles must follow in the same serial stream.
    ControlN(usize),
    PaletteSize(usize),
    Edge { left: usize, right: usize },
}

impl Marble for RiverMarble {
    fn kind(&self) -> &'static str {
        match self {
            RiverMarble::ControlN(_) => "river-control-marble",
            RiverMarble::PaletteSize(_) => "river-palette-size-marble",
            RiverMarble::Edge { .. } => "river-edge-marble",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntakeState {
    WaitingControl,
    FillingPayload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialProblemConveyor {
    pub announced_payload_count: usize,
    pub payload: Vec<RiverMarble>,
}

impl SerialProblemConveyor {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "conveyor-announced-n={}", self.announced_payload_count);
        let _ = writeln!(out, "conveyor-payload-len={}", self.payload.len());
        for (index, marble) in self.payload.iter().enumerate() {
            let _ = writeln!(out, "  {:03}: {:?}", index, marble);
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct WhiteHoleRiverIntakeWidget {
    state: IntakeState,
    announced_payload_count: usize,
    remaining_payload: usize,
    payload: Vec<RiverMarble>,
}

impl WhiteHoleRiverIntakeWidget {
    pub const fn new() -> Self {
        Self {
            state: IntakeState::WaitingControl,
            announced_payload_count: 0,
            remaining_payload: 0,
            payload: Vec::new(),
        }
    }

    pub fn push_serial(&mut self, marble: RiverMarble) -> Result<(), IntakeError> {
        match (self.state, marble) {
            (IntakeState::WaitingControl, RiverMarble::ControlN(n)) => {
                if n == 0 {
                    return Err(IntakeError::ControlCountZero);
                }
                self.announced_payload_count = n;
                self.remaining_payload = n;
                self.payload.clear();
                self.payload.reserve(n);
                self.state = IntakeState::FillingPayload;
                Ok(())
            }
            (IntakeState::WaitingControl, _) => Err(IntakeError::ExpectedControlMarble),
            (IntakeState::FillingPayload, RiverMarble::ControlN(_)) => {
                Err(IntakeError::UnexpectedControlMarble)
            }
            (IntakeState::FillingPayload, payload_marble) => {
                if self.remaining_payload == 0 {
                    return Err(IntakeError::PayloadOverflow);
                }
                self.payload.push(payload_marble);
                self.remaining_payload -= 1;
                Ok(())
            }
        }
    }

    pub fn is_full(&self) -> bool {
        matches!(self.state, IntakeState::FillingPayload) && self.remaining_payload == 0
    }

    pub fn take_full_conveyor(&mut self) -> Result<SerialProblemConveyor, IntakeError> {
        if !self.is_full() {
            return Err(IntakeError::ConveyorNotFull);
        }

        let conveyor = SerialProblemConveyor {
            announced_payload_count: self.announced_payload_count,
            payload: self.payload.clone(),
        };

        self.state = IntakeState::WaitingControl;
        self.announced_payload_count = 0;
        self.remaining_payload = 0;
        self.payload.clear();

        Ok(conveyor)
    }
}

impl Default for WhiteHoleRiverIntakeWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl MarbleGadget for WhiteHoleRiverIntakeWidget {
    fn name(&self) -> &'static str {
        "white-hole-river-intake"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntakeError {
    ExpectedControlMarble,
    UnexpectedControlMarble,
    ControlCountZero,
    PayloadOverflow,
    ConveyorNotFull,
    MissingPaletteSize,
    DuplicatePaletteSize,
    MissingEdges,
    InvalidPaletteSize,
    InvalidEdge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphColoringProblemInstance {
    pub universe: MarbleUniverseId,
    pub vertex_count: usize,
    pub palette_size: usize,
    pub edges: Vec<(usize, usize)>,
}

impl GraphColoringProblemInstance {
    pub fn from_conveyor(
        universe: MarbleUniverseId,
        conveyor: SerialProblemConveyor,
    ) -> Result<Self, IntakeError> {
        let mut palette_size = None;
        let mut edges = Vec::new();

        for marble in conveyor.payload {
            match marble {
                RiverMarble::PaletteSize(n) => {
                    if palette_size.is_some() {
                        return Err(IntakeError::DuplicatePaletteSize);
                    }
                    if n == 0 {
                        return Err(IntakeError::InvalidPaletteSize);
                    }
                    palette_size = Some(n);
                }
                RiverMarble::Edge { left, right } => edges.push((left, right)),
                RiverMarble::ControlN(_) => return Err(IntakeError::UnexpectedControlMarble),
            }
        }

        let palette_size = palette_size.ok_or(IntakeError::MissingPaletteSize)?;
        let mut vertex_count = 0usize;
        let mut saw_edge = false;

        for &(left, right) in &edges {
            saw_edge = true;
            let max_endpoint = core::cmp::max(left, right);
            vertex_count = core::cmp::max(vertex_count, max_endpoint.saturating_add(1));
        }

        if !saw_edge {
            return Err(IntakeError::MissingEdges);
        }

        for &(left, right) in &edges {
            if left >= vertex_count || right >= vertex_count || left == right {
                return Err(IntakeError::InvalidEdge);
            }
        }

        Ok(Self {
            universe,
            vertex_count,
            palette_size,
            edges,
        })
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "problem-kind=graph-coloring");
        let _ = writeln!(out, "universe={}", self.universe.0);
        let _ = writeln!(out, "vertices={}", self.vertex_count);
        let _ = writeln!(out, "palette={}", self.palette_size);
        let _ = writeln!(out, "edges={}", self.edges.len());
        for &(left, right) in &self.edges {
            let _ = writeln!(out, "  edge({}, {})", left, right);
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphWidgetRole {
    IngressWhiteHole,
    RiverConveyor,
    VertexColorLatch,
    EdgeConstraintGate,
    WitnessBlackHole,
}

impl GraphWidgetRole {
    pub const fn name(self) -> &'static str {
        match self {
            GraphWidgetRole::IngressWhiteHole => "ingress-white-hole",
            GraphWidgetRole::RiverConveyor => "river-conveyor",
            GraphWidgetRole::VertexColorLatch => "vertex-color-latch",
            GraphWidgetRole::EdgeConstraintGate => "edge-constraint-gate",
            GraphWidgetRole::WitnessBlackHole => "witness-black-hole",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManualWidgetPlacement {
    pub role: GraphWidgetRole,
    pub location: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualGraphColoringWorld {
    pub instance: GraphColoringProblemInstance,
    pub colors: Vec<u8>,
    pub widgets: Vec<ManualWidgetPlacement>,
    pub collapsed: CollapsedMarbleWorld,
    pub etched: EtchedWorld,
}

impl ManualGraphColoringWorld {
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&self.instance.render());
        let _ = writeln!(out, "manual-widgets");
        for placement in self.widgets.iter().copied() {
            let _ = writeln!(out, "  {} @ {}", placement.role.name(), placement.location);
        }
        let _ = writeln!(out, "color-assignment={:?}", self.colors);
        let _ = writeln!(out);
        out.push_str(&self.collapsed.render());
        let _ = writeln!(out);
        out.push_str(&self.etched.render());
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualPlacementError {
    WrongColorCount,
    ColorOutOfPalette,
    ViolatesEdgeConstraint,
    EtcherRejected,
}

pub fn manual_place_graph_coloring_world(
    instance: GraphColoringProblemInstance,
    colors: &[u8],
) -> Result<ManualGraphColoringWorld, ManualPlacementError> {
    if colors.len() != instance.vertex_count {
        return Err(ManualPlacementError::WrongColorCount);
    }
    if colors
        .iter()
        .copied()
        .any(|color| color as usize >= instance.palette_size)
    {
        return Err(ManualPlacementError::ColorOutOfPalette);
    }

    for &(left, right) in &instance.edges {
        if colors[left] == colors[right] {
            return Err(ManualPlacementError::ViolatesEdgeConstraint);
        }
    }

    let mut tiles = Vec::with_capacity(instance.vertex_count.saturating_add(2));
    tiles.push(CalculatorTile::Source);
    for color in colors.iter().copied() {
        tiles.push(color_to_tile(color));
    }
    tiles.push(CalculatorTile::Sink);

    let mut imported_widgets = Vec::with_capacity(instance.vertex_count.saturating_add(1));
    imported_widgets.push(ImportedWidget {
        kind: WidgetKind::Root,
        preferred_tile: CalculatorTile::Source,
    });

    let mut widget_bindings = Vec::with_capacity(instance.vertex_count.saturating_add(1));
    widget_bindings.push((imported_widgets[0], 0usize));

    for (index, color) in colors.iter().copied().enumerate() {
        let widget = ImportedWidget {
            kind: WidgetKind::Tag,
            preferred_tile: color_to_tile(color),
        };
        imported_widgets.push(widget);
        widget_bindings.push((widget, index + 1));
    }

    let collapsed = CollapsedMarbleWorld {
        layout: CalculatorLayout { tiles: tiles.clone() },
        widgets: imported_widgets,
        placements: widget_bindings,
        contradictions: Vec::new(),
        propagation_steps: 0,
        conditions: Vec::new(),
    };

    let collapsed_marble: CollapsedWorldMarble =
        collapsed_world_marble(instance.universe, collapsed.clone(), tiles.len());

    let mut etchers = initialized_world_etchers();
    let etched = etchers[0]
        .etch(collapsed_marble)
        .map_err(|_| ManualPlacementError::EtcherRejected)?;

    let mut widgets = Vec::with_capacity(instance.vertex_count.saturating_add(4));
    widgets.push(ManualWidgetPlacement {
        role: GraphWidgetRole::IngressWhiteHole,
        location: 0,
    });
    widgets.push(ManualWidgetPlacement {
        role: GraphWidgetRole::RiverConveyor,
        location: 1,
    });
    for vertex in 0..instance.vertex_count {
        widgets.push(ManualWidgetPlacement {
            role: GraphWidgetRole::VertexColorLatch,
            location: vertex + 1,
        });
    }
    widgets.push(ManualWidgetPlacement {
        role: GraphWidgetRole::EdgeConstraintGate,
        location: instance.vertex_count,
    });
    widgets.push(ManualWidgetPlacement {
        role: GraphWidgetRole::WitnessBlackHole,
        location: instance.vertex_count + 1,
    });

    Ok(ManualGraphColoringWorld {
        instance,
        colors: colors.to_vec(),
        widgets,
        collapsed,
        etched,
    })
}

fn color_to_tile(color: u8) -> CalculatorTile {
    match color {
        0 => CalculatorTile::Race,
        1 => CalculatorTile::Gather,
        2 => CalculatorTile::Portal,
        _ => CalculatorTile::BoxStore,
    }
}

pub fn manual_graph_coloring_pipeline_visual() -> String {
    let mut intake = WhiteHoleRiverIntakeWidget::new();
    intake.push_serial(RiverMarble::ControlN(4)).unwrap();
    intake.push_serial(RiverMarble::PaletteSize(3)).unwrap();
    intake.push_serial(RiverMarble::Edge { left: 0, right: 1 })
        .unwrap();
    intake.push_serial(RiverMarble::Edge { left: 1, right: 2 })
        .unwrap();
    intake.push_serial(RiverMarble::Edge { left: 2, right: 0 })
        .unwrap();

    let conveyor = intake.take_full_conveyor().unwrap();
    let instance = GraphColoringProblemInstance::from_conveyor(MarbleUniverseId(200), conveyor).unwrap();
    let world = manual_place_graph_coloring_world(instance, &[0, 1, 2]).unwrap();
    world.render()
}

pub fn to_serial_graph_coloring_stream(
    palette_size: usize,
    edges: &[(usize, usize)],
) -> Vec<RiverMarble> {
    let payload_count = edges.len().saturating_add(1);
    let mut stream = Vec::with_capacity(payload_count.saturating_add(1));
    stream.push(RiverMarble::ControlN(payload_count));
    stream.push(RiverMarble::PaletteSize(palette_size));
    for &(left, right) in edges {
        stream.push(RiverMarble::Edge { left, right });
    }
    stream
}

pub const PETERSEN_EDGES: [(usize, usize); 15] = [
    // Outer 5-cycle
    (0, 1),
    (1, 2),
    (2, 3),
    (3, 4),
    (4, 0),
    // Spokes
    (0, 5),
    (1, 6),
    (2, 7),
    (3, 8),
    (4, 9),
    // Inner star pentagram
    (5, 7),
    (7, 9),
    (9, 6),
    (6, 8),
    (8, 5),
];

pub fn petersen_serial_graph_coloring_stream(palette_size: usize) -> Vec<RiverMarble> {
    to_serial_graph_coloring_stream(palette_size, &PETERSEN_EDGES)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphPipelineError {
    Intake(IntakeError),
    Manual(ManualPlacementError),
    NoColoringFound,
    SearchSpaceTooLarge,
}

fn is_valid_coloring(instance: &GraphColoringProblemInstance, colors: &[u8]) -> bool {
    instance
        .edges
        .iter()
        .all(|&(left, right)| colors[left] != colors[right])
}

fn find_coloring_bruteforce(instance: &GraphColoringProblemInstance) -> Result<Vec<u8>, GraphPipelineError> {
    let n = instance.vertex_count;
    let k = instance.palette_size;

    let mut total: u128 = 1;
    for _ in 0..n {
        total = total
            .checked_mul(k as u128)
            .ok_or(GraphPipelineError::SearchSpaceTooLarge)?;
    }

    let mut colors = vec![0u8; n];
    for mut state in 0..total {
        for slot in &mut colors {
            *slot = (state % (k as u128)) as u8;
            state /= k as u128;
        }

        if is_valid_coloring(instance, &colors) {
            return Ok(colors.clone());
        }
    }

    Err(GraphPipelineError::NoColoringFound)
}

pub fn petersen_whitehole_to_world(
    universe: MarbleUniverseId,
) -> Result<ManualGraphColoringWorld, GraphPipelineError> {
    let mut intake = WhiteHoleRiverIntakeWidget::new();
    for marble in petersen_serial_graph_coloring_stream(3) {
        intake.push_serial(marble).map_err(GraphPipelineError::Intake)?;
    }

    let conveyor = intake
        .take_full_conveyor()
        .map_err(GraphPipelineError::Intake)?;
    let instance =
        GraphColoringProblemInstance::from_conveyor(universe, conveyor).map_err(GraphPipelineError::Intake)?;
    raw_port_log_vertex_marbles(instance.vertex_count);
    let colors = find_coloring_bruteforce(&instance)?;
    raw_port_log_edge_ordered_colors(&instance, &colors);

    let world = manual_place_graph_coloring_world(instance, &colors)
        .map_err(GraphPipelineError::Manual)?;
    Ok(world)
}

pub fn petersen_whitehole_to_world_visual() -> String {
    petersen_whitehole_to_world(MarbleUniverseId(304))
        .unwrap()
        .render()
}

pub fn manual_graph_coloring_placement_record(
    world: &ManualGraphColoringWorld,
) -> MarblePlacementRecord {
    world.etched.placement
}

pub fn manual_graph_coloring_runnable_world(
    world: &ManualGraphColoringWorld,
) -> RunnableMarbleWorld {
    world.etched.runnable.clone()
}

pub fn manual_graph_coloring_gather_policy(
    world: &ManualGraphColoringWorld,
) -> CalculatorGatherPolicy {
    world.etched.runnable.gather_policy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_intake_waits_for_control_then_fills_until_n() {
        let mut intake = WhiteHoleRiverIntakeWidget::new();
        assert!(matches!(
            intake.push_serial(RiverMarble::PaletteSize(3)),
            Err(IntakeError::ExpectedControlMarble)
        ));

        intake.push_serial(RiverMarble::ControlN(2)).unwrap();
        intake.push_serial(RiverMarble::PaletteSize(3)).unwrap();
        assert!(!intake.is_full());
        intake.push_serial(RiverMarble::Edge { left: 0, right: 1 })
            .unwrap();
        assert!(intake.is_full());
    }

    #[test]
    fn conveyor_decodes_problem_instance() {
        let mut intake = WhiteHoleRiverIntakeWidget::new();
        for marble in to_serial_graph_coloring_stream(3, &[(0, 1), (1, 2), (2, 3)]) {
            intake.push_serial(marble).unwrap();
        }

        let conveyor = intake.take_full_conveyor().unwrap();
        let instance = GraphColoringProblemInstance::from_conveyor(MarbleUniverseId(300), conveyor)
            .unwrap();

        assert_eq!(instance.vertex_count, 4);
        assert_eq!(instance.palette_size, 3);
        assert_eq!(instance.edges.len(), 3);
    }

    #[test]
    fn manual_placement_uses_etcher1_and_produces_placement() {
        let mut intake = WhiteHoleRiverIntakeWidget::new();
        for marble in to_serial_graph_coloring_stream(3, &[(0, 1), (1, 2), (2, 0)]) {
            intake.push_serial(marble).unwrap();
        }

        let instance = GraphColoringProblemInstance::from_conveyor(
            MarbleUniverseId(301),
            intake.take_full_conveyor().unwrap(),
        )
        .unwrap();
        let world = manual_place_graph_coloring_world(instance, &[0, 1, 2]).unwrap();

        assert_eq!(world.etched.etcher.name(), "etcher1");
        assert_eq!(world.etched.placement.ingress.index, 0);
        assert_eq!(world.etched.placement.egress.index, 4);
        assert_eq!(manual_graph_coloring_gather_policy(&world), CalculatorGatherPolicy::EmptyFill);
    }

    #[test]
    fn manual_placement_rejects_invalid_coloring() {
        let instance = GraphColoringProblemInstance {
            universe: MarbleUniverseId(302),
            vertex_count: 3,
            palette_size: 3,
            edges: vec![(0, 1), (1, 2), (2, 0)],
        };

        let result = manual_place_graph_coloring_world(instance, &[0, 1, 1]);
        assert_eq!(
            result.err(),
            Some(ManualPlacementError::ViolatesEdgeConstraint)
        );
    }

    #[test]
    fn visual_shows_pipeline_and_etched_world() {
        let rendered = manual_graph_coloring_pipeline_visual();
        assert!(rendered.contains("problem-kind=graph-coloring"));
        assert!(rendered.contains("manual-widgets"));
        assert!(rendered.contains("etcher=etcher1"));
    }

    #[test]
    fn petersen_graph_can_be_dropped_in_from_edges_only_stream() {
        let mut intake = WhiteHoleRiverIntakeWidget::new();
        for marble in petersen_serial_graph_coloring_stream(3) {
            intake.push_serial(marble).unwrap();
        }

        let conveyor = intake.take_full_conveyor().unwrap();
        let instance = GraphColoringProblemInstance::from_conveyor(MarbleUniverseId(303), conveyor)
            .unwrap();

        assert_eq!(instance.vertex_count, 10);
        assert_eq!(instance.edges.len(), 15);
        assert_eq!(instance.palette_size, 3);
    }

    #[test]
    fn petersen_whitehole_pipeline_places_world_with_etcher1() {
        let world = petersen_whitehole_to_world(MarbleUniverseId(305)).unwrap();
        assert_eq!(world.instance.vertex_count, 10);
        assert_eq!(world.instance.edges.len(), 15);
        assert_eq!(world.colors.len(), 10);
        assert!(is_valid_coloring(&world.instance, &world.colors));
        assert_eq!(world.etched.etcher.name(), "etcher1");
        assert!(petersen_whitehole_to_world_visual().contains("problem-kind=graph-coloring"));
    }
}
