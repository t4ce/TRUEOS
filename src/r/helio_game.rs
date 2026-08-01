//! On-demand presentation of the build-produced Helio game artifact.
//!
//! Artifact parsing and CPU-side dynamic-camera lowering are reusable and
//! scene-independent. This module owns only TRUEOS residency, its existing
//! one-GuC Render submission, and UI4 publication.

use alloc::{collections::VecDeque, vec::Vec};
use embassy_time::{Duration, Instant, Timer};

const GAME_ARTIFACT: &[u8] = include_bytes!("../../assets/helio/simple-cube.trueos.intel.helio");
const CHURN_FORWARD_ARTIFACT: &[u8] =
    include_bytes!("../../assets/helio/churn-forward.trueos.intel.helio");
const PLANE_SLOT: usize = crate::ui4::RGB_OVERLAY_PLANE_SLOT_2;
const MARGIN: u32 = 48;
const TRANSPARENT_CLEAR_RGBA: [u8; 4] = [0, 0, 0, 0];
const NATIVE_CHURN_FRAME_PERIOD_US: u64 = 16_667;
const NATIVE_PENDULUM_FRAME_PERIOD_US: u64 = 16_667;
const NATIVE_FIRST_FRAME_INCOMPLETE_RETRY_LIMIT: u64 = 3;
const HELIO_OWNER_BASE: u8 = 8;
pub(crate) const INSTANCE_CAPACITY: usize = 10;
const PENDING_CAPACITY: usize = INSTANCE_CAPACITY;
pub(crate) const CPU_CARRIER_CAPACITY: usize = 3;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct CpuCarrier {
    pub(crate) id: u8,
    pub(crate) worker_slot: u32,
    pub(crate) core_kind: u8,
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct CpuCarrierRegistry {
    count: u8,
    carriers: [Option<CpuCarrier>; CPU_CARRIER_CAPACITY],
}

impl CpuCarrierRegistry {
    const fn new() -> Self {
        Self {
            count: 0,
            carriers: [None; CPU_CARRIER_CAPACITY],
        }
    }

    fn carrier_for_instance(self, instance_id: u32) -> Option<CpuCarrier> {
        if self.count == 0 {
            return None;
        }
        let index = instance_id.wrapping_sub(1) as usize % self.count as usize;
        self.carriers.get(index).copied().flatten()
    }
}

static CPU_CARRIERS: spin::Mutex<CpuCarrierRegistry> = spin::Mutex::new(CpuCarrierRegistry::new());

#[derive(Copy, Clone)]
struct CpuCarrierBootstrap {
    expected: CpuCarrierRegistry,
    scheduled: bool,
    online_mask: u8,
    published: bool,
}

impl CpuCarrierBootstrap {
    const fn new() -> Self {
        Self {
            expected: CpuCarrierRegistry::new(),
            scheduled: false,
            online_mask: 0,
            published: false,
        }
    }
}

static CPU_CARRIER_BOOTSTRAP: spin::Mutex<CpuCarrierBootstrap> =
    spin::Mutex::new(CpuCarrierBootstrap::new());

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum CpuCarrierBootstrapState {
    NeedsSchedule,
    Waiting { online_mask: u8 },
    Online,
}

fn build_cpu_carrier_registry(workers: &[(u32, u8)]) -> Option<CpuCarrierRegistry> {
    if workers.is_empty() || workers.len() > CPU_CARRIER_CAPACITY {
        return None;
    }

    let mut registry = CpuCarrierRegistry::new();
    registry.count = workers.len() as u8;
    for (id, &(worker_slot, core_kind)) in workers.iter().enumerate() {
        if !crate::workers::is_general_background_worker_slot(worker_slot)
            || crate::workers::core_kind_for_slot(worker_slot) != core_kind
            || workers[..id]
                .iter()
                .any(|(registered_slot, _)| *registered_slot == worker_slot)
        {
            return None;
        }
        registry.carriers[id] = Some(CpuCarrier {
            id: id as u8,
            worker_slot,
            core_kind,
        });
    }
    Some(registry)
}

/// Stage a stable AP2+ carrier set without making it visible to launch/status
/// consumers. The public registry is committed only after every remote task
/// has proved that it is executing on its assigned worker.
pub(crate) fn prepare_cpu_carriers(
    workers: &[(u32, u8)],
) -> Option<CpuCarrierBootstrapState> {
    let expected = build_cpu_carrier_registry(workers)?;
    let mut bootstrap = CPU_CARRIER_BOOTSTRAP.lock();
    if bootstrap.expected.count == 0 {
        bootstrap.expected = expected;
    } else if bootstrap.expected != expected {
        return None;
    }

    Some(if bootstrap.published {
        CpuCarrierBootstrapState::Online
    } else if bootstrap.scheduled {
        CpuCarrierBootstrapState::Waiting {
            online_mask: bootstrap.online_mask,
        }
    } else {
        CpuCarrierBootstrapState::NeedsSchedule
    })
}

/// Close the token-reservation phase before any task can report online.
pub(crate) fn mark_cpu_carriers_scheduled(workers: &[(u32, u8)]) -> bool {
    let Some(expected) = build_cpu_carrier_registry(workers) else {
        return false;
    };
    let mut bootstrap = CPU_CARRIER_BOOTSTRAP.lock();
    if bootstrap.expected != expected || bootstrap.published {
        return false;
    }
    bootstrap.scheduled = true;
    true
}

fn cpu_carrier_mask(count: u8) -> u8 {
    (1u8 << count).wrapping_sub(1)
}

/// Admit one carrier after its task validates the current CPU. The last
/// carrier atomically publishes the complete ordered registry; no partial
/// carrier set can ever acquire a deterministic shard.
fn report_cpu_carrier_online(cpu_carrier: CpuCarrier, carrier_count: u8) -> bool {
    let mut bootstrap = CPU_CARRIER_BOOTSTRAP.lock();
    if !bootstrap.scheduled
        || bootstrap.expected.count != carrier_count
        || cpu_carrier.id >= carrier_count
        || bootstrap.expected.carriers[cpu_carrier.id as usize] != Some(cpu_carrier)
    {
        return false;
    }

    bootstrap.online_mask |= 1u8 << cpu_carrier.id;
    if bootstrap.online_mask == cpu_carrier_mask(carrier_count) && !bootstrap.published {
        // Keep the bootstrap lock through publication so tasks observing
        // `published` can never race ahead of the registry contents.
        *CPU_CARRIERS.lock() = bootstrap.expected;
        bootstrap.published = true;
    }
    true
}

fn cpu_carrier_registry_published(cpu_carrier: CpuCarrier, carrier_count: u8) -> bool {
    let bootstrap = CPU_CARRIER_BOOTSTRAP.lock();
    bootstrap.published
        && bootstrap.expected.count == carrier_count
        && bootstrap.expected.carriers[cpu_carrier.id as usize] == Some(cpu_carrier)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum InstanceState {
    Queued,
    Starting,
    Online,
    Stopping,
}

impl InstanceState {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Online => "online",
            Self::Stopping => "stopping",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum LaunchRequest {
    Queued {
        instance_id: u32,
    },
    Replacing {
        instance_id: u32,
        stopping_instance_id: u32,
    },
    Reserved,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum StopRequest {
    Stopping,
    AlreadyStopping,
    CancelledQueued,
    NotFound,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct InstanceStatus {
    pub(crate) instance_id: u32,
    pub(crate) slot: Option<u8>,
    pub(crate) cpu_carrier_id: Option<u8>,
    pub(crate) worker_slot: Option<u32>,
    pub(crate) core_kind: Option<u8>,
    pub(crate) state: InstanceState,
    pub(crate) example_id: u8,
    pub(crate) artifact_name: &'static str,
    pub(crate) artifact_bytes: usize,
    pub(crate) last_error: Option<&'static str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PoolStatus {
    pub(crate) capacity: usize,
    pub(crate) queued: usize,
    pub(crate) cpu_carriers: Vec<CpuCarrier>,
    pub(crate) instances: Vec<InstanceStatus>,
}

#[derive(Copy, Clone)]
struct LaunchJob {
    instance_id: u32,
    example_id: u8,
}

#[derive(Copy, Clone)]
struct InstanceRecord {
    instance_id: u32,
    example_id: u8,
    cpu_carrier: CpuCarrier,
    state: InstanceState,
    last_error: Option<&'static str>,
}

struct GamePool {
    next_instance_id: u32,
    slots: [Option<InstanceRecord>; INSTANCE_CAPACITY],
    pending: VecDeque<LaunchJob>,
}

impl GamePool {
    const fn new() -> Self {
        Self {
            next_instance_id: 1,
            slots: [None; INSTANCE_CAPACITY],
            pending: VecDeque::new(),
        }
    }

    fn allocate_instance_id(&mut self) -> u32 {
        let id = self.next_instance_id;
        self.next_instance_id = self.next_instance_id.wrapping_add(1).max(1);
        id
    }
}

static GAME_POOL: spin::Mutex<GamePool> = spin::Mutex::new(GamePool::new());

const fn artifact_for(example_id: u8) -> (&'static str, usize) {
    if example_id == 2 {
        ("churn-forward.trueos.intel.helio", CHURN_FORWARD_ARTIFACT.len())
    } else {
        ("simple-cube.trueos.intel.helio", GAME_ARTIFACT.len())
    }
}

/// Queue an independent embedded game instance for the resident carrier pool.
/// Once all ten task slots are occupied, the oldest instance is asked to stop
/// at its next safe frame boundary and this newest request remains queued.
pub(crate) fn request_launch(example_id: u8) -> LaunchRequest {
    if !(1..=4).contains(&example_id) {
        return LaunchRequest::Reserved;
    }
    let (instance_id, stopping_instance_id, dropped_instance_id) = {
        let mut pool = GAME_POOL.lock();
        let instance_id = pool.allocate_instance_id();
        // Slots already in Stopping are capacity which is on its way back to
        // the queue. Count only continuing instances here so a manual stop
        // followed by one launch does not evict a second, healthy window.
        let continuing = pool
            .slots
            .iter()
            .flatten()
            .filter(|record| record.state != InstanceState::Stopping)
            .count();
        let demand = continuing.saturating_add(pool.pending.len());
        let stopping_instance_id = if demand >= INSTANCE_CAPACITY {
            let oldest = pool
                .slots
                .iter()
                .flatten()
                .filter(|record| record.state != InstanceState::Stopping)
                .min_by_key(|record| record.instance_id)
                .map(|record| record.instance_id);
            if let Some(oldest) = oldest {
                if let Some(record) = pool
                    .slots
                    .iter_mut()
                    .flatten()
                    .find(|record| record.instance_id == oldest)
                {
                    record.state = InstanceState::Stopping;
                }
            }
            oldest
        } else {
            None
        };
        let dropped_instance_id = if pool.pending.len() >= PENDING_CAPACITY {
            pool.pending.pop_front().map(|job| job.instance_id)
        } else {
            None
        };
        pool.pending.push_back(LaunchJob {
            instance_id,
            example_id,
        });
        (instance_id, stopping_instance_id, dropped_instance_id)
    };
    if let Some(dropped) = dropped_instance_id {
        crate::log_warn!(target: "helio";
            "helio pool pending saturated capacity={} dropped_instance={} newest_instance={} policy=newest-wins\n",
            PENDING_CAPACITY,
            dropped,
            instance_id,
        );
    }
    if let Some(stopping_instance_id) = stopping_instance_id {
        crate::log_warn!(target: "helio";
            "helio pool capacity={} oldest_instance={} action=orderly-stop newest_instance={} action=queue\n",
            INSTANCE_CAPACITY,
            stopping_instance_id,
            instance_id,
        );
        LaunchRequest::Replacing {
            instance_id,
            stopping_instance_id,
        }
    } else {
        LaunchRequest::Queued { instance_id }
    }
}

pub(crate) fn request_stop(instance_id: u32) -> StopRequest {
    let mut pool = GAME_POOL.lock();
    if let Some(index) = pool
        .pending
        .iter()
        .position(|job| job.instance_id == instance_id)
    {
        let _ = pool.pending.remove(index);
        return StopRequest::CancelledQueued;
    }
    let Some(record) = pool
        .slots
        .iter_mut()
        .flatten()
        .find(|record| record.instance_id == instance_id)
    else {
        return StopRequest::NotFound;
    };
    if record.state == InstanceState::Stopping {
        StopRequest::AlreadyStopping
    } else {
        record.state = InstanceState::Stopping;
        StopRequest::Stopping
    }
}

pub(crate) fn request_stop_all() -> usize {
    let mut pool = GAME_POOL.lock();
    let mut stopped = pool.pending.len();
    pool.pending.clear();
    for record in pool.slots.iter_mut().flatten() {
        if record.state != InstanceState::Stopping {
            record.state = InstanceState::Stopping;
            stopped = stopped.saturating_add(1);
        }
    }
    stopped
}

pub(crate) fn status() -> PoolStatus {
    let carrier_registry = *CPU_CARRIERS.lock();
    let pool = GAME_POOL.lock();
    let mut instances = Vec::with_capacity(pool.slots.len() + pool.pending.len());
    for (slot, record) in pool.slots.iter().enumerate() {
        let Some(record) = record else { continue };
        let (artifact_name, artifact_bytes) = artifact_for(record.example_id);
        instances.push(InstanceStatus {
            instance_id: record.instance_id,
            slot: u8::try_from(slot).ok(),
            cpu_carrier_id: Some(record.cpu_carrier.id),
            worker_slot: Some(record.cpu_carrier.worker_slot),
            core_kind: Some(record.cpu_carrier.core_kind),
            state: record.state,
            example_id: record.example_id,
            artifact_name,
            artifact_bytes,
            last_error: record.last_error,
        });
    }
    for job in &pool.pending {
        let (artifact_name, artifact_bytes) = artifact_for(job.example_id);
        let cpu_carrier = carrier_registry.carrier_for_instance(job.instance_id);
        instances.push(InstanceStatus {
            instance_id: job.instance_id,
            slot: None,
            cpu_carrier_id: cpu_carrier.map(|carrier| carrier.id),
            worker_slot: cpu_carrier.map(|carrier| carrier.worker_slot),
            core_kind: cpu_carrier.map(|carrier| carrier.core_kind),
            state: InstanceState::Queued,
            example_id: job.example_id,
            artifact_name,
            artifact_bytes,
            last_error: None,
        });
    }
    instances.sort_unstable_by_key(|instance| instance.instance_id);
    PoolStatus {
        capacity: INSTANCE_CAPACITY,
        queued: pool.pending.len(),
        cpu_carriers: carrier_registry
            .carriers
            .iter()
            .flatten()
            .copied()
            .collect(),
        instances,
    }
}

#[derive(Copy, Clone, Debug)]
enum GameError {
    Artifact(trueos_helio_runtime::Error),
    Frame(crate::ui4::FramePoolError),
    Window(crate::ui4::WindowBrokerError),
    Render(&'static str),
    NativeUnavailable(&'static str),
    InvalidFrame,
}

impl GameError {
    const fn label(self) -> &'static str {
        match self {
            Self::Artifact(_) => "artifact",
            Self::Frame(_) => "frame-pool",
            Self::Window(_) => "window-broker",
            Self::Render(reason) => reason,
            Self::NativeUnavailable(reason) => reason,
            Self::InvalidFrame => "invalid-frame",
        }
    }
}

impl From<crate::ui4::FramePoolError> for GameError {
    fn from(error: crate::ui4::FramePoolError) -> Self {
        Self::Frame(error)
    }
}

impl From<crate::ui4::WindowBrokerError> for GameError {
    fn from(error: crate::ui4::WindowBrokerError) -> Self {
        Self::Window(error)
    }
}

#[derive(Copy, Clone)]
struct InstanceContext {
    slot: usize,
    instance_id: u32,
    example_id: u8,
    cpu_carrier: CpuCarrier,
    owner: crate::ui4::WindowOwner,
}

impl InstanceContext {
    fn is_stopping(self) -> bool {
        let pool = GAME_POOL.lock();
        !matches!(
            pool.slots.get(self.slot).copied().flatten(),
            Some(record)
                if record.instance_id == self.instance_id
                    && record.state != InstanceState::Stopping
        )
    }

    fn request_stop(self, reason: &'static str) {
        let changed = {
            let mut pool = GAME_POOL.lock();
            let Some(record) = pool.slots.get_mut(self.slot).and_then(Option::as_mut) else {
                return;
            };
            if record.instance_id != self.instance_id || record.state == InstanceState::Stopping {
                false
            } else {
                record.state = InstanceState::Stopping;
                true
            }
        };
        if changed {
            crate::log_info!(target: "helio";
                "helio instance={} example={} stop requested reason={} boundary=next-safe-frame\n",
                self.instance_id,
                self.example_id,
                reason,
            );
        }
    }

    fn mark_online(self) -> bool {
        {
            let mut pool = GAME_POOL.lock();
            let Some(record) = pool.slots.get_mut(self.slot).and_then(Option::as_mut) else {
                return false;
            };
            if record.instance_id != self.instance_id || record.state == InstanceState::Stopping {
                return false;
            }
            record.state = InstanceState::Online;
            record.last_error = None;
        }
        crate::log_info!(target: "helio";
            "helio instance={} status=online cpu_carrier={} worker_slot={} current_slot={} core_kind={} cpu_sharding=instance-id-mod-carrier-count gpu_principal=render0 gpu_context=shared-single-render-runtime gpu_affinity=none\n",
            self.instance_id,
            self.cpu_carrier.id,
            self.cpu_carrier.worker_slot,
            crate::percpu::current_slot(),
            self.cpu_carrier.core_kind,
        );
        true
    }

    fn mark_starting_error(self, error: &'static str) -> bool {
        let mut pool = GAME_POOL.lock();
        let Some(record) = pool.slots.get_mut(self.slot).and_then(Option::as_mut) else {
            return false;
        };
        if record.instance_id != self.instance_id || record.state == InstanceState::Stopping {
            return false;
        }
        record.state = InstanceState::Starting;
        record.last_error = Some(error);
        true
    }
}

fn escape_pressed(window: crate::ui4::WindowId, events: &[crate::ui4::winit_input::Event]) -> bool {
    use crate::ui4::winit_input::{ElementState, Event, KeyCode, PhysicalKey, WindowEvent};

    events.iter().any(|event| {
        matches!(
            *event,
            Event::WindowEvent {
                window_id,
                event: WindowEvent::KeyboardInput {
                    event: crate::ui4::winit_input::KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                },
                ..
            } if window_id == window
        )
    })
}

fn claim_next_launch(cpu_carrier: CpuCarrier, carrier_count: u8) -> Option<InstanceContext> {
    if carrier_count == 0 || cpu_carrier.id >= carrier_count {
        return None;
    }
    let mut pool = GAME_POOL.lock();
    let slot = pool.slots.iter().position(Option::is_none)?;
    let pending_index = pool.pending.iter().position(|job| {
        job.instance_id.wrapping_sub(1) % carrier_count as u32 == cpu_carrier.id as u32
    })?;
    let job = pool.pending.remove(pending_index)?;
    pool.slots[slot] = Some(InstanceRecord {
        instance_id: job.instance_id,
        example_id: job.example_id,
        cpu_carrier,
        state: InstanceState::Starting,
        last_error: None,
    });
    Some(InstanceContext {
        slot,
        instance_id: job.instance_id,
        example_id: job.example_id,
        cpu_carrier,
        owner: crate::ui4::WindowOwner::KernelApp(HELIO_OWNER_BASE + slot as u8),
    })
}

fn requeue_failed_spawn(context: InstanceContext) {
    let mut pool = GAME_POOL.lock();
    let Some(record) = pool.slots.get(context.slot).copied().flatten() else {
        return;
    };
    if record.instance_id != context.instance_id {
        return;
    }
    pool.slots[context.slot] = None;
    // A stop can race the executor's task-pool admission failure. In that
    // case freeing the claimed slot completes the stop; requeueing would
    // otherwise resurrect the exact instance the caller just closed.
    if record.state == InstanceState::Stopping {
        return;
    }
    if pool.pending.len() >= PENDING_CAPACITY {
        let _ = pool.pending.pop_front();
    }
    pool.pending.push_front(LaunchJob {
        instance_id: context.instance_id,
        example_id: context.example_id,
    });
}

fn release_instance_slot(context: InstanceContext) {
    let mut pool = GAME_POOL.lock();
    if pool
        .slots
        .get(context.slot)
        .copied()
        .flatten()
        .is_some_and(|record| record.instance_id == context.instance_id)
    {
        pool.slots[context.slot] = None;
    }
}

struct ResidentTriangle {
    mesh: crate::intel::render::ResidentTriangleMesh,
    rgba: [u8; 4],
}

struct FlyCamera {
    camera: trueos_helio_runtime::Camera,
    yaw: f32,
    pitch: f32,
    look_active: bool,
}

impl FlyCamera {
    fn new(camera: trueos_helio_runtime::Camera) -> Self {
        let direction = [
            camera.target[0] - camera.position[0],
            camera.target[1] - camera.position[1],
            camera.target[2] - camera.position[2],
        ];
        let length = libm::sqrtf(
            direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2],
        )
        .max(f32::EPSILON);
        Self {
            camera,
            yaw: libm::atan2f(direction[0], -direction[2]),
            pitch: libm::asinf((direction[1] / length).clamp(-1.0, 1.0)),
            look_active: false,
        }
    }

    fn apply_events(&mut self, events: &[crate::ui4::winit_input::Event]) -> bool {
        use crate::ui4::winit_input::{
            DeviceEvent, ElementState, Event, KeyCode, MouseButton, PhysicalKey, WindowEvent,
        };

        let mut changed = false;
        for event in events {
            match *event {
                Event::WindowEvent {
                    event:
                        WindowEvent::MouseInput {
                            state,
                            button: MouseButton::Left,
                        },
                    ..
                } => self.look_active = state == ElementState::Pressed,
                Event::WindowEvent {
                    event: WindowEvent::Focused(false),
                    ..
                } => self.look_active = false,
                Event::WindowEvent {
                    event:
                        WindowEvent::KeyboardInput {
                            event:
                                crate::ui4::winit_input::KeyEvent {
                                    physical_key: PhysicalKey::Code(KeyCode::Escape),
                                    state: ElementState::Pressed,
                                    ..
                                },
                        },
                    ..
                } => self.look_active = false,
                Event::DeviceEvent {
                    event: DeviceEvent::MouseMotion { delta: (dx, dy) },
                    ..
                } if self.look_active => {
                    self.yaw += dx as f32 * 0.003;
                    self.pitch = (self.pitch - dy as f32 * 0.003).clamp(-1.5, 1.5);
                    changed = true;
                }
                _ => {}
            }
        }
        if changed {
            self.update_target();
        }
        changed
    }

    fn apply_held_keys(
        &mut self,
        input: &crate::ui4::winit_input::EventLoopInput,
        window: crate::ui4::WindowId,
        dt_seconds: f32,
    ) -> bool {
        use crate::ui4::winit_input::KeyCode;

        let forward = [libm::sinf(self.yaw), 0.0, -libm::cosf(self.yaw)];
        let right = [libm::cosf(self.yaw), 0.0, libm::sinf(self.yaw)];
        let mut movement = [0.0f32; 3];
        if input.key_is_down(window, KeyCode::KeyW) {
            add_scaled(&mut movement, forward, 1.0);
        }
        if input.key_is_down(window, KeyCode::KeyS) {
            add_scaled(&mut movement, forward, -1.0);
        }
        if input.key_is_down(window, KeyCode::KeyD) {
            add_scaled(&mut movement, right, 1.0);
        }
        if input.key_is_down(window, KeyCode::KeyA) {
            add_scaled(&mut movement, right, -1.0);
        }
        if input.key_is_down(window, KeyCode::Space) {
            movement[1] += 1.0;
        }
        if input.key_is_down(window, KeyCode::ShiftLeft) {
            movement[1] -= 1.0;
        }
        let length = libm::sqrtf(
            movement[0] * movement[0] + movement[1] * movement[1] + movement[2] * movement[2],
        );
        if length <= f32::EPSILON {
            return false;
        }
        let distance = 4.0 * dt_seconds / length;
        add_scaled(&mut self.camera.position, movement, distance);
        self.update_target();
        true
    }

    fn update_target(&mut self) {
        let cos_pitch = libm::cosf(self.pitch);
        self.camera.target = [
            self.camera.position[0] + libm::sinf(self.yaw) * cos_pitch,
            self.camera.position[1] + libm::sinf(self.pitch),
            self.camera.position[2] - libm::cosf(self.yaw) * cos_pitch,
        ];
    }
}

fn add_scaled(target: &mut [f32; 3], value: [f32; 3], scale: f32) {
    for index in 0..3 {
        target[index] += value[index] * scale;
    }
}

struct GameSurface {
    owner: crate::ui4::WindowOwner,
    pool_slot: usize,
    session: crate::ui4::WindowSessionId,
    frame: crate::ui4::FrameHandle,
    window: Option<crate::ui4::WindowId>,
    width: u32,
    height: u32,
}

#[derive(Copy, Clone)]
struct PendingGameResize {
    frame: crate::ui4::FrameHandle,
    width: u32,
    height: u32,
}

fn create_game_frame(width: u32, height: u32) -> Result<crate::ui4::FrameHandle, GameError> {
    let (max_width, max_height) = crate::intel::render::resident_scene_target_dimensions();
    if width == 0 || height == 0 || width as usize > max_width || height as usize > max_height {
        return Err(GameError::InvalidFrame);
    }
    let output = crate::ui4::OutputId::from_slot(0).expect("UI4 D01 must exist");
    Ok(crate::ui4::create_frame(crate::ui4::FrameSpec {
        output,
        content: crate::ui4::FrameContent::RenderScene3d,
        cadence: crate::ui4::FrameCadence::Streaming,
        buffering: crate::ui4::FrameBuffering::Triple,
        format: crate::ui4::ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(crate::ui4::PremultipliedRgba8::TRANSPARENT),
    })?)
}

fn create_surface(context: InstanceContext) -> Result<GameSurface, GameError> {
    let (render_width, render_height) = crate::intel::render::resident_scene_target_dimensions();
    let (scanout_width, scanout_height) = crate::intel::active_scanout_dimensions()
        .unwrap_or((crate::ui4::DEFAULT_FRAME_WIDTH, crate::ui4::DEFAULT_FRAME_HEIGHT));
    let width = crate::ui4::DEFAULT_FRAME_WIDTH
        .min(scanout_width)
        .min(render_width as u32);
    let height = crate::ui4::DEFAULT_FRAME_HEIGHT
        .min(scanout_height)
        .min(render_height as u32);
    if width == 0 || height == 0 {
        return Err(GameError::InvalidFrame);
    }
    let frame = create_game_frame(width, height)?;
    let session = match crate::ui4::begin_window_session(context.owner) {
        Ok(session) => session,
        Err(error) => {
            let _ = crate::ui4::destroy_frame(frame);
            return Err(error.into());
        }
    };
    Ok(GameSurface {
        owner: context.owner,
        pool_slot: context.slot,
        session,
        frame,
        window: None,
        width,
        height,
    })
}

fn desired_resize(surface: &GameSurface) -> Option<(u32, u32)> {
    let window = surface.window?;
    let placement = crate::ui4::window_placement(surface.owner, window).ok()?;
    let target = helio_backing_extent(placement.width, placement.height);
    (target != (surface.width, surface.height)).then_some(target)
}

/// A maximized Helio scene keeps the broker's full logical extent while the
/// universal display plane scales a half-resolution backing directly. This is
/// UI4's native maximize fast path and prevents a 2560x1440 churn frame from
/// monopolizing the shared RCS lane; ordinary 768x512 windows stay exact 1:1.
const fn helio_backing_extent(logical_width: u32, logical_height: u32) -> (u32, u32) {
    if logical_width > crate::ui4::DEFAULT_FRAME_WIDTH
        && logical_height > crate::ui4::DEFAULT_FRAME_HEIGHT
    {
        (logical_width.div_ceil(2), logical_height.div_ceil(2))
    } else {
        (logical_width, logical_height)
    }
}

fn prepare_resize(
    surface: &GameSurface,
    width: u32,
    height: u32,
) -> Result<PendingGameResize, GameError> {
    surface.window.ok_or(GameError::InvalidFrame)?;
    Ok(PendingGameResize {
        frame: create_game_frame(width, height)?,
        width,
        height,
    })
}

/// Make an already GPU-authored replacement visible in one broker transition.
/// The old frame remains attached if the Ready publication fails.
fn commit_resize(
    surface: &mut GameSurface,
    replacement: PendingGameResize,
) -> Result<crate::ui4::FrameHandle, GameError> {
    let window = surface.window.ok_or(GameError::InvalidFrame)?;
    let previous = surface.frame;
    crate::ui4::replace_window_frame(surface.owner, window, replacement.frame)?;
    if let Err(error) =
        crate::ui4::publish_window_frame(surface.owner, window, crate::ui4::DamageRect::FULL)
    {
        let _ = crate::ui4::replace_window_frame(surface.owner, window, previous);
        let _ =
            crate::ui4::publish_window_frame(surface.owner, window, crate::ui4::DamageRect::FULL);
        return Err(error.into());
    }
    surface.frame = replacement.frame;
    surface.width = replacement.width;
    surface.height = replacement.height;
    Ok(previous)
}

fn retire_frames(frames: &mut Vec<crate::ui4::FrameHandle>) {
    frames.retain(|frame| {
        matches!(crate::ui4::destroy_frame(*frame), Err(crate::ui4::FramePoolError::Busy))
    });
}

fn transfer_retired_frames(frames: &mut Vec<crate::ui4::FrameHandle>) {
    for frame in frames.drain(..) {
        crate::ui4::retire_frame_when_released(frame);
    }
}

fn make_resident_scene(
    scene: &trueos_helio_runtime::Scene,
) -> Result<Vec<ResidentTriangle>, GameError> {
    let mut resident = Vec::with_capacity(scene.triangles.len());
    for (triangle_index, triangle) in scene.triangles.iter().enumerate() {
        let args = match scene.resident_triangle_draw_indexed_indirect(triangle_index) {
            Ok(args) => args,
            Err(error) => {
                release_resident_scene(resident);
                return Err(GameError::Artifact(error));
            }
        };
        match crate::intel::render::create_resident_triangle_mesh(&triangle.vertices, &[0, 1, 2]) {
            Ok(mesh) => {
                if let Err(error) =
                    crate::intel::render::update_resident_triangle_draw_indexed_indirect(
                        &mesh,
                        args.index_count,
                        args.instance_count,
                        args.first_index,
                        args.base_vertex,
                        args.first_instance,
                    )
                {
                    let _ = crate::intel::render::release_resident_triangle_mesh(&mesh);
                    release_resident_scene(resident);
                    return Err(GameError::Render(error));
                }
                resident.push(ResidentTriangle {
                    mesh,
                    rgba: triangle.rgba,
                });
            }
            Err(error) => {
                release_resident_scene(resident);
                return Err(GameError::Render(error));
            }
        }
    }
    if resident.is_empty() {
        return Err(GameError::Render("helio-empty-scene"));
    }
    Ok(resident)
}

fn update_resident_scene(
    resident: &[ResidentTriangle],
    scene: &trueos_helio_runtime::Scene,
) -> Result<(), GameError> {
    if resident.len() != scene.triangles.len() {
        return Err(GameError::Render("helio-resize-triangle-count"));
    }
    for (triangle_index, (resident, triangle)) in resident.iter().zip(&scene.triangles).enumerate()
    {
        crate::intel::render::update_resident_triangle_vertices(&resident.mesh, &triangle.vertices)
            .map_err(GameError::Render)?;
        let args = scene
            .resident_triangle_draw_indexed_indirect(triangle_index)
            .map_err(GameError::Artifact)?;
        crate::intel::render::update_resident_triangle_draw_indexed_indirect(
            &resident.mesh,
            args.index_count,
            args.instance_count,
            args.first_index,
            args.base_vertex,
            args.first_instance,
        )
        .map_err(GameError::Render)?;
    }
    Ok(())
}

fn release_resident_scene(resident: Vec<ResidentTriangle>) {
    for triangle in resident {
        if !crate::intel::render::release_resident_triangle_mesh(&triangle.mesh) {
            core::mem::forget(triangle);
        }
    }
}

fn gpu_logger_sample(
    result: &crate::intel::render::ResidentSceneFrameResult,
    frame_index: u64,
    cadence_us: u64,
    objects: usize,
    resident: &[ResidentTriangle],
    busy_retries: u64,
    incomplete_retries: u64,
) -> crate::spirit::gpu_logger::GpuLoggerSample {
    crate::spirit::gpu_logger::GpuLoggerSample {
        frame_index,
        cadence_us,
        frame_us: result.frame_us,
        geometry_us: result.geometry_us,
        prepare_us: result.geometry_prepare_us,
        retire_wait_us: result.gpu_poll_us,
        poll_iters: result.gpu_poll_iters,
        objects: u64::try_from(objects).unwrap_or(u64::MAX),
        draws: u64::try_from(result.requested_draws).unwrap_or(u64::MAX),
        triangles: resident
            .iter()
            .map(|triangle| u64::from(triangle.mesh.index_count / 3))
            .sum(),
        busy_retries,
        incomplete_retries,
    }
}

fn native_churn_gpu_logger_sample(
    result: &crate::intel::render::ResidentSceneFrameResult,
    frame_index: u64,
    cadence_us: u64,
    objects: usize,
    triangles: u64,
    busy_retries: u64,
    incomplete_retries: u64,
) -> crate::spirit::gpu_logger::GpuLoggerSample {
    crate::spirit::gpu_logger::GpuLoggerSample {
        frame_index,
        cadence_us,
        frame_us: result.frame_us,
        geometry_us: result.geometry_us,
        prepare_us: result.geometry_prepare_us,
        retire_wait_us: result.gpu_poll_us,
        poll_iters: result.gpu_poll_iters,
        objects: u64::try_from(objects).unwrap_or(u64::MAX),
        draws: u64::try_from(result.requested_draws).unwrap_or(u64::MAX),
        triangles,
        busy_retries,
        incomplete_retries,
    }
}

fn publish_gpu_logger_sample(sample: crate::spirit::gpu_logger::GpuLoggerSample) {
    crate::spirit::gpu_logger::publish(crate::spirit::gpu_logger::GpuLoggerSource::Helio, sample);
}

fn destroy_unpublished_surface(surface: GameSurface) {
    if surface.window.is_some() {
        let request = crate::ui4::WindowSessionCloseRequest::default()
            .direct_plane_animate_and_retire_frames();
        if crate::ui4::finish_window_session_with_request(surface.owner, surface.session, request)
            .is_ok()
        {
            return;
        }
    }
    if let Some(window) = surface.window {
        let _ = crate::ui4::close_window(surface.owner, window);
    }
    let _ = crate::ui4::finish_window_session(surface.owner, surface.session);
    crate::ui4::retire_frame_when_released(surface.frame);
}

async fn render_frame(
    frame: crate::ui4::FrameHandle,
    width: u32,
    height: u32,
    resident: &[ResidentTriangle],
    clear_rgba: [u8; 4],
) -> Result<crate::intel::render::ResidentSceneFrameResult, GameError> {
    let lease = crate::ui4::acquire_frame_buffer(frame)?;
    let destination = match crate::ui4::gpgpu_rgba_surface(lease) {
        Ok(destination)
            if destination.width == width
                && destination.height == height
                && destination.pitch_bytes >= width.saturating_mul(4) =>
        {
            destination
        }
        Ok(_) => {
            let _ = crate::ui4::cancel_frame_buffer(lease);
            return Err(GameError::InvalidFrame);
        }
        Err(error) => {
            let _ = crate::ui4::cancel_frame_buffer(lease);
            return Err(error.into());
        }
    };
    let draws = resident
        .iter()
        .map(|triangle| crate::intel::render::ResidentSceneDraw {
            mesh: &triangle.mesh,
            rgba: triangle.rgba,
            viewport_translation_px: [0.0, 0.0],
        })
        .collect::<Vec<_>>();
    let result = match crate::intel::render::
        render_resident_triangle_scene_frame_premultiplied_with_opaque_depth_direct_to_surface(
            &draws,
            Some(clear_rgba),
            destination,
            false,
        ) {
        Ok(result) => result,
        Err(error) => {
            let _ = crate::ui4::cancel_frame_buffer(lease);
            return Err(GameError::Render(error));
        }
    };
    if result.completed_draws != result.requested_draws
        || result.completed_draws != resident.len()
        || result.rgba.is_some()
        || result.present_copy_performed
    {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(GameError::Render("helio-incomplete-direct-frame"));
    }
    let Some(release) = result.release_fence else {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(GameError::Render("helio-missing-release-fence"));
    };
    if let Err(error) = crate::ui4::publish_gpu_frame_buffer(lease, release) {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(error.into());
    }

    Ok(result)
}

async fn render_native_churn_frame(
    frame: crate::ui4::FrameHandle,
    width: u32,
    height: u32,
    resident: &crate::intel::render::ResidentChurnForward,
    clear_rgba: [u8; 4],
) -> Result<crate::intel::render::ResidentSceneFrameResult, GameError> {
    let lease = crate::ui4::acquire_frame_buffer(frame)?;
    let destination = match crate::ui4::gpgpu_rgba_surface(lease) {
        Ok(destination)
            if destination.width == width
                && destination.height == height
                && destination.pitch_bytes >= width.saturating_mul(4) =>
        {
            destination
        }
        Ok(_) => {
            let _ = crate::ui4::cancel_frame_buffer(lease);
            return Err(GameError::InvalidFrame);
        }
        Err(error) => {
            let _ = crate::ui4::cancel_frame_buffer(lease);
            return Err(error.into());
        }
    };
    let result = match crate::intel::render::render_resident_churn_forward_frame_direct_to_surface(
        resident,
        Some(clear_rgba),
        destination,
        false,
    ) {
        Ok(result) => result,
        Err(error) => {
            let _ = crate::ui4::cancel_frame_buffer(lease);
            return Err(GameError::Render(error));
        }
    };
    if result.completed_draws != result.requested_draws
        || result.requested_draws != trueos_helio_runtime::churn::DRAW_GROUP_COUNT
        || result.rgba.is_some()
        || result.present_copy_performed
    {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(GameError::Render("helio-incomplete-native-instanced-frame"));
    }
    let Some(release) = result.release_fence else {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(GameError::Render("helio-missing-release-fence"));
    };
    if let Err(error) = crate::ui4::publish_gpu_frame_buffer(lease, release) {
        let _ = crate::ui4::cancel_frame_buffer(lease);
        return Err(error.into());
    }
    Ok(result)
}

fn publish_surface_window(surface: &mut GameSurface) -> Result<(), GameError> {
    let output = crate::ui4::OutputId::from_slot(0).expect("UI4 D01 must exist");
    let (scanout_width, scanout_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((surface.width, surface.height));
    if surface.window.is_none() {
        let cascade = u32::try_from(surface.pool_slot)
            .unwrap_or(0)
            .saturating_mul(28);
        let x = scanout_width
            .saturating_sub(surface.width.saturating_add(MARGIN))
            .saturating_sub(cascade);
        let y = MARGIN
            .saturating_add(cascade)
            .min(scanout_height.saturating_sub(surface.height));
        surface.window = Some(crate::ui4::create_window(crate::ui4::WindowCreate {
            owner: surface.owner,
            session: surface.session,
            frame: surface.frame,
            output,
            plane: crate::ui4::WindowPlane::Universal(PLANE_SLOT as u8),
            placement: crate::ui4::WindowPlacement {
                x: x as i32,
                y: y as i32,
                width: surface.width,
                height: surface.height,
                z: 78 + i32::try_from(surface.pool_slot).unwrap_or(0),
                opacity: u8::MAX,
                visible: true,
            },
            interaction: crate::ui4::WindowInteraction::APPLICATION,
        })?);
    }
    let window = surface.window.ok_or(GameError::InvalidFrame)?;
    crate::ui4::publish_window_frame(surface.owner, window, crate::ui4::DamageRect::FULL)?;
    Ok(())
}

async fn render_publish(
    surface: &mut GameSurface,
    resident: &[ResidentTriangle],
    clear_rgba: [u8; 4],
) -> Result<crate::intel::render::ResidentSceneFrameResult, GameError> {
    let result =
        render_frame(surface.frame, surface.width, surface.height, resident, clear_rgba).await?;
    publish_surface_window(surface)?;
    Ok(result)
}

fn make_resident_batches(
    batches: &[trueos_helio_runtime::churn::Batch],
) -> Result<Vec<ResidentTriangle>, GameError> {
    let mut resident = Vec::with_capacity(batches.len());
    for batch in batches {
        let args = match batch.draw_indexed_indirect() {
            Ok(args) => args,
            Err(error) => {
                release_resident_scene(resident);
                return Err(GameError::Artifact(error));
            }
        };
        match crate::intel::render::create_resident_triangle_mesh(&batch.vertices, &batch.indices) {
            Ok(mesh) => {
                if let Err(error) =
                    crate::intel::render::update_resident_triangle_draw_indexed_indirect(
                        &mesh,
                        args.index_count,
                        args.instance_count,
                        args.first_index,
                        args.base_vertex,
                        args.first_instance,
                    )
                {
                    let _ = crate::intel::render::release_resident_triangle_mesh(&mesh);
                    release_resident_scene(resident);
                    return Err(GameError::Render(error));
                }
                resident.push(ResidentTriangle {
                    mesh,
                    rgba: batch.rgba,
                });
            }
            Err(error) => {
                release_resident_scene(resident);
                return Err(GameError::Render(error));
            }
        }
    }
    Ok(resident)
}

fn update_resident_batches(
    resident: &mut [ResidentTriangle],
    batches: &[trueos_helio_runtime::churn::Batch],
) -> Result<(), GameError> {
    if resident.len() < batches.len() {
        return Err(GameError::Render("helio-churn-batch-count"));
    }
    for (resident, batch) in resident.iter_mut().zip(batches) {
        crate::intel::render::update_resident_triangle_vertices(&resident.mesh, &batch.vertices)
            .map_err(GameError::Render)?;
        let args = batch.draw_indexed_indirect().map_err(GameError::Artifact)?;
        crate::intel::render::update_resident_triangle_draw_indexed_indirect(
            &resident.mesh,
            args.index_count,
            args.instance_count,
            args.first_index,
            args.base_vertex,
            args.first_instance,
        )
        .map_err(GameError::Render)?;
        resident.rgba = batch.rgba;
    }
    Ok(())
}

fn apply_churn_key_actions(
    engine: &mut trueos_helio_runtime::churn::Engine,
    instance_id: u32,
    window: crate::ui4::WindowId,
    events: &[crate::ui4::winit_input::Event],
) {
    use crate::ui4::winit_input::{ElementState, Event, KeyCode, PhysicalKey, WindowEvent};

    for event in events {
        let Event::WindowEvent {
            window_id,
            event:
                WindowEvent::KeyboardInput {
                    event:
                        crate::ui4::winit_input::KeyEvent {
                            physical_key: PhysicalKey::Code(key),
                            state: ElementState::Pressed,
                            ..
                        },
                },
            ..
        } = *event
        else {
            continue;
        };
        if window_id != window {
            continue;
        }
        match key {
            KeyCode::Equal | KeyCode::NumpadAdd => {
                engine.adjust_spawn_rate(1);
                crate::log_info!(
                    target: "helio";
                    "helio instance={} churn input spawn_rate={} source=ui4-winit-bridge\n",
                    instance_id,
                    engine.spawn_rate(),
                );
            }
            KeyCode::Minus | KeyCode::NumpadSubtract => {
                engine.adjust_spawn_rate(-1);
                crate::log_info!(
                    target: "helio";
                    "helio instance={} churn input spawn_rate={} source=ui4-winit-bridge\n",
                    instance_id,
                    engine.spawn_rate(),
                );
            }
            KeyCode::KeyC => {
                let collisions = engine.toggle_collisions();
                crate::log_info!(
                    target: "helio";
                    "helio instance={} churn input collisions={} action={} mode=bounded-deterministic-separation source=ui4-winit-bridge\n",
                    instance_id,
                    collisions,
                    if collisions { "burst" } else { "orbit-reset" },
                );
            }
            _ => {}
        }
    }
}

fn apply_pendulum_key_actions(
    engine: &mut trueos_helio_runtime::pendulum_bigcloth::Engine,
    instance_id: u32,
    window: crate::ui4::WindowId,
    events: &[crate::ui4::winit_input::Event],
) {
    use crate::ui4::winit_input::{ElementState, Event, KeyCode, PhysicalKey, WindowEvent};

    for event in events {
        let Event::WindowEvent {
            window_id,
            event:
                WindowEvent::KeyboardInput {
                    event:
                        crate::ui4::winit_input::KeyEvent {
                            physical_key: PhysicalKey::Code(key),
                            state: ElementState::Pressed,
                            ..
                        },
                },
            ..
        } = *event
        else {
            continue;
        };
        if window_id != window {
            continue;
        }
        match key {
            KeyCode::KeyC => {
                engine.toggle_enabled();
                crate::log_info!(
                    target: "helio";
                    "helio instance={} pendulum input physics={} source=ui4-winit-bridge\n",
                    instance_id,
                    engine.enabled(),
                );
            }
            KeyCode::KeyR => {
                engine.reset();
                crate::log_info!(
                    target: "helio";
                    "helio instance={} pendulum input action=reset source=ui4-winit-bridge\n",
                    instance_id,
                );
            }
            _ => {}
        }
    }
}

enum PortedSceneEngine {
    Battle(trueos_helio_runtime::battle::Engine),
    Pendulum(trueos_helio_runtime::pendulum_bigcloth::Engine),
}

impl PortedSceneEngine {
    fn decode(example_id: u8) -> Result<Self, trueos_helio_runtime::Error> {
        match example_id {
            3 => trueos_helio_runtime::battle::Spec::decode_artifact(GAME_ARTIFACT)
                .and_then(trueos_helio_runtime::battle::Engine::new)
                .map(Self::Battle),
            4 => trueos_helio_runtime::pendulum_bigcloth::Spec::decode_artifact(GAME_ARTIFACT)
                .and_then(trueos_helio_runtime::pendulum_bigcloth::Engine::new)
                .map(Self::Pendulum),
            _ => Err(trueos_helio_runtime::Error::Artifact),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Battle(_) => "shape-battle-royale",
            Self::Pendulum(_) => "pendulum-bigcloth",
        }
    }

    fn controls(&self) -> &'static str {
        match self {
            Self::Battle(_) => "WASD+Space+Shift,left-drag-look,+/-shape-count",
            Self::Pendulum(_) => "WASD+Space+Shift,left-drag-look,C-pause,R-reset",
        }
    }

    fn object_count(&self) -> usize {
        match self {
            Self::Battle(engine) => engine.shape_count(),
            Self::Pendulum(engine) => engine.segment_count(),
        }
    }

    fn camera(&self) -> trueos_helio_runtime::Camera {
        match self {
            Self::Battle(engine) => engine.camera(),
            Self::Pendulum(engine) => engine.camera(),
        }
    }

    fn set_camera(
        &mut self,
        camera: trueos_helio_runtime::Camera,
    ) -> Result<(), trueos_helio_runtime::Error> {
        match self {
            Self::Battle(engine) => engine.set_camera(camera),
            Self::Pendulum(engine) => engine.set_camera(camera),
        }
    }

    fn step(
        &mut self,
        aspect: f32,
    ) -> Result<&[trueos_helio_runtime::churn::Batch], trueos_helio_runtime::Error> {
        match self {
            Self::Battle(engine) => engine.step(aspect),
            Self::Pendulum(engine) => engine.step(aspect),
        }
    }

    fn batches(&self) -> &[trueos_helio_runtime::churn::Batch] {
        match self {
            Self::Battle(engine) => engine.batches(),
            Self::Pendulum(engine) => engine.batches(),
        }
    }

    fn apply_key_actions(
        &mut self,
        instance_id: u32,
        window: crate::ui4::WindowId,
        events: &[crate::ui4::winit_input::Event],
    ) {
        use crate::ui4::winit_input::{ElementState, Event, KeyCode, PhysicalKey, WindowEvent};

        for event in events {
            let Event::WindowEvent {
                window_id,
                event:
                    WindowEvent::KeyboardInput {
                        event:
                            crate::ui4::winit_input::KeyEvent {
                                physical_key: PhysicalKey::Code(key),
                                state: ElementState::Pressed,
                                ..
                            },
                    },
                ..
            } = *event
            else {
                continue;
            };
            if window_id != window {
                continue;
            }
            match self {
                Self::Battle(engine) => {
                    let delta = match key {
                        KeyCode::Equal | KeyCode::NumpadAdd => 1,
                        KeyCode::Minus | KeyCode::NumpadSubtract => -1,
                        _ => continue,
                    };
                    engine.adjust_shape_count(delta);
                    crate::log_info!(
                        target: "helio";
                        "helio instance={} battle input shape_count={} action=restart source=ui4-winit-bridge\n",
                        instance_id,
                        engine.shape_count(),
                    );
                }
                Self::Pendulum(engine) => match key {
                    KeyCode::KeyC => {
                        engine.toggle_enabled();
                        crate::log_info!(
                            target: "helio";
                            "helio instance={} pendulum input physics={} source=ui4-winit-bridge\n",
                            instance_id,
                            engine.enabled(),
                        );
                    }
                    KeyCode::KeyR => {
                        engine.reset();
                        crate::log_info!(
                            target: "helio";
                            "helio instance={} pendulum input action=reset source=ui4-winit-bridge\n",
                            instance_id,
                        );
                    }
                    _ => {}
                },
            }
        }
    }
}

async fn run_cube(context: InstanceContext) -> Result<(), GameError> {
    let mut surface = create_surface(context)?;
    let initial_scene = match trueos_helio_runtime::decode_artifact_with_replay(
        GAME_ARTIFACT,
        surface.width as f32 / surface.height as f32,
        trueos_helio_runtime::Camera::helio_simple_graph(),
    ) {
        Ok(scene) => scene,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::Artifact(error));
        }
    };
    let clear_rgba = TRANSPARENT_CLEAR_RGBA;
    let triangle_count = initial_scene.triangles.len();
    let resident = match make_resident_scene(&initial_scene) {
        Ok(resident) => resident,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(error);
        }
    };
    let rendered = match render_publish(&mut surface, &resident, clear_rgba).await {
        Ok(rendered) => rendered,
        Err(error) => {
            release_resident_scene(resident);
            destroy_unpublished_surface(surface);
            return Err(error);
        }
    };
    publish_gpu_logger_sample(gpu_logger_sample(&rendered, 1, 0, 1, &resident, 0, 0));

    let window = surface.window.expect("published Helio cube window");
    let mut input = crate::ui4::winit_input::EventLoopInput::new(context.owner);
    if !input.register_window(window) {
        release_resident_scene(resident);
        destroy_unpublished_surface(surface);
        return Err(GameError::Render("helio-input-window-capacity"));
    }
    if !context.mark_online() {
        release_resident_scene(resident);
        destroy_unpublished_surface(surface);
        return Ok(());
    }
    crate::log_info!(
        target: "helio";
        "helio instance={} example=1 name=simple-cube online artifact_bytes={} normalized_triangles={} source_draw_indices={} draw_source={} frame={} window={} extent={}x{} plane={} background=transparent-rgba alpha=premultiplied resize=ui4-broker->render-native-or-half-scale-ring->atomic-frame-swap->direct-plane-1x-or-2x path=helioa-v1+render-ir-v1+artifact-replay-v1->uniform-color-subdraws->helio-indirect-v1->one-guc-schedule->ui4-triple-direct cpu_readback=0 cpu_frame_copy=0\n",
        context.instance_id,
        GAME_ARTIFACT.len(),
        triangle_count,
        initial_scene.source_draw_indexed_indirect.index_count,
        initial_scene.draw_source.label(),
        surface.frame.raw(),
        window.raw(),
        surface.width,
        surface.height,
        PLANE_SLOT,
    );

    let mut retired_frames = Vec::new();
    let mut pending_resize: Option<PendingGameResize> = None;
    let mut frame_index = 1u64;
    loop {
        retire_frames(&mut retired_frames);
        if context.is_stopping() {
            break;
        }
        let events = input.poll();
        if escape_pressed(window, &events) {
            context.request_stop("focused-window-escape");
            break;
        }

        if pending_resize.is_none()
            && let Some((width, height)) = desired_resize(&surface)
        {
            match prepare_resize(&surface, width, height) {
                Ok(replacement) => {
                    let scene = match trueos_helio_runtime::decode_artifact_with_replay(
                        GAME_ARTIFACT,
                        width as f32 / height as f32,
                        trueos_helio_runtime::Camera::helio_simple_graph(),
                    ) {
                        Ok(scene) => scene,
                        Err(error) => {
                            let _ = crate::ui4::destroy_frame(replacement.frame);
                            release_resident_scene(resident);
                            destroy_unpublished_surface(surface);
                            return Err(GameError::Artifact(error));
                        }
                    };
                    if let Err(error) = update_resident_scene(&resident, &scene) {
                        let _ = crate::ui4::destroy_frame(replacement.frame);
                        release_resident_scene(resident);
                        destroy_unpublished_surface(surface);
                        return Err(error);
                    }
                    crate::log_info!(
                        target: "helio";
                        "helio instance={} resize prepared example=1 window={} old={}x{} new={}x{} replacement_frame={} action=render-before-broker-swap old_front=retain-surflive\n",
                        context.instance_id,
                        window.raw(),
                        surface.width,
                        surface.height,
                        width,
                        height,
                        replacement.frame.raw(),
                    );
                    pending_resize = Some(replacement);
                }
                Err(error) => {
                    crate::log_warn!(
                        target: "helio";
                        "helio instance={} resize deferred example=1 window={} old={}x{} requested={}x{} error={:?} action=retain-current-frame-and-reconcile\n",
                        context.instance_id,
                        window.raw(),
                        surface.width,
                        surface.height,
                        width,
                        height,
                        error,
                    );
                }
            }
        }

        let Some(replacement) = pending_resize else {
            Timer::after(Duration::from_millis(16)).await;
            continue;
        };
        let rendered = match render_frame(
            replacement.frame,
            replacement.width,
            replacement.height,
            &resident,
            clear_rgba,
        )
        .await
        {
            Ok(rendered) => rendered,
            Err(GameError::Frame(crate::ui4::FramePoolError::Busy))
            | Err(GameError::Render("helio-incomplete-direct-frame")) => {
                Timer::after(Duration::from_millis(16)).await;
                continue;
            }
            Err(error) => {
                let _ = crate::ui4::destroy_frame(replacement.frame);
                release_resident_scene(resident);
                destroy_unpublished_surface(surface);
                return Err(error);
            }
        };
        pending_resize = None;
        match commit_resize(&mut surface, replacement) {
            Ok(previous) => {
                retired_frames.push(previous);
                frame_index = frame_index.saturating_add(1);
                publish_gpu_logger_sample(gpu_logger_sample(
                    &rendered,
                    frame_index,
                    0,
                    1,
                    &resident,
                    0,
                    0,
                ));
                crate::log_info!(
                    target: "helio";
                    "helio instance={} resize committed example=1 window={} extent={}x{} frame={} action=broker-swap-after-first-guc-release old_release=surflive presentation=1:1-or-direct-plane-2x cpu_readback=0 cpu_frame_copy=0\n",
                    context.instance_id,
                    window.raw(),
                    surface.width,
                    surface.height,
                    surface.frame.raw(),
                );
            }
            Err(error) => {
                let _ = crate::ui4::destroy_frame(replacement.frame);
                crate::log_warn!(
                    target: "helio";
                    "helio instance={} resize commit rejected example=1 window={} requested={}x{} error={:?} action=old-front-restored\n",
                    context.instance_id,
                    window.raw(),
                    replacement.width,
                    replacement.height,
                    error,
                );
            }
        }
    }
    if let Some(replacement) = pending_resize {
        crate::ui4::retire_frame_when_released(replacement.frame);
    }
    transfer_retired_frames(&mut retired_frames);
    release_resident_scene(resident);
    destroy_unpublished_surface(surface);
    Ok(())
}

fn native_churn_triangle_count(frame: &trueos_helio_runtime::churn::InstanceFrame<'_>) -> u64 {
    frame.draws.iter().fold(0u64, |total, draw| {
        total.saturating_add(
            u64::from(draw.index_count / 3).saturating_mul(u64::from(draw.instance_count)),
        )
    })
}

async fn run_churn_native(context: InstanceContext) -> Result<(), GameError> {
    let mut surface = create_surface(context)?;
    // The native artifact owns the WGPU forward program and instance ABI;
    // the shared game artifact owns the authored churn scene parameters.
    let spec = match trueos_helio_runtime::churn::Spec::decode_artifact(GAME_ARTIFACT) {
        Ok(spec) => spec,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::Artifact(error));
        }
    };
    let max_instances = spec.max_objects;
    let mut engine = match trueos_helio_runtime::churn::Engine::new(spec) {
        Ok(engine) => engine,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::Artifact(error));
        }
    };
    let initial = match engine.step_instances(surface.width as f32 / surface.height as f32) {
        Ok(frame) => frame,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::Artifact(error));
        }
    };
    let resident = match crate::intel::render::create_resident_churn_forward(
        CHURN_FORWARD_ARTIFACT,
        max_instances,
        initial.meshes,
    ) {
        Ok(resident) => resident,
        Err(reason) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::NativeUnavailable(reason));
        }
    };
    if let Err(reason) =
        crate::intel::render::update_resident_churn_forward_frame(&resident, &initial)
    {
        let _ = crate::intel::render::release_resident_churn_forward(&resident);
        destroy_unpublished_surface(surface);
        return Err(GameError::NativeUnavailable(reason));
    }
    let mut prepared_triangles = native_churn_triangle_count(&initial);

    let mut input = crate::ui4::winit_input::EventLoopInput::new(context.owner);
    let mut fly_camera = FlyCamera::new(engine.camera());
    let clear_rgba = TRANSPARENT_CLEAR_RGBA;
    let mut first_frame = true;
    let mut frame_prepared = true;
    let mut frame_index = 0u64;
    let mut busy_retries = 0u64;
    let mut incomplete_retries = 0u64;
    let mut last_sample: Option<crate::spirit::gpu_logger::GpuLoggerSample> = None;
    let mut last_successful_publish: Option<Instant> = None;
    let mut retired_frames = Vec::new();
    let mut pending_resize: Option<PendingGameResize> = None;
    let mut next_frame = Instant::now();
    let result = loop {
        retire_frames(&mut retired_frames);
        if context.is_stopping() {
            break Ok(());
        }
        if !first_frame && !frame_prepared {
            let Some(window) = surface.window else {
                break Err(GameError::InvalidFrame);
            };
            let events = input.poll();
            if escape_pressed(window, &events) {
                context.request_stop("focused-window-escape");
                break Ok(());
            }
            apply_churn_key_actions(&mut engine, context.instance_id, window, &events);
            let camera_changed = fly_camera.apply_events(&events)
                | fly_camera.apply_held_keys(&input, window, 0.016);
            if pending_resize.is_none()
                && let Some((width, height)) = desired_resize(&surface)
            {
                match prepare_resize(&surface, width, height) {
                    Ok(replacement) => {
                        crate::log_info!(
                            target: "helio";
                            "helio instance={} native resize prepared example=2 window={} old={}x{} new={}x{} replacement_frame={} action=render-before-broker-swap\n",
                            context.instance_id,
                            window.raw(),
                            surface.width,
                            surface.height,
                            width,
                            height,
                            replacement.frame.raw(),
                        );
                        pending_resize = Some(replacement);
                    }
                    Err(error) => {
                        crate::log_warn!(
                            target: "helio";
                            "helio instance={} native resize deferred example=2 requested={}x{} error={:?}\n",
                            context.instance_id,
                            width,
                            height,
                            error,
                        );
                    }
                }
            }
            let (render_width, render_height) = pending_resize
                .map_or((surface.width, surface.height), |replacement| {
                    (replacement.width, replacement.height)
                });
            if camera_changed && let Err(error) = engine.set_camera(fly_camera.camera) {
                break Err(GameError::Artifact(error));
            }
            let prepared = match engine.step_instances(render_width as f32 / render_height as f32) {
                Ok(frame) => frame,
                Err(error) => break Err(GameError::Artifact(error)),
            };
            if let Err(reason) =
                crate::intel::render::update_resident_churn_forward_frame(&resident, &prepared)
            {
                break Err(GameError::Render(reason));
            }
            prepared_triangles = native_churn_triangle_count(&prepared);
            frame_prepared = true;
        }

        let (render_frame_handle, render_width, render_height) = pending_resize
            .map_or((surface.frame, surface.width, surface.height), |replacement| {
                (replacement.frame, replacement.width, replacement.height)
            });
        match render_native_churn_frame(
            render_frame_handle,
            render_width,
            render_height,
            &resident,
            clear_rgba,
        )
        .await
        {
            Ok(rendered) => {
                if let Some(replacement) = pending_resize.take() {
                    match commit_resize(&mut surface, replacement) {
                        Ok(previous) => {
                            retired_frames.push(previous);
                            crate::log_info!(
                                target: "helio";
                                "helio instance={} native resize committed example=2 window={} extent={}x{} frame={}\n",
                                context.instance_id,
                                surface.window.map_or(0, crate::ui4::WindowId::raw),
                                surface.width,
                                surface.height,
                                surface.frame.raw(),
                            );
                        }
                        Err(error) => {
                            let _ = crate::ui4::destroy_frame(replacement.frame);
                            frame_prepared = false;
                            crate::log_warn!(
                                target: "helio";
                                "helio instance={} native resize commit rejected requested={}x{} error={:?}\n",
                                context.instance_id,
                                replacement.width,
                                replacement.height,
                                error,
                            );
                            next_frame += Duration::from_micros(NATIVE_CHURN_FRAME_PERIOD_US);
                            if next_frame <= Instant::now() {
                                next_frame = Instant::now();
                            }
                            Timer::at(next_frame).await;
                            continue;
                        }
                    }
                } else if let Err(error) = publish_surface_window(&mut surface) {
                    break Err(error);
                }
                frame_prepared = false;
                frame_index = frame_index.saturating_add(1);
                let published_at = Instant::now();
                let cadence_us = last_successful_publish
                    .map(|previous| published_at.saturating_duration_since(previous).as_micros())
                    .unwrap_or(0);
                last_successful_publish = Some(published_at);
                let sample = native_churn_gpu_logger_sample(
                    &rendered,
                    frame_index,
                    cadence_us,
                    engine.active_objects(),
                    prepared_triangles,
                    busy_retries,
                    incomplete_retries,
                );
                publish_gpu_logger_sample(sample);
                last_sample = Some(sample);
            }
            Err(GameError::Frame(crate::ui4::FramePoolError::Busy)) => {
                busy_retries = busy_retries.saturating_add(1);
                if let Some(mut sample) = last_sample {
                    sample.busy_retries = busy_retries;
                    publish_gpu_logger_sample(sample);
                    last_sample = Some(sample);
                }
                next_frame += Duration::from_micros(NATIVE_CHURN_FRAME_PERIOD_US);
                if next_frame <= Instant::now() {
                    next_frame = Instant::now();
                }
                Timer::at(next_frame).await;
                continue;
            }
            Err(GameError::Render("helio-incomplete-native-instanced-frame")) => {
                incomplete_retries = incomplete_retries.saturating_add(1);
                if first_frame && incomplete_retries >= NATIVE_FIRST_FRAME_INCOMPLETE_RETRY_LIMIT {
                    break Err(GameError::NativeUnavailable("helio-native-first-frame-nonretired"));
                }
                if let Some(mut sample) = last_sample {
                    sample.incomplete_retries = incomplete_retries;
                    publish_gpu_logger_sample(sample);
                    last_sample = Some(sample);
                }
                // Do not step the simulation or rewrite buffers while this
                // exact GPU-owned frame is retried.
                next_frame += Duration::from_micros(NATIVE_CHURN_FRAME_PERIOD_US);
                if next_frame <= Instant::now() {
                    next_frame = Instant::now();
                }
                Timer::at(next_frame).await;
                continue;
            }
            Err(error) => break Err(error),
        }
        if first_frame {
            let window = surface.window.expect("published native Helio churn window");
            if !input.register_window(window) {
                break Err(GameError::Render("helio-input-window-capacity"));
            }
            if !context.mark_online() {
                break Ok(());
            }
            crate::log_info!(
                target: "helio";
                "helio instance={} example=2 name=churn-benchmark native=1 online artifact_bytes={} active_objects={} max_instances={} draw_groups=12 geometry=3x-posnormal-cube frame={} window={} extent={}x{} plane={} background=transparent-rgba controls=WASD+Space+Shift,left-drag-look,+/-,C-collision-burst producer_pacing=absolute-deadline period_us=16667 target_fps=60 missed_deadline=skip-extra-delay path=helioa-churn-forward-v1->artifact-native-vs+ps->camera+instance+compacted-storage->12-indexed-indirect->one-guc-schedule->ui4-triple-direct cpu_vertex_projection=0 mutable_upload=camera+dirty-instances+compacted+indirect immutable_geometry=retained cpu_readback=0 cpu_frame_copy=0\n",
                context.instance_id,
                CHURN_FORWARD_ARTIFACT.len(),
                engine.active_objects(),
                resident.max_instances(),
                surface.frame.raw(),
                window.raw(),
                surface.width,
                surface.height,
                PLANE_SLOT,
            );
            first_frame = false;
        }
        next_frame += Duration::from_micros(NATIVE_CHURN_FRAME_PERIOD_US);
        if next_frame <= Instant::now() {
            next_frame = Instant::now();
        }
        Timer::at(next_frame).await;
    };
    if let Some(replacement) = pending_resize {
        crate::ui4::retire_frame_when_released(replacement.frame);
    }
    transfer_retired_frames(&mut retired_frames);
    let _ = crate::intel::render::release_resident_churn_forward(&resident);
    destroy_unpublished_surface(surface);
    result
}

