use core::mem;

use crate::RandomSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GemKind {
    Ruby,
    Sapphire,
    Emerald,
    Topaz,
    Diamond,
    Amethyst,
}

impl GemKind {
    pub const ALL: [Self; 6] = [
        Self::Ruby,
        Self::Sapphire,
        Self::Emerald,
        Self::Topaz,
        Self::Diamond,
        Self::Amethyst,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchEvent {
    pub cleared: usize,
    pub combo: u32,
    pub score_delta: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapOutcome {
    Invalid,
    NoMatch,
    Matched(MatchEvent),
}

pub trait BejewledEvents {
    fn on_swap(&mut self, _ax: usize, _ay: usize, _bx: usize, _by: usize) {}
    fn on_match(&mut self, _event: MatchEvent) {}
    fn on_score_changed(&mut self, _score: u32) {}
    fn on_board_refilled(&mut self) {}
    fn on_board_reshuffled(&mut self) {}
}

pub struct NoopEvents;

impl BejewledEvents for NoopEvents {}

#[derive(Clone)]
pub struct Game<const W: usize, const H: usize>
where
    [(); W]:,
    [(); H]:,
{
    board: [[Option<GemKind>; H]; W],
    score: u32,
    changed: bool,
}

impl<const W: usize, const H: usize> Game<W, H>
where
    [(); W]:,
    [(); H]:,
{
    pub fn new<R: RandomSource, T: BejewledEvents>(rng: &mut R, events: &mut T) -> Self {
        debug_assert!(W > 0 && H > 0);

        let mut game = Self {
            board: [[None; H]; W],
            score: 0,
            changed: true,
        };
        game.reset(rng, events);
        game
    }

    pub fn reset<R: RandomSource, T: BejewledEvents>(&mut self, rng: &mut R, events: &mut T) {
        loop {
            for y in 0..H {
                for x in 0..W {
                    self.board[x][y] = Some(self.random_gem(rng, x, y));
                }
            }

            if !self.has_any_match() && self.has_any_valid_move() {
                self.score = 0;
                self.changed = true;
                events.on_board_reshuffled();
                return;
            }
        }
    }

    pub const fn width(&self) -> usize {
        W
    }

    pub const fn height(&self) -> usize {
        H
    }

    pub fn score(&self) -> u32 {
        self.score
    }

    pub fn has_changed(&self) -> bool {
        self.changed
    }

    pub fn consume_changed(&mut self) -> bool {
        let changed = self.changed;
        self.changed = false;
        changed
    }

    pub fn gem_at(&self, x: usize, y: usize) -> Option<GemKind> {
        if x >= W || y >= H {
            return None;
        }
        Some(self.board[x][y]).flatten()
    }

    pub fn has_any_valid_move(&self) -> bool {
        for y in 0..H {
            for x in 0..W {
                if x + 1 < W && self.swap_would_match(x, y, x + 1, y) {
                    return true;
                }
                if y + 1 < H && self.swap_would_match(x, y, x, y + 1) {
                    return true;
                }
            }
        }
        false
    }

    pub fn swap<R: RandomSource, T: BejewledEvents>(
        &mut self,
        ax: usize,
        ay: usize,
        bx: usize,
        by: usize,
        rng: &mut R,
        events: &mut T,
    ) -> SwapOutcome {
        if !self.are_neighbors(ax, ay, bx, by) {
            return SwapOutcome::Invalid;
        }

        self.swap_cells(ax, ay, bx, by);
        events.on_swap(ax, ay, bx, by);

        if !self.has_any_match() {
            self.swap_cells(ax, ay, bx, by);
            return SwapOutcome::NoMatch;
        }

        let mut total_cleared = 0_usize;
        let mut combo = 0_u32;
        let mut total_score_delta = 0_u32;

        loop {
            let mut matches = [[false; H]; W];
            let cleared = self.mark_matches(&mut matches);
            if cleared == 0 {
                break;
            }

            combo += 1;
            let score_delta = (cleared as u32).saturating_mul(10).saturating_mul(combo);
            total_cleared += cleared;
            total_score_delta = total_score_delta.saturating_add(score_delta);

            self.clear_matches(&matches);
            self.apply_gravity(rng);
            self.changed = true;

            let event = MatchEvent {
                cleared,
                combo,
                score_delta,
            };
            events.on_match(event);
            events.on_board_refilled();
        }

        self.score = self.score.saturating_add(total_score_delta);
        self.changed = true;
        events.on_score_changed(self.score);

        if !self.has_any_valid_move() {
            self.reshuffle(rng, events);
        }

        SwapOutcome::Matched(MatchEvent {
            cleared: total_cleared,
            combo,
            score_delta: total_score_delta,
        })
    }

    fn reshuffle<R: RandomSource, T: BejewledEvents>(&mut self, rng: &mut R, events: &mut T) {
        loop {
            for y in 0..H {
                for x in 0..W {
                    self.board[x][y] = Some(self.random_any_gem(rng));
                }
            }

            if !self.has_any_match() && self.has_any_valid_move() {
                self.changed = true;
                events.on_board_reshuffled();
                return;
            }
        }
    }

    fn are_neighbors(&self, ax: usize, ay: usize, bx: usize, by: usize) -> bool {
        if ax >= W || ay >= H || bx >= W || by >= H {
            return false;
        }

        let dx = ax.abs_diff(bx);
        let dy = ay.abs_diff(by);
        dx + dy == 1
    }

    fn swap_cells(&mut self, ax: usize, ay: usize, bx: usize, by: usize) {
        if ax == bx {
            self.board[ax].swap(ay, by);
            return;
        }

        if ax < bx {
            let (left, right) = self.board.split_at_mut(bx);
            mem::swap(&mut left[ax][ay], &mut right[0][by]);
        } else {
            let (left, right) = self.board.split_at_mut(ax);
            mem::swap(&mut right[0][ay], &mut left[bx][by]);
        }
    }

    fn swap_would_match(&self, ax: usize, ay: usize, bx: usize, by: usize) -> bool {
        let mut clone = self.clone();
        clone.swap_cells(ax, ay, bx, by);
        clone.has_any_match()
    }

    fn has_any_match(&self) -> bool {
        let mut matches = [[false; H]; W];
        self.mark_matches(&mut matches) != 0
    }

    fn mark_matches(&self, matches: &mut [[bool; H]; W]) -> usize {
        let mut cleared = 0_usize;

        for y in 0..H {
            let mut run_start = 0_usize;
            while run_start < W {
                let gem = self.board[run_start][y];
                let mut run_end = run_start + 1;
                while run_end < W && self.board[run_end][y] == gem {
                    run_end += 1;
                }
                if gem.is_some() && run_end - run_start >= 3 {
                    for column in run_start..run_end {
                        if !matches[column][y] {
                            matches[column][y] = true;
                            cleared += 1;
                        }
                    }
                }
                run_start = run_end;
            }
        }

        for x in 0..W {
            let mut run_start = 0_usize;
            while run_start < H {
                let gem = self.board[x][run_start];
                let mut run_end = run_start + 1;
                while run_end < H && self.board[x][run_end] == gem {
                    run_end += 1;
                }
                if gem.is_some() && run_end - run_start >= 3 {
                    for row in run_start..run_end {
                        if !matches[x][row] {
                            matches[x][row] = true;
                            cleared += 1;
                        }
                    }
                }
                run_start = run_end;
            }
        }

        cleared
    }

    fn clear_matches(&mut self, matches: &[[bool; H]; W]) {
        for x in 0..W {
            for y in 0..H {
                if matches[x][y] {
                    self.board[x][y] = None;
                }
            }
        }
    }

    fn apply_gravity<R: RandomSource>(&mut self, rng: &mut R) {
        for x in 0..W {
            let mut write_y = H;

            for y in (0..H).rev() {
                if let Some(gem) = self.board[x][y] {
                    write_y -= 1;
                    self.board[x][write_y] = Some(gem);
                }
            }

            while write_y > 0 {
                write_y -= 1;
                self.board[x][write_y] = Some(self.random_any_gem(rng));
            }
        }

        while self.has_any_match() {
            for y in 0..H {
                for x in 0..W {
                    self.board[x][y] = Some(self.random_gem(rng, x, y));
                }
            }
        }
    }

    fn random_gem<R: RandomSource>(&self, rng: &mut R, x: usize, y: usize) -> GemKind {
        loop {
            let gem = self.random_any_gem(rng);

            let left_1 = x.checked_sub(1).map(|index| self.board[index][y]);
            let left_2 = x.checked_sub(2).map(|index| self.board[index][y]);
            if left_1 == Some(Some(gem)) && left_2 == Some(Some(gem)) {
                continue;
            }

            let up_1 = y.checked_sub(1).map(|index| self.board[x][index]);
            let up_2 = y.checked_sub(2).map(|index| self.board[x][index]);
            if up_1 == Some(Some(gem)) && up_2 == Some(Some(gem)) {
                continue;
            }

            return gem;
        }
    }

    fn random_any_gem<R: RandomSource>(&self, rng: &mut R) -> GemKind {
        GemKind::ALL[(rng.next_u32() as usize) % GemKind::ALL.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::{BejewledEvents, Game, GemKind, MatchEvent, SwapOutcome};
    use crate::RandomSource;

    #[derive(Clone, Copy)]
    struct SeqRng {
        values: [u32; 16],
        index: usize,
    }

    impl SeqRng {
        const fn new(values: [u32; 16]) -> Self {
            Self { values, index: 0 }
        }
    }

    impl RandomSource for SeqRng {
        fn next_u32(&mut self) -> u32 {
            let value = self.values[self.index % self.values.len()];
            self.index += 1;
            value
        }
    }

    #[derive(Default)]
    struct TestEvents {
        matches: usize,
        score_changes: usize,
        refills: usize,
        reshuffles: usize,
    }

    impl BejewledEvents for TestEvents {
        fn on_match(&mut self, _event: MatchEvent) {
            self.matches += 1;
        }

        fn on_score_changed(&mut self, _score: u32) {
            self.score_changes += 1;
        }

        fn on_board_refilled(&mut self) {
            self.refills += 1;
        }

        fn on_board_reshuffled(&mut self) {
            self.reshuffles += 1;
        }
    }

    #[test]
    fn board_starts_without_matches() {
        let mut rng = SeqRng::new([0, 1, 2, 3, 4, 5, 1, 2, 3, 4, 5, 0, 2, 3, 4, 5]);
        let mut events = TestEvents::default();
        let game = Game::<6, 6>::new(&mut rng, &mut events);

        assert!(game.has_any_valid_move());
        assert_eq!(events.reshuffles, 1);
    }

    #[test]
    fn invalid_swap_is_rejected() {
        let mut rng = SeqRng::new([0, 1, 2, 3, 4, 5, 1, 2, 3, 4, 5, 0, 2, 3, 4, 5]);
        let mut events = TestEvents::default();
        let mut game = Game::<4, 4>::new(&mut rng, &mut events);

        let result = game.swap(0, 0, 2, 2, &mut rng, &mut events);
        assert_eq!(result, SwapOutcome::Invalid);
    }

    #[test]
    fn gem_accessor_is_in_bounds() {
        let mut rng = SeqRng::new([0, 1, 2, 3, 4, 5, 1, 2, 3, 4, 5, 0, 2, 3, 4, 5]);
        let mut events = TestEvents::default();
        let game = Game::<4, 4>::new(&mut rng, &mut events);

        assert!(matches!(
            game.gem_at(0, 0),
            Some(
                GemKind::Ruby
                    | GemKind::Sapphire
                    | GemKind::Emerald
                    | GemKind::Topaz
                    | GemKind::Diamond
                    | GemKind::Amethyst
            )
        ));
        assert_eq!(game.gem_at(10, 10), None);
    }
}
