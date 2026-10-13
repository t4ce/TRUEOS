//! Runtime log policy editor and independent live stream reader.
use super::{
    MatrixTarget,
    helper::{self, Action, Input},
    tui::{self, Frontend},
};
use crate::log_os::policy::{self, Profile};
use alloc::{collections::VecDeque, format, string::String, vec::Vec};
use log_os_core::{
    LogArea as A, LogLevel as L, LogLevelFilter as F, LogLevelPolicy as P, LogLevelSet as S,
};
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
    Edit(Option<A>),
    Threshold(Option<A>, bool),
    Only(Option<A>),
    Stream,
}
struct View {
    page: Page,
    selected: usize,
    input: Input,
    status: String,
    all_policy: P,
    scroll: usize,
    changes: u64,
    collecting: bool,
}
impl Default for View {
    fn default() -> Self {
        Self {
            page: Page::Areas,
            selected: 0,
            input: Input::default(),
            status: String::new(),
            all_policy: P::up(F::Info),
            scroll: 0,
            changes: 0,
            collecting: false,
        }
    }
}
fn describe(policy: P) -> (String, &'static str) {
    match policy {
        P::Up(level) => (level.tag().into(), "Up: toward Error"),
        P::Down(level) => (level.tag().into(), "Down: toward Trace"),
        P::Only(levels) if levels == S::ALL => ("all".into(), "Only: all levels"),
        P::Only(levels) if levels == S::NONE => ("none".into(), "Only: no levels"),
        P::Only(levels) => (
            L::ALL
                .iter()
                .filter(|&&l| levels.contains(l))
                .map(|l| l.tag())
                .collect::<Vec<_>>()
                .join(","),
            "Only: exact levels",
        ),
    }
}
impl View {
    fn open(&mut self, page: Page) {
        self.page = page;
        self.selected = 0;
        self.scroll = 0;
        self.status.clear();
    }
    fn current(&self, area: Option<A>) -> P {
        area.map_or(self.all_policy, policy::area_log_policy)
    }
    fn set(&mut self, area: Option<A>, value: P) {
        if let Some(area) = area {
            policy::set_area_log_policy(area, value);
        } else {
            for area in A::ALL {
                policy::set_area_log_policy(area, value);
            }
            self.all_policy = value;
        }
        let (level, mode) = describe(value);
        self.changes = self.changes.wrapping_add(1);
        self.status = format!("Applied: {}  {level}  {mode}", area.map_or("all areas", A::tag));
    }
    fn back(&mut self) -> bool {
        match self.page {
            Page::Areas => return true,
            Page::Threshold(area, _) | Page::Only(area) => self.open(Page::Edit(area)),
            _ => self.open(Page::Areas),
        }
        false
    }
    fn labels(&self) -> Vec<String> {
        match self.page {
            Page::Areas => {
                let mut labels = alloc::vec![
                    format!("Profiles: {}", policy::profile_name()),
                    "Live log stream".into(),
                    "Set all areas…".into()
                ];
                for area in A::ALL {
                    let current = policy::area_log_policy(area);
                    let (level, mode) = describe(current);
                    if let P::Only(set) = current {
                        if set != S::ALL && set != S::NONE {
                            labels.push(format!(
                                "{:<16}  {:<10}  Only: {}",
                                area.tag(),
                                format!("{} levels", set.bits().count_ones()),
                                level
                            ));
                            continue;
                        }
                    }
                    labels.push(format!("{:<16}  {:<10}  {}", area.tag(), level, mode));
                }
                labels
            }
            Page::Profiles => Profile::ALL.iter().map(|p| p.name().into()).collect(),
            Page::Edit(_) => [
                "Up — level and toward Error",
                "Down — level and toward Trace",
                "Only — toggle exact levels",
                "All levels",
                "None",
                "Return",
            ]
            .iter()
            .map(|s| (*s).into())
            .collect(),
            Page::Threshold(_, _) => F::ALL.iter().map(|l| l.tag().into()).collect(),
            Page::Only(area) => {
                let value = self.current(area);
                L::ALL
                    .iter()
                    .map(|&level| {
                        format!(
                            "[{}] {}",
                            if log_os_core::level_enabled(value, level) {
                                'x'
                            } else {
                                ' '
                            },
                            level.tag()
                        )
                    })
                    .chain([
                        String::from("All levels"),
                        String::from("None"),
                        String::from("Return"),
                    ])
                    .collect()
            }
            Page::Stream => Vec::new(),
        }
    }
    fn choose(&mut self, index: usize) {
        match self.page {
            Page::Areas => match index {
                0 => self.open(Page::Profiles),
                1 => self.open(Page::Stream),
                2 => self.open(Page::Edit(None)),
                _ => {
                    if let Some(&area) = A::ALL.get(index - 3) {
                        self.open(Page::Edit(Some(area)));
                    }
                }
            },
            Page::Profiles => {
                policy::apply_profile(index);
                self.changes = self.changes.wrapping_add(1);
                self.open(Page::Areas);
                self.status = format!("Profile applied: {}", policy::profile_name());
            }
            Page::Edit(area) => match index {
                0 | 1 => {
                    let current = self.current(area);
                    self.open(Page::Threshold(area, index == 0));
                    if let P::Up(level) | P::Down(level) = current {
                        self.selected = level as usize;
                    }
                }
                2 => self.open(Page::Only(area)),
                3 => self.set(area, P::only(S::ALL)),
                4 => self.set(area, P::only(S::NONE)),
                _ => {
                    self.back();
                }
            },
            Page::Threshold(area, up) => {
                if let Some(&level) = F::ALL.get(index) {
                    self.open(Page::Edit(area));
                    self.set(area, if up { P::up(level) } else { P::down(level) });
                }
            }
            Page::Only(area) => {
                let value = self.current(area);
                let set = match value {
                    P::Up(l) => log_os_core::threshold_up_set(l),
                    P::Down(l) => log_os_core::threshold_down_set(l),
                    P::Only(s) => s,
                };
                if let Some(&level) = L::ALL.get(index) {
                    self.set(area, P::only(set.toggle(level)));
                } else if index == L::ALL.len() {
                    self.set(area, P::only(S::ALL));
                } else if index == L::ALL.len() + 1 {
                    self.set(area, P::only(S::NONE));
                } else {
                    self.back();
                }
            }
            Page::Stream => {
                self.back();
            }
        }
    }
    fn offset(&self, rows: usize) -> usize {
        self.selected
            .saturating_sub(rows.saturating_sub(3).saturating_sub(1))
    }
    fn action(&mut self, action: Action, rows: usize) -> bool {
        if matches!(action, Action::Quit | Action::Click(1)) {
            return self.back();
        }
        if self.page == Page::Stream {
            match action {
                Action::Up => self.scroll = self.scroll.saturating_add(1),
                Action::Down => self.scroll = self.scroll.saturating_sub(1),
                Action::Choose => {
                    self.back();
                }
                _ => {}
            }
            return false;
        }
        let count = self.labels().len();
        match action {
            Action::Up if count > 0 => self.selected = (self.selected + count - 1) % count,
            Action::Down if count > 0 => self.selected = (self.selected + 1) % count,
            Action::Choose if count > 0 => self.choose(self.selected),
            Action::Click(row) if row >= 3 && row < rows => {
                let index = self.offset(rows) + row - 3;
                if index < count {
                    self.selected = index;
                    self.choose(index);
                }
            }
            _ => {}
        }
        false
    }
    fn frame(&mut self, rows: usize, stream: &VecDeque<String>) -> Vec<String> {
        let mut lines = alloc::vec![String::new(); rows];
        if rows < 5 {
            return lines;
        }
        let area = match self.page {
            Page::Edit(a) | Page::Only(a) | Page::Threshold(a, _) => Some(a),
            _ => None,
        };
        lines[0] = if let Some(area) = area {
            let (level, mode) = describe(self.current(area));
            format!("LOG  {}   {level}   {mode}", area.map_or("all areas", A::tag))
        } else {
            format!("LOG  area policies   Profile: {}", policy::profile_name())
        };
        lines[1] = "↑/↓ j/k select   Enter choose   Esc/q back   Mouse choose / wheel".into();
        lines[2] = if !self.status.is_empty() {
            self.status.clone()
        } else if self.page == Page::Profiles {
            "Live filters only; optional probes depend on the build. EarlyBoot affects future records.".into()
        } else if self.page == Page::Areas {
            "  AREA              LEVEL       POLICY   Changes apply immediately.".into()
        } else if self.page == Page::Stream {
            if self.collecting {
                "Live accepted records; ↑ scroll history, ↓ return toward live; Enter back.".into()
            } else {
                "Collection starts after applying a policy or profile. Enter back.".into()
            }
        } else {
            "Up includes more severe levels; Down includes more verbose levels; Only is exact."
                .into()
        };
        if self.page == Page::Stream {
            let visible = rows - 3;
            self.scroll = self.scroll.min(stream.len().saturating_sub(visible));
            let start = stream.len().saturating_sub(visible + self.scroll);
            for (index, line) in stream.iter().skip(start).take(visible).enumerate() {
                lines[3 + index] = line.clone();
            }
        } else {
            for (index, label) in self
                .labels()
                .iter()
                .skip(self.offset(rows))
                .take(rows - 3)
                .enumerate()
            {
                lines[3 + index] = format!(
                    "{} {}",
                    if self.offset(rows) + index == self.selected {
                        '›'
                    } else {
                        ' '
                    },
                    label
                );
            }
        }
        lines
    }
}

