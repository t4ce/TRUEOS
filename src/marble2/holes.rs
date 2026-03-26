use alloc::vec::Vec;

use super::problem::ProblemToken;

#[derive(Clone, Debug)]
pub enum WhiteHolePayload {
    ProblemTokens(Vec<ProblemToken>),
}

#[derive(Clone, Debug)]
pub enum BlackHolePayload {
    Satisfiable,
    Unsatisfiable,
    Assignment { value_mask: u64, assigned_mask: u64 },
    Trace(&'static str),
}
