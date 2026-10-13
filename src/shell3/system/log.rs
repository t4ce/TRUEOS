//! Runtime log policy editor and independent live stream reader.
use super::{
    MatrixTarget, helper,
    tui::{self, Frontend},
};
use crate::log_os::policy::{self, Profile};
use alloc::{collections::VecDeque, format, string::String, vec::Vec};
use log_os_core::{LogArea as A, LogLevel as L, LogLevelPolicy as P, LogLevelSet as S};
use trueos_time::{Duration, Timer};

pub(super) fn start(frontend: Frontend) -> Result<(), String> {
    let Some((target, spawner)) = helper::admit("log", frontend)? else {
        return Ok(());
    };
    let task = menu_task(target.clone()).map_err(|_| {
        tui::cancel_native_attach(&target);
        String::from("Log menu task pool is full.")
    })?;
    spawner.spawn(task);
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Areas,
    Profiles,
    AllAreas,
    Stream,
}

// Presentation order follows the checkbox UI, independently of severity order.
const COLUMNS: [Option<L>; 10] = [
    None,
    Some(L::Once),
    Some(L::User),
    Some(L::Ai),
    Some(L::Error),
    Some(L::Important),
    Some(L::Warn),
    Some(L::Info),
    Some(L::Debug),
    Some(L::Trace),
];
#[derive(Clone, Copy, Debug, PartialEq)]
enum Event {
    Up,
    Down,
    Left,
    Right,
    Choose,
    Quit,
    Click(usize, usize),
}
#[derive(Default)]
struct GridInput {
    sequence: Vec<u8>,
    escape_at: u64,
    after_cr: bool,
}
impl GridInput {
    fn feed(&mut self, bytes: &[u8], now: u64) -> Vec<Event> {
        let mut events = Vec::new();
        for &byte in bytes {
            if !self.sequence.is_empty() {
                if self.sequence.len() == 1 && !matches!(byte, b'[' | b'O') {
                    self.sequence.clear();
                    events.push(Event::Quit);
                    continue;
                }
                self.sequence.push(byte);
                if self.sequence.len() > 2 && (0x40..=0x7e).contains(&byte) {
                    if let Some(event) = Self::decode(&self.sequence) {
                        events.push(event);
                    }
                    self.sequence.clear();
                } else if self.sequence.len() > 64 {
                    self.sequence.clear();
                }
                continue;
            }
            if byte == b'\n' && self.after_cr {
                self.after_cr = false;
                continue;
            }
            self.after_cr = byte == b'\r';
            let event = match byte {
                27 => {
                    self.sequence.push(byte);
                    self.escape_at = now;
                    None
                }
                b'w' | b'k' => Some(Event::Up),
                b's' | b'j' => Some(Event::Down),
                b'a' | b'h' => Some(Event::Left),
                b'd' | b'l' => Some(Event::Right),
                b'\r' | b'\n' | b' ' => Some(Event::Choose),
                b'q' | 3 => Some(Event::Quit),
                _ => None,
            };
            if let Some(event) = event {
                events.push(event);
            }
        }
        events
    }
    fn timeout(&mut self, now: u64) -> Option<Event> {
        if !self.sequence.is_empty() && now.saturating_sub(self.escape_at) >= 75_000_000 {
            let quit = self.sequence.len() == 1;
            self.sequence.clear();
            return quit.then_some(Event::Quit);
        }
        None
    }
    fn decode(bytes: &[u8]) -> Option<Event> {
        match bytes {
            b"\x1b[A" | b"\x1bOA" => Some(Event::Up),
            b"\x1b[B" | b"\x1bOB" => Some(Event::Down),
            b"\x1b[D" | b"\x1bOD" => Some(Event::Left),
            b"\x1b[C" | b"\x1bOC" => Some(Event::Right),
            _ => {
                let report = bytes.strip_prefix(b"\x1b[<")?.strip_suffix(b"M")?;
                let mut fields = core::str::from_utf8(report).ok()?.split(';');
                let button = fields.next()?.parse::<u16>().ok()?;
                let col = fields.next()?.parse::<usize>().ok()?.checked_sub(1)?;
                let row = fields.next()?.parse::<usize>().ok()?.checked_sub(1)?;
                if fields.next().is_some() {
                    return None;
                }
                match button & !28 {
                    0 => Some(Event::Click(row, col)),
                    64 => Some(Event::Up),
                    65 => Some(Event::Down),
                    _ => None,
                }
            }
        }
    }
}

