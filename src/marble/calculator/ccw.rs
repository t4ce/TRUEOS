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

    pub fn run_once(
        &mut self,
        input: BytePackage,
        control: CalculatorControlMarble,
    ) -> (CalculatorControlRoute, BytePackage) {
        let route = self.selector.route(control);
        let output = match route {
            CalculatorControlRoute::MarblePlus1 => self.marble_plus_1.run(self.bit_width, input),
            CalculatorControlRoute::MarbleTimes2x => {
                self.marble_times_2x.run(self.bit_width, input)
            }
        };
        (route, output)
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
    let (route, output) = world.run_once(input.clone(), control);
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
    let _ = writeln!(out, "selected-route={}", route.name());
    let _ = writeln!(out, "widget-a={}", world.marble_plus_1.name());
    let _ = writeln!(out, "widget-b={}", world.marble_times_2x.name());
    let _ = writeln!(out, "black-hole={}", world.black_hole.index);
    let _ = writeln!(out, "input-kind={}", input.kind());
    let _ = writeln!(out, "input={}", input.render_hex());
    let _ = writeln!(out, "output-kind={}", output.kind());
    let _ = writeln!(out, "output={}", output.render_hex());
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
    }
}
