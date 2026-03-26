use super::holes::BlackHolePayload;
use super::problem::{ClauseMasks, CompiledNSat, FAST_MASK_BITS, FAST_MASK_WORDS};
use super::widget::MarbleWidgetKind;
use super::world::{MarbleWorld, TileId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecOutcome {
    Advanced,
    LostOnEmpty,
    Blocked,
    ReachedBlackHole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClauseEval {
    Satisfied,
    Conflict,
    Undecided,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramEval {
    Satisfiable,
    Conflict,
    Undecided,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchResult {
    Sat,
    Unsat,
    Unknown,
    HistoryExceeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolveOutcome {
    Satisfiable {
        value_mask: [u64; FAST_MASK_WORDS],
        assigned_mask: [u64; FAST_MASK_WORDS],
        decisions: u32,
    },
    Unsatisfiable {
        decisions: u32,
    },
    Unknown {
        decisions: u32,
    },
    HistoryExceeded {
        value_mask: [u64; FAST_MASK_WORDS],
        assigned_mask: [u64; FAST_MASK_WORDS],
        decisions: u32,
    },
}

#[derive(Clone, Debug)]
pub struct MarbleSnapshot {
    pub tile: TileId,
    pub alive: bool,
    pub value_mask: [u64; FAST_MASK_WORDS],
    pub assigned_mask: [u64; FAST_MASK_WORDS],
}

#[derive(Clone, Debug)]
pub struct MarbleState {
    pub tile: TileId,
    pub alive: bool,
    // 1-bit variable values and assigned bits in fixed fast-mode window.
    pub value_mask: [u64; FAST_MASK_WORDS],
    pub assigned_mask: [u64; FAST_MASK_WORDS],
}

impl MarbleState {
    pub fn new(tile: TileId) -> Self {
        Self {
            tile,
            alive: true,
            value_mask: [0u64; FAST_MASK_WORDS],
            assigned_mask: [0u64; FAST_MASK_WORDS],
        }
    }

    pub fn set_var(&mut self, var_index_1based: u16, value: bool) -> bool {
        if var_index_1based == 0 || var_index_1based > FAST_MASK_BITS {
            return false;
        }
        let zero_based = (var_index_1based - 1) as usize;
        let word = zero_based / 64;
        let bit = 1u64 << (zero_based % 64);
        self.assigned_mask[word] |= bit;
        if value {
            self.value_mask[word] |= bit;
        } else {
            self.value_mask[word] &= !bit;
        }
        true
    }

    pub fn var_value(&self, var_index_1based: u16) -> Option<bool> {
        if var_index_1based == 0 || var_index_1based > FAST_MASK_BITS {
            return None;
        }
        let zero_based = (var_index_1based - 1) as usize;
        let word = zero_based / 64;
        let bit = 1u64 << (zero_based % 64);
        let assigned_word = self.assigned_mask[word];
        let value_word = self.value_mask[word];
        if (assigned_word & bit) == 0 {
            return None;
        }
        Some((value_word & bit) != 0)
    }

    pub fn snapshot(&self) -> MarbleSnapshot {
        MarbleSnapshot {
            tile: self.tile,
            alive: self.alive,
            value_mask: self.value_mask.clone(),
            assigned_mask: self.assigned_mask.clone(),
        }
    }

    pub fn restore(&mut self, snapshot: MarbleSnapshot) {
        self.tile = snapshot.tile;
        self.alive = snapshot.alive;
        self.value_mask = snapshot.value_mask;
        self.assigned_mask = snapshot.assigned_mask;
    }
}

pub fn eval_clause_masks(state: &MarbleState, clause: &ClauseMasks) -> ClauseEval {
    let mut any_true = false;
    let mut all_assigned = true;
    for i in 0..FAST_MASK_WORDS {
        let assigned_word = state.assigned_mask[i];
        let value_word = state.value_mask[i];

        let assigned_in_clause = assigned_word & clause.vars_mask[i];
        let true_lits = (value_word & clause.pos_mask[i]) | ((!value_word) & clause.neg_mask[i]);

        if (true_lits & assigned_in_clause) != 0 {
            any_true = true;
            break;
        }

        if assigned_in_clause != clause.vars_mask[i] {
            all_assigned = false;
        }
    }

    if any_true {
        ClauseEval::Satisfied
    } else if all_assigned {
        ClauseEval::Conflict
    } else {
        ClauseEval::Undecided
    }
}

pub fn eval_program_masks(state: &MarbleState, program: &CompiledNSat) -> ProgramEval {
    let mut all_satisfied = true;

    for clause in &program.clauses {
        match eval_clause_masks(state, clause) {
            ClauseEval::Satisfied => {}
            ClauseEval::Conflict => return ProgramEval::Conflict,
            ClauseEval::Undecided => all_satisfied = false,
        }
    }

    if all_satisfied {
        ProgramEval::Satisfiable
    } else {
        ProgramEval::Undecided
    }
}

pub fn is_assignment_complete(state: &MarbleState, program: &CompiledNSat) -> bool {
    if program.vars == 0 {
        return false;
    }

    let words = ((program.vars as usize) + 63) / 64;
    for i in 0..words {
        let expected = if i + 1 == words {
            let rem = (program.vars as usize) % 64;
            if rem == 0 {
                u64::MAX
            } else {
                (1u64 << rem) - 1
            }
        } else {
            u64::MAX
        };

        let assigned = state.assigned_mask[i];
        if (assigned & expected) != expected {
            return false;
        }
    }

    true
}

pub fn witness_is_solution(state: &MarbleState, program: &CompiledNSat) -> bool {
    eval_program_masks(state, program) == ProgramEval::Satisfiable
}

fn choose_next_unassigned_var(state: &MarbleState, vars: u16) -> Option<u16> {
    for var in 1..=vars {
        if state.var_value(var).is_none() {
            return Some(var);
        }
    }
    None
}

fn complete_assignment_with_default_false(state: &mut MarbleState, vars: u16) -> bool {
    for var in 1..=vars {
        if state.var_value(var).is_none() && !state.set_var(var, false) {
            return false;
        }
    }
    true
}

fn dpll_search(
    state: &mut MarbleState,
    program: &CompiledNSat,
    decisions: &mut u32,
    max_decisions: u32,
) -> SearchResult {
    if *decisions >= max_decisions {
        return SearchResult::Unknown;
    }

    match eval_program_masks(state, program) {
        ProgramEval::Satisfiable => return SearchResult::Sat,
        ProgramEval::Conflict => return SearchResult::Unsat,
        ProgramEval::Undecided => {}
    }

    let Some(var) = choose_next_unassigned_var(state, program.vars) else {
        return SearchResult::Unsat;
    };

    let base = state.snapshot();

    *decisions = decisions.saturating_add(1);
    if !state.set_var(var, true) {
        return SearchResult::HistoryExceeded;
    }
    let true_result = dpll_search(state, program, decisions, max_decisions);
    if true_result == SearchResult::Sat {
        return SearchResult::Sat;
    }

    state.restore(base.clone());

    *decisions = decisions.saturating_add(1);
    if !state.set_var(var, false) {
        return SearchResult::HistoryExceeded;
    }
    let false_result = dpll_search(state, program, decisions, max_decisions);
    if false_result == SearchResult::Sat {
        return SearchResult::Sat;
    }

    state.restore(base);

    if true_result == SearchResult::HistoryExceeded || false_result == SearchResult::HistoryExceeded
    {
        SearchResult::HistoryExceeded
    } else if true_result == SearchResult::Unknown || false_result == SearchResult::Unknown {
        SearchResult::Unknown
    } else {
        SearchResult::Unsat
    }
}

pub fn solve_nsat_fast(program: &CompiledNSat, max_decisions: u32) -> SolveOutcome {
    let mut state = MarbleState::new(0);
    let mut decisions = 0u32;

    if program.vars > FAST_MASK_BITS {
        return SolveOutcome::HistoryExceeded {
            value_mask: state.value_mask,
            assigned_mask: state.assigned_mask,
            decisions,
        };
    }

    match dpll_search(&mut state, program, &mut decisions, max_decisions) {
        SearchResult::Sat => {
            if !complete_assignment_with_default_false(&mut state, program.vars) {
                return SolveOutcome::HistoryExceeded {
                    value_mask: state.value_mask,
                    assigned_mask: state.assigned_mask,
                    decisions,
                };
            }
            SolveOutcome::Satisfiable {
                value_mask: state.value_mask,
                assigned_mask: state.assigned_mask,
                decisions,
            }
        }
        SearchResult::Unsat => SolveOutcome::Unsatisfiable { decisions },
        SearchResult::Unknown => SolveOutcome::Unknown { decisions },
        SearchResult::HistoryExceeded => SolveOutcome::HistoryExceeded {
            value_mask: state.value_mask,
            assigned_mask: state.assigned_mask,
            decisions,
        },
    }
}

pub fn blackhole_payload_for_solve_outcome(outcome: SolveOutcome) -> BlackHolePayload {
    match outcome {
        SolveOutcome::Satisfiable {
            value_mask,
            assigned_mask,
            decisions: _,
        } => BlackHolePayload::Assignment {
            value_mask,
            assigned_mask,
        },
        SolveOutcome::Unsatisfiable { decisions: _ } => BlackHolePayload::Unsatisfiable,
        SolveOutcome::Unknown { decisions: _ } => BlackHolePayload::Trace("unknown"),
        SolveOutcome::HistoryExceeded {
            value_mask,
            assigned_mask,
            decisions: _,
        } => BlackHolePayload::HistoryExceeded {
            value_mask,
            assigned_mask,
        },
    }
}

/// Emit only validated black-hole results.
/// A raw mask is never treated as a witness unless clause checks are satisfied.
pub fn blackhole_payload_for_state(
    state: &MarbleState,
    program: &CompiledNSat,
) -> BlackHolePayload {
    if !state.alive {
        return BlackHolePayload::Trace("marble-dead");
    }

    match eval_program_masks(state, program) {
        ProgramEval::Conflict => BlackHolePayload::Unsatisfiable,
        ProgramEval::Undecided => BlackHolePayload::Trace("undecided"),
        ProgramEval::Satisfiable => {
            if is_assignment_complete(state, program) {
                BlackHolePayload::Assignment {
                    value_mask: state.value_mask,
                    assigned_mask: state.assigned_mask,
                }
            } else {
                // Clauses are already satisfied, but assignment is partial.
                BlackHolePayload::Satisfiable
            }
        }
    }
}

pub fn step_marble(world: &MarbleWorld, state: &mut MarbleState, link_index: usize) -> ExecOutcome {
    if !state.alive {
        return ExecOutcome::Blocked;
    }

    let Some(current) = world.widget_at(state.tile) else {
        state.alive = false;
        return ExecOutcome::LostOnEmpty;
    };

    if current.kind == MarbleWidgetKind::BlackHole {
        return ExecOutcome::ReachedBlackHole;
    }

    let Some(Some(next_tile)) = current.links.get(link_index) else {
        state.alive = false;
        return ExecOutcome::LostOnEmpty;
    };

    if world.widget_at(*next_tile).is_none() {
        state.alive = false;
        return ExecOutcome::LostOnEmpty;
    }

    state.tile = *next_tile;
    ExecOutcome::Advanced
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marble2::problem::{Lit, ProblemProgram, ProblemToken};

    #[test]
    fn clause_eval_uses_bitmasks() {
        let tokens = vec![
            ProblemToken::Start3Sat { vars: 3 },
            ProblemToken::Clause([
                Lit { var: 1, neg: false },
                Lit { var: 2, neg: false },
                Lit { var: 3, neg: false },
            ]),
            ProblemToken::End,
        ];
        let program = ProblemProgram::from_tokens(&tokens).expect("valid 3sat");
        let compiled = program.compile_masks();

        let mut state = MarbleState::new(0);
        assert_eq!(
            eval_program_masks(&state, &compiled),
            ProgramEval::Undecided
        );

        assert!(state.set_var(1, false));
        assert!(state.set_var(2, false));
        assert!(state.set_var(3, false));
        assert_eq!(eval_program_masks(&state, &compiled), ProgramEval::Conflict);

        assert!(state.set_var(2, true));
        assert_eq!(
            eval_program_masks(&state, &compiled),
            ProgramEval::Satisfiable
        );
    }

    #[test]
    fn supports_n_variables_beyond_sixty_three() {
        let mut state = MarbleState::new(0);
        assert!(!state.set_var(FAST_MASK_BITS + 1, true));
    }

    #[test]
    fn fast_window_boundary_is_supported() {
        let mut clause = ClauseMasks {
            vars_mask: [0u64; FAST_MASK_WORDS],
            pos_mask: [0u64; FAST_MASK_WORDS],
            neg_mask: [0u64; FAST_MASK_WORDS],
        };

        let last = FAST_MASK_BITS as usize;
        let zero_based = last - 1;
        let word = zero_based / 64;
        let bit = 1u64 << (zero_based % 64);
        clause.vars_mask[word] |= bit;
        clause.pos_mask[word] |= bit;

        let program = CompiledNSat {
            vars: FAST_MASK_BITS,
            literals_per_clause: 1,
            clauses: vec![clause],
        };

        let mut state = MarbleState::new(0);
        assert!(state.set_var(FAST_MASK_BITS, true));
        assert_eq!(
            eval_program_masks(&state, &program),
            ProgramEval::Satisfiable
        );
    }

    #[test]
    fn snapshot_restore_is_constant_size_state() {
        let mut state = MarbleState::new(7);
        assert!(state.set_var(1, true));
        assert!(state.set_var(3, false));
        let snap = state.snapshot();

        assert!(state.set_var(2, true));
        state.tile = 99;
        state.restore(snap);

        assert_eq!(state.tile, 7);
        assert_eq!(state.var_value(1), Some(true));
        assert_eq!(state.var_value(2), None);
        assert_eq!(state.var_value(3), Some(false));
    }

    #[test]
    fn blackhole_payload_requires_verified_witness() {
        let tokens = vec![
            ProblemToken::Start3Sat { vars: 3 },
            ProblemToken::Clause([
                Lit { var: 1, neg: false },
                Lit { var: 2, neg: false },
                Lit { var: 3, neg: false },
            ]),
            ProblemToken::End,
        ];
        let program = ProblemProgram::from_tokens(&tokens).expect("valid 3sat");
        let compiled = program.compile_masks();

        let mut state = MarbleState::new(0);
        assert!(state.set_var(1, true));

        let partial = blackhole_payload_for_state(&state, &compiled);
        assert!(matches!(partial, BlackHolePayload::Satisfiable));

        assert!(state.set_var(2, false));
        assert!(state.set_var(3, false));
        let complete = blackhole_payload_for_state(&state, &compiled);
        assert!(matches!(complete, BlackHolePayload::Assignment { .. }));
    }

    #[test]
    fn dpll_finds_sat_witness() {
        let tokens = vec![
            ProblemToken::Start3Sat { vars: 3 },
            ProblemToken::Clause([
                Lit { var: 1, neg: false },
                Lit { var: 2, neg: false },
                Lit { var: 3, neg: false },
            ]),
            ProblemToken::End,
        ];
        let program = ProblemProgram::from_tokens(&tokens).expect("valid 3sat");
        let compiled = program.compile_masks();

        let outcome = solve_nsat_fast(&compiled, 100);
        assert!(matches!(outcome, SolveOutcome::Satisfiable { .. }));
    }

    #[test]
    fn dpll_reports_unsat() {
        let tokens = vec![
            ProblemToken::StartNSat {
                vars: 1,
                literals_per_clause: 1,
            },
            ProblemToken::ClauseN(vec![Lit { var: 1, neg: false }]),
            ProblemToken::ClauseN(vec![Lit { var: 1, neg: true }]),
            ProblemToken::End,
        ];
        let program = ProblemProgram::from_tokens(&tokens).expect("valid nsat");
        let compiled = program.compile_masks();

        let outcome = solve_nsat_fast(&compiled, 100);
        assert!(matches!(outcome, SolveOutcome::Unsatisfiable { .. }));
    }

    #[test]
    fn dpll_budget_can_return_unknown() {
        let tokens = vec![
            ProblemToken::Start3Sat { vars: 3 },
            ProblemToken::Clause([
                Lit { var: 1, neg: false },
                Lit { var: 2, neg: false },
                Lit { var: 3, neg: false },
            ]),
            ProblemToken::End,
        ];
        let program = ProblemProgram::from_tokens(&tokens).expect("valid 3sat");
        let compiled = program.compile_masks();

        let outcome = solve_nsat_fast(&compiled, 0);
        assert!(matches!(outcome, SolveOutcome::Unknown { .. }));
    }
}
