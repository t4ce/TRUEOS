use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;

use super::sidequest::{
    CalculatorLayout, CalculatorTile, CollapsedMarbleWorld, CollapsedWorldMarble, EtchedWorld,
    ImportedWidget, MarbleUniverseId, collapsed_world_marble, initialized_world_etchers,
};
use super::{Marble, MarbleGadget, WidgetKind};

// ---------------------------------------------------------------------------
// ArithOp — the eight operations the marble world understands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithOp {
    Add,
    Sub,
    And,
    Or,
    Xor,
    Not,
    Shl,
    Shr,
}

impl ArithOp {
    pub const fn name(self) -> &'static str {
        match self {
            ArithOp::Add => "add",
            ArithOp::Sub => "sub",
            ArithOp::And => "and",
            ArithOp::Or => "or",
            ArithOp::Xor => "xor",
            ArithOp::Not => "not",
            ArithOp::Shl => "shl",
            ArithOp::Shr => "shr",
        }
    }

    pub const fn opcode_u8(self) -> u8 {
        match self {
            ArithOp::Add => 0x00,
            ArithOp::Sub => 0x01,
            ArithOp::And => 0x02,
            ArithOp::Or => 0x03,
            ArithOp::Xor => 0x04,
            ArithOp::Not => 0x05,
            ArithOp::Shl => 0x06,
            ArithOp::Shr => 0x07,
        }
    }

    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x00 => Some(ArithOp::Add),
            0x01 => Some(ArithOp::Sub),
            0x02 => Some(ArithOp::And),
            0x03 => Some(ArithOp::Or),
            0x04 => Some(ArithOp::Xor),
            0x05 => Some(ArithOp::Not),
            0x06 => Some(ArithOp::Shl),
            0x07 => Some(ArithOp::Shr),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// ArithMarble — three kinds flow through the white hole
// ---------------------------------------------------------------------------

/// ControlN(3) → OpCode(op) → Operand(a) → Operand(b)
/// For unary ops (Not) operand B is ignored (pass 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithMarble {
    ControlN(usize),
    OpCode(u8),
    Operand(u8),
}

impl Marble for ArithMarble {
    fn kind(&self) -> &'static str {
        match self {
            ArithMarble::ControlN(_) => "arith-control-marble",
            ArithMarble::OpCode(_) => "arith-opcode-marble",
            ArithMarble::Operand(_) => "arith-operand-marble",
        }
    }
}

// ---------------------------------------------------------------------------
// ArithIntakeWidget — serial white-hole intake, same gating pattern as river
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArithIntakeState {
    WaitingControl,
    Filling { remaining: usize },
}

#[derive(Debug, Clone)]
pub struct ArithIntakeWidget {
    state: ArithIntakeState,
    payload: Vec<ArithMarble>,
}

impl ArithIntakeWidget {
    pub const fn new() -> Self {
        Self {
            state: ArithIntakeState::WaitingControl,
            payload: Vec::new(),
        }
    }

    pub fn push(&mut self, marble: ArithMarble) -> Result<(), ArithIntakeError> {
        match (self.state, marble) {
            (ArithIntakeState::WaitingControl, ArithMarble::ControlN(n)) => {
                if n == 0 {
                    return Err(ArithIntakeError::ZeroCount);
                }
                self.payload.clear();
                self.payload.reserve(n);
                self.state = ArithIntakeState::Filling { remaining: n };
                Ok(())
            }
            (ArithIntakeState::WaitingControl, _) => Err(ArithIntakeError::ExpectedControl),
            (ArithIntakeState::Filling { .. }, ArithMarble::ControlN(_)) => {
                Err(ArithIntakeError::UnexpectedControl)
            }
            (ArithIntakeState::Filling { remaining }, payload) => {
                if remaining == 0 {
                    return Err(ArithIntakeError::Overflow);
                }
                self.payload.push(payload);
                self.state = ArithIntakeState::Filling { remaining: remaining - 1 };
                Ok(())
            }
        }
    }

    pub fn is_full(&self) -> bool {
        matches!(self.state, ArithIntakeState::Filling { remaining: 0 })
    }

