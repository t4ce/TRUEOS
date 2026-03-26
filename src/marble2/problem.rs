use alloc::vec::Vec;

// Fast-mode bound. Raise this to 128 later if desired.
pub const FAST_MASK_BITS: u16 = 64;
pub const FAST_MASK_WORDS: usize = (FAST_MASK_BITS as usize + 63) / 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalProblem {
    NSat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lit {
    pub var: u16,
    pub neg: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProblemToken {
    // Backward-compatible 3-SAT marker.
    Start3Sat { vars: u16 },
    Clause([Lit; 3]),
    // Generic N-SAT marker.
    StartNSat { vars: u16, literals_per_clause: u8 },
    ClauseN(Vec<Lit>),
    End,
}

#[derive(Clone, Debug)]
pub struct ProblemProgram {
    pub canonical: CanonicalProblem,
    pub vars: u16,
    pub literals_per_clause: u8,
    pub clauses: Vec<Vec<Lit>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClauseMasks {
    pub vars_mask: [u64; FAST_MASK_WORDS],
    pub pos_mask: [u64; FAST_MASK_WORDS],
    pub neg_mask: [u64; FAST_MASK_WORDS],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledNSat {
    pub vars: u16,
    pub literals_per_clause: u8,
    pub clauses: Vec<ClauseMasks>,
}

impl ProblemProgram {
    pub fn from_tokens(tokens: &[ProblemToken]) -> Option<Self> {
        let mut vars = 0u16;
        let mut literals_per_clause = 0u8;
        let mut clauses: Vec<Vec<Lit>> = Vec::new();
        let mut started = false;
        let mut ended = false;

        for token in tokens {
            match token {
                ProblemToken::Start3Sat { vars: v } => {
                    if started {
                        return None;
                    }
                    started = true;
                    vars = *v;
                    literals_per_clause = 3;
                }
                ProblemToken::StartNSat {
                    vars: v,
                    literals_per_clause: n,
                } => {
                    if started {
                        return None;
                    }
                    started = true;
                    vars = *v;
                    literals_per_clause = *n;
                }
                ProblemToken::Clause(c) => {
                    if !started || ended {
                        return None;
                    }
                    if literals_per_clause != 3 {
                        return None;
                    }
                    clauses.push(c.to_vec());
                }
                ProblemToken::ClauseN(c) => {
                    if !started || ended {
                        return None;
                    }
                    if c.len() != literals_per_clause as usize {
                        return None;
                    }
                    clauses.push(c.clone());
                }
                ProblemToken::End => {
                    if !started || ended {
                        return None;
                    }
                    ended = true;
                }
            }
        }

        if !started || !ended || vars == 0 || vars > FAST_MASK_BITS || literals_per_clause == 0 {
            return None;
        }

        if clauses
            .iter()
            .flat_map(|c| c.iter())
            .any(|lit| lit.var == 0 || lit.var > vars)
        {
            return None;
        }

        Some(Self {
            canonical: CanonicalProblem::NSat,
            vars,
            literals_per_clause,
            clauses,
        })
    }

    pub fn map_to_canonical_3sat(tokens: &[ProblemToken]) -> Option<Self> {
        // Placeholder for reductions from other NP-hard forms into 3-SAT.
        Self::from_tokens(tokens)
    }

    pub fn map_to_canonical_nsat(tokens: &[ProblemToken]) -> Option<Self> {
        // Placeholder for reductions from other NP-hard forms into N-SAT.
        Self::from_tokens(tokens)
    }

    pub fn compile_masks(&self) -> CompiledNSat {
        let mut compiled: Vec<ClauseMasks> = Vec::with_capacity(self.clauses.len());

        for clause in &self.clauses {
            let mut vars_mask = [0u64; FAST_MASK_WORDS];
            let mut pos_mask = [0u64; FAST_MASK_WORDS];
            let mut neg_mask = [0u64; FAST_MASK_WORDS];

            for lit in clause {
                let zero_based = (lit.var - 1) as usize;
                let word = zero_based / 64;
                let bit = 1u64 << (zero_based % 64);
                vars_mask[word] |= bit;
                if lit.neg {
                    neg_mask[word] |= bit;
                } else {
                    pos_mask[word] |= bit;
                }
            }

            compiled.push(ClauseMasks {
                vars_mask,
                pos_mask,
                neg_mask,
            });
        }

        CompiledNSat {
            vars: self.vars,
            literals_per_clause: self.literals_per_clause,
            clauses: compiled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_compile_nsat() {
        let tokens = vec![
            ProblemToken::StartNSat {
                vars: 4,
                literals_per_clause: 4,
            },
            ProblemToken::ClauseN(vec![
                Lit { var: 1, neg: false },
                Lit { var: 2, neg: true },
                Lit { var: 3, neg: false },
                Lit { var: 4, neg: true },
            ]),
            ProblemToken::End,
        ];

        let program = ProblemProgram::from_tokens(&tokens).expect("valid nsat program");
        assert_eq!(program.canonical, CanonicalProblem::NSat);
        assert_eq!(program.literals_per_clause, 4);

        let compiled = program.compile_masks();
        assert_eq!(compiled.clauses.len(), 1);
        assert_eq!(compiled.clauses[0].vars_mask[0], 0b1111);
    }

    #[test]
    fn rejects_vars_beyond_fast_window() {
        let tokens = vec![
            ProblemToken::StartNSat {
                vars: FAST_MASK_BITS + 1,
                literals_per_clause: 3,
            },
            ProblemToken::ClauseN(vec![
                Lit { var: 1, neg: false },
                Lit { var: 2, neg: false },
                Lit { var: 3, neg: false },
            ]),
            ProblemToken::End,
        ];

        assert!(ProblemProgram::from_tokens(&tokens).is_none());
    }
}