async fn run_pendulum_native(context: InstanceContext) -> Result<(), GameError> {
    let mut surface = create_surface(context)?;
    let spec = match trueos_helio_runtime::pendulum_bigcloth::Spec::decode_artifact(GAME_ARTIFACT) {
        Ok(spec) => spec,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::Artifact(error));
        }
    };
    let mut engine = match trueos_helio_runtime::pendulum_bigcloth::Engine::new(spec) {
        Ok(engine) => engine,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::Artifact(error));
        }
    };
    let initial = match engine.step_instances(surface.width as f32 / surface.height as f32) {
        Ok(frame) => frame,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::Artifact(error));
        }
    };
    let max_instances = initial.instances.len();
    let resident = match crate::intel::render::create_resident_churn_forward(
        CHURN_FORWARD_ARTIFACT,
        max_instances,
        initial.meshes,
    ) {
        Ok(resident) => resident,
        Err(reason) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::NativeUnavailable(reason));
        }
    };
    if let Err(reason) =
        crate::intel::render::update_resident_churn_forward_frame(&resident, &initial)
    {
        let _ = crate::intel::render::release_resident_churn_forward(&resident);
        destroy_unpublished_surface(surface);
        return Err(GameError::NativeUnavailable(reason));
    }
    let mut prepared_triangles = native_churn_triangle_count(&initial);

    let mut input = crate::ui4::winit_input::EventLoopInput::new(context.owner);
    let mut fly_camera = FlyCamera::new(engine.camera());
    let mut first_frame = true;
    let mut frame_prepared = true;
    let mut frame_index = 0u64;
    let mut busy_retries = 0u64;
    let mut incomplete_retries = 0u64;
    let mut last_sample: Option<crate::spirit::gpu_logger::GpuLoggerSample> = None;
    let mut last_successful_publish: Option<Instant> = None;
    let mut retired_frames = Vec::new();
    let mut pending_resize: Option<PendingGameResize> = None;
    let mut next_frame = Instant::now();
    let result = loop {
        retire_frames(&mut retired_frames);
        if context.is_stopping() {
            break Ok(());
        }
        if !first_frame && !frame_prepared {
            let Some(window) = surface.window else {
                break Err(GameError::InvalidFrame);
            };
            let events = input.poll();
            if escape_pressed(window, &events) {
                context.request_stop("focused-window-escape");
                break Ok(());
            }
            apply_pendulum_key_actions(&mut engine, context.instance_id, window, &events);
            let camera_changed = fly_camera.apply_events(&events)
                | fly_camera.apply_held_keys(&input, window, 0.016);
            if pending_resize.is_none()
                && let Some((width, height)) = desired_resize(&surface)
            {
                match prepare_resize(&surface, width, height) {
                    Ok(replacement) => {
                        crate::log_info!(
                            target: "helio";
                            "helio instance={} native resize prepared example=4 window={} old={}x{} new={}x{} replacement_frame={} action=render-before-broker-swap\n",
                            context.instance_id,
                            window.raw(),
                            surface.width,
                            surface.height,
                            width,
                            height,
                            replacement.frame.raw(),
                        );
                        pending_resize = Some(replacement);
                    }
                    Err(error) => {
                        crate::log_warn!(
                            target: "helio";
                            "helio instance={} native resize deferred example=4 requested={}x{} error={:?}\n",
                            context.instance_id,
                            width,
                            height,
                            error,
                        );
                    }
                }
            }
            let (render_width, render_height) = pending_resize
                .map_or((surface.width, surface.height), |replacement| {
                    (replacement.width, replacement.height)
                });
            if camera_changed && let Err(error) = engine.set_camera(fly_camera.camera) {
                break Err(GameError::Artifact(error));
            }
            let prepared = match engine.step_instances(render_width as f32 / render_height as f32) {
                Ok(frame) => frame,
                Err(error) => break Err(GameError::Artifact(error)),
            };
            if let Err(reason) =
                crate::intel::render::update_resident_churn_forward_frame(&resident, &prepared)
            {
                break Err(GameError::Render(reason));
            }
            prepared_triangles = native_churn_triangle_count(&prepared);
            frame_prepared = true;
        }

        let (render_frame_handle, render_width, render_height) = pending_resize
            .map_or((surface.frame, surface.width, surface.height), |replacement| {
                (replacement.frame, replacement.width, replacement.height)
            });
        match render_native_churn_frame(
            render_frame_handle,
            render_width,
            render_height,
            &resident,
            TRANSPARENT_CLEAR_RGBA,
        )
        .await
        {
            Ok(rendered) => {
                if let Some(replacement) = pending_resize.take() {
                    match commit_resize(&mut surface, replacement) {
                        Ok(previous) => {
                            retired_frames.push(previous);
                            crate::log_info!(
                                target: "helio";
                                "helio instance={} native resize committed example=4 window={} extent={}x{} frame={}\n",
                                context.instance_id,
                                surface.window.map_or(0, crate::ui4::WindowId::raw),
                                surface.width,
                                surface.height,
                                surface.frame.raw(),
                            );
                        }
                        Err(error) => {
                            let _ = crate::ui4::destroy_frame(replacement.frame);
                            frame_prepared = false;
                            crate::log_warn!(
                                target: "helio";
                                "helio instance={} native resize commit rejected example=4 requested={}x{} error={:?}\n",
                                context.instance_id,
                                replacement.width,
                                replacement.height,
                                error,
                            );
                            next_frame += Duration::from_micros(NATIVE_PENDULUM_FRAME_PERIOD_US);
                            if next_frame <= Instant::now() {
                                next_frame = Instant::now();
                            }
                            Timer::at(next_frame).await;
                            continue;
                        }
                    }
                } else if let Err(error) = publish_surface_window(&mut surface) {
                    break Err(error);
                }
                frame_prepared = false;
                frame_index = frame_index.saturating_add(1);
                let published_at = Instant::now();
                let cadence_us = last_successful_publish
                    .map(|previous| published_at.saturating_duration_since(previous).as_micros())
                    .unwrap_or(0);
                last_successful_publish = Some(published_at);
                let sample = native_churn_gpu_logger_sample(
                    &rendered,
                    frame_index,
                    cadence_us,
                    engine.segment_count() + 1,
                    prepared_triangles,
                    busy_retries,
                    incomplete_retries,
                );
                publish_gpu_logger_sample(sample);
                last_sample = Some(sample);
            }
            Err(GameError::Frame(crate::ui4::FramePoolError::Busy)) => {
                busy_retries = busy_retries.saturating_add(1);
                if let Some(mut sample) = last_sample {
                    sample.busy_retries = busy_retries;
                    publish_gpu_logger_sample(sample);
                    last_sample = Some(sample);
                }
                next_frame += Duration::from_micros(NATIVE_PENDULUM_FRAME_PERIOD_US);
                if next_frame <= Instant::now() {
                    next_frame = Instant::now();
                }
                Timer::at(next_frame).await;
                continue;
            }
            Err(GameError::Render("helio-incomplete-native-instanced-frame")) => {
                incomplete_retries = incomplete_retries.saturating_add(1);
                if first_frame && incomplete_retries >= NATIVE_FIRST_FRAME_INCOMPLETE_RETRY_LIMIT {
                    break Err(GameError::NativeUnavailable("helio-native-first-frame-nonretired"));
                }
                if let Some(mut sample) = last_sample {
                    sample.incomplete_retries = incomplete_retries;
                    publish_gpu_logger_sample(sample);
                    last_sample = Some(sample);
                }
                next_frame += Duration::from_micros(NATIVE_PENDULUM_FRAME_PERIOD_US);
                if next_frame <= Instant::now() {
                    next_frame = Instant::now();
                }
                Timer::at(next_frame).await;
                continue;
            }
            Err(error) => break Err(error),
        }
        if first_frame {
            let window = surface
                .window
                .expect("published native Helio pendulum window");
            if !input.register_window(window) {
                break Err(GameError::Render("helio-input-window-capacity"));
            }
            if !context.mark_online() {
                break Ok(());
            }
            crate::log_info!(
                target: "helio";
                "helio instance={} example=4 name=pendulum-bigcloth native=1 online artifact_bytes={} segments={} instances={} draw_groups=12 geometry=one-retained-posnormal-box frame={} window={} extent={}x{} plane={} controls=WASD+Space+Shift,left-drag-look,C-pause,R-reset producer_pacing=absolute-deadline period_us={} path=helioa-pendulum-v1->compact-centers+transforms->artifact-native-vs-local-to-world-to-clip->gpu-indexed-indirect->one-guc-schedule->ui4-triple-direct cpu_vertex_projection=0 cpu_winding_repair=0 mutable_upload=camera+337-instances+compacted+indirect immutable_geometry=retained cpu_readback=0 cpu_frame_copy=0\n",
                context.instance_id,
                CHURN_FORWARD_ARTIFACT.len(),
                engine.segment_count(),
                resident.max_instances(),
                surface.frame.raw(),
                window.raw(),
                surface.width,
                surface.height,
                PLANE_SLOT,
                NATIVE_PENDULUM_FRAME_PERIOD_US,
            );
            first_frame = false;
        }
        next_frame += Duration::from_micros(NATIVE_PENDULUM_FRAME_PERIOD_US);
        if next_frame <= Instant::now() {
            next_frame = Instant::now();
        }
        Timer::at(next_frame).await;
    };
    if let Some(replacement) = pending_resize {
        crate::ui4::retire_frame_when_released(replacement.frame);
    }
    transfer_retired_frames(&mut retired_frames);
    let _ = crate::intel::render::release_resident_churn_forward(&resident);
    destroy_unpublished_surface(surface);
    result
}

