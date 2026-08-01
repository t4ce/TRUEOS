//! Build-time folding and runtime dirtiness for retained affine graphs.
//!
//! World composition deliberately remains GPU-owned.  This module compiles
//! authored operation lists into a flat, levelized graph, retains baked local
//! affines for constant nodes, and tells the GPU exactly which dynamic locals,
//! worlds, and source rows changed.

use alloc::{vec, vec::Vec};

use crate::Error;
use trueos_helio_artifact::{Artifact, SectionKind};

pub const NO_PARENT: u32 = u32::MAX;
pub const CONSTANT_NODE: u32 = u32::MAX;
pub const MAX_RETAINED_TRANSFORM_DEPTH: u32 = 64;
pub const TEMPLATE_SECTION_NAME: &str = "scene/retained-transform-template-v1.bin";
const TEMPLATE_MAGIC: [u8; 8] = *b"HRTXFM\0\0";
const TEMPLATE_BYTES: usize = 128;
const TEMPLATE_HEADER_BYTES: usize = 80;
const TEMPLATE_FLAGS: u32 = 0x0F;
const TEMPLATE_MAX_ROWS: u32 = 4_096;

/// A row-major affine matrix without the invariant final row `[0, 0, 0, 1]`.
///
/// Points are column vectors. `a.mul(b)` therefore means `a * b`, or `b`
/// followed by `a` when applied to a point. This order is also the order used
/// while folding an authored constant run.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine3x4 {
    pub rows: [f32; 12],
}

impl Affine3x4 {
    pub const BYTE_LEN: usize = 12 * core::mem::size_of::<f32>();
    pub const IDENTITY: Self = Self {
        rows: [
            1.0, 0.0, 0.0, 0.0, // x
            0.0, 1.0, 0.0, 0.0, // y
            0.0, 0.0, 1.0, 0.0, // z
        ],
    };

    pub const fn translation(x: f32, y: f32, z: f32) -> Self {
        Self {
            rows: [1.0, 0.0, 0.0, x, 0.0, 1.0, 0.0, y, 0.0, 0.0, 1.0, z],
        }
    }

    pub const fn scale(x: f32, y: f32, z: f32) -> Self {
        Self {
            rows: [x, 0.0, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, 0.0, z, 0.0],
        }
    }

    pub fn is_finite(self) -> bool {
        self.rows.iter().all(|value| value.is_finite())
    }

    pub fn mul(self, rhs: Self) -> Self {
        let mut rows = [0.0; 12];
        for row in 0..3 {
            for column in 0..3 {
                rows[row * 4 + column] = self.rows[row * 4] * rhs.rows[column]
                    + self.rows[row * 4 + 1] * rhs.rows[4 + column]
                    + self.rows[row * 4 + 2] * rhs.rows[8 + column];
            }
            rows[row * 4 + 3] = self.rows[row * 4] * rhs.rows[3]
                + self.rows[row * 4 + 1] * rhs.rows[7]
                + self.rows[row * 4 + 2] * rhs.rows[11]
                + self.rows[row * 4 + 3];
        }
        Self { rows }
    }

    pub fn transform_point(self, point: [f32; 3]) -> [f32; 3] {
        [
            self.rows[0] * point[0]
                + self.rows[1] * point[1]
                + self.rows[2] * point[2]
                + self.rows[3],
            self.rows[4] * point[0]
                + self.rows[5] * point[1]
                + self.rows[6] * point[2]
                + self.rows[7],
            self.rows[8] * point[0]
                + self.rows[9] * point[1]
                + self.rows[10] * point[2]
                + self.rows[11],
        ]
    }

    pub fn to_le_bytes(self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        for (index, value) in self.rows.into_iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_bits().to_le_bytes());
        }
        bytes
    }
}

impl Default for Affine3x4 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Build-authored seed for the common retained scene shape.
///
/// Its static root is already folded in the `.helio`; instantiation only
/// expands the declared dynamic child template for the render rows that exist.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RetainedTransformTemplate {
    pub root_affine: Affine3x4,
    pub authored_constant_ops: u32,
    pub constant_runs: u32,
    pub emitted_constant_affines: u32,
    pub folded_constant_ops: u32,
    pub max_render_rows: u32,
    pub max_runtime_nodes: u32,
    pub traversal_depth: u32,
}

