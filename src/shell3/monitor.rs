//! Live RAM/SMP views over the existing snapshot tables and natural samplers.
use super::{
    helper::{self, Action, Input, Screen},
    tui::{self, Frontend},
};
use crate::shell2::{
    MatrixTarget,
    cmds::{ram, smp},
};
use alloc::{format, string::String, vec::Vec};
use trueos_time::{Duration, Timer};

const REFRESH_NS: u64 = 250_000_000;
const FOOTER: &str =
    "↑/↓ or j/k scroll   Mouse wheel scroll   Enter Return   ←/h back   Esc/q quit";
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ram,
    Smp,
}
impl Kind {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "ram" => Some(Self::Ram),
            "smp" => Some(Self::Smp),
            _ => None,
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Ram => "RAM  memory use",
            Self::Smp => "SMP  processor activity",
        }
    }
}
pub(super) fn recognizes(name: &str) -> bool {
    Kind::parse(name).is_some()
}
pub(super) fn start(name: &str, frontend: Frontend) -> Result<(), String> {
    let kind = Kind::parse(name).ok_or("Unknown monitor.")?;
    let Some((target, spawner)) = helper::admit(name, frontend)? else {
        return Ok(());
    };
    let token = match monitor_task(kind, target.clone()) {
        Ok(token) => token,
        Err(_) => {
            tui::cancel_native_attach(&target);
            return Err("Monitor task pool is full.".into());
        }
    };
    spawner.spawn(token);
    Ok(())
}
struct View {
    kind: Kind,
    content: Vec<String>,
    modules: Vec<String>,
    width: usize,
    scroll: usize,
    next_sample: u64,
    input: Input,
    screen: Screen,
}
impl View {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            content: Vec::new(),
            modules: Vec::new(),
            width: 0,
            scroll: 0,
            next_sample: 0,
            input: Input::default(),
            screen: Screen::default(),
        }
    }
    fn refresh(&mut self, cols: usize, now: u64) -> bool {
        if cols == self.width && now < self.next_sample {
            return false;
        }
        if self.kind == Kind::Ram && cols != self.width {
            self.modules = ram::memory_modules_snapshot(cols);
        }
        self.width = cols;
        self.content = match self.kind {
            Kind::Ram => {
                let mut lines = self.modules.clone();
                lines.push(String::new());
                lines.extend(ram::usage_snapshot(cols));
                lines
            }
            Kind::Smp => smp::snapshot(cols),
        };
        self.next_sample = now.saturating_add(REFRESH_NS);
        true
    }
    fn max_scroll(&self, rows: usize) -> usize {
        self.content.len().saturating_sub(rows.saturating_sub(3))
    }
    fn action(&mut self, action: Action, target: &MatrixTarget, rows: usize) -> bool {
        match action {
            Action::Up => self.scroll = self.scroll.saturating_sub(1),
            Action::Down => self.scroll = self.scroll.saturating_add(1).min(self.max_scroll(rows)),
            Action::Quit | Action::Choose => {
                tui::native_return(target);
                return true;
            }
            Action::Click(row) if row == rows.saturating_sub(1) => {
                tui::native_return(target);
                return true;
            }
            Action::Click(_) => {}
        }
        false
    }
    fn paint(&mut self, target: &MatrixTarget, cols: usize, rows: usize) {
        if rows == 0 {
            return;
        }
        self.scroll = self.scroll.min(self.max_scroll(rows));
        let visible = rows.saturating_sub(3);
        let mut lines = alloc::vec![String::new(); rows];
        lines[0] = self.kind.title().into();
        let status = if self.max_scroll(rows) == 0 {
            "Live · 250 ms refresh".into()
        } else {
            format!(
                "Live · 250 ms refresh · {}–{} / {}",
                self.scroll + 1,
                (self.scroll + visible).min(self.content.len()),
                self.content.len()
            )
        };
        if rows > 1 {
            lines[1] = status;
        }
        for (row, content) in self
            .content
            .iter()
            .skip(self.scroll)
            .take(visible)
            .enumerate()
        {
            lines[row + 2] = content.clone();
        }
        if rows > 2 {
            lines[rows - 1] = if cols < 76 {
                "↑/↓ j/k scroll  Enter Return  Esc/q quit".into()
            } else {
                FOOTER.into()
            };
        }
        self.screen.paint(target, &lines, cols, rows);
    }
}
#[trueos_executor::task(pool_size = 2)]
async fn monitor_task(kind: Kind, target: MatrixTarget) {
    let mut view = View::new(kind);
    let mut visible = true;
    let mut geometry = (0, 0);
    while let Some((bytes, _)) = tui::native_read(&target) {
        if !tui::native_visible(&target) {
            // Parking keeps the slot online, without repeatedly collecting data.
            visible = false;
            view.input = Input::default();
            Timer::after(Duration::from_millis(20)).await;
            continue;
        }
        if !visible {
            view.next_sample = 0;
            visible = true;
        }
        let Some(surface) = tui::surface(&target) else {
            break;
        };
        let cols = surface.cols as usize;
        let rows = surface.rows as usize;
        let now = crate::chronos::monotonic_nanos();
        let refresh = view.refresh(cols, now);
        let previous_scroll = view.scroll;
        let mut actions = view.input.feed(&bytes, now);
        if let Some(action) = view.input.timeout(now) {
            actions.push(action);
        }
        for action in actions {
            if view.action(action, &target, rows) {
                break;
            }
        }
        if refresh || previous_scroll != view.scroll || geometry != (cols, rows) {
            view.paint(&target, cols, rows);
            geometry = (cols, rows);
        }
        Timer::after(Duration::from_millis(20)).await;
    }
}
