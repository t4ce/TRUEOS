use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use super::sidequest::collapse_calculator_layout;
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
        for marble in &marbles {
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
                out.push('.');
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
    fn example_visual_mentions_layout_and_release() {
        let visual = marble_calculator_example_visual();
        assert!(visual.contains("layout"));
        assert!(visual.contains("collapse 0"));
        assert!(visual.contains("released"));
    }
}
