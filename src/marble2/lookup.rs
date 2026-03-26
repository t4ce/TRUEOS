use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lit {
    pub var: u16,
    pub neg: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clause13 {
    One(Lit),
    Two(Lit, Lit),
    Three(Lit, Lit, Lit),
}

impl Clause13 {
    pub fn arity(self) -> u8 {
        match self {
            Self::One(_) => 1,
            Self::Two(_, _) => 2,
            Self::Three(_, _, _) => 3,
        }
    }

    pub fn for_each_lit(self, mut f: impl FnMut(Lit)) {
        match self {
            Self::One(a) => f(a),
            Self::Two(a, b) => {
                f(a);
                f(b);
            }
            Self::Three(a, b, c) => {
                f(a);
                f(b);
                f(c);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClauseLut {
    pub clause: Clause13,
    pub input_vars: [u16; 3],
    pub input_arity: u8,
    pub truth_bits: u8,
}

impl ClauseLut {
    pub fn entries(&self) -> usize {
        1usize << self.input_arity
    }

    pub fn eval(&self, input_index: usize) -> bool {
        if input_index >= self.entries() {
            return false;
        }
        ((self.truth_bits >> input_index) & 1) != 0
    }

    pub fn packed_bytes(&self) -> [u8; 1] {
        [self.truth_bits]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClauseLutProgram {
    pub vars: u16,
    pub clauses: Vec<Clause13>,
}

impl ClauseLutProgram {
    pub fn new(vars: u16, clauses: Vec<Clause13>) -> Option<Self> {
        if vars == 0 {
            return None;
        }

        for clause in &clauses {
            let mut ok = true;
            clause.for_each_lit(|lit| {
                if lit.var == 0 || lit.var > vars {
                    ok = false;
                }
            });
            if !ok {
                return None;
            }
        }

        Some(Self { vars, clauses })
    }

    pub fn compile(&self) -> ClauseLutSet {
        let mut luts = Vec::with_capacity(self.clauses.len());
        for &clause in &self.clauses {
            luts.push(compile_clause_lut(clause));
        }
        ClauseLutSet {
            vars: self.vars,
            luts,
        }
    }

    // Example from user notes: (!x v y v z)(x v !z)(y v i)
    // Variable map: x=1, y=2, z=3, i=4.
    pub fn example_xyzzi() -> Self {
        Self {
            vars: 4,
            clauses: vec![
                Clause13::Three(
                    Lit { var: 1, neg: true },
                    Lit { var: 2, neg: false },
                    Lit { var: 3, neg: false },
                ),
                Clause13::Two(Lit { var: 1, neg: false }, Lit { var: 3, neg: true }),
                Clause13::Two(Lit { var: 2, neg: false }, Lit { var: 4, neg: false }),
            ],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClauseLutSet {
    pub vars: u16,
    pub luts: Vec<ClauseLut>,
}

impl ClauseLutSet {
    pub fn clause_count(&self) -> usize {
        self.luts.len()
    }

    pub fn packed_truth_bytes(&self) -> Vec<u8> {
        self.luts.iter().map(|lut| lut.truth_bits).collect()
    }

    pub fn packed_truth_bytes_len(&self) -> usize {
        self.luts.len()
    }

    pub fn replicated(&self, times: usize) -> Self {
        if times == 0 || self.luts.is_empty() {
            return Self {
                vars: self.vars,
                luts: Vec::new(),
            };
        }

        let mut luts = Vec::with_capacity(self.luts.len() * times);
        for _ in 0..times {
            luts.extend(self.luts.iter().cloned());
        }

        Self {
            vars: self.vars,
            luts,
        }
    }

    pub fn eval_assignment(&self, assignment_bits: u64) -> bool {
        for lut in &self.luts {
            let mut local_index: usize = 0;
            for p in 0..lut.input_arity as usize {
                let var = lut.input_vars[p] as usize;
                if var == 0 {
                    continue;
                }
                let global_bit = (assignment_bits >> (var - 1)) & 1u64;
                local_index |= (global_bit as usize) << p;
            }
            if !lut.eval(local_index) {
                return false;
            }
        }
        true
    }

    pub fn eval_all_assignments_once(&self) -> ExhaustiveEval {
        let vars = self.vars as usize;
        let total_assignments = if vars >= 64 { 0 } else { 1usize << vars };
        let mut satisfying_assignments: Vec<u64> = Vec::new();

        for assignment in 0..total_assignments {
            if self.eval_assignment(assignment as u64) {
                satisfying_assignments.push(assignment as u64);
            }
        }

        ExhaustiveEval {
            vars: self.vars,
            total_assignments,
            satisfying_assignments,
        }
    }

    pub fn find_first_satisfying_assignment(&self) -> Option<u64> {
        let vars = self.vars as usize;
        let total_assignments = if vars >= 64 { 0 } else { 1usize << vars };

        for assignment in 0..total_assignments {
            let assignment = assignment as u64;
            if self.eval_assignment(assignment) {
                return Some(assignment);
            }
        }

        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExhaustiveEval {
    pub vars: u16,
    pub total_assignments: usize,
    pub satisfying_assignments: Vec<u64>,
}

impl ExhaustiveEval {
    pub fn satisfying_count(&self) -> usize {
        self.satisfying_assignments.len()
    }

    pub fn is_sat(&self) -> bool {
        !self.satisfying_assignments.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitEvalEntry {
    pub unit_index: usize,
    pub assignment_bits: u64,
    pub sat: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitEval {
    pub vars: u16,
    pub clause_luts_per_unit: usize,
    pub unit_count: usize,
    pub entries: Vec<UnitEvalEntry>,
}

impl UnitEval {
    pub fn first_sat(&self) -> Option<&UnitEvalEntry> {
        self.entries.iter().find(|e| e.sat)
    }
}

const RUNTIME_STARTED: u8 = 1 << 0;
const RUNTIME_DONE: u8 = 1 << 1;

static LUT_AP_RUNTIME_STATE: AtomicU8 = AtomicU8::new(0);
static LUT_AP_RUNTIME_ALL_DONE_LOGGED: AtomicBool = AtomicBool::new(false);
static LUT_FALL_STATE: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TwoApNoSyncResult {
    pub lane0_first_sat: Option<UnitEvalEntry>,
    pub lane1_first_sat: Option<UnitEvalEntry>,
}

fn eval_unit_sat(lut_bank: &ClauseLutSet, clause_luts_per_unit: usize, unit_index: usize) -> bool {
    let base = unit_index * clause_luts_per_unit;

    for lut in &lut_bank.luts[base..base + clause_luts_per_unit] {
        let mut local_index: usize = 0;
        for p in 0..lut.input_arity as usize {
            let var = lut.input_vars[p] as usize;
            if var == 0 {
                continue;
            }
            let global_bit = ((unit_index as u64) >> (var - 1)) & 1u64;
            local_index |= (global_bit as usize) << p;
        }
        if !lut.eval(local_index) {
            return false;
        }
    }

    true
}

fn find_first_sat_in_unit_range(
    lut_bank: &ClauseLutSet,
    clause_luts_per_unit: usize,
    start_unit: usize,
    end_unit: usize,
) -> Option<UnitEvalEntry> {
    if clause_luts_per_unit == 0 || lut_bank.luts.is_empty() {
        return None;
    }
    if !lut_bank.luts.len().is_multiple_of(clause_luts_per_unit) {
        return None;
    }

    let unit_count = lut_bank.luts.len() / clause_luts_per_unit;
    let start = start_unit.min(unit_count);
    let end = end_unit.min(unit_count);
    if start >= end {
        return None;
    }

    for unit_index in start..end {
        if eval_unit_sat(lut_bank, clause_luts_per_unit, unit_index) {
            return Some(UnitEvalEntry {
                unit_index,
                assignment_bits: unit_index as u64,
                sat: true,
            });
        }
    }

    None
}

pub fn find_first_sat_in_unit_stream(
    lut_bank: &ClauseLutSet,
    clause_luts_per_unit: usize,
) -> Option<UnitEvalEntry> {
    if clause_luts_per_unit == 0 || lut_bank.luts.is_empty() {
        return None;
    }
    if !lut_bank.luts.len().is_multiple_of(clause_luts_per_unit) {
        return None;
    }

    let unit_count = lut_bank.luts.len() / clause_luts_per_unit;

    for unit_index in 0..unit_count {
        if eval_unit_sat(lut_bank, clause_luts_per_unit, unit_index) {
            return Some(UnitEvalEntry {
                unit_index,
                assignment_bits: unit_index as u64,
                sat: true,
            });
        }
    }

    None
}

pub fn find_first_sat_two_ap_no_sync(
    lut_bank: &ClauseLutSet,
    clause_luts_per_unit: usize,
) -> Option<TwoApNoSyncResult> {
    if clause_luts_per_unit == 0 || lut_bank.luts.is_empty() {
        return None;
    }
    if !lut_bank.luts.len().is_multiple_of(clause_luts_per_unit) {
        return None;
    }

    let unit_count = lut_bank.luts.len() / clause_luts_per_unit;
    if unit_count == 0 {
        return None;
    }

    let split = unit_count / 2;

    let mut lane0_first_sat: Option<UnitEvalEntry> = None;
    for unit_index in 0..split {
        if eval_unit_sat(lut_bank, clause_luts_per_unit, unit_index) {
            lane0_first_sat = Some(UnitEvalEntry {
                unit_index,
                assignment_bits: unit_index as u64,
                sat: true,
            });
            break;
        }
    }

    let mut lane1_first_sat: Option<UnitEvalEntry> = None;
    for unit_index in split..unit_count {
        if eval_unit_sat(lut_bank, clause_luts_per_unit, unit_index) {
            lane1_first_sat = Some(UnitEvalEntry {
                unit_index,
                assignment_bits: unit_index as u64,
                sat: true,
            });
            break;
        }
    }

    Some(TwoApNoSyncResult {
        lane0_first_sat,
        lane1_first_sat,
    })
}

pub fn build_example_xyzzi_clause_luts() -> ClauseLutSet {
    ClauseLutProgram::example_xyzzi().compile()
}

pub fn build_lut_bank_for_program(
    program: &ClauseLutProgram,
    repeats: usize,
) -> Option<ClauseLutSet> {
    if repeats == 0 {
        return None;
    }

    Some(program.compile().replicated(repeats))
}

pub fn build_example_xyzzi_clause_lut_bank_16x() -> ClauseLutSet {
    build_lut_bank_for_program(&ClauseLutProgram::example_xyzzi(), 16)
        .expect("example repeat count must be non-zero")
}

pub fn run_two_ap_no_sync_for_program(
    program: &ClauseLutProgram,
    repeats: usize,
) -> Option<TwoApNoSyncResult> {
    let clause_luts_per_unit = program.clauses.len();
    if clause_luts_per_unit == 0 {
        return None;
    }

    let lut_bank = build_lut_bank_for_program(program, repeats)?;
    find_first_sat_two_ap_no_sync(&lut_bank, clause_luts_per_unit)
}

pub fn eval_unit_stream_once(
    lut_bank: &ClauseLutSet,
    clause_luts_per_unit: usize,
) -> Option<UnitEval> {
    if clause_luts_per_unit == 0 || lut_bank.luts.is_empty() {
        return None;
    }
    if !lut_bank.luts.len().is_multiple_of(clause_luts_per_unit) {
        return None;
    }

    let unit_count = lut_bank.luts.len() / clause_luts_per_unit;
    let mut entries = Vec::with_capacity(unit_count);

    for unit_index in 0..unit_count {
        let assignment_bits = unit_index as u64;
        let base = unit_index * clause_luts_per_unit;
        let mut sat = true;

        for lut in &lut_bank.luts[base..base + clause_luts_per_unit] {
            let mut local_index: usize = 0;
            for p in 0..lut.input_arity as usize {
                let var = lut.input_vars[p] as usize;
                if var == 0 {
                    continue;
                }
                let global_bit = (assignment_bits >> (var - 1)) & 1u64;
                local_index |= (global_bit as usize) << p;
            }
            if !lut.eval(local_index) {
                sat = false;
                break;
            }
        }

        entries.push(UnitEvalEntry {
            unit_index,
            assignment_bits,
            sat,
        });
    }

    Some(UnitEval {
        vars: lut_bank.vars,
        clause_luts_per_unit,
        unit_count,
        entries,
    })
}

pub fn log_example_first_sat_once() {
    let program = ClauseLutProgram::example_xyzzi();
    let clause_luts_per_unit = program.clauses.len();
    let Some(lut_bank) = build_lut_bank_for_program(&program, 16) else {
        crate::log!("marble2-lookup: setup failed (repeat count is zero)\n");
        return;
    };

    crate::log!(
        "marble2-lookup: formula=(!x v y v z)(x v !z)(y v i) vars={} clauses={} clauses_per_unit={} repeats=16 mode=2ap-no-sync\n",
        lut_bank.vars,
        lut_bank.clause_count(),
        clause_luts_per_unit,
    );

    let var_bit =
        |bits: u64, var: u16| -> u8 { (((bits >> ((var as usize) - 1)) & 1u64) != 0) as u8 };

    let Some(two_ap) = find_first_sat_two_ap_no_sync(&lut_bank, clause_luts_per_unit) else {
        crate::log!("marble2-lookup: 2ap-no-sync setup failed\n");
        return;
    };

    match two_ap.lane0_first_sat {
        Some(hit) => {
            crate::log!(
                "marble2-lookup: lane0-first-sat unit={} x={}, y={}, z={}, i={} (bits=0b{:04b})\n",
                hit.unit_index,
                var_bit(hit.assignment_bits, 1),
                var_bit(hit.assignment_bits, 2),
                var_bit(hit.assignment_bits, 3),
                var_bit(hit.assignment_bits, 4),
                hit.assignment_bits,
            );
        }
        None => {
            crate::log!("marble2-lookup: lane0 unsat in range [0..8)\n");
        }
    }

    match two_ap.lane1_first_sat {
        Some(hit) => {
            crate::log!(
                "marble2-lookup: lane1-first-sat unit={} x={}, y={}, z={}, i={} (bits=0b{:04b})\n",
                hit.unit_index,
                var_bit(hit.assignment_bits, 1),
                var_bit(hit.assignment_bits, 2),
                var_bit(hit.assignment_bits, 3),
                var_bit(hit.assignment_bits, 4),
                hit.assignment_bits,
            );
        }
        None => {
            crate::log!("marble2-lookup: lane1 unsat in range [8..16)\n");
        }
    }
}

pub fn poll_example_two_ap_runtime_once() {
    if (LUT_AP_RUNTIME_STATE.load(Ordering::Acquire) & RUNTIME_DONE) != 0 {
        if !LUT_AP_RUNTIME_ALL_DONE_LOGGED.swap(true, Ordering::AcqRel) {
            crate::log!("marble2-lookup-runtime: single lane done\n");
        }
        return;
    }

    // Exactly one AP claims and runs the single lane once.
    if LUT_AP_RUNTIME_STATE
        .compare_exchange(0, RUNTIME_STARTED, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    let program = ClauseLutProgram::example_xyzzi();
    let clause_luts_per_unit = program.clauses.len();
    let Some(lut_bank) = build_lut_bank_for_program(&program, 16) else {
        LUT_AP_RUNTIME_STATE.fetch_or(RUNTIME_DONE, Ordering::AcqRel);
        return;
    };

    let var_bit =
        |bits: u64, var: u16| -> u8 { (((bits >> ((var as usize) - 1)) & 1u64) != 0) as u8 };

    let cpu = crate::percpu::this_cpu().cpu_index();
    match find_first_sat_in_unit_range(&lut_bank, clause_luts_per_unit, 0, 16) {
        Some(hit) => {
            crate::log!(
                "marble2-lookup-runtime: single cpu={} unit={} x={}, y={}, z={}, i={} (bits=0b{:04b})\n",
                cpu,
                hit.unit_index,
                var_bit(hit.assignment_bits, 1),
                var_bit(hit.assignment_bits, 2),
                var_bit(hit.assignment_bits, 3),
                var_bit(hit.assignment_bits, 4),
                hit.assignment_bits,
            );
        }
        None => {
            crate::log!(
                "marble2-lookup-runtime: single cpu={} unsat in range [0..16)\n",
                cpu
            );
        }
    }

    LUT_AP_RUNTIME_STATE.fetch_or(RUNTIME_DONE, Ordering::AcqRel);
}

/// Formula: (!x v y v z)(x v !z)(y v i)  vars: x=bit0 y=bit1 z=bit2 i=bit3
/// 16 possible assignments. No heap — all inline.
/// To split into N lanes later: each lane scans a sub-range [start..end) of 0..16.
#[inline(never)]
pub fn poll_lut_fall() {
    if LUT_FALL_STATE
        .compare_exchange(0, RUNTIME_STARTED, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    crate::log!("marble2-lut: formula=(!x v y v z)(x v !z)(y v i) vars=4 clauses=3 units=16\n");

    // Precomputed truth tables for the 3 clauses:
    //   LUT0 (!x|y|z): false only when x=1,y=0,z=0  -> 0xFD
    //   LUT1 (x|!z):   false only when x=0,z=1       -> 0x0B
    //   LUT2 (y|i):    false only when y=0,i=0        -> 0x0E
    const LUT0: u8 = 0xFD;
    const LUT1: u8 = 0x0B;
    const LUT2: u8 = 0x0E;

    let mut found: Option<u8> = None;
    for bits in 0u8..16 {
        let x = (bits >> 0) & 1;
        let y = (bits >> 1) & 1;
        let z = (bits >> 2) & 1;
        let i = (bits >> 3) & 1;
        let sat = ((LUT0 >> (x | (y << 1) | (z << 2))) & 1) != 0
            && ((LUT1 >> (x | (z << 1))) & 1) != 0
            && ((LUT2 >> (y | (i << 1))) & 1) != 0;
        if sat {
            found = Some(bits);
            break;
        }
    }

    match found {
        Some(bits) => {
            let x = (bits >> 0) & 1;
            let y = (bits >> 1) & 1;
            let z = (bits >> 2) & 1;
            let i = (bits >> 3) & 1;
            crate::log!(
                "marble2-lut: sat unit={} x={} y={} z={} i={} (bits=0b{:04b})\n",
                bits,
                x,
                y,
                z,
                i,
                bits,
            );
        }
        None => {
            crate::log!("marble2-lut: unsat\n");
        }
    }

    LUT_FALL_STATE.fetch_or(RUNTIME_DONE, Ordering::AcqRel);
}

pub fn compile_clause_lut(clause: Clause13) -> ClauseLut {
    let (input_vars, input_arity) = unique_input_vars(clause);
    let entries = 1usize << input_arity;
    let mut truth_bits = 0u8;

    for index in 0..entries {
        if eval_clause_for_index(clause, &input_vars, input_arity, index) {
            truth_bits |= 1u8 << index;
        }
    }

    ClauseLut {
        clause,
        input_vars,
        input_arity,
        truth_bits,
    }
}

fn unique_input_vars(clause: Clause13) -> ([u16; 3], u8) {
    let mut vars = [0u16; 3];
    let mut arity = 0u8;

    clause.for_each_lit(|lit| {
        if vars[..arity as usize].contains(&lit.var) {
            return;
        }
        vars[arity as usize] = lit.var;
        arity += 1;
    });

    (vars, arity)
}

fn eval_clause_for_index(clause: Clause13, vars: &[u16; 3], arity: u8, index: usize) -> bool {
    let mut clause_true = false;

    clause.for_each_lit(|lit| {
        if clause_true {
            return;
        }

        let mut local_idx: Option<usize> = None;
        for p in 0..arity as usize {
            if vars[p] == lit.var {
                local_idx = Some(p);
                break;
            }
        }

        let Some(local_idx) = local_idx else {
            return;
        };

        let mut bit = ((index >> local_idx) & 1usize) != 0;
        if lit.neg {
            bit = !bit;
        }

        if bit {
            clause_true = true;
        }
    });

    clause_true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_example_luts() {
        let compiled = build_example_xyzzi_clause_luts();
        assert_eq!(compiled.vars, 4);
        assert_eq!(compiled.luts.len(), 3);

        // (!x v y v z)
        assert_eq!(compiled.luts[0].input_vars, [1, 2, 3]);
        assert_eq!(compiled.luts[0].input_arity, 3);
        assert_eq!(compiled.luts[0].truth_bits, 0b1111_1101);

        // (x v !z)
        assert_eq!(compiled.luts[1].input_vars, [1, 3, 0]);
        assert_eq!(compiled.luts[1].input_arity, 2);
        assert_eq!(compiled.luts[1].truth_bits, 0b0000_1011);

        // (y v i)
        assert_eq!(compiled.luts[2].input_vars, [2, 4, 0]);
        assert_eq!(compiled.luts[2].input_arity, 2);
        assert_eq!(compiled.luts[2].truth_bits, 0b0000_1110);

        assert_eq!(
            compiled.packed_truth_bytes(),
            vec![0b1111_1101, 0b0000_1011, 0b0000_1110]
        );
    }

    #[test]
    fn evaluates_single_assignment() {
        let compiled = build_example_xyzzi_clause_luts();

        // x=1, y=0, z=1, i=0 -> should be unsatisfied.
        let assignment = 0b0101u64;
        assert!(!compiled.eval_assignment(assignment));

        // x=0, y=1, z=0, i=0 -> should be satisfied.
        let assignment = 0b0010u64;
        assert!(compiled.eval_assignment(assignment));
    }

    #[test]
    fn exhaustively_loops_once_over_all_assignments() {
        let compiled = build_example_xyzzi_clause_luts();
        let result = compiled.eval_all_assignments_once();

        assert_eq!(result.vars, 4);
        assert_eq!(result.total_assignments, 16);
        assert!(result.is_sat());
        assert_eq!(result.satisfying_count(), 9);
    }

    #[test]
    fn returns_first_satisfying_assignment() {
        let compiled = build_example_xyzzi_clause_luts();
        let first = compiled.find_first_satisfying_assignment();

        // Assignment order is ascending integer value; first SAT for this CNF is 0b0000.
        assert_eq!(first, Some(0));
    }

    #[test]
    fn replicates_example_luts_16_times() {
        let bank = build_example_xyzzi_clause_lut_bank_16x();
        assert_eq!(bank.vars, 4);
        assert_eq!(bank.clause_count(), 48);

        // Replication should preserve satisfiability behavior.
        assert_eq!(bank.find_first_satisfying_assignment(), Some(0));
        assert!(!bank.eval_assignment(0b0101));
        assert!(bank.eval_assignment(0b0010));
    }

    #[test]
    fn unit_stream_uses_each_triple_once() {
        let bank = build_example_xyzzi_clause_lut_bank_16x();
        let eval = eval_unit_stream_once(&bank, 3).expect("valid unit stream");

        assert_eq!(eval.unit_count, 16);
        assert_eq!(eval.entries.len(), 16);
        assert_eq!(eval.entries[0].assignment_bits, 0);
        assert_eq!(eval.entries[15].assignment_bits, 15);
        assert!(eval.first_sat().is_some());
    }

    #[test]
    fn unit_stream_early_exit_finds_first_without_full_scan_requirement() {
        let bank = build_example_xyzzi_clause_lut_bank_16x();
        let first = find_first_sat_in_unit_stream(&bank, 3).expect("sat");
        assert_eq!(first.unit_index, 0);
        assert_eq!(first.assignment_bits, 0);
        assert!(first.sat);
    }

    #[test]
    fn two_ap_no_sync_returns_up_to_two_hits() {
        let bank = build_example_xyzzi_clause_lut_bank_16x();
        let result = find_first_sat_two_ap_no_sync(&bank, 3).expect("valid setup");

        let lane0 = result.lane0_first_sat.expect("lane0 sat");
        assert_eq!(lane0.unit_index, 0);
        assert_eq!(lane0.assignment_bits, 0);

        let lane1 = result.lane1_first_sat.expect("lane1 sat");
        assert_eq!(lane1.unit_index, 8);
        assert_eq!(lane1.assignment_bits, 8);
    }

    #[test]
    fn generic_program_expands_to_n_clauses() {
        let program = ClauseLutProgram::new(
            4,
            vec![
                Clause13::Three(
                    Lit { var: 1, neg: true },
                    Lit { var: 2, neg: false },
                    Lit { var: 3, neg: false },
                ),
                Clause13::Two(Lit { var: 1, neg: false }, Lit { var: 3, neg: true }),
                Clause13::Two(Lit { var: 2, neg: false }, Lit { var: 4, neg: false }),
                Clause13::One(Lit { var: 4, neg: false }),
                Clause13::Two(Lit { var: 1, neg: true }, Lit { var: 4, neg: false }),
            ],
        )
        .expect("valid program");

        let bank = build_lut_bank_for_program(&program, 16).expect("non-zero repeats");
        assert_eq!(program.clauses.len(), 5);
        assert_eq!(bank.clause_count(), 80);

        let result = run_two_ap_no_sync_for_program(&program, 16).expect("runnable");
        assert!(result.lane0_first_sat.is_some() || result.lane1_first_sat.is_some());
    }
}
