use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use super::{Marble, MarbleGadget, MarblePool, MarbleTransform};

pub type ParkStateId = u16;
pub type ParkShardId = u16;
pub type ParkSymbol = u8;

pub const PARK_BLANK: ParkSymbol = b'_';
pub const PARK_ACCEPT_STATE: ParkStateId = 1;
pub const PARK_REJECT_STATE: ParkStateId = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarbleParkDir {
    Left,
    Right,
    Stay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarbleParkTransition {
    pub next_state: ParkStateId,
    pub write: ParkSymbol,
    pub dir: MarbleParkDir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarbleParkTableError {
    StateOutOfRange,
    FanoutOutOfRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarbleParkDenseTable {
    state_slots: usize,
    symbol_slots: usize,
    fanout: usize,
    cells: Vec<Option<MarbleParkTransition>>,
}

impl MarbleParkDenseTable {
    pub fn new(state_slots: usize, symbol_slots: usize, fanout: usize) -> Self {
        Self {
            state_slots,
            symbol_slots,
            fanout,
            cells: vec![
                None;
                state_slots
                    .saturating_mul(symbol_slots)
                    .saturating_mul(fanout)
            ],
        }
    }

    pub fn fanout(&self) -> usize {
        self.fanout
    }

    pub fn set(
        &mut self,
        state: ParkStateId,
        read: ParkSymbol,
        lane: usize,
        transition: MarbleParkTransition,
    ) -> Result<(), MarbleParkTableError> {
        let index = self
            .cell_index(state, read, lane)
            .ok_or(if lane >= self.fanout {
                MarbleParkTableError::FanoutOutOfRange
            } else {
                MarbleParkTableError::StateOutOfRange
            })?;
        self.cells[index] = Some(transition);
        Ok(())
    }

    pub fn transitions(
        &self,
        state: ParkStateId,
        read: ParkSymbol,
    ) -> MarbleParkTransitionIter<'_> {
        let Some(base) = self.cell_index(state, read, 0) else {
            return MarbleParkTransitionIter::empty();
        };
        let end = base + self.fanout;
        MarbleParkTransitionIter::new(&self.cells[base..end])
    }

    fn cell_index(&self, state: ParkStateId, read: ParkSymbol, lane: usize) -> Option<usize> {
        let state = state as usize;
        let read = read as usize;
        if state >= self.state_slots || read >= self.symbol_slots || lane >= self.fanout {
            return None;
        }

        Some(((state * self.symbol_slots) + read) * self.fanout + lane)
    }
}

pub struct MarbleParkTransitionIter<'a> {
    slots: &'a [Option<MarbleParkTransition>],
    index: usize,
}

impl<'a> MarbleParkTransitionIter<'a> {
    fn new(slots: &'a [Option<MarbleParkTransition>]) -> Self {
        Self { slots, index: 0 }
    }

    fn empty() -> Self {
        Self {
            slots: &[],
            index: 0,
        }
    }
}

impl Iterator for MarbleParkTransitionIter<'_> {
    type Item = MarbleParkTransition;

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < self.slots.len() {
            let slot = self.slots[self.index];
            self.index += 1;
            if let Some(transition) = slot {
                return Some(transition);
            }
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarbleParkConfigMarble<const WINDOW: usize> {
    pub shard_hint: ParkShardId,
    pub state: ParkStateId,
    pub steps: u32,
    pub head: u16,
    pub blank: ParkSymbol,
    pub cells: [ParkSymbol; WINDOW],
}

impl<const WINDOW: usize> MarbleParkConfigMarble<WINDOW> {
    pub fn new(shard_hint: ParkShardId, blank: ParkSymbol, state: ParkStateId) -> Self {
        Self {
            shard_hint,
            state,
            steps: 0,
            head: 0,
            blank,
            cells: [blank; WINDOW],
        }
    }

    pub fn with_input(
        shard_hint: ParkShardId,
        blank: ParkSymbol,
        state: ParkStateId,
        input: &[u8],
    ) -> Self {
        let mut config = Self::new(shard_hint, blank, state);
        let count = core::cmp::min(WINDOW, input.len());
        config.cells[..count].copy_from_slice(&input[..count]);
        config
    }

    pub fn read(&self) -> ParkSymbol {
        self.cells[self.head as usize]
    }

    pub fn write(&mut self, value: ParkSymbol) {
        self.cells[self.head as usize] = value;
    }

    pub fn apply_transition(&mut self, transition: MarbleParkTransition) {
        self.write(transition.write);
        match transition.dir {
            MarbleParkDir::Left => {
                if self.head == 0 {
                    self.cells.copy_within(0..WINDOW.saturating_sub(1), 1);
                    self.cells[0] = self.blank;
                } else {
                    self.head -= 1;
                }
            }
            MarbleParkDir::Right => {
                if self.head as usize + 1 >= WINDOW {
                    self.cells.copy_within(1..WINDOW, 0);
                    self.cells[WINDOW - 1] = self.blank;
                } else {
                    self.head += 1;
                }
            }
            MarbleParkDir::Stay => {}
        }
        self.state = transition.next_state;
        self.steps = self.steps.saturating_add(1);
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = write!(
            out,
            "shard={} state={} steps={} head={} window=",
            self.shard_hint, self.state, self.steps, self.head
        );
        for (index, byte) in self.cells.iter().copied().enumerate() {
            if index == self.head as usize {
                out.push('[');
                out.push(byte as char);
                out.push(']');
            } else {
                out.push(byte as char);
            }
        }
        out
    }
}

impl<const WINDOW: usize> Marble for MarbleParkConfigMarble<WINDOW> {
    fn kind(&self) -> &'static str {
        "park-config"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarbleParkBatchMarble<const WINDOW: usize> {
    pub shard: ParkShardId,
    pub wave: u32,
    pub configs: Vec<MarbleParkConfigMarble<WINDOW>>,
}

impl<const WINDOW: usize> MarbleParkBatchMarble<WINDOW> {
    pub fn new(
        shard: ParkShardId,
        wave: u32,
        configs: Vec<MarbleParkConfigMarble<WINDOW>>,
    ) -> Self {
        Self {
            shard,
            wave,
            configs,
        }
    }
}

impl<const WINDOW: usize> Marble for MarbleParkBatchMarble<WINDOW> {
    fn kind(&self) -> &'static str {
        "park-batch"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarbleParkWorkerReportMarble<const WINDOW: usize> {
    pub shard: ParkShardId,
    pub wave: u32,
    pub explored: usize,
    pub generated: usize,
    pub step_limited: bool,
    pub frontier_limited: bool,
    pub accepted: Option<MarbleParkConfigMarble<WINDOW>>,
    pub frontier: Vec<MarbleParkConfigMarble<WINDOW>>,
}

impl<const WINDOW: usize> Marble for MarbleParkWorkerReportMarble<WINDOW> {
    fn kind(&self) -> &'static str {
        "park-worker-report"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarbleParkPoolError {
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarbleParkShardPool<const WINDOW: usize> {
    shard: ParkShardId,
    capacity: usize,
    queue: VecDeque<MarbleParkConfigMarble<WINDOW>>,
}

impl<const WINDOW: usize> MarbleParkShardPool<WINDOW> {
    pub fn new(shard: ParkShardId, capacity: usize) -> Self {
        Self {
            shard,
            capacity,
            queue: VecDeque::with_capacity(capacity),
        }
    }

    pub fn shard(&self) -> ParkShardId {
        self.shard
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn take_batch(&mut self, wave: u32, max: usize) -> MarbleParkBatchMarble<WINDOW> {
        let mut configs = Vec::with_capacity(core::cmp::min(max, self.queue.len()));
        for _ in 0..max {
            let Some(config) = self.queue.pop_front() else {
                break;
            };
            configs.push(config);
        }

        MarbleParkBatchMarble::new(self.shard, wave, configs)
    }
}

impl<const WINDOW: usize> MarbleGadget for MarbleParkShardPool<WINDOW> {
    fn name(&self) -> &'static str {
        "marble-park-shard-pool"
    }
}

impl<const WINDOW: usize> MarblePool<MarbleParkConfigMarble<WINDOW>>
    for MarbleParkShardPool<WINDOW>
{
    type Error = MarbleParkPoolError;

    fn push(&mut self, marble: MarbleParkConfigMarble<WINDOW>) -> Result<(), Self::Error> {
        if self.queue.len() >= self.capacity {
            return Err(MarbleParkPoolError::Full);
        }
        self.queue.push_back(marble);
        Ok(())
    }

    fn pop(&mut self) -> Result<Option<MarbleParkConfigMarble<WINDOW>>, Self::Error> {
        Ok(self.queue.pop_front())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarbleParkExpandConfig {
    pub max_steps: u32,
    pub frontier_limit: usize,
    pub accept_state: ParkStateId,
    pub reject_state: ParkStateId,
}

#[derive(Debug, Clone)]
pub struct MarbleParkBatchExpander {
    table: MarbleParkDenseTable,
    config: MarbleParkExpandConfig,
}

impl MarbleParkBatchExpander {
    pub fn new(table: MarbleParkDenseTable, config: MarbleParkExpandConfig) -> Self {
        Self { table, config }
    }
}

impl MarbleGadget for MarbleParkBatchExpander {
    fn name(&self) -> &'static str {
        "marble-park-batch-expander"
    }
}

impl<const WINDOW: usize>
    MarbleTransform<MarbleParkBatchMarble<WINDOW>, MarbleParkWorkerReportMarble<WINDOW>>
    for MarbleParkBatchExpander
{
    type Error = core::convert::Infallible;

    fn transform(
        &mut self,
        marble: MarbleParkBatchMarble<WINDOW>,
    ) -> Result<MarbleParkWorkerReportMarble<WINDOW>, Self::Error> {
        let mut explored = 0usize;
        let mut generated = 0usize;
        let mut step_limited = false;
        let mut frontier_limited = false;
        let mut accepted = None;
        let mut frontier = Vec::new();

        for config in marble.configs {
            explored = explored.saturating_add(1);

            if config.state == self.config.accept_state {
                accepted = Some(config);
                break;
            }

            if config.state == self.config.reject_state {
                continue;
            }

            if config.steps >= self.config.max_steps {
                step_limited = true;
                continue;
            }

            let read = config.read();
            for transition in self.table.transitions(config.state, read) {
                if frontier.len() >= self.config.frontier_limit {
                    frontier_limited = true;
                    break;
                }

                let mut next = config.clone();
                next.apply_transition(transition);
                frontier.push(next);
                generated = generated.saturating_add(1);
            }

            if frontier_limited {
                break;
            }
        }

        Ok(MarbleParkWorkerReportMarble {
            shard: marble.shard,
            wave: marble.wave,
            explored,
            generated,
            step_limited,
            frontier_limited,
            accepted,
            frontier,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarbleParkRunError {
    InvalidShardCount,
    QueueFull { shard: ParkShardId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarbleParkWaveSummary<const WINDOW: usize> {
    pub wave: u32,
    pub explored: usize,
    pub generated: usize,
    pub step_limited: bool,
    pub frontier_limited: bool,
    pub accepted: Option<MarbleParkConfigMarble<WINDOW>>,
    pub shard_loads: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct MarblePark<const WINDOW: usize> {
    pools: Vec<MarbleParkShardPool<WINDOW>>,
    expander: MarbleParkBatchExpander,
    batch_size: usize,
    wave: u32,
}

impl<const WINDOW: usize> MarblePark<WINDOW> {
    pub fn new(
        shard_count: usize,
        capacity_per_shard: usize,
        batch_size: usize,
        expander: MarbleParkBatchExpander,
    ) -> Result<Self, MarbleParkRunError> {
        if shard_count == 0 {
            return Err(MarbleParkRunError::InvalidShardCount);
        }

        let mut pools = Vec::with_capacity(shard_count);
        for shard in 0..shard_count {
            pools.push(MarbleParkShardPool::new(
                shard as ParkShardId,
                capacity_per_shard,
            ));
        }

        Ok(Self {
            pools,
            expander,
            batch_size,
            wave: 0,
        })
    }

    pub fn shard_count(&self) -> usize {
        self.pools.len()
    }

    pub fn wave(&self) -> u32 {
        self.wave
    }

    pub fn shard_loads(&self) -> Vec<usize> {
        self.pools.iter().map(MarbleParkShardPool::len).collect()
    }

    pub fn enqueue(
        &mut self,
        mut config: MarbleParkConfigMarble<WINDOW>,
    ) -> Result<(), MarbleParkRunError> {
        let shard_index = self.route_shard(config.shard_hint);
        config.shard_hint = shard_index as ParkShardId;
        self.pools[shard_index]
            .push(config)
            .map_err(|_| MarbleParkRunError::QueueFull {
                shard: shard_index as ParkShardId,
            })
    }

    pub fn run_wave(&mut self) -> Result<MarbleParkWaveSummary<WINDOW>, MarbleParkRunError> {
        let mut explored = 0usize;
        let mut generated = 0usize;
        let mut step_limited = false;
        let mut frontier_limited = false;
        let mut accepted = None;
        let wave = self.wave;

        for shard_index in 0..self.pools.len() {
            let batch = self.pools[shard_index].take_batch(wave, self.batch_size);
            if batch.configs.is_empty() {
                continue;
            }

            let report = self
                .expander
                .transform(batch)
                .map_err(|never| match never {})?;

            explored = explored.saturating_add(report.explored);
            generated = generated.saturating_add(report.generated);
            step_limited |= report.step_limited;
            frontier_limited |= report.frontier_limited;

            if accepted.is_none() {
                accepted = report.accepted;
            }

            for config in report.frontier {
                self.enqueue(config)?;
            }
        }

        self.wave = self.wave.saturating_add(1);

        Ok(MarbleParkWaveSummary {
            wave,
            explored,
            generated,
            step_limited,
            frontier_limited,
            accepted,
            shard_loads: self.shard_loads(),
        })
    }

    pub fn rebalance_once(&mut self) -> bool {
        if self.pools.len() < 2 {
            return false;
        }

        let mut fullest = 0usize;
        let mut emptiest = 0usize;
        for index in 1..self.pools.len() {
            if self.pools[index].len() > self.pools[fullest].len() {
                fullest = index;
            }
            if self.pools[index].len() < self.pools[emptiest].len() {
                emptiest = index;
            }
        }

        if fullest == emptiest || self.pools[fullest].len() <= self.pools[emptiest].len() + 1 {
            return false;
        }

        let Ok(Some(mut config)) = self.pools[fullest].pop() else {
            return false;
        };
        config.shard_hint = emptiest as ParkShardId;

        self.pools[emptiest].push(config).is_ok()
    }

    fn route_shard(&self, hint: ParkShardId) -> usize {
        hint as usize % self.pools.len()
    }
}

pub fn contains_one_park_table() -> MarbleParkDenseTable {
    let mut table = MarbleParkDenseTable::new(4, 256, 2);

    let _ = table.set(
        0,
        b'0',
        0,
        MarbleParkTransition {
            next_state: 0,
            write: b'0',
            dir: MarbleParkDir::Right,
        },
    );
    let _ = table.set(
        0,
        b'0',
        1,
        MarbleParkTransition {
            next_state: 3,
            write: b'0',
            dir: MarbleParkDir::Right,
        },
    );
    let _ = table.set(
        0,
        b'1',
        0,
        MarbleParkTransition {
            next_state: 0,
            write: b'1',
            dir: MarbleParkDir::Right,
        },
    );
    let _ = table.set(
        0,
        b'1',
        1,
        MarbleParkTransition {
            next_state: PARK_ACCEPT_STATE,
            write: b'1',
            dir: MarbleParkDir::Stay,
        },
    );
    let _ = table.set(
        0,
        PARK_BLANK,
        0,
        MarbleParkTransition {
            next_state: PARK_REJECT_STATE,
            write: PARK_BLANK,
            dir: MarbleParkDir::Stay,
        },
    );
    let _ = table.set(
        3,
        b'0',
        0,
        MarbleParkTransition {
            next_state: 3,
            write: b'0',
            dir: MarbleParkDir::Right,
        },
    );
    let _ = table.set(
        3,
        b'1',
        0,
        MarbleParkTransition {
            next_state: 3,
            write: b'1',
            dir: MarbleParkDir::Right,
        },
    );
    let _ = table.set(
        3,
        PARK_BLANK,
        0,
        MarbleParkTransition {
            next_state: PARK_REJECT_STATE,
            write: PARK_BLANK,
            dir: MarbleParkDir::Stay,
        },
    );

    table
}

pub fn contains_one_park_expander() -> MarbleParkBatchExpander {
    MarbleParkBatchExpander::new(
        contains_one_park_table(),
        MarbleParkExpandConfig {
            max_steps: 16,
            frontier_limit: 64,
            accept_state: PARK_ACCEPT_STATE,
            reject_state: PARK_REJECT_STATE,
        },
    )
}

pub fn contains_one_park<const WINDOW: usize>() -> MarblePark<WINDOW> {
    MarblePark::new(4, 64, 16, contains_one_park_expander())
        .expect("contains_one_park uses a valid shard count")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shard_pool_enforces_capacity() {
        let mut pool = MarbleParkShardPool::<8>::new(0, 1);
        assert!(
            pool.push(MarbleParkConfigMarble::with_input(0, PARK_BLANK, 0, b"0"))
                .is_ok()
        );
        assert_eq!(
            pool.push(MarbleParkConfigMarble::with_input(0, PARK_BLANK, 0, b"1")),
            Err(MarbleParkPoolError::Full)
        );
    }

    #[test]
    fn batch_expander_generates_or_accepts() {
        let mut expander = contains_one_park_expander();
        let batch = MarbleParkBatchMarble::new(
            0,
            0,
            vec![MarbleParkConfigMarble::<8>::with_input(
                0, PARK_BLANK, 0, b"1",
            )],
        );
        let report = expander.transform(batch).unwrap();
        assert_eq!(report.explored, 1);
        assert!(report.generated > 0 || report.accepted.is_some());
    }

    #[test]
    fn park_rebalances_between_shards() {
        let mut park = MarblePark::<8>::new(2, 8, 4, contains_one_park_expander()).unwrap();
        for _ in 0..4 {
            park.enqueue(MarbleParkConfigMarble::with_input(0, PARK_BLANK, 0, b"0"))
                .unwrap();
        }

        assert_eq!(park.shard_loads(), vec![4, 0]);
        assert!(park.rebalance_once());
        assert_eq!(park.shard_loads(), vec![3, 1]);
    }

    #[test]
    fn park_wave_returns_summary() {
        let mut park = contains_one_park::<8>();
        park.enqueue(MarbleParkConfigMarble::with_input(0, PARK_BLANK, 0, b"1"))
            .unwrap();

        let summary = park.run_wave().unwrap();
        assert_eq!(summary.wave, 0);
        assert_eq!(summary.explored, 1);
        assert!(summary.generated > 0 || summary.accepted.is_some());
    }
}