    pub fn take(&mut self) -> Result<ArithProblemInstance, ArithIntakeError> {
        if !self.is_full() {
            return Err(ArithIntakeError::NotFull);
        }

        // payload must be exactly [OpCode, Operand, Operand]
        if self.payload.len() != 3 {
            return Err(ArithIntakeError::WrongPayloadLen);
        }

        let op = match self.payload[0] {
            ArithMarble::OpCode(code) => {
                ArithOp::from_u8(code).ok_or(ArithIntakeError::UnknownOpcode)?
            }
            _ => return Err(ArithIntakeError::ExpectedOpcode),
        };
        let a = match self.payload[1] {
            ArithMarble::Operand(v) => v,
            _ => return Err(ArithIntakeError::ExpectedOperand),
        };
        let b = match self.payload[2] {
            ArithMarble::Operand(v) => v,
            _ => return Err(ArithIntakeError::ExpectedOperand),
        };

        self.payload.clear();
        self.state = ArithIntakeState::WaitingControl;

        Ok(ArithProblemInstance { op, a, b })
    }
}

impl Default for ArithIntakeWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl MarbleGadget for ArithIntakeWidget {
    fn name(&self) -> &'static str {
        "arith-intake"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithIntakeError {
    ExpectedControl,
    UnexpectedControl,
    ZeroCount,
    Overflow,
    NotFull,
    WrongPayloadLen,
    UnknownOpcode,
    ExpectedOpcode,
    ExpectedOperand,
}

// ---------------------------------------------------------------------------
// ArithProblemInstance — decoded from intake
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArithProblemInstance {
    pub op: ArithOp,
    pub a: u8,
    pub b: u8,
}

impl ArithProblemInstance {
    /// Full 16-bit result so callers can inspect bit 8 as carry/borrow.
    pub fn compute(self) -> u16 {
        match self.op {
            ArithOp::Add => (self.a as u16) + (self.b as u16),
            ArithOp::Sub => {
                if self.a >= self.b {
                    (self.a - self.b) as u16
                } else {
                    // bit 8 flags borrow; low byte = wrapping result
                    0x100u16 | (self.a.wrapping_sub(self.b) as u16)
                }
            }
            ArithOp::And => (self.a & self.b) as u16,
            ArithOp::Or => (self.a | self.b) as u16,
            ArithOp::Xor => (self.a ^ self.b) as u16,
            ArithOp::Not => (!self.a) as u16,
            ArithOp::Shl => (self.a as u16) << (self.b & 7),
            ArithOp::Shr => (self.a >> (self.b & 7)) as u16,
        }
    }

    pub fn result_byte(self) -> u8 {
        self.compute() as u8
    }

    /// True when carry (Add/Shl) or borrow (Sub) spills past 8 bits.
    pub fn carry_out(self) -> bool {
        self.compute() > 0xFF
    }
}

// ---------------------------------------------------------------------------
// Bit-to-tile mapping — the heart of the marble encoding
//
// Each of the 8 result bits, together with whether a carry/borrow propagates
// forward from that position, maps to exactly one CalculatorTile:
//
//   Race     — bit is 1, no carry:  the marble races through unimpeded
//   Portal   — bit is 1, carry out: marble portals its carry into next stage
//   Gather   — bit is 0, no carry:  quiescent gather, nothing to pass on
//   BoxStore — bit is 0, carry out: carry latched in a box while bit is low
// ---------------------------------------------------------------------------

fn bit_carry_to_tile(bit: bool, carry: bool) -> CalculatorTile {
    match (bit, carry) {
        (true, false) => CalculatorTile::Race,
        (true, true) => CalculatorTile::Portal,
        (false, false) => CalculatorTile::Gather,
        (false, true) => CalculatorTile::BoxStore,
    }
}

/// Builds the 8-position tile array that semantically encodes the operation.
fn build_bit_tiles(instance: ArithProblemInstance) -> [CalculatorTile; 8] {
    let mut tiles = [CalculatorTile::Gather; 8];
    let result = instance.result_byte();

    match instance.op {
        ArithOp::Add => {
            let mut carry = false;
            for i in 0..8u32 {
                let ba = (instance.a >> i) & 1 == 1;
                let bb = (instance.b >> i) & 1 == 1;
                let sum_bit = ba ^ bb ^ carry;
                let carry_out = (ba & bb) | (carry & (ba ^ bb));
                tiles[i as usize] = bit_carry_to_tile(sum_bit, carry_out);
                carry = carry_out;
            }
        }
        ArithOp::Sub => {
            let mut borrow = false;
            for i in 0..8u32 {
                let ba = (instance.a >> i) & 1 == 1;
                let bb = (instance.b >> i) & 1 == 1;
                let diff_bit = ba ^ bb ^ borrow;
                let borrow_out = (!ba & bb) | (borrow & !(ba ^ bb));
                tiles[i as usize] = bit_carry_to_tile(diff_bit, borrow_out);
                borrow = borrow_out;
            }
        }
        // No carry chain for bitwise / shift ops — result bits stand alone.
        _ => {
            for i in 0..8u32 {
                let bit = (result >> i) & 1 == 1;
                tiles[i as usize] = bit_carry_to_tile(bit, false);
            }
        }
    }

    tiles
}

// ---------------------------------------------------------------------------
// EightBitMathWorld — collapsed + etched world for one arithmetic computation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EightBitMathWorld {
    pub instance: ArithProblemInstance,
    pub result: u8,
    pub carry_out: bool,
    /// The 8 bit-position tiles (index 0 = LSB).
    pub bit_tiles: [CalculatorTile; 8],
    pub collapsed: CollapsedMarbleWorld,
    pub etched: EtchedWorld,
}

