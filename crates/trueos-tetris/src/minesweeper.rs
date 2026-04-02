use core::cmp::min;

use crate::RandomSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    EmptyBoard,
    TooManyMines,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub mine_count: usize,
    pub first_reveal_safe: bool,
}

impl Config {
    pub const fn new(mine_count: usize) -> Self {
        Self {
            mine_count,
            first_reveal_safe: true,
        }
    }

    pub const fn with_first_reveal_safe(mut self, first_reveal_safe: bool) -> Self {
        self.first_reveal_safe = first_reveal_safe;
        self
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new(10)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    Ready,
    Playing,
    Won,
    Lost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionOutcome {
    pub changed: bool,
    pub revealed_cells: usize,
    pub state: GameState,
}

impl ActionOutcome {
    const fn unchanged(state: GameState) -> Self {
        Self {
            changed: false,
            revealed_cells: 0,
            state,
        }
    }

    const fn changed(state: GameState, revealed_cells: usize) -> Self {
        Self {
            changed: true,
            revealed_cells,
            state,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellView {
    pub is_mine: bool,
    pub adjacent_mines: u8,
    pub is_revealed: bool,
    pub is_flagged: bool,
    pub exploded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cell {
    is_mine: bool,
    adjacent_mines: u8,
    is_revealed: bool,
    is_flagged: bool,
}

impl Cell {
    const fn new() -> Self {
        Self {
            is_mine: false,
            adjacent_mines: 0,
            is_revealed: false,
            is_flagged: false,
        }
    }
}

pub trait MinesweeperEvents {
    fn on_game_started(&mut self) {}
    fn on_cell_revealed(&mut self, _x: usize, _y: usize, _adjacent_mines: u8) {}
    fn on_flag_toggled(&mut self, _x: usize, _y: usize, _is_flagged: bool) {}
    fn on_mine_triggered(&mut self, _x: usize, _y: usize) {}
    fn on_game_won(&mut self) {}
    fn on_game_lost(&mut self) {}
}

pub struct NoopEvents;

impl MinesweeperEvents for NoopEvents {}

#[derive(Clone)]
pub struct Game<const W: usize, const H: usize>
where
    [(); W]:,
    [(); H]:,
{
    board: [[Cell; H]; W],
    config: Config,
    state: GameState,
    changed: bool,
    mines_placed: bool,
    revealed_safe_cells: usize,
    flags_used: usize,
    exploded_cell: Option<(usize, usize)>,
}

impl<const W: usize, const H: usize> Game<W, H>
where
    [(); W]:,
    [(); H]:,
{
    pub fn new(config: Config) -> Result<Self, ConfigError> {
        if W == 0 || H == 0 {
            return Err(ConfigError::EmptyBoard);
        }

        let cell_count = W * H;
        let reserved_safe_cells = config.first_reveal_safe as usize;
        if config.mine_count > cell_count.saturating_sub(reserved_safe_cells) {
            return Err(ConfigError::TooManyMines);
        }

        Ok(Self {
            board: [[Cell::new(); H]; W],
            config,
            state: GameState::Ready,
            changed: true,
            mines_placed: false,
            revealed_safe_cells: 0,
            flags_used: 0,
            exploded_cell: None,
        })
    }

    pub const fn width(&self) -> usize {
        W
    }

    pub const fn height(&self) -> usize {
        H
    }

    pub const fn total_cells(&self) -> usize {
        W * H
    }

    pub const fn mine_count(&self) -> usize {
        self.config.mine_count
    }

    pub const fn state(&self) -> GameState {
        self.state
    }

    pub fn has_changed(&self) -> bool {
        self.changed
    }

    pub fn consume_changed(&mut self) -> bool {
        let changed = self.changed;
        self.changed = false;
        changed
    }

    pub fn mines_placed(&self) -> bool {
        self.mines_placed
    }

    pub fn flags_used(&self) -> usize {
        self.flags_used
    }

    pub fn remaining_mines_hint(&self) -> usize {
        self.config.mine_count.saturating_sub(self.flags_used)
    }

    pub fn revealed_safe_cells(&self) -> usize {
        self.revealed_safe_cells
    }

    pub fn exploded_cell(&self) -> Option<(usize, usize)> {
        self.exploded_cell
    }

    pub fn reset(&mut self) {
        self.board = [[Cell::new(); H]; W];
        self.state = GameState::Ready;
        self.changed = true;
        self.mines_placed = false;
        self.revealed_safe_cells = 0;
        self.flags_used = 0;
        self.exploded_cell = None;
    }

    pub fn cell_view_at(&self, x: usize, y: usize) -> Option<CellView> {
        if x >= W || y >= H {
            return None;
        }

        let cell = self.board[x][y];
        Some(CellView {
            is_mine: cell.is_mine,
            adjacent_mines: cell.adjacent_mines,
            is_revealed: cell.is_revealed,
            is_flagged: cell.is_flagged,
            exploded: self.exploded_cell == Some((x, y)),
        })
    }

    pub fn toggle_flag<T: MinesweeperEvents>(
        &mut self,
        x: usize,
        y: usize,
        events: &mut T,
    ) -> ActionOutcome {
        if x >= W || y >= H || matches!(self.state, GameState::Won | GameState::Lost) {
            return ActionOutcome::unchanged(self.state);
        }

        let cell = &mut self.board[x][y];
        if cell.is_revealed {
            return ActionOutcome::unchanged(self.state);
        }

        cell.is_flagged = !cell.is_flagged;
        if cell.is_flagged {
            self.flags_used = self.flags_used.saturating_add(1);
        } else {
            self.flags_used = self.flags_used.saturating_sub(1);
        }

        self.changed = true;
        events.on_flag_toggled(x, y, cell.is_flagged);
        ActionOutcome::changed(self.state, 0)
    }

    pub fn reveal<R: RandomSource, T: MinesweeperEvents>(
        &mut self,
        x: usize,
        y: usize,
        rng: &mut R,
        events: &mut T,
    ) -> ActionOutcome {
        if x >= W || y >= H || matches!(self.state, GameState::Won | GameState::Lost) {
            return ActionOutcome::unchanged(self.state);
        }

        if self.board[x][y].is_flagged || self.board[x][y].is_revealed {
            return ActionOutcome::unchanged(self.state);
        }

        if !self.mines_placed {
            self.place_mines(x, y, rng, events);
        }

        if self.board[x][y].is_mine {
            self.explode_at(x, y, events);
            return ActionOutcome::changed(self.state, 0);
        }

        let revealed_cells = self.reveal_safe_region_from(x, y, events);
        self.finish_win_if_needed(events);

        if revealed_cells == 0 {
            ActionOutcome::unchanged(self.state)
        } else {
            ActionOutcome::changed(self.state, revealed_cells)
        }
    }

    pub fn chord<T: MinesweeperEvents>(
        &mut self,
        x: usize,
        y: usize,
        events: &mut T,
    ) -> ActionOutcome {
        if x >= W || y >= H || !self.mines_placed || !matches!(self.state, GameState::Playing) {
            return ActionOutcome::unchanged(self.state);
        }

        let cell = self.board[x][y];
        if !cell.is_revealed || cell.adjacent_mines == 0 {
            return ActionOutcome::unchanged(self.state);
        }

        if self.flagged_neighbors(x, y) != cell.adjacent_mines as usize {
            return ActionOutcome::unchanged(self.state);
        }

        let mut revealed_cells = 0;
        for neighbor_y in y.saturating_sub(1)..=min(y.saturating_add(1), H.saturating_sub(1)) {
            for neighbor_x in x.saturating_sub(1)..=min(x.saturating_add(1), W.saturating_sub(1)) {
                if neighbor_x == x && neighbor_y == y {
                    continue;
                }

                let neighbor = self.board[neighbor_x][neighbor_y];
                if neighbor.is_revealed || neighbor.is_flagged {
                    continue;
                }

                if neighbor.is_mine {
                    self.explode_at(neighbor_x, neighbor_y, events);
                    return ActionOutcome::changed(self.state, revealed_cells);
                }

                revealed_cells += self.reveal_safe_region_from(neighbor_x, neighbor_y, events);
            }
        }

        if revealed_cells == 0 {
            return ActionOutcome::unchanged(self.state);
        }

        self.finish_win_if_needed(events);
        ActionOutcome::changed(self.state, revealed_cells)
    }

    fn place_mines<R: RandomSource, T: MinesweeperEvents>(
        &mut self,
        safe_x: usize,
        safe_y: usize,
        rng: &mut R,
        events: &mut T,
    ) {
        let mut remaining_mines = self.config.mine_count;
        let mut remaining_slots = self.total_cells() - self.config.first_reveal_safe as usize;

        for y in 0..H {
            for x in 0..W {
                if self.config.first_reveal_safe && x == safe_x && y == safe_y {
                    continue;
                }

                if remaining_slots == 0 {
                    continue;
                }

                let roll = (rng.next_u32() as usize) % remaining_slots;
                if roll < remaining_mines {
                    self.board[x][y].is_mine = true;
                    remaining_mines -= 1;
                }
                remaining_slots -= 1;
            }
        }

        self.compute_adjacency();
        self.mines_placed = true;
        self.state = GameState::Playing;
        self.changed = true;
        events.on_game_started();
    }

    fn compute_adjacency(&mut self) {
        for y in 0..H {
            for x in 0..W {
                let mut adjacent_mines = 0_u8;
                for neighbor_y in y.saturating_sub(1)..=min(y.saturating_add(1), H.saturating_sub(1)) {
                    for neighbor_x in x.saturating_sub(1)..=min(x.saturating_add(1), W.saturating_sub(1)) {
                        if neighbor_x == x && neighbor_y == y {
                            continue;
                        }

                        if self.board[neighbor_x][neighbor_y].is_mine {
                            adjacent_mines = adjacent_mines.saturating_add(1);
                        }
                    }
                }
                self.board[x][y].adjacent_mines = adjacent_mines;
            }
        }
    }

    fn reveal_safe_region_from<T: MinesweeperEvents>(
        &mut self,
        x: usize,
        y: usize,
        events: &mut T,
    ) -> usize {
        let mut revealed_cells = self.reveal_safe_cell(x, y, events);
        if revealed_cells == 0 || self.board[x][y].adjacent_mines != 0 {
            return revealed_cells;
        }

        let mut expanded = true;
        while expanded {
            expanded = false;
            for scan_y in 0..H {
                for scan_x in 0..W {
                    let scan = self.board[scan_x][scan_y];
                    if !scan.is_revealed || scan.is_mine || scan.adjacent_mines != 0 {
                        continue;
                    }

                    for neighbor_y in
                        scan_y.saturating_sub(1)..=min(scan_y.saturating_add(1), H.saturating_sub(1))
                    {
                        for neighbor_x in scan_x.saturating_sub(1)
                            ..=min(scan_x.saturating_add(1), W.saturating_sub(1))
                        {
                            let newly_revealed =
                                self.reveal_safe_cell(neighbor_x, neighbor_y, events);
                            revealed_cells += newly_revealed;
                            if newly_revealed != 0 {
                                expanded = true;
                            }
                        }
                    }
                }
            }
        }

        revealed_cells
    }

    fn reveal_safe_cell<T: MinesweeperEvents>(
        &mut self,
        x: usize,
        y: usize,
        events: &mut T,
    ) -> usize {
        let cell = &mut self.board[x][y];
        if cell.is_revealed || cell.is_flagged || cell.is_mine {
            return 0;
        }

        cell.is_revealed = true;
        self.revealed_safe_cells = self.revealed_safe_cells.saturating_add(1);
        self.changed = true;
        events.on_cell_revealed(x, y, cell.adjacent_mines);
        1
    }

    fn flagged_neighbors(&self, x: usize, y: usize) -> usize {
        let mut flagged = 0_usize;
        for neighbor_y in y.saturating_sub(1)..=min(y.saturating_add(1), H.saturating_sub(1)) {
            for neighbor_x in x.saturating_sub(1)..=min(x.saturating_add(1), W.saturating_sub(1)) {
                if neighbor_x == x && neighbor_y == y {
                    continue;
                }

                if self.board[neighbor_x][neighbor_y].is_flagged {
                    flagged += 1;
                }
            }
        }
        flagged
    }

    fn explode_at<T: MinesweeperEvents>(&mut self, x: usize, y: usize, events: &mut T) {
        self.board[x][y].is_revealed = true;
        self.exploded_cell = Some((x, y));
        self.state = GameState::Lost;
        self.changed = true;
        events.on_mine_triggered(x, y);
        events.on_game_lost();
    }

    fn finish_win_if_needed<T: MinesweeperEvents>(&mut self, events: &mut T) {
        let safe_cells = self.total_cells().saturating_sub(self.config.mine_count);
        if matches!(self.state, GameState::Playing) && self.revealed_safe_cells >= safe_cells {
            self.state = GameState::Won;
            self.changed = true;
            events.on_game_won();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, Game, GameState, MinesweeperEvents};
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
        started: usize,
        revealed: usize,
        won: usize,
        lost: usize,
        mines: usize,
    }

    impl MinesweeperEvents for TestEvents {
        fn on_game_started(&mut self) {
            self.started += 1;
        }

        fn on_cell_revealed(&mut self, _x: usize, _y: usize, _adjacent_mines: u8) {
            self.revealed += 1;
        }

        fn on_mine_triggered(&mut self, _x: usize, _y: usize) {
            self.mines += 1;
        }

        fn on_game_won(&mut self) {
            self.won += 1;
        }

        fn on_game_lost(&mut self) {
            self.lost += 1;
        }
    }

    #[test]
    fn first_reveal_is_safe() {
        let mut game = Game::<2, 2>::new(Config::new(3)).unwrap();
        let mut rng = FixedRng(0);
        let mut events = TestEvents::default();

        let outcome = game.reveal(0, 0, &mut rng, &mut events);

        assert_eq!(outcome.state, GameState::Won);
        assert_eq!(outcome.revealed_cells, 1);
        assert!(game.cell_view_at(0, 0).unwrap().is_revealed);
        assert!(!game.cell_view_at(0, 0).unwrap().is_mine);
        assert_eq!(events.started, 1);
        assert_eq!(events.won, 1);
    }

    #[test]
    fn zero_mines_reveal_clears_board() {
        let mut game = Game::<3, 3>::new(Config::new(0)).unwrap();
        let mut rng = FixedRng(0);
        let mut events = TestEvents::default();

        let outcome = game.reveal(1, 1, &mut rng, &mut events);

        assert_eq!(outcome.state, GameState::Won);
        assert_eq!(outcome.revealed_cells, 9);
        assert_eq!(game.revealed_safe_cells(), 9);
        assert_eq!(events.revealed, 9);
    }

    #[test]
    fn can_lose_without_first_click_protection() {
        let config = Config::new(1).with_first_reveal_safe(false);
        let mut game = Game::<1, 1>::new(config).unwrap();
        let mut rng = FixedRng(0);
        let mut events = TestEvents::default();

        let outcome = game.reveal(0, 0, &mut rng, &mut events);

        assert_eq!(outcome.state, GameState::Lost);
        assert_eq!(events.lost, 1);
        assert_eq!(events.mines, 1);
    }

    #[test]
    fn toggling_flags_updates_hint() {
        let mut game = Game::<4, 4>::new(Config::new(3)).unwrap();
        let mut events = TestEvents::default();

        let flagged = game.toggle_flag(1, 1, &mut events);
        assert!(flagged.changed);
        assert_eq!(game.flags_used(), 1);
        assert_eq!(game.remaining_mines_hint(), 2);

        let unflagged = game.toggle_flag(1, 1, &mut events);
        assert!(unflagged.changed);
        assert_eq!(game.flags_used(), 0);
        assert_eq!(game.remaining_mines_hint(), 3);
    }
}