async fn run_pendulum(context: InstanceContext) -> Result<(), GameError> {
    match run_pendulum_native(context).await {
        Ok(()) => Ok(()),
        Err(GameError::NativeUnavailable(reason)) => {
            crate::log_warn!(
                target: "helio";
                "helio instance={} example=4 native activation unavailable reason={} artifact_bytes={} action=compatibility-resident-vertices\n",
                context.instance_id,
                reason,
                CHURN_FORWARD_ARTIFACT.len(),
            );
            run_ported_scene(context).await
        }
        Err(error) => Err(error),
    }
}

async fn run_churn(context: InstanceContext) -> Result<(), GameError> {
    match run_churn_native(context).await {
        Ok(()) => return Ok(()),
        Err(GameError::NativeUnavailable(reason)) => {
            crate::log_warn!(
                target: "helio";
                "helio instance={} example=2 native activation unavailable reason={} artifact_bytes={} action=compatibility-resident-vertices fallback_preserves=simple-cube+legacy-churn\n",
                context.instance_id,
                reason,
                CHURN_FORWARD_ARTIFACT.len(),
            );
        }
        Err(error) => return Err(error),
    }
    let mut surface = create_surface(context)?;
    let initial_aspect = surface.width as f32 / surface.height as f32;
    let spec = match trueos_helio_runtime::churn::Spec::decode_artifact(GAME_ARTIFACT) {
        Ok(spec) => spec,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::Artifact(error));
        }
    };
    let clear_rgba = TRANSPARENT_CLEAR_RGBA;
    let mut engine = match trueos_helio_runtime::churn::Engine::new(spec) {
        Ok(engine) => engine,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::Artifact(error));
        }
    };
    if let Err(error) = engine.step(initial_aspect) {
        destroy_unpublished_surface(surface);
        return Err(GameError::Artifact(error));
    }
    let mut resident = match make_resident_batches(engine.batches()) {
        Ok(resident) => resident,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(error);
        }
    };

    let mut input = crate::ui4::winit_input::EventLoopInput::new(context.owner);
    let mut fly_camera = FlyCamera::new(engine.camera());

    let mut first_frame = true;
    let mut frame_prepared = true;
    let mut frame_index = 0u64;
    let mut busy_retries = 0u64;
    let mut incomplete_retries = 0u64;
    let mut last_sample: Option<crate::spirit::gpu_logger::GpuLoggerSample> = None;
    let mut last_successful_publish: Option<Instant> = None;
    let mut retired_frames = Vec::new();
    let mut pending_resize: Option<PendingGameResize> = None;
    let result = loop {
        retire_frames(&mut retired_frames);
        if context.is_stopping() {
            break Ok(());
        }
        if !first_frame && !frame_prepared {
            let Some(window) = surface.window else {
                break Err(GameError::InvalidFrame);
            };
            let events = input.poll();
            if escape_pressed(window, &events) {
                context.request_stop("focused-window-escape");
                break Ok(());
            }
            apply_churn_key_actions(&mut engine, context.instance_id, window, &events);
            let camera_changed = fly_camera.apply_events(&events)
                | fly_camera.apply_held_keys(&input, window, 0.033);
            if pending_resize.is_none()
                && let Some((width, height)) = desired_resize(&surface)
            {
                match prepare_resize(&surface, width, height) {
                    Ok(replacement) => {
                        crate::log_info!(
                            target: "helio";
                            "helio instance={} resize prepared example=2 window={} old={}x{} new={}x{} replacement_frame={} action=render-before-broker-swap old_front=retain-surflive\n",
                            context.instance_id,
                            window.raw(),
                            surface.width,
                            surface.height,
                            width,
                            height,
                            replacement.frame.raw(),
                        );
                        pending_resize = Some(replacement);
                    }
                    Err(error) => {
                        crate::log_warn!(
                            target: "helio";
                            "helio instance={} resize deferred example=2 window={} old={}x{} requested={}x{} error={:?} action=retain-current-frame-and-reconcile\n",
                            context.instance_id,
                            window.raw(),
                            surface.width,
                            surface.height,
                            width,
                            height,
                            error,
                        );
                    }
                }
            }
            let (render_width, render_height) = pending_resize
                .map_or((surface.width, surface.height), |replacement| {
                    (replacement.width, replacement.height)
                });
            let aspect = render_width as f32 / render_height as f32;
            if camera_changed {
                if let Err(error) = engine.set_camera(fly_camera.camera) {
                    break Err(GameError::Artifact(error));
                }
            }
            if let Err(error) = engine.step(aspect) {
                break Err(GameError::Artifact(error));
            }
            if let Err(error) = update_resident_batches(&mut resident, engine.batches()) {
                break Err(error);
            }
            frame_prepared = true;
        }
        let (render_frame_handle, render_width, render_height) = pending_resize
            .map_or((surface.frame, surface.width, surface.height), |replacement| {
                (replacement.frame, replacement.width, replacement.height)
            });
        match render_frame(render_frame_handle, render_width, render_height, &resident, clear_rgba)
            .await
        {
            Ok(rendered) => {
                if let Some(replacement) = pending_resize.take() {
                    match commit_resize(&mut surface, replacement) {
                        Ok(previous) => {
                            retired_frames.push(previous);
                            crate::log_info!(
                                target: "helio";
                                "helio instance={} resize committed example=2 window={} extent={}x{} frame={} action=broker-swap-after-first-guc-release old_release=surflive presentation=1:1-or-direct-plane-2x cpu_readback=0 cpu_frame_copy=0\n",
                                context.instance_id,
                                surface.window.map_or(0, crate::ui4::WindowId::raw),
                                surface.width,
                                surface.height,
                                surface.frame.raw(),
                            );
                        }
                        Err(error) => {
                            let _ = crate::ui4::destroy_frame(replacement.frame);
                            frame_prepared = false;
                            crate::log_warn!(
                                target: "helio";
                                "helio instance={} resize commit rejected example=2 requested={}x{} error={:?} action=old-front-restored-and-reproject\n",
                                context.instance_id,
                                replacement.width,
                                replacement.height,
                                error,
                            );
                            Timer::after(Duration::from_millis(16)).await;
                            continue;
                        }
                    }
                } else if let Err(error) = publish_surface_window(&mut surface) {
                    break Err(error);
                }
                frame_prepared = false;
                frame_index = frame_index.saturating_add(1);
                let published_at = Instant::now();
                let cadence_us = last_successful_publish
                    .map(|previous| published_at.saturating_duration_since(previous).as_micros())
                    .unwrap_or(0);
                last_successful_publish = Some(published_at);
                let sample = gpu_logger_sample(
                    &rendered,
                    frame_index,
                    cadence_us,
                    engine.active_objects(),
                    &resident,
                    busy_retries,
                    incomplete_retries,
                );
                publish_gpu_logger_sample(sample);
                last_sample = Some(sample);
            }
            Err(GameError::Frame(crate::ui4::FramePoolError::Busy)) => {
                // Triple-buffer ownership can legitimately lag one producer
                // tick. Keep these exact prepared bytes and retry after UI4
                // has had a display-release opportunity.
                busy_retries = busy_retries.saturating_add(1);
                if let Some(mut sample) = last_sample {
                    sample.busy_retries = busy_retries;
                    publish_gpu_logger_sample(sample);
                    last_sample = Some(sample);
                }
                Timer::after(Duration::from_millis(16)).await;
                continue;
            }
            Err(GameError::Render("helio-incomplete-direct-frame")) => {
                // Keep the last complete UI4 buffer visible and retry these
                // exact resident bytes. A bounded GuC poll miss must not
                // advance the game or rewrite late-read geometry.
                incomplete_retries = incomplete_retries.saturating_add(1);
                if let Some(mut sample) = last_sample {
                    sample.incomplete_retries = incomplete_retries;
                    publish_gpu_logger_sample(sample);
                    last_sample = Some(sample);
                }
                Timer::after(Duration::from_millis(16)).await;
                continue;
            }
            Err(error) => break Err(error),
        }
        if first_frame {
            let window = surface.window.expect("published Helio churn window");
            if !input.register_window(window) {
                break Err(GameError::Render("helio-input-window-capacity"));
            }
            if !context.mark_online() {
                break Ok(());
            }
            crate::log_info!(
                target: "helio";
                "helio instance={} example=2 name=churn-benchmark online artifact_bytes={} active_objects={} resident_batches={} frame={} window={} extent={}x{} plane={} background=transparent-rgba floor=none alpha=premultiplied animation_rate=1.5x lighting=churn-light-v1+24-face-batches lights=2 ambient=hemisphere controls=WASD+Space+Shift,left-drag-look,+/-,C-collision-burst producer_pacing=relative-post-work delay_ms=33 nominal_ceiling_fps=30 input=ui4-owner-broker->winit-shaped-events resize=ui4-broker->render-native-or-half-scale-ring->atomic-frame-swap->direct-plane-1x-or-2x path=helioa-v1+churn-v1+flat-light-v1->resident-batches->helio-indirect-v1->one-guc-schedule->ui4-triple-direct mutable_upload=vertices-only immutable_indices=retained cpu_readback=0 cpu_frame_copy=0\n",
                context.instance_id,
                GAME_ARTIFACT.len(),
                engine.active_objects(),
                resident.len(),
                surface.frame.raw(),
                window.raw(),
                surface.width,
                surface.height,
                PLANE_SLOT,
            );
            first_frame = false;
        }
        Timer::after(Duration::from_millis(33)).await;
    };
    if let Some(replacement) = pending_resize {
        crate::ui4::retire_frame_when_released(replacement.frame);
    }
    transfer_retired_frames(&mut retired_frames);
    release_resident_scene(resident);
    destroy_unpublished_surface(surface);
    result
}

