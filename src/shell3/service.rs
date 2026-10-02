//! AP-backed worker startup for Shell3 draw execution.

use alloc::vec::Vec;
use trueos_executor::{SpawnError, SpawnToken, Spawner};
use trueos_time::{Duration, Timer};

const TOPOLOGY_TASK_POOL_CAPACITY: usize = crate::percpu::CPU_SLOT_LIMIT;
static DRAW_WORK_AVAILABLE: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
static SHELL3_ESCAPE_EVENTS: spin::Mutex<heapless::Deque<crate::ui4::Ui4KeyboardEvent, 64>> =
    spin::Mutex::new(heapless::Deque::new());
static SHELL3_RESIZE_EVENTS: spin::Mutex<heapless::Deque<crate::ui4::Ui4ResizeEvent, 256>> =
    spin::Mutex::new(heapless::Deque::new());

struct ShellOwnership {
    worker_slots: Vec<u32>,
    shells_by_slot: [usize; crate::percpu::CPU_SLOT_LIMIT],
    pending_by_slot: [usize; crate::percpu::CPU_SLOT_LIMIT],
    live_shells: usize,
    next_round_robin: usize,
}

struct AppDbNames {
    generation: u64,
    names: Vec<alloc::string::String>,
}

impl AppDbNames {
    const fn new() -> Self {
        Self {
            generation: 0,
            names: Vec::new(),
        }
    }
}

impl ShellOwnership {
    const fn new() -> Self {
        Self {
            worker_slots: Vec::new(),
            shells_by_slot: [0; crate::percpu::CPU_SLOT_LIMIT],
            pending_by_slot: [0; crate::percpu::CPU_SLOT_LIMIT],
            live_shells: 0,
            next_round_robin: 0,
        }
    }

    fn preferred_slot(&self) -> Option<u32> {
        if self.worker_slots.is_empty() {
            return None;
        }

        // Fill the first selected executor to three shells, then each next
        // executor, preserving the specified startup distribution.
        if let Some(slot) = self
            .worker_slots
            .iter()
            .copied()
            .find(|slot| {
                self.shells_by_slot[*slot as usize]
                    + self.pending_by_slot[*slot as usize]
                    < 3
            })
        {
            return Some(slot);
        }

        // Once every executor has three, give the next shell to the least
        // loaded executor, scanning ties in round-robin order.
        let minimum = self
            .worker_slots
            .iter()
            .map(|slot| self.shells_by_slot[*slot as usize] + self.pending_by_slot[*slot as usize])
            .min()?;
        for offset in 0..self.worker_slots.len() {
            let index = (self.next_round_robin + offset) % self.worker_slots.len();
            let slot = self.worker_slots[index];
            if self.shells_by_slot[slot as usize] + self.pending_by_slot[slot as usize] == minimum {
                return Some(slot);
            }
        }
        None
    }
}

static SHELL_OWNERSHIP: spin::Mutex<ShellOwnership> = spin::Mutex::new(ShellOwnership::new());
static APPDB_NAMES: spin::Mutex<AppDbNames> = spin::Mutex::new(AppDbNames::new());

/// Read app names through the current app.db API, using the archive basename
/// convention shared by the Shell2 titlebar.
pub fn read_appdb_names() -> Vec<alloc::string::String> {
    crate::app_db::list()
        .unwrap_or_default()
        .into_iter()
        .map(|entry| {
            entry
                .archive
                .strip_suffix(".bp")
                .unwrap_or(entry.archive.as_str())
                .into()
        })
        .collect()
}

/// Refresh the shared app-name list. Active AP executors apply a changed
/// snapshot to every Shell3 they own on their next service pass.
pub fn refresh_appdb_names() {
    let names = read_appdb_names();
    let mut current = APPDB_NAMES.lock();
    if current.names != names {
        current.names = names;
        current.generation = current.generation.wrapping_add(1);
        drop(current);
        DRAW_WORK_AVAILABLE.notify_all();
    }
}

fn appdb_names_snapshot() -> (u64, Vec<alloc::string::String>) {
    let current = APPDB_NAMES.lock();
    (current.generation, current.names.clone())
}

/// Runtime worker limit: at most half of all discovered AP cores.
pub fn worker_limit() -> usize {
    let ap_cores = crate::workers::topology_core_slot_count().saturating_sub(1);
    ap_cores / 2
}

/// Choose the next AP according to the live per-executor shell counts.
/// Executors receive their first three shells in order; later shells go to
/// the least-loaded executor, with ties resolved round-robin.
pub fn next_executor_for_shell() -> Option<u32> {
    let ownership = SHELL_OWNERSHIP.lock();
    (ownership.live_shells + ownership.pending_by_slot.iter().sum::<usize>()
        < super::MAX_SHELL3_INSTANCES)
        .then(|| ownership.preferred_slot())
        .flatten()
}

