//! Raw no-std Turing machine core used by the kernel build.

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
pub struct StepRecord {
    pub prev_state: StateId,
    pub next_state: StateId,
    pub read: Symbol,
    pub write: Symbol,
    pub dir: Dir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HaltReason {
    Accept,
    Reject,
    MissingTransition,
    StepLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchHalt {
    Accept,
    Exhausted,
    StepLimit,
    BranchLimit,
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

#[derive(Debug, Clone)]
pub struct RawMachine {
    tape: Tape,
    state: StateId,
    accept_state: StateId,
    reject_state: StateId,
    rules: Vec<TransitionRule>,
    steps: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchBranch {
    pub tape: Tape,
    pub state: StateId,
    pub steps: usize,
    pub trace: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub halt: SearchHalt,
    pub explored: usize,
    pub frontier: usize,
    pub accepted: Option<SearchBranch>,
}

#[derive(Debug, Clone)]
pub struct NondeterministicMachine {
    blank: Symbol,
    start_state: StateId,
    accept_state: StateId,
    reject_state: StateId,
    rules: Vec<TransitionRule>,
}

impl RawMachine {
    pub fn new(
        input: &[u8],
        blank: Symbol,
        start_state: StateId,
        accept_state: StateId,
        reject_state: StateId,
        rules: Vec<TransitionRule>,
    ) -> Self {
        Self {
            tape: Tape::from_bytes(input, blank),
            state: start_state,
            accept_state,
            reject_state,
            rules,
            steps: 0,
        }
    }

    pub fn state(&self) -> StateId {
        self.state
    }

    pub fn steps(&self) -> usize {
        self.steps
    }

    pub fn tape(&self) -> &Tape {
        &self.tape
    }

    pub fn step(&mut self) -> Result<StepRecord, HaltReason> {
        if self.state == self.accept_state {
            return Err(HaltReason::Accept);
        }
        if self.state == self.reject_state {
            return Err(HaltReason::Reject);
        }

        let read = self.tape.read();
        let Some(rule) = self.find_rule(self.state, read) else {
            return Err(HaltReason::MissingTransition);
        };

        self.tape.write(rule.write);
        self.tape.move_dir(rule.dir);
        let prev_state = self.state;
        self.state = rule.next_state;
        self.steps = self.steps.saturating_add(1);

        Ok(StepRecord {
            prev_state,
            next_state: self.state,
            read,
            write: rule.write,
            dir: rule.dir,
        })
    }

    pub fn run(&mut self, max_steps: usize) -> HaltReason {
        for _ in 0..max_steps {
            match self.step() {
                Ok(_) => {}
                Err(reason) => return reason,
            }
        }
        HaltReason::StepLimit
    }

    pub fn trace(&mut self, max_steps: usize) -> String {
        let mut out = String::new();
        let _ = write!(
            out,
            "state={} tape={}",
            self.state,
            self.tape.render_with_head()
        );

        for _ in 0..max_steps {
            match self.step() {
                Ok(step) => {
                    let _ = write!(
                        out,
                        "\n{} --{}:{}:{}--> {} tape={}",
                        step.prev_state,
                        step.read as char,
                        step.write as char,
                        dir_name(step.dir),
                        step.next_state,
                        self.tape.render_with_head()
                    );
                }
                Err(reason) => {
                    let _ = write!(out, "\nhalt={}", halt_name(reason));
                    return out;
                }
            }
        }

        let _ = write!(out, "\nhalt={}", halt_name(HaltReason::StepLimit));
        out
    }

    fn find_rule(&self, state: StateId, read: Symbol) -> Option<TransitionRule> {
        self.rules
            .iter()
            .copied()
            .find(|rule| rule.state == state && rule.read == read)
    }
}

impl NondeterministicMachine {
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

    pub fn run_bfs(&self, input: &[u8], max_steps: usize, max_branches: usize) -> SearchResult {
        let start_tape = Tape::from_bytes(input, self.blank);
        let mut frontier = VecDeque::new();
        frontier.push_back(SearchBranch {
            tape: start_tape.clone(),
            state: self.start_state,
            steps: 0,
            trace: format!(
                "state={} tape={}",
                self.start_state,
                start_tape.render_with_head()
            ),
        });

        let mut explored = 0usize;
        let mut saw_step_limit = false;

        while let Some(branch) = frontier.pop_front() {
            if branch.state == self.accept_state {
                return SearchResult {
                    halt: SearchHalt::Accept,
                    explored,
                    frontier: frontier.len(),
                    accepted: Some(branch),
                };
            }

            if branch.state == self.reject_state {
                explored = explored.saturating_add(1);
                continue;
            }

            if branch.steps >= max_steps {
                explored = explored.saturating_add(1);
                saw_step_limit = true;
                continue;
            }

            let next = self.step_branch(&branch);
            explored = explored.saturating_add(1);

            if next.is_empty() {
                continue;
            }

            let next_total = frontier.len().saturating_add(next.len());
            if next_total > max_branches {
                return SearchResult {
                    halt: SearchHalt::BranchLimit,
                    explored,
                    frontier: frontier.len(),
                    accepted: None,
                };
            }

            for child in next {
                frontier.push_back(child);
            }
        }

        SearchResult {
            halt: if saw_step_limit {
                SearchHalt::StepLimit
            } else {
                SearchHalt::Exhausted
            },
            explored,
            frontier: 0,
            accepted: None,
        }
    }

    fn step_branch(&self, branch: &SearchBranch) -> Vec<SearchBranch> {
        let read = branch.tape.read();
        let mut out = Vec::new();

        for rule in self
            .rules
            .iter()
            .copied()
            .filter(|rule| rule.state == branch.state && rule.read == read)
        {
            let mut tape = branch.tape.clone();
            tape.write(rule.write);
            tape.move_dir(rule.dir);

            let mut trace = branch.trace.clone();
            let _ = write!(
                trace,
                "\n{} --{}:{}:{}--> {} tape={}",
                branch.state,
                read as char,
                rule.write as char,
                dir_name(rule.dir),
                rule.next_state,
                tape.render_with_head()
            );

            out.push(SearchBranch {
                tape,
                state: rule.next_state,
                steps: branch.steps.saturating_add(1),
                trace,
            });
        }

        out
    }
}

pub fn binary_increment_machine(input: &[u8]) -> RawMachine {
    const STATE_SCAN_RIGHT: StateId = STATE_START;
    const STATE_CARRY_LEFT: StateId = 3;

    RawMachine::new(
        input,
        BLANK,
        STATE_START,
        STATE_ACCEPT,
        STATE_REJECT,
        vec![
            TransitionRule {
                state: STATE_SCAN_RIGHT,
                read: b'0',
                next_state: STATE_SCAN_RIGHT,
                write: b'0',
                dir: Dir::Right,
            },
            TransitionRule {
                state: STATE_SCAN_RIGHT,
                read: b'1',
                next_state: STATE_SCAN_RIGHT,
                write: b'1',
                dir: Dir::Right,
            },
            TransitionRule {
                state: STATE_SCAN_RIGHT,
                read: BLANK,
                next_state: STATE_CARRY_LEFT,
                write: BLANK,
                dir: Dir::Left,
            },
            TransitionRule {
                state: STATE_CARRY_LEFT,
                read: b'0',
                next_state: STATE_ACCEPT,
                write: b'1',
                dir: Dir::Stay,
            },
            TransitionRule {
                state: STATE_CARRY_LEFT,
                read: b'1',
                next_state: STATE_CARRY_LEFT,
                write: b'0',
                dir: Dir::Left,
            },
            TransitionRule {
                state: STATE_CARRY_LEFT,
                read: BLANK,
                next_state: STATE_ACCEPT,
                write: b'1',
                dir: Dir::Stay,
            },
        ],
    )
}

pub fn binary_increment_trace(input: &[u8], max_steps: usize) -> String {
    let mut machine = binary_increment_machine(input);
    machine.trace(max_steps)
}

pub fn contains_one_ntm() -> NondeterministicMachine {
    const STATE_SCAN: StateId = STATE_START;
    const STATE_SKIP_TO_END: StateId = 3;

    NondeterministicMachine::new(
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
                read: BLANK,
                next_state: STATE_REJECT,
                write: BLANK,
                dir: Dir::Stay,
            },
            TransitionRule {
                state: STATE_SCAN,
                read: b'0',
                next_state: STATE_SKIP_TO_END,
                write: b'0',
                dir: Dir::Right,
            },
            TransitionRule {
                state: STATE_SKIP_TO_END,
                read: b'0',
                next_state: STATE_SKIP_TO_END,
                write: b'0',
                dir: Dir::Right,
            },
            TransitionRule {
                state: STATE_SKIP_TO_END,
                read: b'1',
                next_state: STATE_SKIP_TO_END,
                write: b'1',
                dir: Dir::Right,
            },
            TransitionRule {
                state: STATE_SKIP_TO_END,
                read: BLANK,
                next_state: STATE_REJECT,
                write: BLANK,
                dir: Dir::Stay,
            },
        ],
    )
}

pub fn contains_one_ntm_trace(input: &[u8], max_steps: usize, max_branches: usize) -> String {
    let result = contains_one_ntm().run_bfs(input, max_steps, max_branches);
    match result.accepted {
        Some(branch) => branch.trace,
        None => {
            let mut out = String::new();
            let _ = write!(
                out,
                "halt={} explored={} frontier={}",
                search_halt_name(result.halt),
                result.explored,
                result.frontier
            );
            out
        }
    }
}

fn dir_name(dir: Dir) -> &'static str {
    match dir {
        Dir::Left => "L",
        Dir::Right => "R",
        Dir::Stay => "S",
    }
}

fn halt_name(reason: HaltReason) -> &'static str {
    match reason {
        HaltReason::Accept => "accept",
        HaltReason::Reject => "reject",
        HaltReason::MissingTransition => "missing-transition",
        HaltReason::StepLimit => "step-limit",
    }
}

fn search_halt_name(reason: SearchHalt) -> &'static str {
    match reason {
        SearchHalt::Accept => "accept",
        SearchHalt::Exhausted => "exhausted",
        SearchHalt::StepLimit => "step-limit",
        SearchHalt::BranchLimit => "branch-limit",
    }
}
