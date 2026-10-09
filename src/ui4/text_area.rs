//! Bounded character rectangles for immediate producers and retained rasters.
//!
//! Document coordinates, producer revisions and a two-dimensional cell ring.
//! Panning never copies pixels within an allocation. Missing strips are
//! produced once; the viewport copies out in at most four pieces. GPU lifetime,
//! fonts, parsing and history policy belong to the adapter, not this API.
use alloc::sync::Arc;
use alloc::{vec, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};

/// Clone this budget when one producer owns several rectangular areas. Every
/// raster, including a temporary resize destination, reserves from one cap.
#[derive(Clone)]
pub(crate) struct RasterBudget(Arc<BudgetInner>);
struct BudgetInner {
    cap: usize,
    used: AtomicUsize,
}
pub(crate) struct RasterReservation {
    budget: RasterBudget,
    bytes: usize,
}
impl RasterBudget {
    pub fn new(cap: usize) -> Self {
        Self(Arc::new(BudgetInner {
            cap,
            used: AtomicUsize::new(0),
        }))
    }
    pub fn used(&self) -> usize {
        self.0.used.load(Ordering::Acquire)
    }
    pub fn reserve(&self, bytes: usize) -> Option<RasterReservation> {
        self.0
            .used
            .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|next| *next <= self.0.cap)
            })
            .ok()?;
        Some(RasterReservation {
            budget: self.clone(),
            bytes,
        })
    }
}
impl Drop for RasterReservation {
    fn drop(&mut self) {
        self.budget.0.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

impl RasterReservation {
    pub fn belongs_to(&self, budget: &RasterBudget) -> bool {
        Arc::ptr_eq(&self.budget.0, &budget.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CellRect {
    pub x: i64,
    pub y: i64,
    pub columns: u32,
    pub rows: u32,
}

impl CellRect {
    pub fn contains(self, x: i64, y: i64) -> bool {
        x >= self.x
            && y >= self.y
            && x < self.x + i64::from(self.columns)
            && y < self.y + i64::from(self.rows)
    }
    fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + i64::from(self.columns)).min(other.x + i64::from(other.columns));
        let bottom = (self.y + i64::from(self.rows)).min(other.y + i64::from(other.rows));
        (right > x && bottom > y).then_some(Self {
            x,
            y,
            columns: (right - x) as u32,
            rows: (bottom - y) as u32,
        })
    }
    fn valid(self) -> bool {
        self.columns > 0
            && self.rows > 0
            && self.x.checked_add(i64::from(self.columns)).is_some()
            && self.y.checked_add(i64::from(self.rows)).is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RasterLayout {
    pub columns: u32,
    pub rows: u32,
    pub cell_width: u32,
    pub cell_height: u32,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bytes: usize,
}

impl RasterLayout {
    /// Includes allocator pitch/page padding. Reduce guards before falling back
    /// to direct rendering; a large viewport is never truncated to meet budget.
    pub fn fit(view: CellRect, cell: (u32, u32), guard: u32, budget: usize) -> Option<Self> {
        if !view.valid() || cell.0 == 0 || cell.1 == 0 {
            return None;
        }
        if Self::new(view.columns, view.rows, cell)?.bytes > budget {
            return None;
        }
        let guard = guard
            .min((u16::MAX as u32 / cell.0 - view.columns) / 2)
            .min((u16::MAX as u32 / cell.1 - view.rows) / 2);
        for extra in (0..=guard).rev() {
            let Some(columns) = extra
                .checked_mul(2)
                .and_then(|n| view.columns.checked_add(n))
            else {
                continue;
            };
            let Some(rows) = extra.checked_mul(2).and_then(|n| view.rows.checked_add(n)) else {
                continue;
            };
            if let Some(layout) = Self::new(columns, rows, cell)
                && layout.bytes <= budget
            {
                return Some(layout);
            }
        }
        None
    }
    fn new(columns: u32, rows: u32, cell: (u32, u32)) -> Option<Self> {
        let width = columns.checked_mul(cell.0)?;
        let height = rows.checked_mul(cell.1)?;
        if width > u16::MAX as u32 || height > u16::MAX as u32 {
            return None;
        }
        let pitch = width.checked_mul(4)?.checked_add(63)? & !63;
        let raw = (pitch as usize).checked_mul(height as usize)?;
        let bytes = raw.checked_add(4095)? & !4095;
        Some(Self {
            columns,
            rows,
            cell_width: cell.0,
            cell_height: cell.1,
            width,
            height,
            pitch,
            bytes,
        })
    }
    fn window(self, view: CellRect) -> Option<CellRect> {
        if !view.valid() || view.columns > self.columns || view.rows > self.rows {
            return None;
        }
        let x = view
            .x
            .checked_sub(i64::from((self.columns - view.columns) / 2))?;
        let y = view.y.checked_sub(i64::from((self.rows - view.rows) / 2))?;
        let rect = CellRect {
            x,
            y,
            columns: self.columns,
            rows: self.rows,
        };
        rect.valid().then_some(rect)
    }
    pub fn slot(self, x: i64, y: i64) -> (u32, u32) {
        (x.rem_euclid(i64::from(self.columns)) as u32, y.rem_euclid(i64::from(self.rows)) as u32)
    }
    fn index(self, x: i64, y: i64) -> usize {
        let (x, y) = self.slot(x, y);
        y as usize * self.columns as usize + x as usize
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CellWrite<C> {
    pub x: u32,
    pub y: u32,
    pub cell: C,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CellCopy {
    pub source_x: u32,
    pub source_y: u32,
    pub destination_x: u32,
    pub destination_y: u32,
    pub columns: u32,
    pub rows: u32,
}

#[derive(Debug)]
pub(crate) struct AreaUpdate<C> {
    pub writes: Vec<CellWrite<C>>,
    pub reused: usize,
    pub produced: usize,
}

pub(crate) struct TextArea<C> {
    layout: RasterLayout,
    valid: Option<CellRect>,
    revision: Option<u64>,
    cells: Vec<Option<C>>,
}

impl<C: Clone + PartialEq> TextArea<C> {
    pub fn new(layout: RasterLayout) -> Self {
        Self {
            layout,
            valid: None,
            revision: None,
            cells: vec![None; layout.columns as usize * layout.rows as usize],
        }
    }
    pub fn layout(&self) -> RasterLayout {
        self.layout
    }
    /// Producer identity/font changes and incomplete raster updates invalidate
    /// metadata. The next successful update reconstructs all required cells.
    pub fn invalidate(&mut self) {
        self.valid = None;
        self.revision = None;
    }
    pub fn update(
        &mut self,
        view: CellRect,
        revision: u64,
        mut produce: impl FnMut(i64, i64) -> C,
    ) -> Option<AreaUpdate<C>> {
        let window = self.layout.window(view)?;
        let mut result = AreaUpdate {
            writes: Vec::new(),
            reused: 0,
            produced: 0,
        };
        for y in window.y..window.y + i64::from(window.rows) {
            for x in window.x..window.x + i64::from(window.columns) {
                let index = self.layout.index(x, y);
                let retained = self.valid.is_some_and(|old| old.contains(x, y));
                if retained && self.revision == Some(revision) {
                    result.reused += 1;
                    continue;
                }
                let cell = produce(x, y);
                result.produced += 1;
                if retained && self.cells[index].as_ref() == Some(&cell) {
                    result.reused += 1;
                    continue;
                }
                let (x, y) = self.layout.slot(x, y);
                result.writes.push(CellWrite {
                    x,
                    y,
                    cell: cell.clone(),
                });
                self.cells[index] = Some(cell);
            }
        }
        self.valid = Some(window);
        self.revision = Some(revision);
        Some(result)
    }
    /// The destination frame is separate from this ring allocation.
    pub fn view_copies(&self, view: CellRect) -> Option<Vec<CellCopy>> {
        self.layout.window(view)?;
        if !self
            .valid
            .is_some_and(|r| r.intersection(view) == Some(view))
        {
            return None;
        }
        Some(split_copies(view, self.layout, None))
    }
    /// Retire the returned overlap transfers between separate allocations
    /// before dropping the old backing; invalidate the result on failure.
    pub fn resized(&self, layout: RasterLayout, view: CellRect) -> Option<(Self, Vec<CellCopy>)> {
        let window = layout.window(view)?;
        let mut next = Self::new(layout);
        if (layout.cell_width, layout.cell_height)
            != (self.layout.cell_width, self.layout.cell_height)
        {
            return Some((next, Vec::new()));
        }
        let Some(overlap) = self.valid.and_then(|old| old.intersection(window)) else {
            return Some((next, Vec::new()));
        };
        for y in overlap.y..overlap.y + i64::from(overlap.rows) {
            for x in overlap.x..overlap.x + i64::from(overlap.columns) {
                next.cells[layout.index(x, y)] = self.cells[self.layout.index(x, y)].clone();
            }
        }
        next.valid = Some(overlap);
        next.revision = self.revision;
        Some((next, split_copies(overlap, self.layout, Some(layout))))
    }
}

fn split_copies(
    rect: CellRect,
    source: RasterLayout,
    destination: Option<RasterLayout>,
) -> Vec<CellCopy> {
    let mut copies = Vec::new();
    let mut row = 0;
    while row < rect.rows {
        let y = rect.y + i64::from(row);
        let (_, sy) = source.slot(rect.x, y);
        let dy = destination.map_or(row, |d| d.slot(rect.x, y).1);
        let rows = (rect.rows - row)
            .min(source.rows - sy)
            .min(destination.map_or(u32::MAX, |d| d.rows - dy));
        let mut column = 0;
        while column < rect.columns {
            let x = rect.x + i64::from(column);
            let (sx, _) = source.slot(x, y);
            let dx = destination.map_or(column, |d| d.slot(x, y).0);
            let columns = (rect.columns - column)
                .min(source.columns - sx)
                .min(destination.map_or(u32::MAX, |d| d.columns - dx));
            copies.push(CellCopy {
                source_x: sx,
                source_y: sy,
                destination_x: dx,
                destination_y: dy,
                columns,
                rows,
            });
            column += columns;
        }
        row += rows;
    }
    copies
}
