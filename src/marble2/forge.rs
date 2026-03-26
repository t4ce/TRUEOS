use super::holes::WhiteHolePayload;
use super::problem::{CompiledNSat, ProblemProgram};
use super::waver_etcher::{Etcher, Waver, WaverPlan};
use super::world::MarbleWorld;

#[derive(Clone, Debug)]
pub struct ForgedRunnableWorld {
    pub program: ProblemProgram,
    pub compiled: CompiledNSat,
    pub plan: WaverPlan,
    pub world: MarbleWorld,
}

/// End-to-end forge path:
/// white-hole tokens -> canonical NSAT program -> compiled masks -> waver plan -> etched runnable world.
pub fn forge_from_whitehole(payload: WhiteHolePayload) -> Option<ForgedRunnableWorld> {
    let WhiteHolePayload::ProblemTokens(tokens) = payload;

    let program = ProblemProgram::map_to_canonical_nsat(&tokens)?;
    let compiled = program.compile_masks();
    let plan = Waver::plan(&program);

    let mut world = MarbleWorld::new_empty(plan.required_world_size());
    if !Etcher::apply(&mut world, &plan) {
        return None;
    }

    Some(ForgedRunnableWorld {
        program,
        compiled,
        plan,
        world,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marble2::holes::WhiteHolePayload;
    use crate::marble2::problem::{Lit, ProblemToken};

    #[test]
    fn forge_nsat_from_whitehole_tokens() {
        let payload = WhiteHolePayload::ProblemTokens(vec![
            ProblemToken::StartNSat {
                vars: 3,
                literals_per_clause: 3,
            },
            ProblemToken::ClauseN(vec![
                Lit { var: 1, neg: false },
                Lit { var: 2, neg: true },
                Lit { var: 3, neg: false },
            ]),
            ProblemToken::End,
        ]);

        let forged = forge_from_whitehole(payload).expect("forge from whitehole");
        assert_eq!(forged.program.vars, 3);
        assert_eq!(forged.compiled.clauses.len(), 1);
        assert!(forged.world.size() >= forged.plan.placements.len());
    }
}