impl EightBitMathWorld {
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "eight-bit-math-world");
        let _ = writeln!(
            out,
            "op={} a=0x{:02x} b=0x{:02x}",
            self.instance.op.name(),
            self.instance.a,
            self.instance.b
        );
        let _ = writeln!(out, "result=0x{:02x} carry={}", self.result, self.carry_out);
        let _ = write!(out, "bit-tiles:");
        for (i, tile) in self.bit_tiles.iter().enumerate() {
            let _ = write!(out, " bit{}={}", i, tile_short(*tile));
        }
        let _ = writeln!(out);
        out.push_str(&self.collapsed.render());
        let _ = writeln!(out);
        out.push_str(&self.etched.render());
        out
    }
}

fn tile_short(tile: CalculatorTile) -> &'static str {
    match tile {
        CalculatorTile::Source => "SRC",
        CalculatorTile::Race => "RACE",
        CalculatorTile::Gather => "GATH",
        CalculatorTile::Portal => "PRTS",
        CalculatorTile::BoxStore => "BSTR",
        CalculatorTile::Sink => "SINK",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathWorldError {
    EtcherRejected,
}

/// Build the collapsed marble world from a problem instance and etch via Etcher1.
///
/// Layout: Source | bit[0..8] | Sink  (10 tiles total)
/// - Source @ tile 0 — ingress white-hole
/// - Sink   @ tile 9 — egress black-hole
/// - Each of the 8 bit tiles gets a Root (tile 0) or Tag (tiles 1..8) widget
///   whose `preferred_tile` matches the computed bit-carry tile.
pub fn place_eight_bit_math_world(
    universe: MarbleUniverseId,
    instance: ArithProblemInstance,
) -> Result<EightBitMathWorld, MathWorldError> {
    let result = instance.result_byte();
    let carry_out = instance.carry_out();
    let bit_tiles = build_bit_tiles(instance);

    // 10-tile layout
    let mut tiles = Vec::with_capacity(10);
    tiles.push(CalculatorTile::Source);
    for &bt in &bit_tiles {
        tiles.push(bt);
    }
    tiles.push(CalculatorTile::Sink);

    // Widgets: Root anchors tile 0, Tag per bit position
    let root = ImportedWidget {
        kind: WidgetKind::Root,
        preferred_tile: CalculatorTile::Source,
    };
    let mut imported_widgets = Vec::with_capacity(9);
    let mut placements: Vec<(ImportedWidget, usize)> = Vec::with_capacity(9);
    imported_widgets.push(root);
    placements.push((root, 0));

    for (i, &bt) in bit_tiles.iter().enumerate() {
        let tag = ImportedWidget { kind: WidgetKind::Tag, preferred_tile: bt };
        imported_widgets.push(tag);
        placements.push((tag, i + 1));
    }

    let collapsed = CollapsedMarbleWorld {
        layout: CalculatorLayout { tiles: tiles.clone() },
        widgets: imported_widgets,
        placements,
        contradictions: Vec::new(),
        propagation_steps: 0,
        conditions: Vec::new(),
    };

    let collapsed_marble = collapsed_world_marble(universe, collapsed.clone(), tiles.len());

    let mut etchers = initialized_world_etchers();
    let etched = etchers[0].etch(collapsed_marble).map_err(|_| MathWorldError::EtcherRejected)?;

    Ok(EightBitMathWorld { instance, result, carry_out, bit_tiles, collapsed, etched })
}

// ---------------------------------------------------------------------------
// Pipeline helpers
// ---------------------------------------------------------------------------

/// Encode one arithmetic operation as a serial marble stream ready for intake.
/// Always 4 marbles: ControlN(3) | OpCode | Operand(a) | Operand(b).
pub fn to_serial_arith_stream(op: ArithOp, a: u8, b: u8) -> Vec<ArithMarble> {
    vec![
        ArithMarble::ControlN(3),
        ArithMarble::OpCode(op.opcode_u8()),
        ArithMarble::Operand(a),
        ArithMarble::Operand(b),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithPipelineError {
    Intake(ArithIntakeError),
    Placement(MathWorldError),
}

/// Feed a marble stream through intake, decode the operation, then collapse +
/// etch the 8-bit math world via Etcher1.
pub fn arith_intake_and_place(
    universe: MarbleUniverseId,
    stream: &[ArithMarble],
) -> Result<EightBitMathWorld, ArithPipelineError> {
    let mut intake = ArithIntakeWidget::new();
    for &marble in stream {
        intake.push(marble).map_err(ArithPipelineError::Intake)?;
    }
    let instance = intake.take().map_err(ArithPipelineError::Intake)?;
    place_eight_bit_math_world(universe, instance).map_err(ArithPipelineError::Placement)
}

/// Demo visual: ADD 0x2A + 0x15 = 0x3F, no carry.
pub fn arith_pipeline_visual() -> String {
    let stream = to_serial_arith_stream(ArithOp::Add, 0x2A, 0x15);
    let world = arith_intake_and_place(MarbleUniverseId(400), &stream).unwrap();
    world.render()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_0x2a_plus_0x15_gives_0x3f() {
        let inst = ArithProblemInstance { op: ArithOp::Add, a: 0x2A, b: 0x15 };
        assert_eq!(inst.result_byte(), 0x3F);
        assert!(!inst.carry_out());
    }

    #[test]
    fn add_overflow_sets_carry() {
        let inst = ArithProblemInstance { op: ArithOp::Add, a: 0xFF, b: 0x01 };
        assert_eq!(inst.result_byte(), 0x00);
        assert!(inst.carry_out());
    }

    #[test]
    fn sub_borrow_sets_carry() {
        let inst = ArithProblemInstance { op: ArithOp::Sub, a: 0x10, b: 0x20 };
        assert_eq!(inst.result_byte(), 0xF0);
        assert!(inst.carry_out());
    }

    #[test]
    fn not_inverts_byte() {
        let stream = to_serial_arith_stream(ArithOp::Not, 0xAA, 0x00);
        let world = arith_intake_and_place(MarbleUniverseId(402), &stream).unwrap();
        assert_eq!(world.result, 0x55);
        assert!(!world.carry_out);
    }

    #[test]
    fn and_masks_nibbles() {
        let stream = to_serial_arith_stream(ArithOp::And, 0xF0, 0x0F);
        let world = arith_intake_and_place(MarbleUniverseId(403), &stream).unwrap();
        assert_eq!(world.result, 0x00);
    }

    #[test]
    fn xor_uses_race_for_set_bits() {
        // 0b10101010 ^ 0b11001100 = 0b01100110 = 0x66
        let inst = ArithProblemInstance { op: ArithOp::Xor, a: 0xAA, b: 0xCC };
        let result = inst.result_byte();
        let tiles = build_bit_tiles(inst);
        for i in 0..8usize {
            let bit_set = (result >> i) & 1 == 1;
            let expected = if bit_set { CalculatorTile::Race } else { CalculatorTile::Gather };
            assert_eq!(tiles[i], expected, "bit {i}");
        }
    }

    #[test]
    fn add_carry_chain_tiles_include_portal_and_boxstore() {
        // 0xFF + 0x01: bit 0..6 should all carry → Portal or BoxStore
        let inst = ArithProblemInstance { op: ArithOp::Add, a: 0xFF, b: 0x01 };
        let tiles = build_bit_tiles(inst);
        // bit 0: 1^1^0=0, carry=(1&1)|(0)=1 → BoxStore
        assert_eq!(tiles[0], CalculatorTile::BoxStore);
    }

    #[test]
    fn intake_rejects_payload_before_control() {
        let mut intake = ArithIntakeWidget::new();
        assert_eq!(
            intake.push(ArithMarble::Operand(0x01)),
            Err(ArithIntakeError::ExpectedControl)
        );
    }

    #[test]
    fn full_pipeline_uses_etcher1() {
        let stream = to_serial_arith_stream(ArithOp::Or, 0xAA, 0x55);
        let world = arith_intake_and_place(MarbleUniverseId(404), &stream).unwrap();
        assert_eq!(world.result, 0xFF);
        assert_eq!(world.etched.etcher.name(), "etcher1");
        // 10 tiles: Source + 8 bit tiles + Sink
        assert_eq!(world.collapsed.layout.tiles.len(), 10);
    }

    #[test]
    fn visual_renders_correctly() {
        let v = arith_pipeline_visual();
        assert!(v.contains("eight-bit-math-world"));
        assert!(v.contains("op=add"));
        assert!(v.contains("result=0x3f"));
        assert!(v.contains("etcher=etcher1"));
    }
}
