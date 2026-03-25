use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use super::{Marble, MarbleGadget, MarblePackage, MarbleTransform};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteMarble {
    pub lane: usize,
    pub byte: u8,
}

impl Marble for ByteMarble {
    fn kind(&self) -> &'static str {
        "byte-marble"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytePackage {
    lanes: Vec<ByteMarble>,
    pub bit_width: usize,
    pub storage_bytes: usize,
    pub overflowed: bool,
}

impl BytePackage {
    pub fn from_bytes(bit_width: usize, bytes_le: &[u8]) -> Self {
        Self::from_bytes_with_storage(bit_width, byte_count_for_bits(bit_width), bytes_le)
    }

    pub fn from_bytes_with_storage(
        bit_width: usize,
        storage_bytes: usize,
        bytes_le: &[u8],
    ) -> Self {
        let byte_count = byte_count_for_bits(bit_width);
        let storage_bytes = storage_bytes.max(byte_count);
        let mut lanes = Vec::with_capacity(storage_bytes);
        for lane in 0..storage_bytes {
            let byte = bytes_le.get(lane).copied().unwrap_or(0);
            lanes.push(ByteMarble { lane, byte });
        }

        let mut out = Self {
            lanes,
            bit_width,
            storage_bytes,
            overflowed: false,
        };
        out.mask_unused_high_bits();
        out
    }

    pub fn from_words(bit_width: usize, words_le: &[u64]) -> Self {
        Self::from_words_with_storage(bit_width, byte_count_for_bits(bit_width), words_le)
    }

    pub fn from_words_with_storage(
        bit_width: usize,
        storage_bytes: usize,
        words_le: &[u64],
    ) -> Self {
        let mut bytes = Vec::with_capacity(words_le.len().saturating_mul(8));
        for word in words_le.iter().copied() {
            for shift in 0..8 {
                bytes.push(((word >> (shift * 8)) & 0xFF) as u8);
            }
        }
        Self::from_bytes_with_storage(bit_width, storage_bytes, &bytes)
    }

    pub fn bytes_le(&self) -> Vec<u8> {
        self.lanes.iter().map(|lane| lane.byte).collect()
    }

    pub fn words_le(&self) -> Vec<u64> {
        let bytes = self.bytes_le();
        let mut words = Vec::with_capacity(bytes.len().div_ceil(8));
        for chunk in bytes.chunks(8) {
            let mut word = 0u64;
            for (index, byte) in chunk.iter().copied().enumerate() {
                word |= (byte as u64) << (index * 8);
            }
            words.push(word);
        }
        words
    }

    pub fn lane_count(&self) -> usize {
        self.lanes.len()
    }

    pub fn mapped_u64_count(&self) -> usize {
        self.lanes.len().div_ceil(8)
    }

    pub fn render_hex(&self) -> String {
        let mut out = String::new();
        let _ = write!(
            out,
            "bits={} overflow={} bytes_le=[",
            self.bit_width, self.overflowed
        );
        for (index, lane) in self.lanes.iter().enumerate() {
            if index != 0 {
                out.push(' ');
            }
            let _ = write!(out, "{:02X}", lane.byte);
        }
        out.push(']');
        out
    }

    fn mask_unused_high_bits(&mut self) {
        let live_bits = self.bit_width % 8;
        if live_bits == 0 || self.lanes.is_empty() {
            return;
        }
        let mask = (1u8 << live_bits) - 1;
        let last_index = byte_count_for_bits(self.bit_width) - 1;
        self.lanes[last_index].byte &= mask;
    }
}

impl Marble for BytePackage {
    fn kind(&self) -> &'static str {
        "byte-package"
    }
}

impl MarblePackage<ByteMarble> for BytePackage {
    fn width(&self) -> usize {
        self.lanes.len()
    }

