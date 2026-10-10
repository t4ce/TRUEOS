//! Online launch/download and live VM status/stop over the existing backends.
use super::{
    helper::{self, Action, Input, Screen},
    tui::{self, Frontend},
};
use crate::shell2::{
    self, MatrixTarget,
    shell2_dl::{self, OnlineApp},
};
use alloc::{format, string::String, sync::Arc, vec::Vec};
use spin::Mutex;
use trueos_time::{Duration, Timer};

const REFRESH_NS: u64 = 250_000_000;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Online,
    Status,
}
impl Kind {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "online" => Some(Self::Online),
            "status" => Some(Self::Status),
            _ => None,
        }
    }
}
pub(super) fn recognizes(name: &str) -> bool {
    Kind::parse(name).is_some()
}
pub(super) fn start(name: &str, frontend: Frontend) -> Result<(), String> {
    let kind = Kind::parse(name).ok_or("Unknown app view.")?;
    let Some((target, spawner)) = helper::admit(name, frontend)? else {
        return Ok(());
    };
    match apps_task(kind, target.clone()) {
        Ok(token) => {
            spawner.spawn(token);
            Ok(())
        }
        Err(_) => {
            tui::cancel_native_attach(&target);
            Err("App view task pool is full.".into())
        }
    }
}
struct Vm {
    id: u8,
    run: u64,
    label: String,
    state: &'static str,
    stored: bool,
    stoppable: bool,
}
enum Request {
    Catalog,
    Fetch(OnlineApp, bool),
    Stop(Vec<(u8, u64)>),
}
enum Outcome {
    Catalog(Vec<OnlineApp>),
    Launch(OnlineApp, Vec<u8>),
    Message(String),
}
struct Job {
    progress: String,
    result: Option<Result<Outcome, String>>,
}
type Reply = Arc<Mutex<Job>>;
impl Request {
    async fn run(self, target: &MatrixTarget, reply: &Reply) -> Result<Outcome, String> {
        match self {
            Self::Catalog => shell2_dl::catalog().await.map(Outcome::Catalog),
            Self::Fetch(app, launch) => {
                let bytes = shell2_dl::fetch_app(&app).await?;
                if !shell2::matrix_slot_is_live(&shell2::matrix_target_slot_lease(target)) {
                    return Err("Slot closed.".into());
                }
                if launch {
                    Ok(Outcome::Launch(app, bytes))
                } else {
                    crate::app_db::insert_download(&app.archive_name, &bytes)?;
                    super::service::refresh_appdb_names();
                    Ok(Outcome::Message(format!(
                        "Saved app.db:{} · {} bytes",
                        app.archive_name,
                        bytes.len()
                    )))
                }
            }
            Self::Stop(vms) => {
                let mut waiting = Vec::new();
                for (id, run) in vms {
                    if crate::hv::stop_for_generation(id, run)
                        .map_err(|err| format!("Stop failed: {err:?}"))?
                    {
                        waiting.push((id, run));
                    }
                }
                if waiting.is_empty() {
                    return Ok(Outcome::Message("Selected VM is no longer running.".into()));
                }
                let count = waiting.len();
                let deadline = crate::chronos::monotonic_nanos().saturating_add(45_000_000_000);
                loop {
                    waiting.retain(|&(id, run)| {
                        let state = crate::hv::vm_state(id);
                        crate::hv::vm_lifecycle_generation(id) == Some(run)
                            && (state.running || state.starting)
                    });
                    if waiting.is_empty() {
                        return Ok(Outcome::Message(format!("Stopped {count} VM(s).")));
                    }
                    if crate::chronos::monotonic_nanos() >= deadline {
                        return Err("Stop requested; still waiting for VM cleanup.".into());
                    }
                    if !shell2::matrix_slot_is_live(&shell2::matrix_target_slot_lease(target)) {
                        return Err("Slot closed.".into());
                    }
                    reply.lock().progress = format!("Stopping {} VM(s)…", waiting.len());
                    Timer::after(Duration::from_millis(250)).await;
                }
            }
        }
    }
}
struct View {
    kind: Kind,
    apps: Vec<OnlineApp>,
    vms: Vec<Vm>,
    summary: Vec<String>,
    selected: usize,
    app: usize,
    vm: usize,
    scroll: usize,
    next_sample: u64,
    result: String,
    pending: Option<Reply>,
    ready: Option<(OnlineApp, Vec<u8>)>,
    input: Input,
    screen: Screen,
}
impl View {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            apps: Vec::new(),
            vms: Vec::new(),
            summary: Vec::new(),
            selected: if kind == Kind::Online { 4 } else { 3 },
            app: 0,
            vm: 0,
            scroll: 0,
            next_sample: 0,
            result: String::new(),
            pending: None,
            ready: None,
            input: Input::default(),
            screen: Screen::default(),
        }
    }
    fn toolbar_len(&self) -> usize {
        if self.kind == Kind::Online { 4 } else { 3 }
    }
    fn list_start(&self) -> usize {
        if self.kind == Kind::Online { 4 } else { 7 }
    }
    fn toolbar(&self) -> &'static [&'static str] {
        if self.kind == Kind::Online {
            &["Launch", "dl", "Refresh", "Return"]
        } else {
            &["Stop", "Stop all", "Return"]
        }
    }
    fn count(&self) -> usize {
        self.toolbar_len()
            + if self.kind == Kind::Online {
                self.apps.len()
            } else {
                self.vms.len()
            }
    }
    fn submit(&mut self, target: &MatrixTarget, request: Request, progress: String) {
        if self.pending.is_some() || self.ready.is_some() {
            return;
        }
        let reply = Arc::new(Mutex::new(Job {
            progress,
            result: None,
        }));
        self.pending = Some(reply.clone());
        let work = helper::Work::new(target);
        let target = target.clone();
        crate::wait::spawn_local_detached(async move {
            let _work = work;
            Timer::after(Duration::from_millis(1)).await;
            let result = if shell2::matrix_slot_is_live(&shell2::matrix_target_slot_lease(&target))
            {
                request.run(&target, &reply).await
            } else {
                Err("Slot closed.".into())
            };
            reply.lock().result = Some(result);
            super::service::notify_work();
        });
    }
    fn complete(&mut self) {
        let Some(reply) = &self.pending else {
            return;
        };
        let Some(result) = reply.lock().result.take() else {
            return;
        };
        self.pending = None;
        match result {
            Ok(Outcome::Catalog(apps)) => {
                self.apps = apps;
                self.app = self.app.min(self.apps.len().saturating_sub(1));
                self.selected = if self.apps.is_empty() {
                    2
                } else {
                    4 + self.app
                };
                self.scroll = 0;
                self.result = if self.apps.is_empty() {
                    "Online catalog is empty.".into()
                } else {
                    format!("{} apps · Click/Enter launch; [dl] saves to app.db", self.apps.len())
                };
            }
            Ok(Outcome::Launch(app, bytes)) => {
                self.result = format!("Ready to launch {} on reentry.", app.name);
                self.ready = Some((app, bytes));
            }
            Ok(Outcome::Message(message)) | Err(message) => self.result = one_line(&message),
        }
    }
    fn refresh(&mut self, now: u64) {
        if self.kind != Kind::Status || now < self.next_sample {
            return;
        }
        self.next_sample = now.saturating_add(REFRESH_NS);
        let status = crate::hv::status();
        self.summary = alloc::vec![
            format!(
                "Running {} · Starting {} · Limit {} · Active {}",
                status.running_count,
                status.starting_count,
                status.vm_id_limit,
                status
                    .active_vm_ids
                    .iter()
                    .flatten()
                    .map(|id| format!("{id}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            format!(
                "Shared heap used {} MiB / {} MiB · Free {} MiB",
                status
                    .vm_shared_heap_total_bytes
                    .saturating_sub(status.vm_shared_heap_free_bytes)
                    / 1048576,
                status.vm_shared_heap_total_bytes / 1048576,
                status.vm_shared_heap_free_bytes / 1048576
            ),
            format!(
                "Stack {} MiB · VMX state {} MiB · Stored snapshots {}",
                status.vm_shared_stack_bytes / 1048576,
                status.vm_shared_vmx_bytes / 1048576,
                status.stored_vm_count
            ),
            format!(
                "Intel {} · VMX {} · Feature control locked {} · Outside SMX {} · Guest module {}",
                status.vendor_intel,
                status.has_vmx,
                status.feature_control_locked,
                status.feature_control_vmx_outside_smx,
                status.guest_module_present
            ),
        ];
        self.vms = (0..status.vm_id_limit)
            .filter_map(|idx| {
                let id = idx as u8;
                let run = crate::hv::vm_lifecycle_generation(id).unwrap_or(0);
                let state = crate::hv::vm_state(id);
                let same_run = crate::hv::vm_lifecycle_generation(id) == Some(run);
                state.supported.then(|| Vm {
                    id,
                    run,
                    label: crate::hv::app_vm_display_label(id).unwrap_or_else(|| "-".into()),
                    state: shell2::shell2_apps::vm_state_label(state),
                    stored: crate::hv::store::has_committed_vm(id),
                    stoppable: same_run
                        && (state.running || state.starting)
                        && !state.stop_requested,
                })
            })
            .collect();
        let visible = self
            .vms
            .iter()
            .rposition(|vm| vm.label != "-" || vm.stored || vm.state != "offline")
            .map_or(4, |index| index.saturating_add(5));
        self.vms.truncate(visible);
        self.vm = self.vm.min(self.vms.len().saturating_sub(1));
    }
    fn fetch_selected(&mut self, target: &MatrixTarget, launch: bool) {
        if let Some(app) = self.apps.get(self.app).cloned() {
            let progress =
                format!("{} {}…", if launch { "Fetching" } else { "Downloading" }, app.name);
            self.submit(target, Request::Fetch(app, launch), progress);
        } else {
            self.result = "Choose an app first.".into();
        }
    }
    fn stop_selected(&mut self, target: &MatrixTarget, all: bool) {
        let vms = self
            .vms
            .iter()
            .enumerate()
            .filter(|(index, vm)| vm.stoppable && (all || *index == self.vm))
            .map(|(_, vm)| (vm.id, vm.run))
            .collect::<Vec<_>>();
        if vms.is_empty() {
            self.result = "No running VM selected.".into();
        } else {
            self.submit(target, Request::Stop(vms), "Requesting stop…".into());
        }
    }
    fn choose(&mut self, target: &MatrixTarget) {
        if self.kind == Kind::Online {
            match self.selected {
                0 => self.fetch_selected(target, true),
                1 => self.fetch_selected(target, false),
                2 => self.submit(target, Request::Catalog, "Refreshing catalog…".into()),
                3 => tui::native_return(target),
                _ => self.fetch_selected(target, true),
            }
        } else {
            match self.selected {
                0 => self.stop_selected(target, false),
                1 => self.stop_selected(target, true),
                2 => tui::native_return(target),
                _ => self.stop_selected(target, false),
            }
        }
    }
    fn action(&mut self, action: Action, col: Option<usize>, target: &MatrixTarget, rows: usize) {
        match action {
            Action::Quit => tui::native_return(target),
            Action::Up => self.selected = self.selected.saturating_sub(1),
            Action::Down => self.selected = (self.selected + 1).min(self.count().saturating_sub(1)),
            Action::Choose => self.choose(target),
            Action::Click(row) => {
                if row == rows.saturating_sub(1) {
                    tui::native_return(target);
                    return;
                }
                let toolbar = self.list_start() - 2;
                if row == toolbar {
                    let col = col.unwrap_or(0);
                    let mut offset = 0;
                    let hit = self.toolbar().iter().position(|label| {
                        let start = offset;
                        offset += label.chars().count() + 4;
                        col >= start && col < offset - 1
                    });
                    if let Some(index) = hit {
                        self.selected = index;
                        self.choose(target);
                    }
                } else if row >= self.list_start() && row < rows.saturating_sub(2) {
                    let index = self.scroll + row - self.list_start();
                    if index + self.toolbar_len() >= self.count() {
                        return;
                    }
                    self.selected = self.toolbar_len() + index;
                    if self.kind == Kind::Status {
                        self.vm = index;
                    }
                    if self.kind == Kind::Online {
                        self.app = index;
                        self.fetch_selected(target, !col.is_some_and(|col| (3..7).contains(&col)));
                    } else if col.is_some_and(|col| (3..9).contains(&col)) {
                        self.stop_selected(target, false);
                    }
                }
            }
        }
        if self.kind == Kind::Online && self.selected >= 4 {
            self.app = self.selected - 4;
        }
        if self.kind == Kind::Status && self.selected >= 3 {
            self.vm = self.selected - 3;
        }
    }
    fn paint(&mut self, target: &MatrixTarget, cols: usize, rows: usize, now: u64) {
        if rows == 0 {
            return;
        }
        self.selected = self.selected.min(self.count().saturating_sub(1));
        let start = self.list_start();
        let visible = rows.saturating_sub(start + 2);
        let length = self.count() - self.toolbar_len();
        if let Some(index) = self.selected.checked_sub(self.toolbar_len()) {
            if index < self.scroll {
                self.scroll = index;
            }
            if visible > 0 && index >= self.scroll + visible {
                self.scroll = index + 1 - visible;
            }
        }
        self.scroll = self.scroll.min(length.saturating_sub(visible));
        let mut lines = alloc::vec![String::new();rows];
        lines[0] = if self.kind == Kind::Online {
            "ONLINE  apps · launch & download"
        } else {
            "STATUS  virtual machines · stop"
        }
        .into();
        if self.kind == Kind::Status {
            for (row, line) in self.summary.iter().enumerate() {
                if row + 1 < rows.saturating_sub(2) {
                    lines[row + 1] = line.clone();
                }
            }
        }
        if start >= 2 && start - 2 < rows.saturating_sub(2) {
            let labels = self.toolbar();
            lines[start - 2] = labels
                .iter()
                .enumerate()
                .map(|(i, label)| {
                    format!("{}[{}]", if self.selected == i { "›" } else { " " }, label)
                })
                .collect::<Vec<_>>()
                .join(" ");
        }
        if start > 0 && start - 1 < rows.saturating_sub(2) {
            lines[start - 1] = if self.kind == Kind::Online {
                "   action  id   app / sha"
            } else {
                "   action  vmid  blueprint / state / store"
            }
            .into();
        }
        for (row, index) in (self.scroll..length).take(visible).enumerate() {
            let marker = if self.selected == self.toolbar_len() + index {
                "›"
            } else {
                " "
            };
            lines[start + row] = if self.kind == Kind::Online {
                let app = &self.apps[index];
                let sha = if app.sha256.len() == 64 {
                    format!("{}…{}", &app.sha256[..4], &app.sha256[60..])
                } else {
                    app.sha256.clone()
                };
                let name = fit(&app.name, cols.saturating_sub(23));
                format!(" {marker} [dl] {index:>3} {name} {sha}")
            } else {
                let vm = &self.vms[index];
                let name = fit(&vm.label, cols.saturating_sub(40));
                format!(
                    " {marker} {} {:>4} {name} {:<16} {}",
                    if vm.stoppable { "[Stop]" } else { "      " },
                    vm.id,
                    vm.state,
                    if vm.stored { "saved" } else { "-" }
                )
            };
        }
        if rows > 2 {
            let message = if let Some(reply) = &self.pending {
                format!("{} {}", helper::spinner(now), reply.lock().progress)
            } else {
                self.result.clone()
            };
            lines[rows - 2] = one_line(&message);
            lines[rows - 1] =
                "↑/↓ j/k select  Enter choose  Mouse choose  ←/h back  Esc/q quit".into();
        }
        self.screen.paint(target, &lines, cols, rows);
    }
}
fn one_line(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .take(200)
        .collect()
}
fn fit(text: &str, width: usize) -> String {
    let mut out: String = text
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .take(width)
        .collect();
    let len = out.chars().count();
    for _ in len..width {
        out.push(' ');
    }
    out
}
#[trueos_executor::task(pool_size = 2)]
async fn apps_task(kind: Kind, target: MatrixTarget) {
    let mut view = View::new(kind);
    let mut visible = true;
    if kind == Kind::Online {
        view.submit(&target, Request::Catalog, "Fetching online catalog…".into());
    }
    while let Some((bytes, _)) = tui::native_read(&target) {
        view.complete();
        if !tui::native_visible(&target) {
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
        let rows = surface.rows as usize;
        let cols = surface.cols as usize;
        let now = crate::chronos::monotonic_nanos();
        // Act on the displayed incarnation before replacing the live snapshot.
        let mut actions = view.input.feed_cells(&bytes, now);
        if let Some(action) = view.input.timeout(now) {
            actions.push((action, None));
        }
        for (action, col) in actions {
            view.action(action, col, &target, rows);
            if !tui::native_visible(&target) {
                break;
            }
        }
        if !tui::native_visible(&target) {
            continue;
        }
        if let Some((app, bytes)) = view.ready.take() {
            if let Some(frontend) = tui::native_frontend(&target) {
                match super::service::launch_bytes(app.archive_name, bytes, "", frontend) {
                    Ok(receipt) => {
                        view.result = format!("Launched {} · §{}", app.name, receipt.slot);
                        tui::native_launch(&target, receipt);
                        continue;
                    }
                    Err(error) => view.result = one_line(&error),
                }
            }
        }
        view.refresh(now);
        view.paint(&target, cols, rows, now);
        Timer::after(Duration::from_millis(20)).await;
    }
}
