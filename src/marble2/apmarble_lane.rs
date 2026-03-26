use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time_driver::{TICK_HZ, now};

use crate::marble2::{
    BlackHolePayload, Lit, MarbleState, MarbleUniverse, ProblemToken, ProgramEval,
    WhiteHolePayload, blackhole_payload_for_solve_outcome, eval_program_masks,
    forge_from_whitehole, is_assignment_complete, solve_nsat_fast, witness_is_solution,
};

pub const APMARBLE_SLOT: u32 = 2;
pub const BOOT_JOB_DELAY_MS: u64 = 2_500;
pub const MAX_SOLVER_DECISIONS: u32 = 200_000;

static BOOT_ONCE: AtomicBool = AtomicBool::new(false);
static BOOT_NOT_BEFORE_TICK: AtomicU64 = AtomicU64::new(0);

#[inline(always)]
fn raw_port_write_bytes(bytes: &[u8]) {
    for &byte in bytes {
        unsafe { crate::portio::outb(0xE9, byte) };
    }
}

#[inline(always)]
fn raw_port_write_hex_u64(v: u64) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    raw_port_write_bytes(b"0x");
    for shift in (0..16).rev() {
        let nibble = ((v >> (shift * 4)) & 0xF) as usize;
        raw_port_write_bytes(&[HEX[nibble]]);
    }
}

#[inline(always)]
fn raw_port_write_u32(v: u32) {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = buf.len();
    if n == 0 {
        raw_port_write_bytes(b"0");
        return;
    }
    while n != 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    raw_port_write_bytes(&buf[i..]);
}

#[inline(always)]
fn ms_to_ticks_ceil(ms: u64) -> u64 {
    (TICK_HZ as u64).saturating_mul(ms).saturating_add(999) / 1000
}

