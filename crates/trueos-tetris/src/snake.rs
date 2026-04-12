use crate::RandomSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    const fn is_opposite(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Up, Self::Down)
                | (Self::Down, Self::Up)
                | (Self::Left, Self::Right)
                | (Self::Right, Self::Left)
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    Running,
    Won,
    Lost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickResult {
    Moved,
    AteFood,
    Won,
    Lost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
    Empty,
    Body,
    Head,
    Food,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepInfo {
    pub head_x: usize,
    pub head_y: usize,
    pub tail_x: usize,
    pub tail_y: usize,
    pub grew: bool,
}

pub trait SnakeEvents {
    fn on_food_spawned(&mut self, _x: usize, _y: usize) {}
    fn on_food_eaten(&mut self, _x: usize, _y: usize, _score: u32, _length: usize) {}
    fn on_step(&mut self, _info: StepInfo) {}
    fn on_game_won(&mut self) {}
    fn on_game_lost(&mut self) {}
}

pub struct NoopEvents;

impl SnakeEvents for NoopEvents {}

#[derive(Clone)]
pub struct Game<const W: usize, const H: usize>
where
    [(); W]:,
    [(); H]:,
{
    occupancy: [[u16; H]; W],
    head_x: usize,
    head_y: usize,
    direction: Direction,
    queued_direction: Direction,
    food: Option<(usize, usize)>,
    length: usize,
    score: u32,
    changed: bool,
    state: GameState,
}

impl<const W: usize, const H: usize> Game<W, H>
where
    [(); W]:,
    [(); H]:,
{
    pub fn new<R: RandomSource, T: SnakeEvents>(rng: &mut R, events: &mut T) -> Self {
        debug_assert!(W > 0 && H > 0);

        let mut game = Self {
            occupancy: [[0; H]; W],
            head_x: W / 2,
            head_y: H / 2,
            direction: Direction::Right,
            queued_direction: Direction::Right,
            food: None,
            length: 0,
            score: 0,
            changed: true,
            state: GameState::Running,
        };

        game.reset(rng, events);
        game
    }

    pub fn reset<R: RandomSource, T: SnakeEvents>(&mut self, rng: &mut R, events: &mut T) {
        self.occupancy = [[0; H]; W];
        self.direction = Direction::Right;
        self.queued_direction = Direction::Right;
        self.score = 0;
        self.changed = true;
        self.state = GameState::Running;

        let base_length = if W >= 3 { 3 } else { 1 };
        self.length = base_length;
        self.head_x = W / 2;
        self.head_y = H / 2;

        for offset in 0..base_length {
            let x = self.head_x.saturating_sub(offset);
            self.occupancy[x][self.head_y] = (base_length - offset) as u16;
        }

        self.spawn_food(rng, events);
    }

    pub const fn width(&self) -> usize {
        W
    }

    pub const fn height(&self) -> usize {
        H
    }

    pub fn state(&self) -> GameState {
        self.state
    }

    pub fn score(&self) -> u32 {
        self.score
    }

    pub fn length(&self) -> usize {
        self.length
    }

    pub fn food(&self) -> Option<(usize, usize)> {
        self.food
    }

    pub fn head(&self) -> (usize, usize) {
        (self.head_x, self.head_y)
    }

    pub fn has_changed(&self) -> bool {
        self.changed
    }

    pub fn consume_changed(&mut self) -> bool {
        let changed = self.changed;
        self.changed = false;
        changed
    }

    pub fn set_direction(&mut self, direction: Direction) -> bool {
        if self.length > 1 && direction.is_opposite(self.direction) {
            return false;
        }

        self.queued_direction = direction;
        true
    }

    pub fn cell_kind_at(&self, x: usize, y: usize) -> Option<CellKind> {
        if x >= W || y >= H {
            return None;
        }

        if self.food == Some((x, y)) {
            return Some(CellKind::Food);
        }

        let occupancy = self.occupancy[x][y];
        if occupancy == 0 {
            return Some(CellKind::Empty);
        }

        if x == self.head_x && y == self.head_y {
            Some(CellKind::Head)
        } else {
            Some(CellKind::Body)
        }
    }

    pub fn tick<R: RandomSource, T: SnakeEvents>(
        &mut self,
        rng: &mut R,
        events: &mut T,
    ) -> TickResult {
        if !matches!(self.state, GameState::Running) {
            return match self.state {
                GameState::Won => TickResult::Won,
                GameState::Lost => TickResult::Lost,
                GameState::Running => TickResult::Moved,
            };
        }

        if !(self.length > 1 && self.queued_direction.is_opposite(self.direction)) {
            self.direction = self.queued_direction;
        }

        let (next_x, next_y) = match self.next_head_position() {
            Some(next) => next,
            None => {
                self.state = GameState::Lost;
                self.changed = true;
                events.on_game_lost();
                return TickResult::Lost;
            }
        };

        let grows = self.food == Some((next_x, next_y));
        let occupied = self.occupancy[next_x][next_y] as usize;
        let collision_limit = if grows { 0 } else { 1 };
        if occupied > collision_limit {
            self.state = GameState::Lost;
            self.changed = true;
            events.on_game_lost();
            return TickResult::Lost;
        }

        let (tail_x, tail_y) = self.find_tail().unwrap_or((self.head_x, self.head_y));
        if !grows {
            self.decay_body();
        }

        if grows {
            self.length = self.length.saturating_add(1);
            self.score = self.score.saturating_add(10);
        }

        self.head_x = next_x;
        self.head_y = next_y;
        self.occupancy[next_x][next_y] = self.length as u16;
        self.changed = true;

        events.on_step(StepInfo {
            head_x: self.head_x,
            head_y: self.head_y,
            tail_x,
            tail_y,
            grew: grows,
        });

        if grows {
            self.food = None;
            events.on_food_eaten(next_x, next_y, self.score, self.length);
            if self.length == W * H {
                self.state = GameState::Won;
                events.on_game_won();
                return TickResult::Won;
            }

            self.spawn_food(rng, events);
            return TickResult::AteFood;
        }

        TickResult::Moved
    }

    fn next_head_position(&self) -> Option<(usize, usize)> {
        match self.direction {
            Direction::Up => self
                .head_y
                .checked_sub(1)
                .map(|next_y| (self.head_x, next_y)),
            Direction::Down => {
                let next_y = self.head_y + 1;
                (next_y < H).then_some((self.head_x, next_y))
            }
            Direction::Left => self
                .head_x
                .checked_sub(1)
                .map(|next_x| (next_x, self.head_y)),
            Direction::Right => {
                let next_x = self.head_x + 1;
                (next_x < W).then_some((next_x, self.head_y))
            }
        }
    }

    fn decay_body(&mut self) {
        for y in 0..H {
            for x in 0..W {
                let value = self.occupancy[x][y];
                if value > 0 {
                    self.occupancy[x][y] = value - 1;
                }
            }
        }
    }

    fn find_tail(&self) -> Option<(usize, usize)> {
        for y in 0..H {
            for x in 0..W {
                if self.occupancy[x][y] == 1 {
                    return Some((x, y));
                }
            }
        }
        None
    }

    fn spawn_food<R: RandomSource, T: SnakeEvents>(&mut self, rng: &mut R, events: &mut T) {
        let mut empty_count = 0_usize;
        for y in 0..H {
            for x in 0..W {
                if self.occupancy[x][y] == 0 {
                    empty_count += 1;
                }
            }
        }

        if empty_count == 0 {
            self.food = None;
            self.state = GameState::Won;
            self.changed = true;
            events.on_game_won();
            return;
        }

        let target = (rng.next_u32() as usize) % empty_count;
        let mut seen = 0_usize;
        for y in 0..H {
            for x in 0..W {
                if self.occupancy[x][y] != 0 {
                    continue;
                }

                if seen == target {
                    self.food = Some((x, y));
                    self.changed = true;
                    events.on_food_spawned(x, y);
                    return;
                }
                seen += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CellKind, Direction, Game, GameState, SnakeEvents, TickResult};
    use crate::RandomSource;

    #[derive(Clone, Copy)]
    struct FixedRng(u32);

    impl RandomSource for FixedRng {
        fn next_u32(&mut self) -> u32 {
            self.0
        }
    }

    #[derive(Default)]
    struct TestEvents {
        food_spawns: usize,
        food_eaten: usize,
        won: usize,
        lost: usize,
    }

    impl SnakeEvents for TestEvents {
        fn on_food_spawned(&mut self, _x: usize, _y: usize) {
            self.food_spawns += 1;
        }

        fn on_food_eaten(&mut self, _x: usize, _y: usize, _score: u32, _length: usize) {
            self.food_eaten += 1;
        }

        fn on_game_won(&mut self) {
            self.won += 1;
        }

        fn on_game_lost(&mut self) {
            self.lost += 1;
        }
    }

    #[test]
    fn snake_spawns_food_and_moves() {
        let mut rng = FixedRng(0);
        let mut events = TestEvents::default();
        let mut game = Game::<6, 4>::new(&mut rng, &mut events);

        assert_eq!(events.food_spawns, 1);
        assert_eq!(game.cell_kind_at(game.head().0, game.head().1), Some(CellKind::Head));

        let result = game.tick(&mut rng, &mut events);
        assert_eq!(result, TickResult::Moved);
        assert_eq!(game.state(), GameState::Running);
    }

    #[test]
    fn snake_eats_food_and_scores() {
        let mut rng = FixedRng(0);
        let mut events = TestEvents::default();
        let mut game = Game::<4, 1>::new(&mut rng, &mut events);

        game.set_direction(Direction::Right);
        let result = game.tick(&mut rng, &mut events);

        assert_eq!(result, TickResult::AteFood);
        assert_eq!(game.length(), 4);
        assert_eq!(game.score(), 10);
        assert_eq!(events.food_eaten, 1);
    }

    #[test]
    fn snake_loses_on_wall_collision() {
        let mut rng = FixedRng(0);
        let mut events = TestEvents::default();
        let mut game = Game::<3, 3>::new(&mut rng, &mut events);

        let _ = game.tick(&mut rng, &mut events);
        let result = game.tick(&mut rng, &mut events);

        assert_eq!(result, TickResult::Lost);
        assert_eq!(game.state(), GameState::Lost);
        assert_eq!(events.lost, 1);
    }
}