impl RetainedTransformTemplate {
    pub const CANONICAL: Self = Self {
        root_affine: Affine3x4::IDENTITY,
        authored_constant_ops: 2,
        constant_runs: 1,
        emitted_constant_affines: 1,
        folded_constant_ops: 1,
        max_render_rows: TEMPLATE_MAX_ROWS,
        max_runtime_nodes: TEMPLATE_MAX_ROWS + 1,
        traversal_depth: 2,
    };

    pub fn decode_artifact(bytes: &[u8]) -> Result<Self, Error> {
        let artifact = Artifact::parse(bytes).map_err(|_| Error::Artifact)?;
        Self::decode(&artifact)
    }

    pub(crate) fn decode(artifact: &Artifact<'_>) -> Result<Self, Error> {
        let section = artifact
            .section(TEMPLATE_SECTION_NAME)
            .ok_or(Error::MissingRetainedTransformTemplate)?;
        let bytes = section.data;
        if section.kind != SectionKind::Unknown(u16::MAX)
            || bytes.len() != TEMPLATE_BYTES
            || bytes.get(..8) != Some(TEMPLATE_MAGIC.as_slice())
            || read_template_u16(bytes, 8) != Some(1)
            || read_template_u16(bytes, 10) != Some(TEMPLATE_HEADER_BYTES as u16)
            || read_template_u32(bytes, 12) != Some(TEMPLATE_BYTES as u32)
            || read_template_u32(bytes, 16) != Some(TEMPLATE_FLAGS)
            || read_template_u32(bytes, 20) != Some(Affine3x4::BYTE_LEN as u32)
            || read_template_u32(bytes, 24) != Some(TEMPLATE_HEADER_BYTES as u32)
            || read_template_u32(bytes, 28) != Some(1)
            || read_template_u32(bytes, 32) != Some(2)
            || read_template_u32(bytes, 36) != Some(1)
            || read_template_u32(bytes, 40) != Some(1)
            || read_template_u32(bytes, 44) != Some(1)
            || read_template_u32(bytes, 48) != Some(1)
            || read_template_u32(bytes, 52) != Some(TEMPLATE_MAX_ROWS)
            || read_template_u32(bytes, 56) != Some(TEMPLATE_MAX_ROWS + 1)
            || read_template_u32(bytes, 60) != Some(2)
            || read_template_u32(bytes, 64) != Some(0)
            || read_template_u32(bytes, 68) != Some(0)
            || read_template_u32(bytes, 72) != Some(1)
            || read_template_u32(bytes, 76) != Some(0)
            || bytes.get(TEMPLATE_HEADER_BYTES..)
                != Some(Affine3x4::IDENTITY.to_le_bytes().as_slice())
        {
            return Err(Error::InvalidRetainedTransformTemplate);
        }
        Ok(Self::CANONICAL)
    }
}

fn read_template_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?))
}