async fn run_ported_scene(context: InstanceContext) -> Result<(), GameError> {
    let example_id = context.example_id;
    let mut surface = create_surface(context)?;
    let initial_aspect = surface.width as f32 / surface.height as f32;
    let mut engine = match PortedSceneEngine::decode(example_id) {
        Ok(engine) => engine,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(GameError::Artifact(error));
        }
    };
    if let Err(error) = engine.step(initial_aspect) {
        destroy_unpublished_surface(surface);
        return Err(GameError::Artifact(error));
    }
    let mut resident = match make_resident_batches(engine.batches()) {
        Ok(resident) => resident,
        Err(error) => {
            destroy_unpublished_surface(surface);
            return Err(error);
        }
    };

    let mut input = crate::ui4::winit_input::EventLoopInput::new(context.owner);
    let mut fly_camera = FlyCamera::new(engine.camera());
    let mut first_frame = true;
    let mut frame_prepared = true;
    let mut frame_index = 0u64;
    let mut busy_retries = 0u64;
    let mut incomplete_retries = 0u64;
    let mut last_sample: Option<crate::spirit::gpu_logger::GpuLoggerSample> = None;
    let mut last_successful_publish: Option<Instant> = None;
    let mut retired_frames = Vec::new();
    let mut pending_resize: Option<PendingGameResize> = None;
    let result = loop {
        retire_frames(&mut retired_frames);
        if context.is_stopping() {
            break Ok(());
        }
        if !first_frame && !frame_prepared {
            let Some(window) = surface.window else {
                break Err(GameError::InvalidFrame);
            };
            let events = input.poll();
            if escape_pressed(window, &events) {
                context.request_stop("focused-window-escape");
                break Ok(());
            }
            engine.apply_key_actions(context.instance_id, window, &events);
            let camera_changed = fly_camera.apply_events(&events)
                | fly_camera.apply_held_keys(&input, window, 0.033);
            if pending_resize.is_none()
                && let Some((width, height)) = desired_resize(&surface)
            {
                match prepare_resize(&surface, width, height) {
                    Ok(replacement) => {
                        crate::log_info!(
                            target: "helio";
                            "helio instance={} resize prepared example={} window={} old={}x{} new={}x{} replacement_frame={} action=render-before-broker-swap old_front=retain-surflive\n",
                            context.instance_id,
                            example_id,
                            window.raw(),
                            surface.width,
                            surface.height,
                            width,
                            height,
                            replacement.frame.raw(),
                        );
                        pending_resize = Some(replacement);
                    }
                    Err(error) => {
                        crate::log_warn!(
                            target: "helio";
                            "helio instance={} resize deferred example={} window={} old={}x{} requested={}x{} error={:?} action=retain-current-frame-and-reconcile\n",
                            context.instance_id,
                            example_id,
                            window.raw(),
                            surface.width,
                            surface.height,
                            width,
                            height,
                            error,
                        );
                    }
                }
            }
            let (render_width, render_height) = pending_resize
                .map_or((surface.width, surface.height), |replacement| {
                    (replacement.width, replacement.height)
                });
            if camera_changed && let Err(error) = engine.set_camera(fly_camera.camera) {
                break Err(GameError::Artifact(error));
            }
            if let Err(error) = engine.step(render_width as f32 / render_height as f32) {
                break Err(GameError::Artifact(error));
            }
            if let Err(error) = update_resident_batches(&mut resident, engine.batches()) {
                break Err(error);
            }
            frame_prepared = true;
        }

        let (render_frame_handle, render_width, render_height) = pending_resize
            .map_or((surface.frame, surface.width, surface.height), |replacement| {
                (replacement.frame, replacement.width, replacement.height)
            });
        match render_frame(
            render_frame_handle,
            render_width,
            render_height,
            &resident,
            TRANSPARENT_CLEAR_RGBA,
        )
        .await
        {
            Ok(rendered) => {
                if let Some(replacement) = pending_resize.take() {
                    match commit_resize(&mut surface, replacement) {
                        Ok(previous) => {
                            retired_frames.push(previous);
                            crate::log_info!(
                                target: "helio";
                                "helio instance={} resize committed example={} window={} extent={}x{} frame={} action=broker-swap-after-first-guc-release old_release=surflive presentation=1:1-or-direct-plane-2x cpu_readback=0 cpu_frame_copy=0\n",
                                context.instance_id,
                                example_id,
                                surface.window.map_or(0, crate::ui4::WindowId::raw),
                                surface.width,
                                surface.height,
                                surface.frame.raw(),
                            );
                        }
                        Err(error) => {
                            let _ = crate::ui4::destroy_frame(replacement.frame);
                            frame_prepared = false;
                            crate::log_warn!(
                                target: "helio";
                                "helio instance={} resize commit rejected example={} requested={}x{} error={:?} action=old-front-restored-and-reproject\n",
                                context.instance_id,
                                example_id,
                                replacement.width,
                                replacement.height,
                                error,
                            );
                            Timer::after(Duration::from_millis(16)).await;
                            continue;
                        }
                    }
                } else if let Err(error) = publish_surface_window(&mut surface) {
                    break Err(error);
                }
                frame_prepared = false;
                frame_index = frame_index.saturating_add(1);
                let published_at = Instant::now();
                let cadence_us = last_successful_publish
                    .map(|previous| published_at.saturating_duration_since(previous).as_micros())
                    .unwrap_or(0);
                last_successful_publish = Some(published_at);
                let sample = gpu_logger_sample(
                    &rendered,
                    frame_index,
                    cadence_us,
                    engine.object_count(),
                    &resident,
                    busy_retries,
                    incomplete_retries,
                );
                publish_gpu_logger_sample(sample);
                last_sample = Some(sample);
            }
            Err(GameError::Frame(crate::ui4::FramePoolError::Busy)) => {
                busy_retries = busy_retries.saturating_add(1);
                if let Some(mut sample) = last_sample {
                    sample.busy_retries = busy_retries;
                    publish_gpu_logger_sample(sample);
                    last_sample = Some(sample);
                }
                Timer::after(Duration::from_millis(16)).await;
                continue;
            }
            Err(GameError::Render("helio-incomplete-direct-frame")) => {
                incomplete_retries = incomplete_retries.saturating_add(1);
                if let Some(mut sample) = last_sample {
                    sample.incomplete_retries = incomplete_retries;
                    publish_gpu_logger_sample(sample);
                    last_sample = Some(sample);
                }
                Timer::after(Duration::from_millis(16)).await;
                continue;
            }
            Err(error) => break Err(error),
        }
        if first_frame {
            let window = surface.window.expect("published ported Helio window");
            if !input.register_window(window) {
                break Err(GameError::Render("helio-input-window-capacity"));
            }
            if !context.mark_online() {
                break Ok(());
            }
            crate::log_info!(
                target: "helio";
                "helio instance={} example={} name={} online artifact_bytes={} objects={} resident_batches={} frame={} window={} extent={}x{} plane={} background=transparent-rgba alpha=premultiplied input=ui4-owner-broker->winit-shaped-events controls={} resize=ui4-broker->render-native-or-half-scale-ring->atomic-frame-swap->direct-plane-1x-or-2x path=helioa-v1+scene-v1->resident-batches->helio-indirect-v1->one-guc-schedule->ui4-triple-direct cpu_readback=0 cpu_frame_copy=0\n",
                context.instance_id,
                example_id,
                engine.name(),
                GAME_ARTIFACT.len(),
                engine.object_count(),
                resident.len(),
                surface.frame.raw(),
                window.raw(),
                surface.width,
                surface.height,
                PLANE_SLOT,
                engine.controls(),
            );
            first_frame = false;
        }
        Timer::after(Duration::from_millis(33)).await;
    };
    if let Some(replacement) = pending_resize {
        crate::ui4::retire_frame_when_released(replacement.frame);
    }
    transfer_retired_frames(&mut retired_frames);
    release_resident_scene(resident);
    destroy_unpublished_surface(surface);
    result
}