/// Queue a fresh Shell3 on the executor selected by the shell distribution policy.
pub fn request_shell3() -> Result<u32, super::Shell3Error> {
    refresh_appdb_names();
    let mut ownership = SHELL_OWNERSHIP.lock();
    if ownership.live_shells + ownership.pending_by_slot.iter().sum::<usize>()
        >= super::MAX_SHELL3_INSTANCES
    {
        return Err(super::Shell3Error::InstanceLimit);
    }
    let Some(slot) = ownership.preferred_slot() else {
        return Err(super::Shell3Error::NoExecutor);
    };
    ownership.pending_by_slot[slot as usize] += 1;
    drop(ownership);
    DRAW_WORK_AVAILABLE.notify_all();
    Ok(slot)
}

fn take_pending_for_executor(slot: u32) -> bool {
    let mut ownership = SHELL_OWNERSHIP.lock();
    let Some(pending) = ownership.pending_by_slot.get_mut(slot as usize) else {
        return false;
    };
    if *pending == 0 {
        return false;
    }
    *pending -= 1;
    ownership.shells_by_slot[slot as usize] += 1;
    ownership.live_shells += 1;
    if let Some(index) = ownership.worker_slots.iter().position(|worker_slot| *worker_slot == slot)
        && !ownership.worker_slots.is_empty()
    {
        ownership.next_round_robin = (index + 1) % ownership.worker_slots.len();
    }
    true
}

fn has_pending_for_executor(slot: u32) -> bool {
    SHELL_OWNERSHIP
        .lock()
        .pending_by_slot
        .get(slot as usize)
        .is_some_and(|pending| *pending != 0)
}

pub(super) fn live_shell_count() -> usize {
    SHELL_OWNERSHIP.lock().live_shells
}

pub(super) fn reserve_shell_on_executor(slot: u32) -> Result<(), super::Shell3Error> {
    let mut ownership = SHELL_OWNERSHIP.lock();
    if ownership.live_shells + ownership.pending_by_slot.iter().sum::<usize>()
        >= super::MAX_SHELL3_INSTANCES
    {
        return Err(super::Shell3Error::InstanceLimit);
    }
    if let Some(expected) = ownership.preferred_slot()
        && expected != slot
    {
        return Err(super::Shell3Error::WrongExecutor {
            expected,
            actual: slot,
        });
    }
    let Some(shell_count) = ownership.shells_by_slot.get_mut(slot as usize) else {
        return Err(super::Shell3Error::InstanceLimit);
    };
    *shell_count += 1;
    ownership.live_shells += 1;
    if let Some(index) = ownership
        .worker_slots
        .iter()
        .position(|worker_slot| *worker_slot == slot)
        && !ownership.worker_slots.is_empty()
    {
        ownership.next_round_robin = (index + 1) % ownership.worker_slots.len();
    }
    Ok(())
}

pub(super) fn release_shell_on_executor(slot: u32) {
    let mut ownership = SHELL_OWNERSHIP.lock();
    if let Some(shell_count) = ownership.shells_by_slot.get_mut(slot as usize) {
        *shell_count = shell_count.saturating_sub(1);
        ownership.live_shells = ownership.live_shells.saturating_sub(1);
    }
}

fn install_worker_slots(slots: Vec<u32>) {
    let mut ownership = SHELL_OWNERSHIP.lock();
    ownership.worker_slots = slots;
    ownership.next_round_robin = 0;
}

/// Start one caller-provided worker task on each selected background AP.
///
/// Uses the kernel's P-core-first worker policy, filling from E/unknown cores.
/// The task factory receives each stable CPU slot for affinity checking.
pub fn start<S: Send>(
    mut task: impl FnMut(usize, u32) -> Result<SpawnToken<S>, SpawnError>,
) -> Result<usize, SpawnError> {
    let target = worker_limit();
    if target == 0 {
        return Ok(0);
    }

    let spawners = crate::workers::pick_background_spawners_with_slots(target);

    install_worker_slots(spawners.iter().map(|(slot, _, _)| *slot).collect());

    let mut started = 0;
    for (worker_id, (slot, _core_kind, spawner)) in spawners.into_iter().enumerate() {
        let token = task(worker_id, slot)?;
        spawner.spawn(token);
        started += 1;
    }
    Ok(started)
}

/// Notify resident workers that draw work may be available.
pub fn notify_draw_work() {
    DRAW_WORK_AVAILABLE.notify_all();
}

/// Executor-local collection. Shells created through this owner remain in
/// its task's list and are dropped there when removed or when the owner exits.
pub struct Shell3Executor {
    slot: u32,
    shells: Vec<super::Shell3>,
}