fn static_instances() -> [WhiteHolePayload; 4] {
    [
        // Example 0 (SAT): (x or !x or x) and (a or y or q) and x and (u or !a or y)
        // Encoded as 3-SAT by repeating x in the 1-literal clause.
        WhiteHolePayload::ProblemTokens(vec![
            ProblemToken::Start3Sat { vars: 6 },
            ProblemToken::Clause([
                Lit { var: 1, neg: false },
                Lit { var: 1, neg: true },
                Lit { var: 1, neg: false },
            ]),
            ProblemToken::Clause([
                Lit { var: 2, neg: false },
                Lit { var: 3, neg: false },
                Lit { var: 4, neg: false },
            ]),
            ProblemToken::Clause([
                Lit { var: 1, neg: false },
                Lit { var: 1, neg: false },
                Lit { var: 1, neg: false },
            ]),
            ProblemToken::Clause([
                Lit { var: 6, neg: false },
                Lit { var: 2, neg: true },
                Lit { var: 3, neg: false },
            ]),
            ProblemToken::End,
        ]),
        // Example 1 (UNSAT): (x) and (!x)
        WhiteHolePayload::ProblemTokens(vec![
            ProblemToken::StartNSat {
                vars: 1,
                literals_per_clause: 1,
            },
            ProblemToken::ClauseN(vec![Lit { var: 1, neg: false }]),
            ProblemToken::ClauseN(vec![Lit { var: 1, neg: true }]),
            ProblemToken::End,
        ]),
        // Example 2 (SAT, 4-SAT style): (a or y or q or p)
        WhiteHolePayload::ProblemTokens(vec![
            ProblemToken::StartNSat {
                vars: 4,
                literals_per_clause: 4,
            },
            ProblemToken::ClauseN(vec![
                Lit { var: 1, neg: false },
                Lit { var: 2, neg: false },
                Lit { var: 3, neg: false },
                Lit { var: 4, neg: false },
            ]),
            ProblemToken::End,
        ]),
        // Example 3 (DIMACS-style):
        // p cnf 50 10
        // 1 -3 7 0
        // -2 0
        // 4 5 -6 8 -9 0
        // -10 11 -12 13 14 -15 16 0
        // 17 -18 0
        // -19 20 21 -22 23 24 0
        // 25 0
        // -26 27 -28 29 0
        // 30 -31 32 -33 34 -35 36 0
        // -37 38 -39 40 41 0
        //
        // Fixed-width fast mode here uses literals_per_clause=7.
        // Shorter clauses are padded by repeating an existing literal from the clause,
        // which preserves disjunction semantics.
        WhiteHolePayload::ProblemTokens(vec![
            ProblemToken::StartNSat {
                vars: 50,
                literals_per_clause: 7,
            },
            ProblemToken::ClauseN(vec![
                Lit { var: 1, neg: false },
                Lit { var: 3, neg: true },
                Lit { var: 7, neg: false },
                Lit { var: 1, neg: false },
                Lit { var: 1, neg: false },
                Lit { var: 1, neg: false },
                Lit { var: 1, neg: false },
            ]),
            ProblemToken::ClauseN(vec![
                Lit { var: 2, neg: true },
                Lit { var: 2, neg: true },
                Lit { var: 2, neg: true },
                Lit { var: 2, neg: true },
                Lit { var: 2, neg: true },
                Lit { var: 2, neg: true },
                Lit { var: 2, neg: true },
            ]),
            ProblemToken::ClauseN(vec![
                Lit { var: 4, neg: false },
                Lit { var: 5, neg: false },
                Lit { var: 6, neg: true },
                Lit { var: 8, neg: false },
                Lit { var: 9, neg: true },
                Lit { var: 4, neg: false },
                Lit { var: 4, neg: false },
            ]),
            ProblemToken::ClauseN(vec![
                Lit { var: 10, neg: true },
                Lit {
                    var: 11,
                    neg: false,
                },
                Lit { var: 12, neg: true },
                Lit {
                    var: 13,
                    neg: false,
                },
                Lit {
                    var: 14,
                    neg: false,
                },
                Lit { var: 15, neg: true },
                Lit {
                    var: 16,
                    neg: false,
                },
            ]),
            ProblemToken::ClauseN(vec![
                Lit {
                    var: 17,
                    neg: false,
                },
                Lit { var: 18, neg: true },
                Lit {
                    var: 17,
                    neg: false,
                },
                Lit {
                    var: 17,
                    neg: false,
                },
                Lit {
                    var: 17,
                    neg: false,
                },
                Lit {
                    var: 17,
                    neg: false,
                },
                Lit {
                    var: 17,
                    neg: false,
                },
            ]),
            ProblemToken::ClauseN(vec![
                Lit { var: 19, neg: true },
                Lit {
                    var: 20,
                    neg: false,
                },
                Lit {
                    var: 21,
                    neg: false,
                },
                Lit { var: 22, neg: true },
                Lit {
                    var: 23,
                    neg: false,
                },
                Lit {
                    var: 24,
                    neg: false,
                },
                Lit {
                    var: 20,
                    neg: false,
                },
            ]),
            ProblemToken::ClauseN(vec![
                Lit {
                    var: 25,
                    neg: false,
                },
                Lit {
                    var: 25,
                    neg: false,
                },
                Lit {
                    var: 25,
                    neg: false,
                },
                Lit {
                    var: 25,
                    neg: false,
                },
                Lit {
                    var: 25,
                    neg: false,
                },
                Lit {
                    var: 25,
                    neg: false,
                },
                Lit {
                    var: 25,
                    neg: false,
                },
            ]),
            ProblemToken::ClauseN(vec![
                Lit { var: 26, neg: true },
                Lit {
                    var: 27,
                    neg: false,
                },
                Lit { var: 28, neg: true },
                Lit {
                    var: 29,
                    neg: false,
                },
                Lit {
                    var: 27,
                    neg: false,
                },
                Lit {
                    var: 27,
                    neg: false,
                },
                Lit {
                    var: 27,
                    neg: false,
                },
            ]),
            ProblemToken::ClauseN(vec![
                Lit {
                    var: 30,
                    neg: false,
                },
                Lit { var: 31, neg: true },
                Lit {
                    var: 32,
                    neg: false,
                },
                Lit { var: 33, neg: true },
                Lit {
                    var: 34,
                    neg: false,
                },
                Lit { var: 35, neg: true },
                Lit {
                    var: 36,
                    neg: false,
                },
            ]),
            ProblemToken::ClauseN(vec![
                Lit { var: 37, neg: true },
                Lit {
                    var: 38,
                    neg: false,
                },
                Lit { var: 39, neg: true },
                Lit {
                    var: 40,
                    neg: false,
                },
                Lit {
                    var: 41,
                    neg: false,
                },
                Lit {
                    var: 38,
                    neg: false,
                },
                Lit {
                    var: 38,
                    neg: false,
                },
            ]),
            ProblemToken::End,
        ]),
    ]
}