    fn lane(&self, index: usize) -> Option<&ByteMarble> {
        self.lanes.get(index)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalculatorTopologyNodeKind {
    WhiteHole,
    Selector,
    MarblePlus1,
    MarbleTimes2x,
    BlackHole,
}

impl CalculatorTopologyNodeKind {
    pub const fn name(self) -> &'static str {
        match self {
            CalculatorTopologyNodeKind::WhiteHole => "white-hole",
            CalculatorTopologyNodeKind::Selector => "selector-widget",
            CalculatorTopologyNodeKind::MarblePlus1 => "marble_+1",
            CalculatorTopologyNodeKind::MarbleTimes2x => "marble_times_2x",
            CalculatorTopologyNodeKind::BlackHole => "black-hole",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalculatorTopologyLaneKind {
    Data37,
    Control1,
    Data38,
}

impl CalculatorTopologyLaneKind {
    pub const fn name(self) -> &'static str {
        match self {
            CalculatorTopologyLaneKind::Data37 => "data[37]",
            CalculatorTopologyLaneKind::Control1 => "control[1]",
            CalculatorTopologyLaneKind::Data38 => "data[38]",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalculatorTopologyNode {
    pub id: usize,
    pub kind: CalculatorTopologyNodeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalculatorTopologyEdge {
    pub id: usize,
    pub from: usize,
    pub to: usize,
    pub lane: CalculatorTopologyLaneKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalculatorCcwTopology {
    pub nodes: Vec<CalculatorTopologyNode>,
    pub edges: Vec<CalculatorTopologyEdge>,
}

impl CalculatorCcwTopology {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "topology-nodes");
        for node in &self.nodes {
            let _ = writeln!(out, "node{}={}", node.id, node.kind.name());
        }
        let _ = writeln!(out, "topology-edges");
        for edge in &self.edges {
            let from = self
                .nodes
                .get(edge.from)
                .map(|node| node.kind.name())
                .unwrap_or("?");
            let to = self
                .nodes
                .get(edge.to)
                .map(|node| node.kind.name())
                .unwrap_or("?");
            let _ = writeln!(
                out,
                "edge{}={} -> {} via {}",
                edge.id,
                from,
                to,
                edge.lane.name()
            );
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalculatorEdgeLoad {
    pub edge_id: usize,
    pub marble_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalculatorCcwRunReport {
    pub route: CalculatorControlRoute,
    pub output: BytePackage,
    pub edge_loads: Vec<CalculatorEdgeLoad>,
}

impl CalculatorCcwRunReport {
    pub fn render(&self, topology: &CalculatorCcwTopology) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "route={}", self.route.name());
        let _ = writeln!(out, "edge-loads");
        for load in &self.edge_loads {
            let edge = topology.edges.get(load.edge_id);
            let from = edge
                .and_then(|edge| topology.nodes.get(edge.from))
                .map(|node| node.kind.name())
                .unwrap_or("?");
            let to = edge
                .and_then(|edge| topology.nodes.get(edge.to))
                .map(|node| node.kind.name())
                .unwrap_or("?");
            let lane = edge.map(|edge| edge.lane.name()).unwrap_or("?");
            let _ = writeln!(
                out,
                "edge{}={} -> {} via {} marbles={}",
                load.edge_id, from, to, lane, load.marble_count
            );
        }
        let _ = writeln!(out, "output={}", self.output.render_hex());
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CcwHole {
    pub index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalculatorControlMarble {
    pub bit: u8,
}

impl CalculatorControlMarble {
    pub const fn zero() -> Self {
        Self { bit: 0 }
    }

    pub const fn one() -> Self {
        Self { bit: 1 }
    }

    pub const fn normalized(self) -> u8 {
        self.bit & 1
    }
}

impl Marble for CalculatorControlMarble {
    fn kind(&self) -> &'static str {
        "calculator-control-marble"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalculatorControlRoute {
    MarblePlus1,
    MarbleTimes2x,
}

impl CalculatorControlRoute {
    pub const fn name(self) -> &'static str {
        match self {
            CalculatorControlRoute::MarblePlus1 => "marble_+1",
            CalculatorControlRoute::MarbleTimes2x => "marble_times_2x",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CalculatorSelectorWidget;

impl MarbleGadget for CalculatorSelectorWidget {
    fn name(&self) -> &'static str {
        "calculator-selector-widget"
    }
}

impl CalculatorSelectorWidget {
    pub fn route(&mut self, control: CalculatorControlMarble) -> CalculatorControlRoute {
        if control.normalized() == 0 {
            CalculatorControlRoute::MarblePlus1
        } else {
            CalculatorControlRoute::MarbleTimes2x
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ByteIncrementCarryMarble {
    lane: usize,
    byte: u8,
    carry: bool,
    live_bits: u8,
}

impl Marble for ByteIncrementCarryMarble {
    fn kind(&self) -> &'static str {
        "byte-increment-carry-marble"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ByteTimes2xCarryMarble {
    lane: usize,
    byte: u8,
    carry: bool,
    live_bits: u8,
}

impl Marble for ByteTimes2xCarryMarble {
    fn kind(&self) -> &'static str {
        "byte-times-2x-carry-marble"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ByteIncrementCellGadget;

impl MarbleGadget for ByteIncrementCellGadget {
    fn name(&self) -> &'static str {
        "byte-increment-cell-gadget"
    }
}

impl MarbleTransform<ByteIncrementCarryMarble, ByteIncrementCarryMarble>
    for ByteIncrementCellGadget
{
    type Error = ();

    fn transform(
        &mut self,
        marble: ByteIncrementCarryMarble,
    ) -> Result<ByteIncrementCarryMarble, Self::Error> {
        let width = marble.live_bits.clamp(1, 8);
        let mask = if width == 8 {
            u16::from(u8::MAX)
        } else {
            (1u16 << width) - 1
        };
        let sum = u16::from(marble.byte & mask as u8) + u16::from(marble.carry);
        Ok(ByteIncrementCarryMarble {
            lane: marble.lane,
            byte: (sum & mask) as u8,
            carry: sum > mask,
            live_bits: width,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ByteTimes2xCellGadget;

impl MarbleGadget for ByteTimes2xCellGadget {
    fn name(&self) -> &'static str {
        "byte-times-2x-cell-gadget"
    }
}

impl MarbleTransform<ByteTimes2xCarryMarble, ByteTimes2xCarryMarble> for ByteTimes2xCellGadget {
    type Error = ();

    fn transform(
        &mut self,
        marble: ByteTimes2xCarryMarble,
    ) -> Result<ByteTimes2xCarryMarble, Self::Error> {
        let width = marble.live_bits.clamp(1, 8);
        let mask = if width == 8 {
            u16::from(u8::MAX)
        } else {
            (1u16 << width) - 1
        };
        let sum = (u16::from(marble.byte & mask as u8) << 1) + u16::from(marble.carry);
        Ok(ByteTimes2xCarryMarble {
            lane: marble.lane,
            byte: (sum & mask) as u8,
            carry: sum > mask,
            live_bits: width,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarblePlus1Widget {
    incrementer: ByteIncrementCellGadget,
}

impl MarblePlus1Widget {
    pub const fn new() -> Self {
        Self {
            incrementer: ByteIncrementCellGadget,
        }
    }

    pub fn run(&mut self, bit_width: usize, input: BytePackage) -> BytePackage {
        let input_len = byte_count_for_bits(bit_width);
        let input_bytes = input.bytes_le();
        let mut out = Vec::with_capacity(input_len + 1);
        let mut carry = true;

        for lane in 0..input_len {
            let live_bits = live_bits_for_lane(bit_width, lane, input_len);
            let step = ByteIncrementCarryMarble {
                lane,
                byte: input_bytes.get(lane).copied().unwrap_or(0),
                carry,
                live_bits,
            };
            let result = self
                .incrementer
                .transform(step)
                .unwrap_or(ByteIncrementCarryMarble {
                    lane,
                    byte: 0,
                    carry: true,
                    live_bits,
                });
            out.push(result.byte);
            carry = result.carry;
        }

        build_ccw_output(bit_width, out, carry)
    }
}

impl Default for MarblePlus1Widget {
    fn default() -> Self {
        Self::new()
    }
}

impl MarbleGadget for MarblePlus1Widget {
    fn name(&self) -> &'static str {
        CalculatorControlRoute::MarblePlus1.name()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarbleTimes2xWidget {
    doubler: ByteTimes2xCellGadget,
}

impl MarbleTimes2xWidget {
    pub const fn new() -> Self {
        Self {
            doubler: ByteTimes2xCellGadget,
        }
    }

    pub fn run(&mut self, bit_width: usize, input: BytePackage) -> BytePackage {
        let input_len = byte_count_for_bits(bit_width);
        let input_bytes = input.bytes_le();
        let mut out = Vec::with_capacity(input_len + 1);
        let mut carry = false;

        for lane in 0..input_len {
            let live_bits = live_bits_for_lane(bit_width, lane, input_len);
            let step = ByteTimes2xCarryMarble {
                lane,
                byte: input_bytes.get(lane).copied().unwrap_or(0),
                carry,
                live_bits,
            };
            let result = self
                .doubler
                .transform(step)
                .unwrap_or(ByteTimes2xCarryMarble {
                    lane,
                    byte: 0,
                    carry: true,
                    live_bits,
                });
            out.push(result.byte);
            carry = result.carry;
        }

        build_ccw_output(bit_width, out, carry)
    }
}

impl Default for MarbleTimes2xWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl MarbleGadget for MarbleTimes2xWidget {
    fn name(&self) -> &'static str {
        CalculatorControlRoute::MarbleTimes2x.name()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalculatorCollapsedWorld {
    pub bit_width: usize,
    pub white_hole: CcwHole,
    pub control_lane: usize,
    pub black_hole: CcwHole,
    pub selector: CalculatorSelectorWidget,
    pub marble_plus_1: MarblePlus1Widget,
    pub marble_times_2x: MarbleTimes2xWidget,
}

impl CalculatorCollapsedWorld {
    pub const NAME: &'static str = "calculator-ccw";
    pub const WHITE_TO_SELECTOR_DATA_EDGE: usize = 0;
    pub const WHITE_TO_SELECTOR_CONTROL_EDGE: usize = 1;
    pub const SELECTOR_TO_PLUS1_EDGE: usize = 2;
    pub const SELECTOR_TO_TIMES2X_EDGE: usize = 3;
    pub const PLUS1_TO_BLACK_EDGE: usize = 4;
    pub const TIMES2X_TO_BLACK_EDGE: usize = 5;

    pub const fn new(bit_width: usize) -> Self {
        Self {
            bit_width,
            white_hole: CcwHole { index: 0 },
            control_lane: 37,
            black_hole: CcwHole { index: 1 },
            selector: CalculatorSelectorWidget,
            marble_plus_1: MarblePlus1Widget::new(),
            marble_times_2x: MarbleTimes2xWidget::new(),
        }
    }

    pub fn topology(&self) -> CalculatorCcwTopology {
        CalculatorCcwTopology {
            nodes: vec![
                CalculatorTopologyNode {
                    id: 0,
                    kind: CalculatorTopologyNodeKind::WhiteHole,
                },
                CalculatorTopologyNode {
                    id: 1,
                    kind: CalculatorTopologyNodeKind::Selector,
                },
                CalculatorTopologyNode {
                    id: 2,
                    kind: CalculatorTopologyNodeKind::MarblePlus1,
                },
                CalculatorTopologyNode {
                    id: 3,
                    kind: CalculatorTopologyNodeKind::MarbleTimes2x,
                },
                CalculatorTopologyNode {
                    id: 4,
                    kind: CalculatorTopologyNodeKind::BlackHole,
                },
            ],
            edges: vec![
                CalculatorTopologyEdge {
                    id: Self::WHITE_TO_SELECTOR_DATA_EDGE,
                    from: 0,
                    to: 1,
                    lane: CalculatorTopologyLaneKind::Data37,
                },
                CalculatorTopologyEdge {
                    id: Self::WHITE_TO_SELECTOR_CONTROL_EDGE,
                    from: 0,
                    to: 1,
                    lane: CalculatorTopologyLaneKind::Control1,
                },
                CalculatorTopologyEdge {
                    id: Self::SELECTOR_TO_PLUS1_EDGE,
                    from: 1,
                    to: 2,
                    lane: CalculatorTopologyLaneKind::Data37,
                },
                CalculatorTopologyEdge {
                    id: Self::SELECTOR_TO_TIMES2X_EDGE,
                    from: 1,
                    to: 3,
                    lane: CalculatorTopologyLaneKind::Data37,
                },
                CalculatorTopologyEdge {
                    id: Self::PLUS1_TO_BLACK_EDGE,
                    from: 2,
                    to: 4,
                    lane: CalculatorTopologyLaneKind::Data38,
                },
                CalculatorTopologyEdge {
                    id: Self::TIMES2X_TO_BLACK_EDGE,
                    from: 3,
                    to: 4,
                    lane: CalculatorTopologyLaneKind::Data38,
                },
            ],
        }
    }

    pub fn execute(
        &mut self,
        input: BytePackage,
        control: CalculatorControlMarble,
    ) -> CalculatorCcwRunReport {
        let route = self.selector.route(control);
        let output = match route {
            CalculatorControlRoute::MarblePlus1 => self.marble_plus_1.run(self.bit_width, input),
            CalculatorControlRoute::MarbleTimes2x => {
                self.marble_times_2x.run(self.bit_width, input)
            }
        };

        let input_width = byte_count_for_bits(self.bit_width);
        let mut edge_loads = vec![
            CalculatorEdgeLoad {
                edge_id: Self::WHITE_TO_SELECTOR_DATA_EDGE,
                marble_count: input_width,
            },
            CalculatorEdgeLoad {
                edge_id: Self::WHITE_TO_SELECTOR_CONTROL_EDGE,
                marble_count: 1,
            },
        ];

        match route {
            CalculatorControlRoute::MarblePlus1 => {
                edge_loads.push(CalculatorEdgeLoad {
                    edge_id: Self::SELECTOR_TO_PLUS1_EDGE,
                    marble_count: input_width,
                });
                edge_loads.push(CalculatorEdgeLoad {
                    edge_id: Self::PLUS1_TO_BLACK_EDGE,
                    marble_count: output.width(),
                });
            }
            CalculatorControlRoute::MarbleTimes2x => {
                edge_loads.push(CalculatorEdgeLoad {
                    edge_id: Self::SELECTOR_TO_TIMES2X_EDGE,
                    marble_count: input_width,
                });
                edge_loads.push(CalculatorEdgeLoad {
                    edge_id: Self::TIMES2X_TO_BLACK_EDGE,
                    marble_count: output.width(),
                });
            }
        }

        CalculatorCcwRunReport {
            route,
            output,
            edge_loads,
        }
    }

    pub fn run_once(
        &mut self,
        input: BytePackage,
        control: CalculatorControlMarble,
    ) -> (CalculatorControlRoute, BytePackage) {
        let report = self.execute(input, control);
        (report.route, report.output)
    }
}

pub fn calculator_ccw_289() -> CalculatorCollapsedWorld {
    CalculatorCollapsedWorld::new(289)
}

pub fn calculator_ccw_marble_plus_1_visual() -> String {
    let mut world = calculator_ccw_289();
    let input = BytePackage::from_words(289, &[0xFFFF_FFFF_FFFF_FFFF, 0, 0, 0, 0x1]);
    let control = CalculatorControlMarble::zero();
    render_ccw_run(&mut world, input, control)
}

pub fn calculator_ccw_marble_times_2x_visual() -> String {
    let mut world = calculator_ccw_289();
    let input = BytePackage::from_words(289, &[0x80, 0, 0, 0, 0x1]);
    let control = CalculatorControlMarble::one();
    render_ccw_run(&mut world, input, control)
}

fn render_ccw_run(
    world: &mut CalculatorCollapsedWorld,
    input: BytePackage,
    control: CalculatorControlMarble,
) -> String {
    let topology = world.topology();
    let report = world.execute(input.clone(), control);
    let mut out = String::new();
    let _ = writeln!(out, "{}", CalculatorCollapsedWorld::NAME);
    let _ = writeln!(out, "white-hole={}", world.white_hole.index);
    let _ = writeln!(
        out,
        "data-lanes-in={}",
        byte_count_for_bits(world.bit_width)
    );
    let _ = writeln!(out, "control-lane={} via white-hole", world.control_lane);
    let _ = writeln!(out, "selector={}", world.selector.name());
    let _ = writeln!(out, "selected-route={}", report.route.name());
    let _ = writeln!(out, "widget-a={}", world.marble_plus_1.name());
    let _ = writeln!(out, "widget-b={}", world.marble_times_2x.name());
    let _ = writeln!(out, "black-hole={}", world.black_hole.index);
    let _ = writeln!(out, "input-kind={}", input.kind());
    let _ = writeln!(out, "input={}", input.render_hex());
    let _ = writeln!(out, "output-kind={}", report.output.kind());
    let _ = writeln!(out, "output={}", report.output.render_hex());
    let _ = writeln!(out);
    let _ = write!(out, "{}", topology.render());
    let _ = write!(out, "{}", report.render(&topology));
    out
}

fn build_ccw_output(bit_width: usize, mut out: Vec<u8>, carry: bool) -> BytePackage {
    let input_len = byte_count_for_bits(bit_width);
    out.push(u8::from(carry));
    let mut output = BytePackage::from_bytes_with_storage(bit_width, input_len + 1, &out);
    output.overflowed = carry;
    output
}

fn live_bits_for_lane(bit_width: usize, lane: usize, input_len: usize) -> u8 {
    if lane + 1 == input_len && !bit_width.is_multiple_of(8) {
        (bit_width % 8) as u8
    } else {
        8
    }
}

fn byte_count_for_bits(bit_width: usize) -> usize {
    bit_width.div_ceil(8).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculator_ccw_routes_control_zero_to_marble_plus_1() {
        let mut world = calculator_ccw_289();
        let input = BytePackage::from_words(289, &[0, 0, 0, 0, 0]);

        let (route, output) = world.run_once(input, CalculatorControlMarble::zero());

        assert_eq!(route, CalculatorControlRoute::MarblePlus1);
        assert_eq!(output.width(), 38);
        assert_eq!(output.mapped_u64_count(), 5);
        assert_eq!(output.bytes_le()[37], 0);
        assert_eq!(output.words_le(), vec![1, 0, 0, 0, 0]);
        assert!(!output.overflowed);
    }

    #[test]
    fn calculator_ccw_routes_control_one_to_marble_times_2x() {
        let mut world = calculator_ccw_289();
        let input = BytePackage::from_words(289, &[0x15, 0, 0, 0, 0]);

        let (route, output) = world.run_once(input, CalculatorControlMarble::one());

        assert_eq!(route, CalculatorControlRoute::MarbleTimes2x);
        assert_eq!(output.width(), 38);
        assert_eq!(output.words_le(), vec![0x2A, 0, 0, 0, 0]);
        assert_eq!(output.bytes_le()[37], 0);
        assert!(!output.overflowed);
    }

    #[test]
    fn calculator_ccw_topology_is_explicit_and_stable() {
        let world = calculator_ccw_289();
        let topology = world.topology();

        assert_eq!(topology.nodes.len(), 5);
        assert_eq!(topology.edges.len(), 6);
        assert_eq!(
            topology.nodes[0].kind,
            CalculatorTopologyNodeKind::WhiteHole
        );
        assert_eq!(topology.nodes[1].kind, CalculatorTopologyNodeKind::Selector);
        assert_eq!(
            topology.nodes[4].kind,
            CalculatorTopologyNodeKind::BlackHole
        );
        assert_eq!(topology.edges[0].lane, CalculatorTopologyLaneKind::Data37);
        assert_eq!(topology.edges[1].lane, CalculatorTopologyLaneKind::Control1);
        assert_eq!(topology.edges[4].lane, CalculatorTopologyLaneKind::Data38);
    }

    #[test]
    fn calculator_ccw_report_marks_active_plus1_branch() {
        let mut world = calculator_ccw_289();
        let input = BytePackage::from_words(289, &[0, 0, 0, 0, 0]);

        let report = world.execute(input, CalculatorControlMarble::zero());

        assert_eq!(report.route, CalculatorControlRoute::MarblePlus1);
        assert_eq!(report.edge_loads.len(), 4);
        assert!(report.edge_loads.iter().any(|load| {
            load.edge_id == CalculatorCollapsedWorld::WHITE_TO_SELECTOR_DATA_EDGE
                && load.marble_count == 37
        }));
        assert!(report.edge_loads.iter().any(|load| {
            load.edge_id == CalculatorCollapsedWorld::WHITE_TO_SELECTOR_CONTROL_EDGE
                && load.marble_count == 1
        }));
        assert!(report.edge_loads.iter().any(|load| {
            load.edge_id == CalculatorCollapsedWorld::SELECTOR_TO_PLUS1_EDGE
                && load.marble_count == 37
        }));
        assert!(report.edge_loads.iter().any(|load| {
            load.edge_id == CalculatorCollapsedWorld::PLUS1_TO_BLACK_EDGE && load.marble_count == 38
        }));
    }

    #[test]
    fn calculator_ccw_report_marks_active_times2x_branch() {
        let mut world = calculator_ccw_289();
        let input = BytePackage::from_words(289, &[1, 0, 0, 0, 0]);

        let report = world.execute(input, CalculatorControlMarble::one());

        assert_eq!(report.route, CalculatorControlRoute::MarbleTimes2x);
        assert_eq!(report.edge_loads.len(), 4);
        assert!(report.edge_loads.iter().any(|load| {
            load.edge_id == CalculatorCollapsedWorld::SELECTOR_TO_TIMES2X_EDGE
                && load.marble_count == 37
        }));
        assert!(report.edge_loads.iter().any(|load| {
            load.edge_id == CalculatorCollapsedWorld::TIMES2X_TO_BLACK_EDGE
                && load.marble_count == 38
        }));
    }

    #[test]
    fn marble_plus_1_keeps_overflow_explicit_at_black_hole() {
        let mut world = calculator_ccw_289();
        let input = BytePackage::from_words(
            289,
            &[u64::MAX, u64::MAX, u64::MAX, u64::MAX, 0x1_FFFF_FFFF],
        );

        let (route, output) = world.run_once(input, CalculatorControlMarble::zero());

        assert_eq!(route.name(), "marble_+1");
        assert_eq!(output.bytes_le()[..37], [0; 37]);
        assert_eq!(output.bytes_le()[37], 1);
        assert!(output.overflowed);
        assert!(calculator_ccw_marble_plus_1_visual().contains("selected-route=marble_+1"));
        assert!(calculator_ccw_marble_plus_1_visual().contains("topology-nodes"));
    }

    #[test]
    fn marble_times_2x_keeps_overflow_explicit_at_black_hole() {
        let mut world = calculator_ccw_289();
        let input = BytePackage::from_words(
            289,
            &[u64::MAX, u64::MAX, u64::MAX, u64::MAX, 0x1_FFFF_FFFF],
        );

        let (route, output) = world.run_once(input, CalculatorControlMarble::one());
        let mut expected = vec![0xFE; 36];
        expected.push(0x01);

        assert_eq!(route.name(), "marble_times_2x");
        assert_eq!(output.bytes_le()[..37], expected);
        assert_eq!(output.bytes_le()[37], 1);
        assert!(output.overflowed);
        assert!(calculator_ccw_marble_times_2x_visual().contains("selected-route=marble_times_2x"));
        assert!(calculator_ccw_marble_times_2x_visual().contains("edge-loads"));
    }
}
