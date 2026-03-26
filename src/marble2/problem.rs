use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalProblem {
    Sat3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lit {
    pub var: u8,
    pub neg: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProblemToken {
    Start3Sat { vars: u8 },
    Clause([Lit; 3]),
    End,
}

#[derive(Clone, Debug)]
pub struct ProblemProgram {
    pub canonical: CanonicalProblem,
    pub vars: u8,
    pub clauses: Vec<[Lit; 3]>,
}

impl ProblemProgram {
    pub fn from_tokens(tokens: &[ProblemToken]) -> Option<Self> {
        let mut vars = 0u8;
        let mut clauses: Vec<[Lit; 3]> = Vec::new();
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
                }
                ProblemToken::Clause(c) => {
                    if !started || ended {
                        return None;
                    }
                    clauses.push(*c);
                }
                ProblemToken::End => {
                    if !started || ended {
                        return None;
                    }
                    ended = true;
                }
            }
        }

        if !started || !ended || vars == 0 || vars > 63 {
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
            canonical: CanonicalProblem::Sat3,
            vars,
            clauses,
        })
    }

    pub fn map_to_canonical_3sat(tokens: &[ProblemToken]) -> Option<Self> {
        // Placeholder for reductions from other NP-hard forms.
        Self::from_tokens(tokens)
    }
}
