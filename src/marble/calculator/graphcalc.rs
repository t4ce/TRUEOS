use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;

use super::sidequest::{
    CalculatorGatherPolicy, CalculatorLayout, CalculatorTile, CollapsedMarbleWorld,
    CollapsedWorldMarble, EtchedWorld, ImportedWidget, MarbleCollapseError,
    MarblePlacementRecord, MarbleSingularityKind, MarbleSingularityStub, MarbleTileLocation,
    MarbleUniverseId, RunnableMarbleWorld, RunnableWidgetPlacement, collapsed_world_marble,
    initialized_world_etchers,
};
use super::{Marble, MarbleGadget, WidgetKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphProblemKind {
    VertexCover,
    IndependentSet,
    GraphColoring,
}

impl GraphProblemKind {
    pub const fn name(self) -> &'static str {
        match self {
            GraphProblemKind::VertexCover => "vertex-cover",
            GraphProblemKind::IndependentSet => "independent-set",
            GraphProblemKind::GraphColoring => "graph-coloring",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphTopology {
    pub vertex_count: usize,
    pub edges: Vec<(usize, usize)>,
}

impl GraphTopology {
    pub fn new(vertex_count: usize, edges: &[(usize, usize)]) -> Result<Self, GraphProblemError> {
        let mut normalized = Vec::with_capacity(edges.len());
        for &(left, right) in edges {
            if left >= vertex_count || right >= vertex_count {
                return Err(GraphProblemError::InvalidVertex);
            }
            if left == right {
                return Err(GraphProblemError::SelfEdge);
            }
            let edge = if left < right {
                (left, right)
            } else {
                (right, left)
            };
            if !normalized.contains(&edge) {
                normalized.push(edge);
            }
        }
        normalized.sort_unstable();

        Ok(Self {
            vertex_count,
            edges: normalized,
        })
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "graph vertices={}", self.vertex_count);
        let _ = writeln!(out, "edges={}", self.edges.len());
        for &(left, right) in &self.edges {
            let _ = writeln!(out, "edge({}, {})", left, right);
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphProblem {
    pub universe: MarbleUniverseId,
    pub kind: GraphProblemKind,
    pub graph: GraphTopology,
    pub color_count: usize,
}

impl GraphProblem {
    pub fn vertex_cover(
        universe: MarbleUniverseId,
        vertex_count: usize,
        edges: &[(usize, usize)],
    ) -> Result<Self, GraphProblemError> {
        Ok(Self {
            universe,
            kind: GraphProblemKind::VertexCover,
            graph: GraphTopology::new(vertex_count, edges)?,
            color_count: 0,
        })
    }

    pub fn independent_set(
        universe: MarbleUniverseId,
        vertex_count: usize,
        edges: &[(usize, usize)],
    ) -> Result<Self, GraphProblemError> {
        Ok(Self {
            universe,
            kind: GraphProblemKind::IndependentSet,
            graph: GraphTopology::new(vertex_count, edges)?,
            color_count: 0,
        })
    }

    pub fn graph_coloring(
        universe: MarbleUniverseId,
        vertex_count: usize,
        edges: &[(usize, usize)],
        color_count: usize,
    ) -> Result<Self, GraphProblemError> {
        if color_count == 0 || color_count > 4 {
            return Err(GraphProblemError::UnsupportedColorCount);
        }
        Ok(Self {
            universe,
            kind: GraphProblemKind::GraphColoring,
            graph: GraphTopology::new(vertex_count, edges)?,
            color_count,
        })
    }

    pub fn render_language(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "problem {} {{", self.kind.name());
        let _ = writeln!(out, "  universe {};", self.universe.0);
        let _ = writeln!(out, "  vertices V = 0..{};", self.graph.vertex_count);
        out.push_str("  edges E = {");
        for (index, &(left, right)) in self.graph.edges.iter().enumerate() {
            if index != 0 {
                out.push_str(", ");
            }
            let _ = write!(out, "({}, {})", left, right);
        }
        out.push_str("};\n");
        match self.kind {
            GraphProblemKind::VertexCover => {
                out.push_str("  var bool x[v in V];\n");
                out.push_str("  constraint forall((u,v) in E): x[u] + x[v] >= 1;\n");
                out.push_str("  minimize sum(v in V)(x[v]);\n");
            }
            GraphProblemKind::IndependentSet => {
                out.push_str("  var bool x[v in V];\n");
                out.push_str("  constraint forall((u,v) in E): x[u] + x[v] <= 1;\n");
                out.push_str("  maximize sum(v in V)(x[v]);\n");
            }
            GraphProblemKind::GraphColoring => {
                let _ = writeln!(out, "  var color<{}> c[v in V];", self.color_count);
                out.push_str("  constraint forall((u,v) in E): c[u] != c[v];\n");
                out.push_str("  satisfy;\n");
            }
        }
        out.push('}');
        out
    }

    pub fn parser_state(&self) -> GProgramState {
        GProgramState::from_problem(self)
    }

    pub fn parser_memory_map(&self) -> GProgramMemoryMap {
        self.parser_state().to_memory_map()
    }
}

impl Marble for GraphProblem {
    fn kind(&self) -> &'static str {
        "graph-problem"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GVariableType {
    Bool,
    Color { cardinality: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GVariableDomain {
    Vertex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GVariableDecl {
    pub slot: usize,
    pub name: &'static str,
    pub ty: GVariableType,
    pub domain: GVariableDomain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GConstraintDecl {
    EdgeBoolSumAtLeast {
        var_slot: usize,
        threshold: u8,
    },
    EdgeBoolSumAtMost {
        var_slot: usize,
        threshold: u8,
    },
    EdgeColorNotEqual {
        var_slot: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GObjectiveDecl {
    MinimizeVertexBoolSum { var_slot: usize },
    MaximizeVertexBoolSum { var_slot: usize },
    Satisfy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GProgramState {
    pub universe: MarbleUniverseId,
    pub kind: GraphProblemKind,
    pub vertex_count: usize,
    pub edges: Vec<(usize, usize)>,
    pub variables: Vec<GVariableDecl>,
    pub constraints: Vec<GConstraintDecl>,
    pub objective: GObjectiveDecl,
    pub output_var_slot: usize,
}

impl GProgramState {
    pub fn from_problem(problem: &GraphProblem) -> Self {
        let variable = match problem.kind {
            GraphProblemKind::VertexCover | GraphProblemKind::IndependentSet => GVariableDecl {
                slot: 0,
                name: "x",
                ty: GVariableType::Bool,
                domain: GVariableDomain::Vertex,
            },
            GraphProblemKind::GraphColoring => GVariableDecl {
                slot: 0,
                name: "c",
                ty: GVariableType::Color {
                    cardinality: problem.color_count,
                },
                domain: GVariableDomain::Vertex,
            },
        };

        let (constraints, objective) = match problem.kind {
            GraphProblemKind::VertexCover => (
                vec![GConstraintDecl::EdgeBoolSumAtLeast {
                    var_slot: variable.slot,
                    threshold: 1,
                }],
                GObjectiveDecl::MinimizeVertexBoolSum {
                    var_slot: variable.slot,
                },
            ),
            GraphProblemKind::IndependentSet => (
                vec![GConstraintDecl::EdgeBoolSumAtMost {
                    var_slot: variable.slot,
                    threshold: 1,
                }],
                GObjectiveDecl::MaximizeVertexBoolSum {
                    var_slot: variable.slot,
                },
            ),
            GraphProblemKind::GraphColoring => (
                vec![GConstraintDecl::EdgeColorNotEqual {
                    var_slot: variable.slot,
                }],
                GObjectiveDecl::Satisfy,
            ),
        };

        Self {
            universe: problem.universe,
            kind: problem.kind,
            vertex_count: problem.graph.vertex_count,
            edges: problem.graph.edges.clone(),
            variables: vec![variable],
            constraints,
            objective,
            output_var_slot: variable.slot,
        }
    }

    pub fn to_memory_map(&self) -> GProgramMemoryMap {
        let mut cells = Vec::with_capacity(
            3usize
                .saturating_add(self.edges.len())
                .saturating_add(self.variables.len())
                .saturating_add(self.constraints.len())
                .saturating_add(self.vertex_count),
        );

        cells.push(GMemoryCell::Header {
            version: 1,
            kind: self.kind,
            universe: self.universe.0,
        });
        cells.push(GMemoryCell::VertexCount(self.vertex_count));
        for &(left, right) in &self.edges {
            cells.push(GMemoryCell::Edge { left, right });
        }
        for variable in self.variables.iter().copied() {
            cells.push(GMemoryCell::Variable(variable));
        }
        for constraint in self.constraints.iter().copied() {
            cells.push(GMemoryCell::Constraint(constraint));
        }
        cells.push(GMemoryCell::Objective(self.objective));
        for vertex in 0..self.vertex_count {
            cells.push(GMemoryCell::OutputSlot {
                var_slot: self.output_var_slot,
                vertex,
            });
        }

        GProgramMemoryMap { cells }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GMemoryCell {
    Header {
        version: u8,
        kind: GraphProblemKind,
        universe: u64,
    },
    VertexCount(usize),
    Edge {
        left: usize,
        right: usize,
    },
    Variable(GVariableDecl),
    Constraint(GConstraintDecl),
    Objective(GObjectiveDecl),
    OutputSlot {
        var_slot: usize,
        vertex: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GProgramMemoryMap {
    pub cells: Vec<GMemoryCell>,
}

impl GProgramMemoryMap {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "g-memory-map cells={}", self.cells.len());
        for (index, cell) in self.cells.iter().enumerate() {
            let _ = writeln!(out, "{:03}: {:?}", index, cell);
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphWidgetRole {
    IngressWhiteHole,
    VertexColorLatch,
    EdgeConstraintGate,
    PaletteBank,
    WitnessBlackHole,
}

impl GraphWidgetRole {
    pub const fn name(self) -> &'static str {
        match self {
            GraphWidgetRole::IngressWhiteHole => "ingress-white-hole",
            GraphWidgetRole::VertexColorLatch => "vertex-color-latch",
            GraphWidgetRole::EdgeConstraintGate => "edge-constraint-gate",
            GraphWidgetRole::PaletteBank => "palette-bank",
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
pub struct ManualCollapsedWorld {
    pub universe: MarbleUniverseId,
    pub placements: Vec<ManualWidgetPlacement>,
    pub collapsed: CollapsedMarbleWorld,
    pub runnable: RunnableMarbleWorld,
    pub placement: MarblePlacementRecord,
}

impl ManualCollapsedWorld {
    pub fn as_collapsed_world_marble(&self) -> CollapsedWorldMarble {
        collapsed_world_marble(
            self.universe,
            self.collapsed.clone(),
            self.collapsed.layout.tiles.len(),
        )
    }

    pub fn etch(self) -> Result<EtchedWorld, MarbleCollapseError> {
        let mut etchers = initialized_world_etchers();
        etchers[0].etch(self.as_collapsed_world_marble())
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "manual-collapse-universe={}", self.universe.0);
        let _ = writeln!(out, "manual-widgets");
        for placement in self.placements.iter().copied() {
            let _ = writeln!(out, "{} @ {}", placement.role.name(), placement.location);
        }
        let _ = writeln!(out);
        out.push_str(&self.collapsed.render());
        let _ = writeln!(out);
        out.push_str(&self.placement.render());
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphAssignment {
    Bool(Vec<bool>),
    Color(Vec<u8>),
}

impl GraphAssignment {
    fn vertex_count(&self) -> usize {
        match self {
            GraphAssignment::Bool(values) => values.len(),
            GraphAssignment::Color(values) => values.len(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphSolution {
    pub assignment: GraphAssignment,
    pub objective_value: usize,
    pub search_states: usize,
}

impl GraphSolution {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "objective-value={}", self.objective_value);
        let _ = writeln!(out, "search-states={}", self.search_states);
        match &self.assignment {
            GraphAssignment::Bool(values) => {
                out.push_str("assignment=");
                for (index, value) in values.iter().copied().enumerate() {
                    if index != 0 {
                        out.push(' ');
                    }
                    let _ = write!(out, "x{}={}", index, if value { 1 } else { 0 });
                }
                out.push('\n');
            }
            GraphAssignment::Color(values) => {
                out.push_str("assignment=");
                for (index, value) in values.iter().copied().enumerate() {
                    if index != 0 {
                        out.push(' ');
                    }
                    let _ = write!(out, "c{}={}", index, value);
                }
                out.push('\n');
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphBootstrapCollapse {
    pub problem: GraphProblem,
    pub solution: GraphSolution,
    pub collapsed: CollapsedMarbleWorld,
    pub runnable: RunnableMarbleWorld,
    pub placement: MarblePlacementRecord,
}

impl GraphBootstrapCollapse {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "bootstrap-kind={}", self.problem.kind.name());
        let _ = writeln!(out, "problem-universe={}", self.problem.universe.0);
        let _ = writeln!(out, "vertex-count={}", self.problem.graph.vertex_count);
        let _ = writeln!(out, "edge-count={}", self.problem.graph.edges.len());
        let _ = writeln!(out);
        out.push_str("language\n");
        out.push_str(&self.problem.render_language());
        let _ = writeln!(out);
        let _ = writeln!(out);
        out.push_str("solution\n");
        out.push_str(&self.solution.render());
        let _ = writeln!(out);
        out.push_str(&self.collapsed.render());
        let _ = writeln!(out);
        out.push_str(&self.placement.render());
        out
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GraphCalculator;

impl GraphCalculator {
    pub const fn new() -> Self {
        Self
    }

    pub fn solve_bootstrap(
        &self,
        problem: &GraphProblem,
    ) -> Result<GraphSolution, GraphProblemError> {
        match problem.kind {
            GraphProblemKind::VertexCover => solve_vertex_cover(problem),
            GraphProblemKind::IndependentSet => solve_independent_set(problem),
            GraphProblemKind::GraphColoring => solve_graph_coloring(problem),
        }
    }

    pub fn collapse_problem(
        &self,
        problem: GraphProblem,
    ) -> Result<GraphBootstrapCollapse, GraphProblemError> {
        let solution = self.solve_bootstrap(&problem)?;
        let collapsed = collapse_solution_to_world(&problem, &solution);
        let runnable = runnable_world_from_collapsed(&problem, &collapsed);
        let placement = placement_for_runnable(&problem, &runnable);
        Ok(GraphBootstrapCollapse {
            problem,
            solution,
            collapsed,
            runnable,
            placement,
        })
    }
}

impl MarbleGadget for GraphCalculator {
    fn name(&self) -> &'static str {
        "graph-calculator"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphProblemError {
    InvalidVertex,
    SelfEdge,
    TooManyVertices,
    UnsupportedColorCount,
    Unsatisfiable,
}

fn solve_vertex_cover(problem: &GraphProblem) -> Result<GraphSolution, GraphProblemError> {
    let vertex_count = problem.graph.vertex_count;
    if vertex_count > 24 {
        return Err(GraphProblemError::TooManyVertices);
    }

    let mut best_mask = None;
    let mut best_cost = usize::MAX;
    let mut search_states = 0usize;
    let limit = 1u128 << vertex_count;

    for mask in 0..limit {
        search_states = search_states.saturating_add(1);
        let cost = mask.count_ones() as usize;
        if cost >= best_cost {
            continue;
        }
        if problem
            .graph
            .edges
            .iter()
            .all(|&(left, right)| ((mask >> left) & 1) == 1 || ((mask >> right) & 1) == 1)
        {
            best_mask = Some(mask);
            best_cost = cost;
        }
    }

    let mask = best_mask.ok_or(GraphProblemError::Unsatisfiable)?;
    Ok(GraphSolution {
        assignment: GraphAssignment::Bool(mask_to_bool_vec(mask, vertex_count)),
        objective_value: best_cost,
        search_states,
    })
}

fn solve_independent_set(problem: &GraphProblem) -> Result<GraphSolution, GraphProblemError> {
    let vertex_count = problem.graph.vertex_count;
    if vertex_count > 24 {
        return Err(GraphProblemError::TooManyVertices);
    }

    let mut best_mask = None;
    let mut best_size = 0usize;
    let mut search_states = 0usize;
    let limit = 1u128 << vertex_count;

    for mask in 0..limit {
        search_states = search_states.saturating_add(1);
        let size = mask.count_ones() as usize;
        if size < best_size {
            continue;
        }
        if problem
            .graph
            .edges
            .iter()
            .all(|&(left, right)| !((((mask >> left) & 1) == 1) && (((mask >> right) & 1) == 1)))
        {
            best_mask = Some(mask);
            best_size = size;
        }
    }

    let mask = best_mask.ok_or(GraphProblemError::Unsatisfiable)?;
    Ok(GraphSolution {
        assignment: GraphAssignment::Bool(mask_to_bool_vec(mask, vertex_count)),
        objective_value: best_size,
        search_states,
    })
}

fn solve_graph_coloring(problem: &GraphProblem) -> Result<GraphSolution, GraphProblemError> {
    if problem.graph.vertex_count > 14 {
        return Err(GraphProblemError::TooManyVertices);
    }
    if problem.color_count == 0 || problem.color_count > 4 {
        return Err(GraphProblemError::UnsupportedColorCount);
    }

    let mut assignment = vec![u8::MAX; problem.graph.vertex_count];
    let mut search_states = 0usize;
    if !search_coloring(problem, 0, &mut assignment, &mut search_states) {
        return Err(GraphProblemError::Unsatisfiable);
    }

    Ok(GraphSolution {
        assignment: GraphAssignment::Color(assignment),
        objective_value: problem.color_count,
        search_states,
    })
}

fn search_coloring(
    problem: &GraphProblem,
    vertex: usize,
    assignment: &mut [u8],
    search_states: &mut usize,
) -> bool {
    if vertex == assignment.len() {
        return true;
    }

    for color in 0..problem.color_count as u8 {
        *search_states = search_states.saturating_add(1);
        if color_allowed(problem, assignment, vertex, color) {
            assignment[vertex] = color;
            if search_coloring(problem, vertex + 1, assignment, search_states) {
                return true;
            }
            assignment[vertex] = u8::MAX;
        }
    }
    false
}

fn color_allowed(problem: &GraphProblem, assignment: &[u8], vertex: usize, color: u8) -> bool {
    problem.graph.edges.iter().all(|&(left, right)| {
        if left == vertex {
            assignment[right] == u8::MAX || assignment[right] != color
        } else if right == vertex {
            assignment[left] == u8::MAX || assignment[left] != color
        } else {
            true
        }
    })
}

fn mask_to_bool_vec(mask: u128, vertex_count: usize) -> Vec<bool> {
    let mut values = Vec::with_capacity(vertex_count);
    for vertex in 0..vertex_count {
        values.push(((mask >> vertex) & 1) == 1);
    }
    values
}

fn collapse_solution_to_world(
    problem: &GraphProblem,
    solution: &GraphSolution,
) -> CollapsedMarbleWorld {
    let tile_count = problem.graph.vertex_count.saturating_add(2).max(3);
    let mut tiles = Vec::with_capacity(tile_count);
    tiles.push(CalculatorTile::Source);
    match &solution.assignment {
        GraphAssignment::Bool(values) => {
            for value in values.iter().copied() {
                tiles.push(bool_tile_for(problem.kind, value));
            }
        }
        GraphAssignment::Color(values) => {
            for value in values.iter().copied() {
                tiles.push(color_tile_for(value));
            }
        }
    }
    tiles.push(CalculatorTile::Sink);

    let mut widgets = Vec::with_capacity(solution.assignment.vertex_count().saturating_add(1));
    widgets.push(ImportedWidget {
        kind: WidgetKind::Root,
        preferred_tile: CalculatorTile::Source,
    });

    let mut placements = Vec::with_capacity(widgets.len().saturating_add(problem.graph.vertex_count));
    placements.push((widgets[0], 0));

    match &solution.assignment {
        GraphAssignment::Bool(values) => {
            for (index, value) in values.iter().copied().enumerate() {
                let widget = ImportedWidget {
                    kind: WidgetKind::Tag,
                    preferred_tile: bool_tile_for(problem.kind, value),
                };
                widgets.push(widget);
                placements.push((widget, index + 1));
            }
        }
        GraphAssignment::Color(values) => {
            for (index, value) in values.iter().copied().enumerate() {
                let widget = ImportedWidget {
                    kind: WidgetKind::Tag,
                    preferred_tile: color_tile_for(value),
                };
                widgets.push(widget);
                placements.push((widget, index + 1));
            }
        }
    }

    CollapsedMarbleWorld {
        layout: CalculatorLayout { tiles },
        widgets,
        placements,
        contradictions: Vec::new(),
        propagation_steps: solution.search_states,
        conditions: Vec::new(),
    }
}

fn runnable_world_from_collapsed(
    problem: &GraphProblem,
    collapsed: &CollapsedMarbleWorld,
) -> RunnableMarbleWorld {
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
        universe: problem.universe,
        layout: collapsed.layout.clone(),
        widgets,
        singularities: vec![
            MarbleSingularityStub {
                kind: MarbleSingularityKind::WhiteHole,
                location: MarbleTileLocation::new(0),
            },
            MarbleSingularityStub {
                kind: MarbleSingularityKind::BlackHole,
                location: MarbleTileLocation::new(collapsed.layout.tiles.len().saturating_sub(1)),
            },
        ],
        contradictions: collapsed.contradictions.clone(),
        propagation_steps: collapsed.propagation_steps,
        gather_policy: CalculatorGatherPolicy::EmptyFill,
    }
}

fn placement_for_runnable(
    problem: &GraphProblem,
    runnable: &RunnableMarbleWorld,
) -> MarblePlacementRecord {
    MarblePlacementRecord {
        universe: problem.universe,
        ingress: MarbleTileLocation::new(0),
        egress: MarbleTileLocation::new(runnable.layout.tiles.len().saturating_sub(1)),
        memory_word_offset: ((problem.universe.0 as usize) << 4)
            .saturating_add(runnable.layout.tiles.len()),
        tile_count: runnable.layout.tiles.len(),
    }
}

fn bool_tile_for(kind: GraphProblemKind, value: bool) -> CalculatorTile {
    match kind {
        GraphProblemKind::VertexCover => {
            if value {
                CalculatorTile::BoxStore
            } else {
                CalculatorTile::Portal
            }
        }
        GraphProblemKind::IndependentSet => {
            if value {
                CalculatorTile::Race
            } else {
                CalculatorTile::Gather
            }
        }
        GraphProblemKind::GraphColoring => {
            if value {
                CalculatorTile::BoxStore
            } else {
                CalculatorTile::Portal
            }
        }
    }
}

fn color_tile_for(color: u8) -> CalculatorTile {
    match color {
        0 => CalculatorTile::Race,
        1 => CalculatorTile::Gather,
        2 => CalculatorTile::Portal,
        _ => CalculatorTile::BoxStore,
    }
}

pub fn graph_problem_bootstrap_visual() -> String {
    let problem = GraphProblem::vertex_cover(MarbleUniverseId(99), 3, &[(0, 1), (1, 2), (0, 2)])
        .unwrap();
    GraphCalculator::new().collapse_problem(problem).unwrap().render()
}

pub fn manual_graph_coloring_collapsed_world(
    universe: MarbleUniverseId,
    edges: &[(usize, usize)],
    colors: &[u8],
    color_count: usize,
) -> Result<ManualCollapsedWorld, GraphProblemError> {
    let problem = GraphProblem::graph_coloring(universe, colors.len(), edges, color_count)?;

    for &(left, right) in &problem.graph.edges {
        if colors[left] == colors[right] {
            return Err(GraphProblemError::Unsatisfiable);
        }
    }
    if colors.iter().copied().any(|color| color as usize >= color_count) {
        return Err(GraphProblemError::UnsupportedColorCount);
    }

    let solution = GraphSolution {
        assignment: GraphAssignment::Color(colors.to_vec()),
        objective_value: color_count,
        search_states: 0,
    };
    let collapsed = collapse_solution_to_world(&problem, &solution);
    let runnable = runnable_world_from_collapsed(&problem, &collapsed);
    let placement = placement_for_runnable(&problem, &runnable);

    let mut placements = Vec::with_capacity(problem.graph.vertex_count.saturating_add(4));
    placements.push(ManualWidgetPlacement {
        role: GraphWidgetRole::IngressWhiteHole,
        location: 0,
    });
    placements.push(ManualWidgetPlacement {
        role: GraphWidgetRole::PaletteBank,
        location: 1,
    });

    for vertex in 0..problem.graph.vertex_count {
        placements.push(ManualWidgetPlacement {
            role: GraphWidgetRole::VertexColorLatch,
            location: vertex + 1,
        });
    }

    placements.push(ManualWidgetPlacement {
        role: GraphWidgetRole::EdgeConstraintGate,
        location: problem.graph.vertex_count,
    });
    placements.push(ManualWidgetPlacement {
        role: GraphWidgetRole::WitnessBlackHole,
        location: problem.graph.vertex_count + 1,
    });

    Ok(ManualCollapsedWorld {
        universe,
        placements,
        collapsed,
        runnable,
        placement,
    })
}

pub fn manual_graph_coloring_visual() -> String {
    manual_graph_coloring_collapsed_world(
        MarbleUniverseId(101),
        &[(0, 1), (1, 2), (2, 0)],
        &[0, 1, 2],
        3,
    )
    .unwrap()
    .render()
}

pub fn manual_graph_coloring_etch_visual() -> String {
    manual_graph_coloring_collapsed_world(
        MarbleUniverseId(102),
        &[(0, 1), (1, 2), (2, 0)],
        &[0, 1, 2],
        3,
    )
    .unwrap()
    .etch()
    .unwrap()
    .render()
}

pub fn graph_problem_memory_map_visual() -> String {
    let problem = GraphProblem::graph_coloring(
        MarbleUniverseId(100),
        4,
        &[(0, 1), (1, 2), (2, 3), (3, 0)],
        3,
    )
    .unwrap();
    let state = problem.parser_state();
    let map = state.to_memory_map();

    let mut out = String::new();
    out.push_str("language\n");
    out.push_str(&problem.render_language());
    let _ = writeln!(out);
    let _ = writeln!(out);
    out.push_str("parser-state\n");
    let _ = writeln!(out, "kind={}", state.kind.name());
    let _ = writeln!(out, "vars={}", state.variables.len());
    let _ = writeln!(out, "constraints={}", state.constraints.len());
    let _ = writeln!(out, "output-var-slot={}", state.output_var_slot);
    let _ = writeln!(out);
    out.push_str(&map.render());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_cover_language_renders_expected_form() {
        let problem =
            GraphProblem::vertex_cover(MarbleUniverseId(5), 3, &[(0, 1), (1, 2), (0, 2)])
                .unwrap();

        let rendered = problem.render_language();

        assert!(rendered.contains("problem vertex-cover"));
        assert!(rendered.contains("var bool x[v in V]"));
        assert!(rendered.contains("minimize sum(v in V)(x[v])"));
    }

    #[test]
    fn vertex_cover_bootstrap_solves_triangle() {
        let problem =
            GraphProblem::vertex_cover(MarbleUniverseId(11), 3, &[(0, 1), (1, 2), (0, 2)])
                .unwrap();

        let solution = GraphCalculator::new().solve_bootstrap(&problem).unwrap();

        assert_eq!(solution.objective_value, 2);
        assert!(matches!(solution.assignment, GraphAssignment::Bool(_)));
    }

    #[test]
    fn independent_set_bootstrap_solves_small_path() {
        let problem =
            GraphProblem::independent_set(MarbleUniverseId(12), 3, &[(0, 1), (1, 2)])
                .unwrap();

        let solution = GraphCalculator::new().solve_bootstrap(&problem).unwrap();

        assert_eq!(solution.objective_value, 2);
    }

    #[test]
    fn graph_coloring_bootstrap_colors_triangle() {
        let problem =
            GraphProblem::graph_coloring(MarbleUniverseId(13), 3, &[(0, 1), (1, 2), (0, 2)], 3)
                .unwrap();

        let solution = GraphCalculator::new().solve_bootstrap(&problem).unwrap();

        match solution.assignment {
            GraphAssignment::Color(values) => {
                assert_eq!(values.len(), 3);
                assert_ne!(values[0], values[1]);
                assert_ne!(values[1], values[2]);
                assert_ne!(values[0], values[2]);
            }
            GraphAssignment::Bool(_) => panic!("expected color assignment"),
        }
    }

    #[test]
    fn collapse_problem_emits_runnable_world_and_placement() {
        let problem =
            GraphProblem::vertex_cover(MarbleUniverseId(21), 4, &[(0, 1), (1, 2), (2, 3)])
                .unwrap();

        let collapsed = GraphCalculator::new().collapse_problem(problem).unwrap();

        assert!(collapsed.collapsed.contradictions.is_empty());
        assert_eq!(collapsed.runnable.widgets.len(), 5);
        assert_eq!(collapsed.placement.ingress.index, 0);
        assert_eq!(collapsed.placement.egress.index, 5);
        assert!(graph_problem_bootstrap_visual().contains("bootstrap-kind=vertex-cover"));
    }

    #[test]
    fn graph_coloring_parser_state_and_memory_map_are_expressible() {
        let problem = GraphProblem::graph_coloring(
            MarbleUniverseId(33),
            4,
            &[(0, 1), (1, 2), (2, 3), (3, 0)],
            3,
        )
        .unwrap();

        let state = problem.parser_state();
        let map = state.to_memory_map();

        assert_eq!(state.kind, GraphProblemKind::GraphColoring);
        assert_eq!(state.variables.len(), 1);
        assert_eq!(state.constraints.len(), 1);
        assert!(matches!(state.constraints[0], GConstraintDecl::EdgeColorNotEqual { .. }));
        assert!(map
            .cells
            .iter()
            .any(|cell| matches!(cell, GMemoryCell::Objective(GObjectiveDecl::Satisfy))));
        assert_eq!(
            map.cells
                .iter()
                .filter(|cell| matches!(cell, GMemoryCell::OutputSlot { .. }))
                .count(),
            4
        );
        assert!(graph_problem_memory_map_visual().contains("g-memory-map"));
    }

    #[test]
    fn manual_graph_coloring_world_can_be_placed() {
        let manual = manual_graph_coloring_collapsed_world(
            MarbleUniverseId(34),
            &[(0, 1), (1, 2), (2, 0)],
            &[0, 1, 2],
            3,
        )
        .unwrap();

        assert_eq!(manual.collapsed.layout.tiles.first(), Some(&CalculatorTile::Source));
        assert_eq!(manual.collapsed.layout.tiles.last(), Some(&CalculatorTile::Sink));
        assert!(manual
            .placements
            .iter()
            .any(|entry| entry.role == GraphWidgetRole::WitnessBlackHole));
        assert!(manual_graph_coloring_visual().contains("manual-widgets"));
    }

    #[test]
    fn manual_graph_world_can_be_fed_into_etcher() {
        let manual = manual_graph_coloring_collapsed_world(
            MarbleUniverseId(35),
            &[(0, 1), (1, 2), (2, 0)],
            &[0, 1, 2],
            3,
        )
        .unwrap();

        let etched = manual.etch().unwrap();

        assert_eq!(etched.etcher.name(), "etcher1");
        assert_eq!(etched.placement.ingress.index, 0);
        assert_eq!(etched.placement.egress.index, 4);
        assert!(manual_graph_coloring_etch_visual().contains("etcher=etcher1"));
    }
}