// Changed rows only; the first three rows use Shell3's control background.
#[derive(Default)]
struct Screen {
    previous: Vec<String>,
    geometry: (usize, usize),
}
impl Screen {
    fn diff(&mut self, lines: &[String], cols: usize, rows: usize) -> String {
        let resized = self.geometry != (cols, rows);
        let mut bytes = String::new();
        if resized {
            bytes.push_str("\x1b[?25l\x1b[?1000h\x1b[?1006h\x1b[0m\x1b[2J");
            self.previous.clear();
            self.geometry = (cols, rows);
        }
        for (row, line) in lines.iter().take(rows).enumerate() {
            let mut text: String = line
                .chars()
                .take(cols)
                .map(|ch| if ch.is_control() { ' ' } else { ch })
                .collect();
            text.extend(core::iter::repeat_n(' ', cols.saturating_sub(text.chars().count())));
            if self.previous.get(row) != Some(&text) {
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
            }
            if row == self.previous.len() {
                self.previous.push(text);
            } else {
                self.previous[row] = text;
            }
        }
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

    #[test]
    fn browsing_does_not_arm_collection_but_applying_a_setting_does() {
        let mut view = View::default();
        view.choose(1);
        assert_eq!(view.changes, 0);
        assert!(view.frame(10, &VecDeque::new())[2].contains("Collection starts after"));
        view.back();
        view.choose(7);
        view.choose(0);
        assert_eq!(view.changes, 0);
        view.choose(F::Trace as usize);
        assert_eq!(view.changes, 1);
        view.back();
        view.choose(0);
        assert_eq!(view.changes, 1);
        view.choose(0);
        assert_eq!(view.changes, 2);
        policy::apply_profile(0);
    }

    #[test]
    fn mouse_and_keyboard_edit_the_same_area_and_policy() {
        policy::apply_profile(0);
        let mut view = View::default();
        // USB is entry 7: three controls, then Global/Boot/Service/Net/USB.
        for action in view.input.feed(b"\x1b[<0;5;11M", 0) {
            view.action(action, 30);
        }
        assert!(view.page == Page::Edit(Some(A::Usb)));
        for action in view.input.feed(b"j\r", 1) {
            view.action(action, 30);
        }
        assert!(view.page == Page::Threshold(Some(A::Usb), false));
        view.choose(F::Debug as usize);
        assert_eq!(policy::area_log_policy(A::Usb), P::down(F::Debug));
        assert!(!log_os_core::level_enabled(policy::area_log_policy(A::Usb), L::Error));
        assert!(log_os_core::level_enabled(policy::area_log_policy(A::Usb), L::Trace));
        view.choose(2);
        view.choose(L::User as usize);
        assert!(log_os_core::level_enabled(policy::area_log_policy(A::Usb), L::User));
        view.choose(L::Ai as usize);
        assert!(log_os_core::level_enabled(policy::area_log_policy(A::Usb), L::Ai));
        policy::apply_profile(0);
    }
    #[test]
    fn scrolled_click_selects_the_displayed_area() {
        let mut view = View::default();
        view.selected = 18;
        let lines = view.frame(5, &VecDeque::new());
        assert!(lines[4].contains("blueprint"));
        view.action(Action::Click(4), 5);
        assert!(view.page == Page::Edit(Some(A::Blueprint)));
        assert!(!view.back());
        assert!(view.back());
    }
    #[test]
    fn all_none_and_exact_sets_apply_to_every_area() {
        let mut view = View::default();
        view.open(Page::Edit(None));
        view.choose(4);
        for area in A::ALL {
            assert_eq!(policy::area_log_policy(area), P::only(S::NONE));
        }
        view.choose(3);
        for area in A::ALL {
            assert_eq!(policy::area_log_policy(area), P::only(S::ALL));
        }
        view.choose(2);
        view.choose(L::Ai as usize);
        for area in A::ALL {
            let p = policy::area_log_policy(area);
            assert!(!log_os_core::level_enabled(p, L::Ai));
            assert!(log_os_core::level_enabled(p, L::User));
        }
        policy::apply_profile(0);
    }
    #[test]
    fn three_control_rows_and_default_body_survive_changed_rows_and_resize() {
        use trueos_terminal::{Terminal, TerminalColor};
        let mut screen = Screen::default();
        let mut terminal = Terminal::new(30, 8);
        let mut view = View::default();
        let lines = view.frame(8, &VecDeque::new());
        terminal.feed(screen.diff(&lines, 30, 8).as_bytes());
        for row in 0..8 {
            for cell in &terminal.cells()[row * 30..(row + 1) * 30] {
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
        assert!(screen.diff(&lines, 30, 8).is_empty());
        let mut hostile = lines.clone();
        hostile[4] = "\x1b[2J malicious".into();
        let output = screen.diff(&hostile, 30, 8);
        assert!(!output.contains("\x1b[2J"));
        assert!(
            screen
                .diff(&view.frame(5, &VecDeque::new()), 20, 5)
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
            if view.action(action, rows) {
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
            let lines = view.frame(rows, &stream.lines);
            let output = screen.diff(&lines, cols, rows);
            if !output.is_empty() {
                tui::native_write(&target, output.as_bytes());
            }
        }
        Timer::after(Duration::from_millis(20)).await;
    }
}