fn effective(policy: P) -> S {
    match policy {
        P::Up(level) => log_os_core::threshold_up_set(level),
        P::Down(level) => log_os_core::threshold_down_set(level),
        P::Only(set) => set,
    }
}
#[derive(Default)]
struct Frame {
    lines: Vec<String>,
    focus: Option<(usize, usize)>,
}
struct Hit {
    col: usize,
    width: usize,
    item: usize,
    column: Option<usize>,
}
#[derive(Default)]
struct Content {
    lines: Vec<String>,
    hits: Vec<Vec<Hit>>,
    focus: Option<(usize, usize)>,
}
impl Content {
    fn push(&mut self, line: String, hits: Vec<Hit>) {
        self.lines.push(line);
        self.hits.push(hits);
    }
}
struct View {
    page: Page,
    selected: usize,
    column: usize,
    input: GridInput,
    status: String,
    scroll: usize,
    changes: u64,
    collecting: bool,
}
impl Default for View {
    fn default() -> Self {
        Self {
            page: Page::Areas,
            selected: 0,
            column: 0,
            input: GridInput::default(),
            status: String::new(),
            scroll: 0,
            changes: 0,
            collecting: false,
        }
    }
}
impl View {
    fn open(&mut self, page: Page) {
        self.page = page;
        self.selected = 0;
        self.scroll = 0;
        self.status.clear();
    }
    fn back(&mut self) -> bool {
        if self.page == Page::Areas {
            true
        } else {
            self.open(Page::Areas);
            false
        }
    }
    fn checked(&self, area: Option<A>, column: usize) -> bool {
        let enabled = |area| {
            let set = effective(policy::area_log_policy(area));
            COLUMNS[column].map_or(set == S::NONE, |level| set.contains(level))
        };
        area.map_or_else(|| A::ALL.into_iter().all(enabled), enabled)
    }
    fn toggle(&mut self, area: Option<A>, column: usize) {
        let enabled = self.checked(area, column);
        let update = |area| {
            let set = effective(policy::area_log_policy(area));
            let next = match COLUMNS[column] {
                None => S::NONE,
                Some(level) if enabled => S::from_bits(set.bits() & !level.set().bits()),
                Some(level) => set.union(level.set()),
            };
            if next != set {
                policy::set_area_log_policy(area, P::only(next));
                true
            } else {
                false
            }
        };
        let changed = if let Some(area) = area {
            update(area)
        } else {
            let mut changed = false;
            for area in A::ALL {
                changed |= update(area);
            }
            changed
        };
        if changed {
            self.changes = self.changes.wrapping_add(1);
            self.status = format!(
                "{}: {} {}",
                area.map_or("all areas", A::tag),
                COLUMNS[column].map_or("none", L::tag),
                if COLUMNS[column].is_none() || !enabled {
                    "selected"
                } else {
                    "cleared"
                }
            );
        }
    }
    fn count(&self) -> usize {
        match self.page {
            Page::Areas => A::ALL.len() + 3,
            Page::Profiles => Profile::ALL.len() + 1,
            Page::AllAreas => 3,
            Page::Stream => 0,
        }
    }
    fn is_grid_item(&self) -> bool {
        match self.page {
            Page::Areas => self.selected < A::ALL.len(),
            Page::AllAreas => self.selected == 0,
            _ => false,
        }
    }
    fn choose(&mut self) {
        match self.page {
            Page::Areas if self.is_grid_item() => {
                self.toggle(Some(A::ALL[self.selected]), self.column)
            }
            Page::Areas => match self.selected - A::ALL.len() {
                0 => self.open(Page::Profiles),
                1 => self.open(Page::Stream),
                _ => self.open(Page::AllAreas),
            },
            Page::Profiles if self.selected < Profile::ALL.len() => {
                policy::apply_profile(self.selected);
                self.changes = self.changes.wrapping_add(1);
                self.open(Page::Areas);
                self.status = format!("Profile applied: {}", policy::profile_name());
            }
            Page::Profiles => {
                self.back();
            }
            Page::AllAreas => match self.selected {
                0 => self.toggle(None, self.column),
                1 => {
                    let mut changed = false;
                    for area in A::ALL {
                        if effective(policy::area_log_policy(area)) != S::ALL {
                            policy::set_area_log_policy(area, P::only(S::ALL));
                            changed = true;
                        }
                    }
                    if changed {
                        self.changes = self.changes.wrapping_add(1);
                        self.status = "All areas: all levels selected".into();
                    }
                }
                _ => {
                    self.back();
                }
            },
            Page::Stream => {
                self.back();
            }
        }
    }
    fn grid_row(&self, content: &mut Content, area: Option<A>, item: usize, cols: usize) {
        content.push(area.map_or("all areas", A::tag).into(), Vec::new());
        let mut line = String::new();
        let mut hits = Vec::new();
        let mut col = 0;
        for column in 0..COLUMNS.len() {
            let tag = COLUMNS[column].map_or("none", L::tag);
            let width = tag.len() + 1;
            let gap = usize::from(col > 0);
            if col > 0 && col + gap + width > cols {
                content.push(line, hits);
                line = String::new();
                hits = Vec::new();
                col = 0;
            }
            if col > 0 {
                line.push(' ');
                col += 1;
            }
            if self.selected == item && self.column == column {
                content.focus = Some((content.lines.len(), col));
            }
            hits.push(Hit {
                col,
                width: 1,
                item,
                column: Some(column),
            });
            line.push(if self.checked(area, column) {
                '🗹'
            } else {
                '☐'
            });
            line.push_str(tag);
            col += width;
        }
        content.push(line, hits);
        content.push(String::new(), Vec::new());
    }
    fn menu_row(&self, content: &mut Content, item: usize, label: &str) {
        if self.selected == item {
            content.focus = Some((content.lines.len(), 0));
        }
        content.push(
            format!("{} {label}", if self.selected == item { '›' } else { ' ' }),
            alloc::vec![Hit {
                col: 2,
                width: label.chars().count(),
                item,
                column: None
            }],
        );
    }
    fn content(&self, cols: usize) -> Content {
        let mut content = Content::default();
        content.push(self.status.clone(), Vec::new());
        match self.page {
            Page::Areas => {
                for (item, area) in A::ALL.into_iter().enumerate() {
                    self.grid_row(&mut content, Some(area), item, cols);
                }
                for (index, label) in [
                    "Profiles: Choose a Preset",
                    "Live log stream",
                    "Set all areas…",
                ]
                .iter()
                .enumerate()
                {
                    self.menu_row(&mut content, A::ALL.len() + index, label);
                }
            }
            Page::AllAreas => {
                self.grid_row(&mut content, None, 0, cols);
                self.menu_row(&mut content, 1, "All levels");
                self.menu_row(&mut content, 2, "Return");
            }
            Page::Profiles => {
                for (index, profile) in Profile::ALL.iter().enumerate() {
                    self.menu_row(&mut content, index, profile.name());
                }
                self.menu_row(&mut content, Profile::ALL.len(), "Return");
            }
            Page::Stream => {}
        }
        content
    }
    fn align_scroll(&mut self, content: &Content, rows: usize) {
        let visible = rows.saturating_sub(3);
        if let Some((row, _)) = content.focus {
            if row < self.scroll {
                self.scroll = row.saturating_sub(1);
            } else if row >= self.scroll + visible {
                self.scroll = row.saturating_add(1).saturating_sub(visible);
            }
        }
        self.scroll = self.scroll.min(content.lines.len().saturating_sub(visible));
    }
    fn event(&mut self, event: Event, cols: usize, rows: usize) -> bool {
        if event == Event::Quit {
            return self.back();
        }
        if self.page == Page::Stream {
            match event {
                Event::Up => self.scroll = self.scroll.saturating_add(1),
                Event::Down => self.scroll = self.scroll.saturating_sub(1),
                Event::Choose | Event::Left => {
                    self.back();
                }
                _ => {}
            }
            return false;
        }
        match event {
            Event::Up => self.selected = self.selected.saturating_sub(1),
            Event::Down => self.selected = (self.selected + 1).min(self.count().saturating_sub(1)),
            Event::Left if self.is_grid_item() => self.column = self.column.saturating_sub(1),
            Event::Right if self.is_grid_item() => {
                self.column = (self.column + 1).min(COLUMNS.len() - 1)
            }
            Event::Left if self.page != Page::Areas => {
                self.back();
            }
            Event::Choose => self.choose(),
            Event::Click(row, col) if row >= 3 && row < rows && col < cols => {
                let content = self.content(cols);
                self.align_scroll(&content, rows);
                if let Some(hit) = content.hits.get(self.scroll + row - 3).and_then(|hits| {
                    hits.iter()
                        .find(|hit| (hit.col..hit.col + hit.width).contains(&col))
                }) {
                    self.selected = hit.item;
                    if let Some(column) = hit.column {
                        self.column = column;
                    }
                    self.choose();
                }
            }
            _ => {}
        }
        false
    }
    fn frame(&mut self, cols: usize, rows: usize, stream: &VecDeque<String>) -> Frame {
        let mut frame = Frame {
            lines: alloc::vec![String::new(); rows],
            focus: None,
        };
        if rows < 5 {
            return frame;
        }
        frame.lines[0] = format!("LOG  area policies   Profile: {}", policy::profile_name());
        frame.lines[1] = match self.page {
            Page::Areas | Page::AllAreas => {
                "↑/↓/←/→ WASD move   Enter toggle   Esc/q back   Mouse Click toggle"
            }
            Page::Profiles => "↑/↓ W/S move   Enter choose   Esc/q back   Mouse Click choose",
            Page::Stream => "↑/↓ W/S scroll   Enter/Esc/q back   Mouse wheel scroll",
        }
        .into();
        frame.lines[2] = match self.page {
            Page::Areas => "LOG AREAS",
            Page::AllAreas => "ALL LOG AREAS",
            Page::Profiles => "PRESETS  Live filters; optional probes depend on the build.",
            Page::Stream => "LIVE LOG STREAM",
        }
        .into();
        if self.page == Page::Stream {
            if !self.collecting {
                frame.lines[3] = "Collection starts after applying a setting. Enter back.".into();
            } else {
                let visible = rows - 3;
                self.scroll = self.scroll.min(stream.len().saturating_sub(visible));
                let start = stream.len().saturating_sub(visible + self.scroll);
                for (index, line) in stream.iter().skip(start).take(visible).enumerate() {
                    frame.lines[3 + index] = line.clone();
                }
            }
        } else {
            let content = self.content(cols);
            self.align_scroll(&content, rows);
            for (index, line) in content
                .lines
                .iter()
                .skip(self.scroll)
                .take(rows - 3)
                .enumerate()
            {
                frame.lines[3 + index] = line.clone();
            }
            frame.focus = content.focus.and_then(|(row, col)| {
                if (self.scroll..self.scroll + rows - 3).contains(&row) {
                    Some((row - self.scroll + 3, col))
                } else {
                    None
                }
            });
        }
        frame
    }
}