impl Shell3Executor {
    pub fn new(expected_slot: u32) -> Result<Self, super::Shell3Error> {
        let actual_slot = crate::percpu::current_slot() as u32;
        if actual_slot != expected_slot {
            return Err(super::Shell3Error::WrongExecutor {
                expected: expected_slot,
                actual: actual_slot,
            });
        }
        Ok(Self {
            slot: expected_slot,
            shells: Vec::new(),
        })
    }

    pub const fn slot(&self) -> u32 {
        self.slot
    }

    pub fn len(&self) -> usize {
        self.shells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shells.is_empty()
    }

    pub fn create_shell(
        &mut self,
        time: &str,
        aka_names: Vec<alloc::string::String>,
        update_callbacks: Vec<super::UpdateCallback>,
        columns: usize,
        rows: usize,
    ) -> Result<usize, super::Shell3Error> {
        self.ensure_current_executor()?;
        refresh_appdb_names();
        let shell = super::Shell3::new(
            time,
            aka_names,
            appdb_names_snapshot().1,
            update_callbacks,
            columns,
            rows,
        )?;
        let index = self.shells.len();
        self.shells.push(shell);
        Ok(index)
    }

    fn create_shell_reserved(
        &mut self,
        time: &str,
        aka_names: Vec<alloc::string::String>,
        appdb_names: Vec<alloc::string::String>,
        update_callbacks: Vec<super::UpdateCallback>,
        columns: usize,
        rows: usize,
    ) -> Result<usize, super::Shell3Error> {
        self.ensure_current_executor()?;
        let shell = super::Shell3::new_reserved(
            time,
            aka_names,
            appdb_names,
            update_callbacks,
            columns,
            rows,
            self.slot,
        )?;
        let index = self.shells.len();
        self.shells.push(shell);
        Ok(index)
    }