fn run_single_instance(universe: &mut MarbleUniverse, idx: u32, payload: WhiteHolePayload) {
    let Some(forged) = forge_from_whitehole(payload) else {
        raw_port_write_bytes(b"m2:forge-fail idx=");
        raw_port_write_u32(idx);
        raw_port_write_bytes(b"\n");
        return;
    };

    // Universe activity in this test: exactly one etched world per injected instance.
    let world_id = universe.push_world(forged.world.clone()) as u32;
    raw_port_write_bytes(b"m2:etcher1 idx=");
    raw_port_write_u32(idx);
    raw_port_write_bytes(b" world=");
    raw_port_write_u32(world_id);
    raw_port_write_bytes(b" widgets=");
    raw_port_write_u32(forged.plan.placements.len() as u32);
    raw_port_write_bytes(b"\n");

    let outcome = solve_nsat_fast(&forged.compiled, MAX_SOLVER_DECISIONS);
    let payload = blackhole_payload_for_solve_outcome(outcome);

    match payload {
        BlackHolePayload::Assignment {
            value_mask,
            assigned_mask,
        } => {
            // Validation step: verify the emitted winning assignment against input clauses.
            let state = MarbleState {
                tile: 0,
                alive: true,
                value_mask,
                assigned_mask,
            };

            let eval = eval_program_masks(&state, &forged.compiled);
            let valid = eval == ProgramEval::Satisfiable
                && witness_is_solution(&state, &forged.compiled)
                && is_assignment_complete(&state, &forged.compiled);

            raw_port_write_bytes(b"m2:solve idx=");
            raw_port_write_u32(idx);
            raw_port_write_bytes(b" sat value=");
            raw_port_write_hex_u64(state.value_mask[0]);
            raw_port_write_bytes(b" assigned=");
            raw_port_write_hex_u64(state.assigned_mask[0]);
            raw_port_write_bytes(b" validate=");
            raw_port_write_bytes(if valid { b"ok" } else { b"fail" });
            raw_port_write_bytes(b"\n");
        }
        BlackHolePayload::Unsatisfiable => {
            raw_port_write_bytes(b"m2:solve idx=");
            raw_port_write_u32(idx);
            raw_port_write_bytes(b" unsat\n");
        }
        BlackHolePayload::HistoryExceeded {
            value_mask,
            assigned_mask,
        } => {
            raw_port_write_bytes(b"m2:solve idx=");
            raw_port_write_u32(idx);
            raw_port_write_bytes(b" history-exceeded value=");
            raw_port_write_hex_u64(value_mask[0]);
            raw_port_write_bytes(b" assigned=");
            raw_port_write_hex_u64(assigned_mask[0]);
            raw_port_write_bytes(b"\n");
        }
        BlackHolePayload::Trace(tag) => {
            raw_port_write_bytes(b"m2:solve idx=");
            raw_port_write_u32(idx);
            raw_port_write_bytes(b" ");
            raw_port_write_bytes(tag.as_bytes());
            raw_port_write_bytes(b"\n");
        }
        BlackHolePayload::Satisfiable => {
            raw_port_write_bytes(b"m2:solve idx=");
            raw_port_write_u32(idx);
            raw_port_write_bytes(b" sat-partial\n");
        }
    }
}

fn run_ap2_boot_job_once() {
    raw_port_write_bytes(b"m2:start\n");

    let mut universe = MarbleUniverse::new();
    let instances = static_instances();
    for (idx, payload) in instances.into_iter().enumerate() {
        run_single_instance(&mut universe, idx as u32, payload);
    }

    raw_port_write_bytes(b"m2:done\n");
}

#[inline]
pub fn poll_for_current_slot(current_slot: u32, total_cpus: usize) {
    if total_cpus <= APMARBLE_SLOT as usize || current_slot != APMARBLE_SLOT {
        return;
    }

    let mut deadline = BOOT_NOT_BEFORE_TICK.load(Ordering::Acquire);
    if deadline == 0 {
        let d = now().saturating_add(ms_to_ticks_ceil(BOOT_JOB_DELAY_MS));
        let _ = BOOT_NOT_BEFORE_TICK.compare_exchange(0, d, Ordering::AcqRel, Ordering::Acquire);
        deadline = BOOT_NOT_BEFORE_TICK.load(Ordering::Acquire);
    }
    if now() < deadline {
        return;
    }

    if BOOT_ONCE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        run_ap2_boot_job_once();
    }
}
