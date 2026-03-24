//! Raw no-std NTM playground built around configuration frontiers.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

pub type StateId = u16;
pub type Symbol = u8;

pub const STATE_START: StateId = 0;
pub const STATE_ACCEPT: StateId = 1;
pub const STATE_REJECT: StateId = 2;
pub const BLANK: Symbol = b'_';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Stay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionRule {
    pub state: StateId,
    pub read: Symbol,
    pub next_state: StateId,
    pub write: Symbol,
    pub dir: Dir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NtmHalt {
    Accept,
    Exhausted,
    StepLimit,
    FrontierLimit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tape {
    cells: VecDeque<Symbol>,
    head: usize,
    blank: Symbol,
}

impl Tape {
    pub fn from_bytes(input: &[u8], blank: Symbol) -> Self {
        let mut cells = VecDeque::with_capacity(input.len().saturating_add(2));
        cells.push_back(blank);
        for &byte in input {
            cells.push_back(byte);
        }
        cells.push_back(blank);

        Self {
            cells,
            head: if input.is_empty() { 0 } else { 1 },
            blank,
        }
    }

    pub fn read(&self) -> Symbol {
        *self.cells.get(self.head).unwrap_or(&self.blank)
    }

    pub fn write(&mut self, value: Symbol) {
        while self.head >= self.cells.len() {
            self.cells.push_back(self.blank);
        }
        self.cells[self.head] = value;
    }

    pub fn move_dir(&mut self, dir: Dir) {
        match dir {
            Dir::Left => {
                if self.head == 0 {
                    self.cells.push_front(self.blank);
                } else {
                    self.head -= 1;
                }
            }
            Dir::Right => {
                self.head += 1;
                if self.head == self.cells.len() {
                    self.cells.push_back(self.blank);
                }
            }
            Dir::Stay => {}
        }
    }

    pub fn head_index(&self) -> usize {
        self.head
    }

    pub fn trimmed_bytes(&self) -> Vec<u8> {
        let mut left = 0usize;
        let mut right = self.cells.len();

        while left < right && self.cells[left] == self.blank {
            left += 1;
        }
        while right > left && self.cells[right - 1] == self.blank {
            right -= 1;
        }

        self.cells
            .iter()
            .skip(left)
            .take(right.saturating_sub(left))
            .copied()
            .collect()
    }

    pub fn render_with_head(&self) -> String {
        let mut out = String::new();
        for (index, &byte) in self.cells.iter().enumerate() {
            if index == self.head {
                out.push('[');
                out.push(byte as char);
                out.push(']');
            } else {
                out.push(byte as char);
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NtmConfig {
    pub tape: Tape,
    state: StateId,
    steps: usize,
}

impl NtmConfig {
    pub fn new(input: &[u8], blank: Symbol, start_state: StateId) -> Self {
        Self {
            tape: Tape::from_bytes(input, blank),
            state: start_state,
            steps: 0,
        }
    }

    pub fn state(&self) -> StateId {
        self.state
    }

    pub fn steps(&self) -> usize {
        self.steps
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = write!(
            out,
            "state={} steps={} tape={}",
            self.state,
            self.steps,
            self.tape.render_with_head()
        );
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NtmFrontier {
    slots: VecDeque<NtmConfig>,
}

impl NtmFrontier {
    pub fn new() -> Self {
        Self {
            slots: VecDeque::new(),
        }
    }

    pub fn push(&mut self, config: NtmConfig) {
        self.slots.push_back(config);
    }

    pub fn pop(&mut self) -> Option<NtmConfig> {
        self.slots.pop_front()
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn clear(&mut self) {
        self.slots.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = &NtmConfig> {
        self.slots.iter()
    }

    pub fn take_batch(&mut self, max: usize) -> Vec<NtmConfig> {
        let mut out = Vec::with_capacity(max);
        for _ in 0..max {
            let Some(config) = self.pop() else {
                break;
            };
            out.push(config);
        }
        out
    }
}

impl Default for NtmFrontier {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NtmStats {
    pub explored: usize,
    pub expanded: usize,
    pub generated: usize,
    pub waves: usize,
    pub peak_frontier: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NtmRunResult {
    pub halt: NtmHalt,
    pub accepted: Option<NtmConfig>,
    pub stats: NtmStats,
}

#[derive(Debug, Clone)]
pub struct NtmProgram {
    blank: Symbol,
    start_state: StateId,
    accept_state: StateId,
    reject_state: StateId,
    rules: Vec<TransitionRule>,
}

impl NtmProgram {
    pub fn new(
        blank: Symbol,
        start_state: StateId,
        accept_state: StateId,
        reject_state: StateId,
        rules: Vec<TransitionRule>,
    ) -> Self {
        Self {
            blank,
            start_state,
            accept_state,
            reject_state,
            rules,
        }
    }

    pub fn seed(&self, input: &[u8]) -> NtmConfig {
        NtmConfig::new(input, self.blank, self.start_state)
    }

    fn is_accept(&self, state: StateId) -> bool {
        state == self.accept_state
    }

    fn is_reject(&self, state: StateId) -> bool {
        state == self.reject_state
    }

    fn matching_rules(
        &self,
        state: StateId,
        read: Symbol,
    ) -> impl Iterator<Item = TransitionRule> + '_ {
        self.rules
            .iter()
            .copied()
            .filter(move |rule| rule.state == state && rule.read == read)
    }
}

#[derive(Debug, Clone)]
pub struct NtmEngine {
    program: NtmProgram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NtmWaveReport {
    pub explored: usize,
    pub generated: usize,
    pub accepted: bool,
    pub step_limited: bool,
    pub frontier_limited: bool,
}

impl NtmEngine {
    pub fn new(program: NtmProgram) -> Self {
        Self { program }
    }

    pub fn program(&self) -> &NtmProgram {
        &self.program
    }

    pub fn seed_frontier(&self, input: &[u8]) -> NtmFrontier {
        let mut frontier = NtmFrontier::new();
        frontier.push(self.program.seed(input));
        frontier
    }

    pub fn expand_batch(
        &self,
        batch: impl IntoIterator<Item = NtmConfig>,
        output: &mut NtmFrontier,
        max_steps: usize,
        frontier_limit: usize,
    ) -> NtmWaveReport {
        let mut explored = 0usize;
        let mut generated = 0usize;
        let mut step_limited = false;

        for config in batch {
            explored = explored.saturating_add(1);

            if self.program.is_accept(config.state) {
                output.push(config);
                return NtmWaveReport {
                    explored,
                    generated,
                    accepted: true,
                    step_limited,
                    frontier_limited: false,
                };
            }

            if self.program.is_reject(config.state) {
                continue;
            }

            if config.steps >= max_steps {
                step_limited = true;
                continue;
            }

            let read = config.tape.read();
            for rule in self.program.matching_rules(config.state, read) {
                if output.len() >= frontier_limit {
                    return NtmWaveReport {
                        explored,
                        generated,
                        accepted: false,
                        step_limited,
                        frontier_limited: true,
                    };
                }

                let mut tape = config.tape.clone();
                tape.write(rule.write);
                tape.move_dir(rule.dir);
                output.push(NtmConfig {
                    tape,
                    state: rule.next_state,
                    steps: config.steps.saturating_add(1),
                });
                generated = generated.saturating_add(1);
            }
        }

        NtmWaveReport {
            explored,
            generated,
            accepted: false,
            step_limited,
            frontier_limited: false,
        }
    }

    pub fn expand_frontier(
        &self,
        current: &mut NtmFrontier,
        next: &mut NtmFrontier,
        max_steps: usize,
        frontier_limit: usize,
    ) -> NtmWaveReport {
        let batch = current.take_batch(current.len());
        self.expand_batch(batch, next, max_steps, frontier_limit)
    }

    pub fn run_wavefront(
        &self,
        input: &[u8],
        max_steps: usize,
        frontier_limit: usize,
    ) -> NtmRunResult {
        let mut current = self.seed_frontier(input);
        let mut next = NtmFrontier::new();
        let mut stats = NtmStats {
            explored: 0,
            expanded: 0,
            generated: 0,
            waves: 0,
            peak_frontier: current.len(),
        };
        let mut saw_step_limit = false;

        while !current.is_empty() {
            stats.waves = stats.waves.saturating_add(1);
            let report = self.expand_frontier(&mut current, &mut next, max_steps, frontier_limit);
            stats.explored = stats.explored.saturating_add(report.explored);
            stats.expanded = stats.expanded.saturating_add(report.explored);
            stats.generated = stats.generated.saturating_add(report.generated);
            stats.peak_frontier = stats.peak_frontier.max(next.len());
            saw_step_limit = saw_step_limit || report.step_limited;

            if report.accepted {
                return NtmRunResult {
                    halt: NtmHalt::Accept,
                    accepted: next.pop(),
                    stats,
                };
            }

            if report.frontier_limited {
                return NtmRunResult {
                    halt: NtmHalt::FrontierLimit,
                    accepted: None,
                    stats,
                };
            }

            core::mem::swap(&mut current, &mut next);
            next.clear();
        }

        NtmRunResult {
            halt: if saw_step_limit {
                NtmHalt::StepLimit
            } else {
                NtmHalt::Exhausted
            },
            accepted: None,
            stats,
        }
    }
}

pub fn contains_one_program() -> NtmProgram {
    const STATE_SCAN: StateId = STATE_START;
    const STATE_SKIP: StateId = 3;

    NtmProgram::new(
        BLANK,
        STATE_START,
        STATE_ACCEPT,
        STATE_REJECT,
        vec![
            TransitionRule {
                state: STATE_SCAN,
                read: b'0',
                next_state: STATE_SCAN,
                write: b'0',
                dir: Dir::Right,
            },
            TransitionRule {
                state: STATE_SCAN,
                read: b'1',
                next_state: STATE_SCAN,
                write: b'1',
                dir: Dir::Right,
            },
            TransitionRule {
                state: STATE_SCAN,
                read: b'1',
                next_state: STATE_ACCEPT,
                write: b'1',
                dir: Dir::Stay,
            },
            TransitionRule {
                state: STATE_SCAN,
                read: b'0',
                next_state: STATE_SKIP,
                write: b'0',
                dir: Dir::Right,
            },
            TransitionRule {
                state: STATE_SCAN,
                read: BLANK,
                next_state: STATE_REJECT,
                write: BLANK,
                dir: Dir::Stay,
            },
            TransitionRule {
                state: STATE_SKIP,
                read: b'0',
                next_state: STATE_SKIP,
                write: b'0',
                dir: Dir::Right,
            },
            TransitionRule {
                state: STATE_SKIP,
                read: b'1',
                next_state: STATE_SKIP,
                write: b'1',
                dir: Dir::Right,
            },
            TransitionRule {
                state: STATE_SKIP,
                read: BLANK,
                next_state: STATE_REJECT,
                write: BLANK,
                dir: Dir::Stay,
            },
        ],
    )
}

pub fn contains_one_engine() -> NtmEngine {
    NtmEngine::new(contains_one_program())
}

pub fn contains_one_summary(input: &[u8], max_steps: usize, frontier_limit: usize) -> String {
    let result = contains_one_engine().run_wavefront(input, max_steps, frontier_limit);
    let mut out = String::new();
    let _ = write!(
        out,
        "halt={} explored={} expanded={} generated={} waves={} peak_frontier={}",
        ntm_halt_name(result.halt),
        result.stats.explored,
        result.stats.expanded,
        result.stats.generated,
        result.stats.waves,
        result.stats.peak_frontier
    );

    if let Some(config) = result.accepted {
        let _ = write!(out, " accepted=({})", config.render());
    }

    out
}

pub fn expand_contains_one_once(input: &[u8], frontier_limit: usize) -> String {
    let engine = contains_one_engine();
    let mut current = engine.seed_frontier(input);
    let mut next = NtmFrontier::new();
    let report = engine.expand_frontier(&mut current, &mut next, usize::MAX, frontier_limit);

    let mut out = String::new();
    let _ = write!(
        out,
        "explored={} generated={} accepted={} next_frontier={}",
        report.explored,
        report.generated,
        report.accepted,
        next.len()
    );

    for (index, config) in next.iter().enumerate() {
        let _ = write!(out, "\nbranch[{}] {}", index, config.render());
    }

    out
}

fn ntm_halt_name(reason: NtmHalt) -> &'static str {
    match reason {
        NtmHalt::Accept => "accept",
        NtmHalt::Exhausted => "exhausted",
        NtmHalt::StepLimit => "step-limit",
        NtmHalt::FrontierLimit => "frontier-limit",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_one_accepts_when_input_has_one() {
        let result = contains_one_engine().run_wavefront(b"00100", 16, 64);
        assert_eq!(result.halt, NtmHalt::Accept);
        assert!(result.accepted.is_some());
    }

    #[test]
    fn contains_one_exhausts_when_input_has_no_one() {
        let result = contains_one_engine().run_wavefront(b"00000", 16, 64);
        assert_eq!(result.halt, NtmHalt::Exhausted);
        assert!(result.accepted.is_none());
    }
}