    pub fn get(&self, index: usize) -> Option<&super::Shell3> {
        self.ensure_current_executor().ok()?;
        self.shells.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut super::Shell3> {
        self.ensure_current_executor().ok()?;
        self.shells.get_mut(index)
    }

    pub fn drop_shell(&mut self, index: usize) -> bool {
        if self.ensure_current_executor().is_err() || index >= self.shells.len() {
            return false;
        }
        drop(self.shells.remove(index));
        true
    }

    fn ensure_current_executor(&self) -> Result<(), super::Shell3Error> {
        let actual_slot = crate::percpu::current_slot() as u32;
        if actual_slot == self.slot {
            Ok(())
        } else {
            Err(super::Shell3Error::WrongExecutor {
                expected: self.slot,
                actual: actual_slot,
            })
        }
    }
}

#[trueos_executor::task(pool_size = TOPOLOGY_TASK_POOL_CAPACITY)]
async fn draw_worker_task(worker_id: usize, expected_slot: u32) {
    let actual_slot = u32::try_from(crate::percpu::current_slot()).unwrap_or(u32::MAX);
    if actual_slot != expected_slot {
        crate::log_warn!(target: "service";
            "sh3srv: worker placement mismatch worker={} expected_slot={} actual_slot={} action=retire-worker\n",
            worker_id,
            expected_slot,
            actual_slot,
        );
        return;
    }

    let mut owned_shells = match Shell3Executor::new(expected_slot) {
        Ok(owner) => owner,
        Err(error) => {
            crate::log_warn!(target: "service";
                "sh3srv: owner initialization failed worker={} slot={} error={:?}\n",
                worker_id,
                expected_slot,
                error,
            );
            return;
        }
    };
    let mut appdb_generation = appdb_names_snapshot().0;

    loop {
        while has_pending_for_executor(expected_slot) && take_pending_for_executor(expected_slot) {
            let aka_names = crate::r::restart::startup_alias_names();
            let appdb_names = appdb_names_snapshot().1;
            let startup_time = super::TitleTime::current();
            match owned_shells.create_shell_reserved(
                &startup_time, aka_names, appdb_names, Vec::new(), 80, 5,
            ) {
                Ok(index) => {
                    if let Some(shell) = owned_shells.get_mut(index)
                        && let Err(error) = shell.present().await
                    {
                        crate::log_warn!(target: "service";
                            "sh3srv: initial presentation failed slot={} shell={} error={}\n",
                            expected_slot, index, error,
                        );
                    }
                }
                Err(error) => crate::log_warn!(target: "service";
                    "sh3srv: queued shell creation failed slot={} error={:?}\n",
                    expected_slot, error,
                ),
            }
        }

        let (current_appdb_generation, appdb_names) = appdb_names_snapshot();
        if current_appdb_generation != appdb_generation {
            for index in 0..owned_shells.len() {
                if let Some(shell) = owned_shells.get_mut(index) {
                    shell.set_appdb_names(&appdb_names);
                    if let Err(error) = shell.present().await {
                        crate::log_warn!(target: "service";
                            "sh3srv: appdb title refresh failed slot={} shell={} error={}\n",
                            expected_slot, index, error,
                        );
                    }
                }
            }
            appdb_generation = current_appdb_generation;
        }

        for event in crate::ui4::take_owner_input_events(crate::ui4::WindowOwner::SHELL3_SERVICE) {
            match event {
                crate::ui4::Ui4InputEvent::Keyboard(event)
                    if event.event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
                        && event.event.key_code == crate::r::keyboard::KEYBOARD_KEY_ESCAPE =>
                {
                    let _ = SHELL3_ESCAPE_EVENTS.lock().push_back(event);
                }
                crate::ui4::Ui4InputEvent::Resize(event) => {
                    let mut events = SHELL3_RESIZE_EVENTS.lock();
                    if let Some(index) = events.iter().position(|queued| queued.window == event.window) {
                        let _ = events.swap_remove_front(index);
                    }
                    let _ = events.push_back(event);
                }
                _ => {}
            }
        }

        loop {
            let event = {
                let mut events = SHELL3_ESCAPE_EVENTS.lock();
                let index = events.iter().position(|event| {
                    (0..owned_shells.len()).any(|index| {
                        owned_shells
                            .get(index)
                            .is_some_and(|shell| shell.show_handles_window(event.window))
                    })
                });
                index.and_then(|index| events.swap_remove_front(index))
            };
            let Some(event) = event else { break };
            let input = crate::ui4::Ui4InputEvent::Keyboard(event);
            for index in 0..owned_shells.len() {
                if let Some(shell) = owned_shells.get_mut(index) {
                    shell.show.handle_escape(&input);
                }
            }
        }

        loop {
            let event = {
                let mut events = SHELL3_RESIZE_EVENTS.lock();
                let index = events.iter().position(|event| {
                    (0..owned_shells.len()).any(|index| {
                        owned_shells.get(index)
                            .is_some_and(|shell| shell.show_handles_window(event.window))
                    })
                });
                index.and_then(|index| events.swap_remove_front(index))
            };
            let Some(event) = event else { break };
            for index in 0..owned_shells.len() {
                if let Some(shell) = owned_shells.get_mut(index)
                    && shell.show_handles_window(event.window)
                    && let Err(error) = shell.resize_ui4_to_current().await
                {
                    crate::log_warn!(target: "service";
                        "sh3srv: resize failed slot={} window={} epoch={} extent={}x{} error={}\n",
                        expected_slot, event.window.raw(), event.resize_epoch,
                        event.width, event.height, error,
                    );
                }
            }
        }

        for index in 0..owned_shells.len() {
            if let Some(shell) = owned_shells.get_mut(index)
                && shell.presentation_pending()
                && let Err(error) = shell.present().await
            {
                crate::log_warn!(target: "service";
                    "sh3srv: pending presentation retry failed slot={} shell={} error={}\n",
                    expected_slot, index, error,
                );
            }
        }

        // Reconcile from the broker too, so superseded resize events cannot
        // leave a shell at an obsolete extent.
        for index in 0..owned_shells.len() {
            if let Some(shell) = owned_shells.get_mut(index)
                && shell.ui4_resize_needed()
                && let Err(error) = shell.resize_ui4_to_current().await
            {
                crate::log_warn!(target: "service";
                    "sh3srv: resize reconciliation failed slot={} shell={} error={}\n",
                    expected_slot, index, error,
                );
            }
        }

        // Keep each owner alive through UI4's close animation, then drop the
        // Shell3 and its leased frame on the same AP.
        for index in (0..owned_shells.len()).rev() {
            let closed = owned_shells
                .get(index)
                .is_some_and(super::Shell3::show_is_closed);
            if closed {
                owned_shells.drop_shell(index);
            }
        }

        if owned_shells.is_empty() {
            let observed = DRAW_WORK_AVAILABLE.observe();
            if !has_pending_for_executor(expected_slot) {
                DRAW_WORK_AVAILABLE.wait_after(observed).await;
            }
        } else {
            Timer::after(Duration::from_millis(50)).await;
        }
    }
}

/// Resident Shell3 service controller. It waits for the full discovered AP
/// topology to register, then starts at most half of those APs as draw workers.
#[trueos_executor::task]
pub async fn sh3srv_service_task(_spawner: Spawner) {
    while !crate::workers::all_topology_spawners_registered() {
        Timer::after(Duration::from_millis(25)).await;
    }

    refresh_appdb_names();

    match start(|worker_id, slot| draw_worker_task(worker_id, slot)) {
        Ok(started) => crate::log_info!(target: "service";
            "sh3srv: online workers={} ap_total={} policy=half-ap-topology\n",
            started,
            crate::workers::topology_core_slot_count().saturating_sub(1),
        ),
        Err(error) => crate::log_error!(target: "service";
            "sh3srv: worker startup failed error={:?}\n",
            error,
        ),
    }
}