#[embassy_executor::task(pool_size = INSTANCE_CAPACITY)]
async fn helio_game_instance_task(context: InstanceContext) {
    let mut last_error = None;
    loop {
        if context.is_stopping() {
            break;
        }
        let current_slot = crate::percpu::current_slot() as u32;
        if current_slot != context.cpu_carrier.worker_slot {
            if !context.mark_starting_error("helio-cpu-carrier-mismatch") {
                break;
            }
            crate::log_warn!(target: "helio";
                "helio instance={} example={} cpu carrier mismatch cpu_carrier={} assigned_slot={} current_slot={} action=refuse-wrong-executor\n",
                context.instance_id,
                context.example_id,
                context.cpu_carrier.id,
                context.cpu_carrier.worker_slot,
                current_slot,
            );
            Timer::after(Duration::from_millis(250)).await;
            continue;
        }
        let result = match context.example_id {
            1 => run_cube(context).await,
            2 => run_churn(context).await,
            3 => run_ported_scene(context).await,
            4 => run_pendulum(context).await,
            _ => Err(GameError::Render("helio-example-reserved")),
        };
        match result {
            Ok(()) => break,
            Err(error) => {
                if !context.mark_starting_error(error.label()) {
                    break;
                }
                if last_error != Some(core::mem::discriminant(&error)) {
                    crate::log_warn!(
                        target: "helio";
                        "helio instance={} example={} start pending error={:?} action=retry\n",
                        context.instance_id,
                        context.example_id,
                        error,
                    );
                    last_error = Some(core::mem::discriminant(&error));
                }
                Timer::after(Duration::from_millis(250)).await;
            }
        }
    }
    crate::log_info!(target: "helio";
        "helio instance={} example={} offline cpu_carrier={} worker_slot={} core_kind={} action=pool-slot-release\n",
        context.instance_id,
        context.example_id,
        context.cpu_carrier.id,
        context.cpu_carrier.worker_slot,
        context.cpu_carrier.core_kind,
    );
    release_instance_slot(context);
}