// Changed rows only; the first three rows use Shell3's control background.
#[derive(Default)]
struct Screen {
    previous: Vec<String>,
    geometry: (usize, usize),
    focus: Option<(usize, usize)>,
}
impl Screen {
    fn diff(&mut self, frame: &Frame, cols: usize, rows: usize) -> String {
        let resized = self.geometry != (cols, rows);
        let mut bytes = String::new();
        if resized {
            bytes.push_str("\x1b[?25l\x1b[?1000h\x1b[?1006h\x1b[0m\x1b[2J");
            self.previous.clear();
            self.geometry = (cols, rows);
        }
        for (row, line) in frame.lines.iter().take(rows).enumerate() {
            let mut text: String = line
                .chars()
                .take(cols)
                .map(|ch| if ch.is_control() { ' ' } else { ch })
                .collect();
            text.extend(core::iter::repeat_n(' ', cols.saturating_sub(text.chars().count())));
            let focus_changed = self.focus != frame.focus
                && (self.focus.is_some_and(|cell| cell.0 == row)
                    || frame.focus.is_some_and(|cell| cell.0 == row));
            if self.previous.get(row) != Some(&text) || focus_changed {
                bytes.push_str(&format!(
                    "\x1b[{};1H{}{}\x1b[0m",
                    row + 1,
                    if row < 3 {
                        "\x1b[48;2;16;16;16m\x1b[38;2;255;105;180m"
                    } else {
                        "\x1b[0m"
                    },
                    text
                ));
                if let Some((_, col)) = frame.focus.filter(|cell| cell.0 == row && cell.1 < cols) {
                    if let Some(glyph) = text.chars().nth(col) {
                        bytes.push_str(&format!(
                            "\x1b[{};{}H\x1b[4;38;2;255;105;180m{}\x1b[0m",
                            row + 1,
                            col + 1,
                            glyph
                        ));
                    }
                }
            }
            if row == self.previous.len() {
                self.previous.push(text);
            } else {
                self.previous[row] = text;
            }
        }
        self.focus = frame.focus;
        bytes
    }
}
#[derive(Default)]
struct Stream {
    lines: VecDeque<String>,
    pending: Vec<u8>,
}
impl Stream {
    fn feed(&mut self, bytes: &[u8], lost: u64) {
        if lost != 0 {
            self.pending.clear();
            self.push(format!("log: {lost} bytes overwritten before display"));
        }
        for &byte in bytes {
            if byte == b'\n' {
                self.flush();
            } else {
                self.pending.push(byte);
                if self.pending.len() == 4096 {
                    self.flush();
                }
            }
        }
        if bytes.is_empty() && !self.pending.is_empty() {
            self.flush();
        }
    }
    fn flush(&mut self) {
        let line = String::from(String::from_utf8_lossy(&self.pending).trim_end_matches('\r'));
        self.pending.clear();
        self.push(line);
    }
    fn push(&mut self, line: String) {
        if self.lines.len() == 256 {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn reset() {
        policy::apply_profile(0);
    }
    #[test]
    fn area_labels_checkbox_order_and_bottom_actions_follow_the_requested_layout() {
        reset();
        let mut view = View::default();
        let frame = view.frame(100, 60, &VecDeque::new());
        assert!(frame.lines[0].contains("Profile: Build defaults"));
        assert!(frame.lines[1].contains("Enter toggle"));
        assert_eq!(frame.lines[2], "LOG AREAS");
        assert_eq!(frame.lines[4], "global");
        assert_eq!(frame.lines[7], "boot");
        assert_eq!(frame.lines[10], "service");
        assert_eq!(frame.lines[6], "");
        let tags: Vec<_> = frame.lines[5]
            .split_whitespace()
            .map(|s| s.chars().skip(1).collect::<String>())
            .collect();
        assert_eq!(
            tags,
            [
                "none",
                "once",
                "user",
                "ai",
                "error",
                "important",
                "warn",
                "info",
                "debug",
                "trace"
            ]
        );
        assert_eq!(frame.lines[52].trim(), "Profiles: Choose a Preset");
        assert_eq!(frame.lines[53].trim(), "Live log stream");
        assert_eq!(frame.lines[54].trim(), "Set all areas…");
        assert_eq!(view.changes, 0);
    }
    #[test]
    fn only_checkbox_glyphs_toggle_and_mouse_labels_headers_and_gaps_do_nothing() {
        reset();
        let mut view = View::default();
        let before = effective(policy::area_log_policy(A::Global));
        let content = view.content(100);
        let col = content.hits[2]
            .iter()
            .find(|h| h.column == Some(9))
            .unwrap()
            .col;
        for (row, column) in [(4, 0), (5, col + 1), (5, col - 1), (1, 0), (6, 0)] {
            assert!(!view.event(Event::Click(row, column), 100, 60));
        }
        assert_eq!(view.changes, 0);
        assert_eq!(effective(policy::area_log_policy(A::Global)), before);
        let report = format!("\x1b[<0;{};6M", col + 1);
        assert!(view.input.feed(&report.as_bytes()[..5], 0).is_empty());
        for event in view.input.feed(&report.as_bytes()[5..], 1) {
            view.event(event, 100, 60);
        }
        assert_eq!(view.selected, 0);
        assert_eq!(view.column, 9);
        assert_eq!(policy::area_log_policy(A::Global), P::only(before.toggle(L::Trace)));
        assert_eq!(view.changes, 1);
        assert!(
            view.input
                .feed(format!("\x1b[<0;{};6m", col + 1).as_bytes(), 2)
                .is_empty()
        );
        view.event(Event::Choose, 100, 60);
        assert_eq!(policy::area_log_policy(A::Global), P::only(before));
        reset();
    }
    #[test]
    fn arrows_and_wasd_move_across_areas_and_columns_without_applying_settings() {
        reset();
        let mut arrows = View::default();
        let mut wasd = View::default();
        for event in arrows
            .input
            .feed(b"\x1b[C\x1b[C\x1b[B\x1b[B\x1b[D\x1b[A", 0)
        {
            arrows.event(event, 100, 30);
        }
        for event in wasd.input.feed(b"ddssaw", 0) {
            wasd.event(event, 100, 30);
        }
        assert_eq!((arrows.selected, arrows.column), (1, 1));
        assert_eq!((wasd.selected, wasd.column), (1, 1));
        assert_eq!(arrows.changes, 0);
        let before = effective(policy::area_log_policy(A::Boot));
        for event in wasd.input.feed(b"\r\n", 1) {
            wasd.event(event, 100, 30);
        }
        assert_eq!(wasd.changes, 1);
        assert_eq!(policy::area_log_policy(A::Boot), P::only(before.toggle(L::Once)));
        assert!(wasd.input.feed(b"\x1b", 2).is_empty());
        assert_eq!(wasd.input.timeout(75_000_002), Some(Event::Quit));
        reset();
    }
    #[test]
    fn narrow_wrapped_grid_scroll_and_mouse_use_the_same_checkbox_coordinates() {
        reset();
        let mut view = View::default();
        view.selected = A::ALL.len() - 1;
        view.column = 9;
        let frame = view.frame(20, 7, &VecDeque::new());
        let (row, col) = frame.focus.unwrap();
        assert_eq!(
            frame.lines[row].chars().nth(col),
            Some(if view.checked(Some(A::Blueprint), 9) {
                '🗹'
            } else {
                '☐'
            })
        );
        assert!(view.scroll > 0);
        let before = effective(policy::area_log_policy(A::Blueprint));
        view.event(Event::Click(row, col), 20, 7);
        assert_eq!(policy::area_log_policy(A::Blueprint), P::only(before.toggle(L::Trace)));
        view.event(Event::Down, 20, 7);
        assert_eq!(view.selected, A::ALL.len());
        let frame = view.frame(20, 7, &VecDeque::new());
        assert!(frame.lines.iter().any(|l| l.starts_with("› Profiles")));
        reset();
    }
    #[test]
    fn browsing_presets_and_stream_does_not_start_collection() {
        reset();
        let mut view = View::default();
        view.selected = A::ALL.len() + 1;
        view.choose();
        assert_eq!(view.changes, 0);
        assert!(view.frame(100, 10, &VecDeque::new()).lines[3].contains("Collection starts after"));
        view.back();
        view.selected = A::ALL.len();
        view.choose();
        assert_eq!(view.changes, 0);
        view.selected = 1;
        view.choose();
        assert_eq!(view.changes, 1);
        assert_eq!(policy::profile_name(), "Normal");
        reset();
    }
    #[test]
    fn all_area_grid_preserves_unrelated_bits_and_none_is_exclusive() {
        reset();
        for area in A::ALL {
            policy::set_area_log_policy(area, P::only(S::ERROR));
        }
        policy::set_area_log_policy(A::Usb, P::only(S::WARN));
        let mut view = View::default();
        view.open(Page::AllAreas);
        assert!(!view.checked(None, 0));
        view.column = 2;
        view.choose();
        assert_eq!(policy::area_log_policy(A::Usb), P::only(S::WARN.union(S::USER)));
        assert_eq!(policy::area_log_policy(A::Boot), P::only(S::ERROR.union(S::USER)));
        view.column = 0;
        view.choose();
        for area in A::ALL {
            assert_eq!(policy::area_log_policy(area), P::only(S::NONE));
        }
        let changes = view.changes;
        view.choose();
        assert_eq!(view.changes, changes);
        view.selected = 1;
        view.choose();
        for area in A::ALL {
            assert_eq!(policy::area_log_policy(area), P::only(S::ALL));
        }
        reset();
    }
    #[test]
    fn focus_moves_without_leaving_old_underlines_and_header_background_stays_three_rows() {
        use trueos_terminal::{Terminal, TerminalColor};
        let mut screen = Screen::default();
        let mut terminal = Terminal::new(100, 12);
        let mut view = View::default();
        let frame = view.frame(100, 12, &VecDeque::new());
        terminal.feed(screen.diff(&frame, 100, 12).as_bytes());
        for row in 0..12 {
            for cell in &terminal.cells()[row * 100..(row + 1) * 100] {
                assert_eq!(
                    cell.style.background,
                    if row < 3 {
                        TerminalColor::Rgb {
                            red: 16,
                            green: 16,
                            blue: 16,
                        }
                    } else {
                        TerminalColor::Default
                    }
                );
            }
        }
        assert!(screen.diff(&frame, 100, 12).is_empty());
        let old = frame.focus.unwrap();
        view.event(Event::Right, 100, 12);
        let frame = view.frame(100, 12, &VecDeque::new());
        terminal.feed(screen.diff(&frame, 100, 12).as_bytes());
        assert!(!terminal.cells()[old.0 * 100 + old.1].style.underline);
        let focus = frame.focus.unwrap();
        assert!(terminal.cells()[focus.0 * 100 + focus.1].style.underline);
        let mut hostile = frame;
        hostile.lines[4] = "\x1b[2J malicious".into();
        assert!(!screen.diff(&hostile, 100, 12).contains("\x1b[2J"));
        assert!(
            screen
                .diff(&view.frame(20, 5, &VecDeque::new()), 20, 5)
                .contains("\x1b[2J")
        );
    }
    #[test]
    fn live_reader_bounds_history_and_reports_loss_without_splicing_records() {
        let mut stream = Stream::default();
        stream.feed(b"partial", 0);
        stream.feed(b"new\r\n", 10);
        assert_eq!(stream.lines[0], "log: 10 bytes overwritten before display");
        assert_eq!(stream.lines[1], "new");
        stream.feed(&alloc::vec![b'x'; 9000], 0);
        assert!(stream.pending.len() < 4096);
        for _ in 0..300 {
            stream.feed(b"record\n", 0);
        }
        assert_eq!(stream.lines.len(), 256);
    }
}
#[trueos_executor::task(pool_size = 2)]
async fn menu_task(target: MatrixTarget) {
    let mut view = View::default();
    let mut screen = Screen::default();
    let mut stream = Stream::default();
    let mut cursor = None;
    let mut buffer = [0u8; 4096];
    while let Some((bytes, notices)) = tui::native_read(&target) {
        if !tui::native_visible(&target) {
            Timer::after(Duration::from_millis(20)).await;
            continue;
        }
        let Some(surface) = tui::surface(&target) else {
            break;
        };
        let (cols, rows) = (surface.cols as usize, surface.rows as usize);
        if let Some(notice) = notices.last() {
            view.status = notice.clone();
        }
        if let Some(cursor) = cursor.as_mut() {
            if let Some((count, lost)) =
                crate::log_os::logtotcp::copy_for_screen(cursor, &mut buffer)
            {
                stream.feed(&buffer[..count], lost);
            }
        }
        let now = crate::chronos::monotonic_nanos();
        let mut actions = view.input.feed(&bytes, now);
        if let Some(action) = view.input.timeout(now) {
            actions.push(action);
        }
        for action in actions {
            let before = view.changes;
            if view.event(action, cols, rows) {
                tui::native_write(&target, b"\x1b[?1000l\x1b[?1006l\x1b[?25h\x1b[0m");
                tui::native_return(&target);
                screen.geometry = (0, 0);
                break;
            }
            if view.changes != before {
                let lease = super::matrix_target_slot_lease(&target);
                // Browsing never starts collection. The first explicit policy
                // or profile application arms both independent readers.
                match super::log_tap::start(lease) {
                    Ok(()) => {
                        cursor.get_or_insert_with(crate::log_os::logtotcp::live_cursor);
                        view.collecting = true;
                    }
                    Err(error) => view.status = format!("Policy applied; {error}"),
                }
            }
        }
        if tui::native_visible(&target) {
            let lines = view.frame(cols, rows, &stream.lines);
            let output = screen.diff(&lines, cols, rows);
            if !output.is_empty() {
                tui::native_write(&target, output.as_bytes());
            }
        }
        Timer::after(Duration::from_millis(20)).await;
    }
}
