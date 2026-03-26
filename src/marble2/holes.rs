use alloc::vec::Vec;

use super::problem::{FAST_MASK_WORDS, ProblemToken};

#[derive(Clone, Debug)]
pub enum WhiteHolePayload {
    ProblemTokens(Vec<ProblemToken>),
}

#[derive(Clone, Debug)]
pub enum BlackHolePayload {
    Satisfiable,
    Unsatisfiable,
    Assignment {
        value_mask: [u64; FAST_MASK_WORDS],
        assigned_mask: [u64; FAST_MASK_WORDS],
    },
    HistoryExceeded {
        value_mask: [u64; FAST_MASK_WORDS],
        assigned_mask: [u64; FAST_MASK_WORDS],
    },
    Trace(&'static str),
}