/// Dispatches numbered Shell2/Blueprint requests into ten independent Helio
/// tasks deterministically sharded across up to three AP2+ executors.
#[embassy_executor::task(pool_size = CPU_CARRIER_CAPACITY)]
pub async fn helio_game_service_task(
    cpu_carrier_id: u8,
    carrier_count: u8,
    worker_slot: u32,
    core_kind: u8,
) {
    let cpu_carrier = CpuCarrier {
        id: cpu_carrier_id,
        worker_slot,
        core_kind,
    };
    let mut placement_warning_logged = false;
    let mut bootstrap_warning_logged = false;
    loop {
        let current_slot = crate::percpu::current_slot() as u32;
        let current_core_kind = crate::workers::core_kind_for_slot(current_slot);
        let placement_valid = current_slot == worker_slot
            && current_core_kind == core_kind
            && crate::workers::is_general_background_worker_slot(current_slot);
        if placement_valid && report_cpu_carrier_online(cpu_carrier, carrier_count) {
            if placement_warning_logged || bootstrap_warning_logged {
                crate::log_info!(target: "helio";
                    "helio cpu carrier={} validation recovered assigned_slot={} current_slot={} core_kind={} action=join-carrier-barrier\n",
                    cpu_carrier_id,
                    worker_slot,
                    current_slot,
                    core_kind,
                );
            }
            break;
        }
        if !placement_valid && !placement_warning_logged {
            crate::log_warn!(target: "helio";
                "helio cpu carrier={} placement invalid assigned_slot={} current_slot={} assigned_core_kind={} current_core_kind={} expected=background-ap2+ action=retry-before-claim registry=withheld\n",
                cpu_carrier_id,
                worker_slot,
                current_slot,
                core_kind,
                current_core_kind,
            );
            placement_warning_logged = true;
        } else if placement_valid && !bootstrap_warning_logged {
            crate::log_warn!(target: "helio";
                "helio cpu carrier={} bootstrap admission pending carrier_count={} worker_slot={} core_kind={} action=retry-before-claim registry=withheld\n",
                cpu_carrier_id,
                carrier_count,
                worker_slot,
                core_kind,
            );
            bootstrap_warning_logged = true;
        }
        Timer::after(Duration::from_millis(250)).await;
    }

    while !cpu_carrier_registry_published(cpu_carrier, carrier_count) {
        Timer::after(Duration::from_millis(10)).await;
    }
    let current_slot = crate::percpu::current_slot() as u32;
    crate::log_info!(target: "helio";
        "helio cpu carrier={} online carrier_count={} worker_slot={} current_slot={} core_kind={} placement=background-ap2+ sharding=instance-id-mod-carrier-count gpu_principal=render0 gpu_context=shared-single-render-runtime gpu_affinity=none\n",
        cpu_carrier_id,
        carrier_count,
        worker_slot,
        current_slot,
        core_kind,
    );
    let spawner: embassy_executor::Spawner =
        unsafe { embassy_executor::Spawner::for_current_executor().await };
    loop {
        while let Some(context) = claim_next_launch(cpu_carrier, carrier_count) {
            match helio_game_instance_task(context) {
                Ok(token) => {
                    spawner.spawn(token);
                    crate::log_info!(target: "helio";
                        "helio instance={} example={} launch dispatched pool_slot={}/{} owner=kernel-app-{} cpu_carrier={} worker_slot={} core_kind={} executor=current-background-worker gpu_principal=render0 gpu_context=shared-single-render-runtime\n",
                        context.instance_id,
                        context.example_id,
                        context.slot + 1,
                        INSTANCE_CAPACITY,
                        HELIO_OWNER_BASE + context.slot as u8,
                        context.cpu_carrier.id,
                        context.cpu_carrier.worker_slot,
                        context.cpu_carrier.core_kind,
                    );
                }
                Err(error) => {
                    requeue_failed_spawn(context);
                    crate::log_warn!(target: "helio";
                        "helio instance={} example={} dispatch deferred cpu_carrier={} worker_slot={} error={:?} action=requeue-same-shard\n",
                        context.instance_id,
                        context.example_id,
                        context.cpu_carrier.id,
                        context.cpu_carrier.worker_slot,
                        error,
                    );
                    break;
                }
            }
        }
        Timer::after(Duration::from_millis(10)).await;
    }
}