fn read_template_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AuthoredTransformOp {
    Constant(Affine3x4),
    /// Index of a GPU-authored dynamic source (a retained TRS seed row in
    /// Churn). Dynamic values do not enter `local_affines` on the CPU.
    Dynamic(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct AuthoredTransformNode {
    pub parent: Option<u32>,
    pub ops: Vec<AuthoredTransformOp>,
}

/// Stable GPU graph record. Local and world affine buffers are node-indexed.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetainedTransformNode {
    pub parent: u32,
    pub level: u32,
    pub local_generation: u32,
    pub world_generation: u32,
}

impl RetainedTransformNode {
    pub const BYTE_LEN: usize = 4 * core::mem::size_of::<u32>();

    pub fn to_le_bytes(self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        for (index, value) in [
            self.parent,
            self.level,
            self.local_generation,
            self.world_generation,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

/// Slice in `level_indices`; nodes retain authored order within one level.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetainedTransformLevel {
    pub first: u32,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TransformReport {
    pub authored_ops: u32,
    /// Number of authored constant operations consumed by build-time folding.
    pub constant_ops_folded: u32,
    pub runtime_nodes: u32,
    pub max_depth: u32,
    pub dirty_local: u32,
    pub dirty_world: u32,
}

/// Borrowed graph payload attached to a retained render frame.
pub struct TransformHierarchyFrame<'a> {
    pub nodes: &'a [RetainedTransformNode],
    pub local_affines: &'a [Affine3x4],
    /// Per-node source row, or [`CONSTANT_NODE`] for a baked local affine.
    pub dynamic_bindings: &'a [u32],
    pub level_indices: &'a [u32],
    pub levels: &'a [RetainedTransformLevel],
    pub dirty_local_nodes: &'a [u32],
    pub dirty_world_nodes: &'a [u32],
    pub dirty_rows: &'a [u32],
    /// Render-row to terminal transform-node mapping.
    pub row_leaf_nodes: &'a [u32],
    pub report: TransformReport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompileError {
    ParentOutOfRange,
    RowLeafOutOfRange,
    DynamicSlotOutOfRange,
    Cycle,
    DepthExceeded,
    NonFiniteAffine,
    CountOverflow,
}

#[derive(Clone, Copy)]
enum Boundary {
    Constant(Affine3x4),
    Dynamic(u32),
}

/// Compiled retained graph and its current GPU worklists.
///
/// A constant node has `dynamic_bindings[node] == CONSTANT_NODE` and a valid
/// `local_affines[node]`. A dynamic node contains an identity placeholder and
/// its binding names the source row that the GPU must turn into its local
/// affine. `authored_leaf_nodes` maps authored objects to the last node in
/// their expanded operation chain.
pub struct RetainedTransformProgram {
    pub nodes: Vec<RetainedTransformNode>,
    pub local_affines: Vec<Affine3x4>,
    pub dynamic_bindings: Vec<u32>,
    pub authored_leaf_nodes: Vec<u32>,
    /// Render-row to terminal transform-node mapping. Unlike authored nodes,
    /// rows name only drawable leaves; helper roots never enter row worklists.
    pub row_leaf_nodes: Vec<u32>,
    pub level_indices: Vec<u32>,
    pub levels: Vec<RetainedTransformLevel>,
    pub dirty_local_node_ids: Vec<u32>,
    pub dirty_world_node_ids: Vec<u32>,
    pub dirty_row_ids: Vec<u32>,
    pending_local_dirty: Vec<bool>,
    report: TransformReport,
}

impl RetainedTransformProgram {
    /// Instantiate the canonical build-folded root plus one independently
    /// GPU-authored dynamic child per retained render row.
    pub fn compile_rooted_dynamic_rows(row_count: usize) -> Result<Self, CompileError> {
        RetainedTransformTemplate::CANONICAL.instantiate_dynamic_rows(row_count)
    }

    pub fn from_template(
        template: RetainedTransformTemplate,
        row_count: usize,
    ) -> Result<Self, CompileError> {
        template.instantiate_dynamic_rows(row_count)
    }

    pub fn compile(authored: Vec<AuthoredTransformNode>) -> Result<Self, CompileError> {
        validate_authored_hierarchy(&authored)?;

        let mut authored_ops = 0u32;
        let mut constant_ops_folded = 0u32;
        let mut boundaries = Vec::with_capacity(authored.len());
        for node in &authored {
            let mut compiled = Vec::new();
            let mut constant_run: Option<Affine3x4> = None;
            for op in &node.ops {
                authored_ops = authored_ops
                    .checked_add(1)
                    .ok_or(CompileError::CountOverflow)?;
                match *op {
                    AuthoredTransformOp::Constant(affine) => {
                        if !affine.is_finite() {
                            return Err(CompileError::NonFiniteAffine);
                        }
                        constant_ops_folded = constant_ops_folded
                            .checked_add(1)
                            .ok_or(CompileError::CountOverflow)?;
                        constant_run = Some(match constant_run {
                            Some(run) => run.mul(affine),
                            None => affine,
                        });
                    }
                    AuthoredTransformOp::Dynamic(slot) => {
                        if slot == CONSTANT_NODE {
                            return Err(CompileError::DynamicSlotOutOfRange);
                        }
                        if let Some(run) = constant_run.take() {
                            compiled.push(Boundary::Constant(run));
                        }
                        compiled.push(Boundary::Dynamic(slot));
                    }
                }
            }
            if let Some(run) = constant_run {
                compiled.push(Boundary::Constant(run));
            }
            // Every authored node needs a leaf so children can name it.
            if compiled.is_empty() {
                compiled.push(Boundary::Constant(Affine3x4::IDENTITY));
            }
            boundaries.push(compiled);
        }

        let mut starts = Vec::with_capacity(boundaries.len());
        let mut authored_leaf_nodes = Vec::with_capacity(boundaries.len());
        let mut runtime_count = 0usize;
        for chain in &boundaries {
            starts.push(u32::try_from(runtime_count).map_err(|_| CompileError::CountOverflow)?);
            runtime_count = runtime_count
                .checked_add(chain.len())
                .ok_or(CompileError::CountOverflow)?;
            authored_leaf_nodes
                .push(u32::try_from(runtime_count - 1).map_err(|_| CompileError::CountOverflow)?);
        }

        let mut nodes = Vec::with_capacity(runtime_count);
        let mut local_affines = Vec::with_capacity(runtime_count);
        let mut dynamic_bindings = Vec::with_capacity(runtime_count);
        for (authored_id, chain) in boundaries.iter().enumerate() {
            let first_parent = match authored[authored_id].parent {
                Some(parent) => authored_leaf_nodes[parent as usize],
                None => NO_PARENT,
            };
            for (op_index, boundary) in chain.iter().enumerate() {
                let runtime_id = starts[authored_id] + op_index as u32;
                nodes.push(RetainedTransformNode {
                    parent: if op_index == 0 {
                        first_parent
                    } else {
                        runtime_id - 1
                    },
                    ..RetainedTransformNode::default()
                });
                match *boundary {
                    Boundary::Constant(affine) => {
                        local_affines.push(affine);
                        dynamic_bindings.push(CONSTANT_NODE);
                    }
                    Boundary::Dynamic(slot) => {
                        local_affines.push(Affine3x4::IDENTITY);
                        dynamic_bindings.push(slot);
                    }
                }
            }
        }

        let (level_indices, levels, max_depth) = levelize(&mut nodes)?;
        let pending_local_dirty = vec![true; runtime_count];
        let row_leaf_nodes = authored_leaf_nodes.clone();
        let mut program = Self {
            nodes,
            local_affines,
            dynamic_bindings,
            authored_leaf_nodes,
            row_leaf_nodes,
            level_indices,
            levels,
            dirty_local_node_ids: Vec::with_capacity(runtime_count),
            dirty_world_node_ids: Vec::with_capacity(runtime_count),
            dirty_row_ids: Vec::new(),
            pending_local_dirty,
            report: TransformReport {
                authored_ops,
                constant_ops_folded,
                runtime_nodes: u32::try_from(runtime_count)
                    .map_err(|_| CompileError::CountOverflow)?,
                max_depth,
                ..TransformReport::default()
            },
        };
        program.propagate_dirty();
        Ok(program)
    }
}

impl RetainedTransformTemplate {
    pub fn instantiate_dynamic_rows(
        self,
        row_count: usize,
    ) -> Result<RetainedTransformProgram, CompileError> {
        if row_count > self.max_render_rows as usize {
            return Err(CompileError::CountOverflow);
        }
        let runtime_count = row_count
            .checked_add(1)
            .ok_or(CompileError::CountOverflow)?;
        if runtime_count > self.max_runtime_nodes as usize {
            return Err(CompileError::CountOverflow);
        }

        let mut nodes = Vec::with_capacity(runtime_count);
        let mut local_affines = Vec::with_capacity(runtime_count);
        let mut dynamic_bindings = Vec::with_capacity(runtime_count);
        let mut authored_leaf_nodes = Vec::with_capacity(runtime_count);
        let mut row_leaf_nodes = Vec::with_capacity(row_count);

        nodes.push(RetainedTransformNode {
            parent: NO_PARENT,
            level: 0,
            ..RetainedTransformNode::default()
        });
        local_affines.push(self.root_affine);
        dynamic_bindings.push(CONSTANT_NODE);
        authored_leaf_nodes.push(0);
        for row in 0..row_count {
            let node = u32::try_from(row + 1).map_err(|_| CompileError::CountOverflow)?;
            nodes.push(RetainedTransformNode {
                parent: 0,
                level: 1,
                ..RetainedTransformNode::default()
            });
            local_affines.push(Affine3x4::IDENTITY);
            dynamic_bindings.push(row as u32);
            authored_leaf_nodes.push(node);
            row_leaf_nodes.push(node);
        }

        let mut level_indices = Vec::with_capacity(runtime_count);
        level_indices.extend(0..runtime_count as u32);
        let mut levels = Vec::with_capacity(if row_count == 0 { 1 } else { 2 });
        levels.push(RetainedTransformLevel { first: 0, count: 1 });
        if row_count != 0 {
            levels.push(RetainedTransformLevel {
                first: 1,
                count: row_count as u32,
            });
        }
        let max_depth = if row_count == 0 {
            1
        } else {
            self.traversal_depth
        };
        let authored_ops = self
            .authored_constant_ops
            .checked_add(row_count as u32)
            .ok_or(CompileError::CountOverflow)?;
        let mut program = RetainedTransformProgram {
            nodes,
            local_affines,
            dynamic_bindings,
            authored_leaf_nodes,
            row_leaf_nodes,
            level_indices,
            levels,
            dirty_local_node_ids: Vec::with_capacity(row_count),
            dirty_world_node_ids: Vec::with_capacity(runtime_count),
            dirty_row_ids: Vec::with_capacity(row_count),
            pending_local_dirty: vec![true; runtime_count],
            report: TransformReport {
                authored_ops,
                constant_ops_folded: self.authored_constant_ops,
                runtime_nodes: runtime_count as u32,
                max_depth,
                ..TransformReport::default()
            },
        };
        program.propagate_dirty();
        Ok(program)
    }
}

impl RetainedTransformProgram {
    pub const fn report(&self) -> TransformReport {
        self.report
    }

    pub fn per_level_counts(&self) -> impl ExactSizeIterator<Item = u32> + '_ {
        self.levels.iter().map(|level| level.count)
    }

    /// Replace the drawable-row mapping without changing graph topology.
    /// This is normally set once by the scene compiler; it is public so a
    /// retained scene with non-drawable helper nodes can name its real leaves.
    pub fn set_row_leaf_nodes(&mut self, row_leaf_nodes: Vec<u32>) -> Result<(), CompileError> {
        if row_leaf_nodes
            .iter()
            .any(|&leaf| leaf as usize >= self.nodes.len())
        {
            return Err(CompileError::RowLeafOutOfRange);
        }
        self.row_leaf_nodes = row_leaf_nodes;
        self.refresh_dirty_rows_from_world_worklist();
        Ok(())
    }

    /// Start collecting a new frame's work without changing graph state.
    pub fn begin_update(&mut self) {
        self.dirty_local_node_ids.clear();
        self.dirty_world_node_ids.clear();
        self.dirty_row_ids.clear();
        self.report.dirty_local = 0;
        self.report.dirty_world = 0;
    }

    /// Mark every dynamic boundary fed by `slot`. The GPU remains responsible
    /// for converting that row's TRS into `local_affines[node]`.
    pub fn mark_dynamic_slot_dirty(&mut self, slot: u32) -> Result<bool, CompileError> {
        if slot == CONSTANT_NODE {
            return Err(CompileError::DynamicSlotOutOfRange);
        }
        let mut found = false;
        for (node, &binding) in self.dynamic_bindings.iter().enumerate() {
            if binding == slot {
                self.pending_local_dirty[node] = true;
                found = true;
            }
        }
        Ok(found)
    }

    /// Propagate pending locals through descendants in level order. This only
    /// maintains generations and worklists; it never composes world matrices.
    pub fn propagate_dirty(&mut self) {
        self.dirty_local_node_ids.clear();
        self.dirty_world_node_ids.clear();
        let mut world_dirty = vec![false; self.nodes.len()];
        for &node_id in &self.level_indices {
            let index = node_id as usize;
            let local_dirty = self.pending_local_dirty[index];
            if local_dirty {
                self.nodes[index].local_generation =
                    next_generation(self.nodes[index].local_generation);
                // Constant locals are already present in the baked affine
                // upload. Only dynamic nodes need the GPU TRS-to-local pass.
                if self.dynamic_bindings[index] != CONSTANT_NODE {
                    self.dirty_local_node_ids.push(node_id);
                }
                self.pending_local_dirty[index] = false;
            }
            let parent_dirty = self.nodes[index].parent != NO_PARENT
                && world_dirty[self.nodes[index].parent as usize];
            if local_dirty || parent_dirty {
                self.nodes[index].world_generation =
                    next_generation(self.nodes[index].world_generation);
                world_dirty[index] = true;
                self.dirty_world_node_ids.push(node_id);
            }
        }
        self.dirty_row_ids.clear();
        for (row, &leaf) in self.row_leaf_nodes.iter().enumerate() {
            if world_dirty[leaf as usize] {
                self.dirty_row_ids.push(row as u32);
            }
        }
        self.report.dirty_local = self.dirty_local_node_ids.len() as u32;
        self.report.dirty_world = self.dirty_world_node_ids.len() as u32;
    }

    fn refresh_dirty_rows_from_world_worklist(&mut self) {
        self.dirty_row_ids.clear();
        for (row, &leaf) in self.row_leaf_nodes.iter().enumerate() {
            if self.dirty_world_node_ids.contains(&leaf) {
                self.dirty_row_ids.push(row as u32);
            }
        }
    }
}

fn next_generation(generation: u32) -> u32 {
    let next = generation.wrapping_add(1);
    if next == 0 { 1 } else { next }
}

fn validate_authored_hierarchy(authored: &[AuthoredTransformNode]) -> Result<(), CompileError> {
    let count = authored.len();
    let mut indegree = vec![0u8; count];
    let mut children = vec![Vec::new(); count];
    for (node, authored_node) in authored.iter().enumerate() {
        if let Some(parent) = authored_node.parent {
            let parent = parent as usize;
            if parent >= count {
                return Err(CompileError::ParentOutOfRange);
            }
            indegree[node] = 1;
            children[parent].push(node);
        }
    }
    let mut queue = Vec::with_capacity(count);
    for (node, &degree) in indegree.iter().enumerate() {
        if degree == 0 {
            queue.push(node);
        }
    }
    let mut cursor = 0;
    while cursor < queue.len() {
        let node = queue[cursor];
        cursor += 1;
        for &child in &children[node] {
            indegree[child] -= 1;
            if indegree[child] == 0 {
                queue.push(child);
            }
        }
    }
    if queue.len() != count {
        return Err(CompileError::Cycle);
    }
    Ok(())
}

fn levelize(
    nodes: &mut [RetainedTransformNode],
) -> Result<(Vec<u32>, Vec<RetainedTransformLevel>, u32), CompileError> {
    let count = nodes.len();
    let mut indegree = vec![0u8; count];
    let mut children = vec![Vec::new(); count];
    let mut queue = Vec::with_capacity(count);
    for node in 0..count {
        if nodes[node].parent == NO_PARENT {
            queue.push(node);
        } else {
            let parent = nodes[node].parent as usize;
            if parent >= count {
                return Err(CompileError::ParentOutOfRange);
            }
            indegree[node] = 1;
            children[parent].push(node);
        }
    }
    let mut cursor = 0;
    let mut max_level = 0u32;
    while cursor < queue.len() {
        let node = queue[cursor];
        cursor += 1;
        let level = if nodes[node].parent == NO_PARENT {
            0
        } else {
            nodes[nodes[node].parent as usize]
                .level
                .checked_add(1)
                .ok_or(CompileError::DepthExceeded)?
        };
        if level >= MAX_RETAINED_TRANSFORM_DEPTH {
            return Err(CompileError::DepthExceeded);
        }
        nodes[node].level = level;
        max_level = max_level.max(level);
        for &child in &children[node] {
            indegree[child] -= 1;
            if indegree[child] == 0 {
                queue.push(child);
            }
        }
    }
    if queue.len() != count {
        return Err(CompileError::Cycle);
    }

    if count == 0 {
        return Ok((Vec::new(), Vec::new(), 0));
    }
    let mut counts = vec![0u32; max_level as usize + 1];
    for node in nodes.iter() {
        counts[node.level as usize] += 1;
    }
    let mut levels = Vec::with_capacity(counts.len());
    let mut first = 0u32;
    for &level_count in &counts {
        levels.push(RetainedTransformLevel {
            first,
            count: level_count,
        });
        first += level_count;
    }
    let mut cursors: Vec<u32> = levels.iter().map(|level| level.first).collect();
    let mut level_indices = vec![0u32; count];
    for (node_id, node) in nodes.iter().enumerate() {
        let level = node.level as usize;
        level_indices[cursors[level] as usize] = node_id as u32;
        cursors[level] += 1;
    }
    // GPU ancestor traversal is bounded by the number of nodes in the deepest
    // chain, not by its zero-based final level.
    Ok((level_indices, levels, max_level + 1))
}

const _: () = {
    assert!(core::mem::size_of::<Affine3x4>() == 48);
    assert!(core::mem::align_of::<Affine3x4>() == 16);
    assert!(core::mem::size_of::<RetainedTransformNode>() == 16);
    assert!(core::mem::size_of::<RetainedTransformLevel>() == 8);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximal_constant_runs_fold_without_reordering() {
        let program = RetainedTransformProgram::compile(vec![AuthoredTransformNode {
            parent: None,
            ops: vec![
                AuthoredTransformOp::Constant(Affine3x4::translation(3.0, 0.0, 0.0)),
                AuthoredTransformOp::Constant(Affine3x4::scale(2.0, 2.0, 2.0)),
                AuthoredTransformOp::Dynamic(7),
                AuthoredTransformOp::Constant(Affine3x4::translation(0.0, 5.0, 0.0)),
                AuthoredTransformOp::Constant(Affine3x4::scale(1.0, 3.0, 1.0)),
            ],
        }])
        .unwrap();

        assert_eq!(program.nodes.len(), 3);
        assert_eq!(program.dynamic_bindings, vec![CONSTANT_NODE, 7, CONSTANT_NODE]);
        assert_eq!(program.report().authored_ops, 5);
        assert_eq!(program.report().constant_ops_folded, 4);
        assert_eq!(program.local_affines[0].transform_point([1.0, 0.0, 0.0]), [5.0, 0.0, 0.0]);
        assert_eq!(program.local_affines[2].transform_point([0.0, 1.0, 0.0]), [0.0, 8.0, 0.0]);
    }

    #[test]
    fn hierarchy_is_levelized_and_cycle_is_rejected() {
        let program = RetainedTransformProgram::compile(vec![
            AuthoredTransformNode {
                parent: Some(1),
                ops: vec![AuthoredTransformOp::Dynamic(0)],
            },
            AuthoredTransformNode {
                parent: None,
                ops: vec![AuthoredTransformOp::Dynamic(1)],
            },
            AuthoredTransformNode {
                parent: Some(0),
                ops: vec![AuthoredTransformOp::Dynamic(2)],
            },
        ])
        .unwrap();
        assert_eq!(program.per_level_counts().collect::<Vec<_>>(), vec![1, 1, 1]);
        assert_eq!(program.level_indices, vec![1, 0, 2]);
        assert_eq!(program.report().max_depth, 3);

        let cycle = RetainedTransformProgram::compile(vec![
            AuthoredTransformNode {
                parent: Some(1),
                ops: vec![],
            },
            AuthoredTransformNode {
                parent: Some(0),
                ops: vec![],
            },
        ]);
        assert!(matches!(cycle, Err(CompileError::Cycle)));
    }

    #[test]
    fn one_dynamic_change_dirties_itself_and_descendants_only() {
        let mut program = RetainedTransformProgram::compile(vec![
            AuthoredTransformNode {
                parent: None,
                ops: vec![AuthoredTransformOp::Dynamic(0)],
            },
            AuthoredTransformNode {
                parent: Some(0),
                ops: vec![AuthoredTransformOp::Dynamic(1)],
            },
            AuthoredTransformNode {
                parent: Some(1),
                ops: vec![AuthoredTransformOp::Dynamic(2)],
            },
            AuthoredTransformNode {
                parent: None,
                ops: vec![AuthoredTransformOp::Dynamic(3)],
            },
        ])
        .unwrap();
        assert_eq!(program.report().dirty_local, 4);
        assert_eq!(program.report().dirty_world, 4);

        program.begin_update();
        assert!(program.mark_dynamic_slot_dirty(1).unwrap());
        program.propagate_dirty();
        assert_eq!(program.dirty_row_ids, vec![1]);
        assert_eq!(program.dirty_local_node_ids, vec![1]);
        assert_eq!(program.dirty_world_node_ids, vec![1, 2]);
        assert_eq!(program.nodes[1].local_generation, 2);
        assert_eq!(program.nodes[2].local_generation, 1);
        assert_eq!(program.nodes[2].world_generation, 2);
        assert_eq!(program.nodes[3].world_generation, 1);
    }
}
