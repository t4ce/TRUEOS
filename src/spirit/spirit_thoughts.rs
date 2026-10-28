//! Small, owner-bound agent presences around the one resident Lilly.
//!
//! Codex supplies facts, not cursor programs. This module retains their current
//! presentation, lays out nonoverlapping resting places, and lends visual-only
//! glyph cursors to the existing mouse-motion station. It never focuses a frame,
//! sends a key, or changes Lilly's animation or VFX policy.

extern crate alloc;

use alloc::string::String;
use heapless::Vec;
use v::spirit_agents::{AgentEntry, AgentLifecycle, AgentRuntime, AgentSnapshot};

const CAPACITY: usize = v::spirit_agents::MAX_AGENTS;
const SOURCE_CAPACITY: usize = crate::allcaps::hv::VM_ID_LIMIT;
const MARKER_SIDE: i32 = 16;
const MARKER_GAP: i32 = 6;
const SHUTDOWN_DWELL_MS: u64 = 1_200;
const ORBIT_PERIOD_MS: u64 = 24_000;

pub(crate) const ERROR_INVALID: i32 = -3;
pub(crate) const ERROR_CAPACITY: i32 = -4;
pub(crate) const ERROR_STALE: i32 = -9;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Owner {
    vm: u8,
    run: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Appearance {
    Hidden,
    Initializing,
    Running,
    Waiting,
    Complete,
    Error,
    Interrupted,
    Shutdown,
}

impl Appearance {
    fn from_agent(agent: &AgentEntry) -> Self {
        if agent.waiting_approval || agent.waiting_input {
            return Self::Waiting;
        }
        if agent.runtime == Some(AgentRuntime::SystemError) {
            return Self::Error;
        }
        if agent.runtime == Some(AgentRuntime::Active) {
            return Self::Running;
        }
        match agent.lifecycle {
            Some(AgentLifecycle::Completed) => Self::Complete,
            Some(AgentLifecycle::Errored) => Self::Error,
            Some(AgentLifecycle::Interrupted) => Self::Interrupted,
            Some(AgentLifecycle::Shutdown) => Self::Shutdown,
            Some(AgentLifecycle::NotFound) => Self::Hidden,
            Some(AgentLifecycle::PendingInit) => Self::Initializing,
            Some(AgentLifecycle::Running) if agent.loaded => Self::Running,
            // Unloaded, idle and unknown are not evidence of completion.
            _ => Self::Hidden,
        }
    }

    fn rgba(self) -> [u8; 4] {
        match self {
            Self::Hidden | Self::Initializing | Self::Running => [0, 0, 0, 255],
            Self::Waiting => [32, 128, 255, 255],
            Self::Complete => [32, 208, 112, 255],
            Self::Error => [255, 64, 64, 255],
            Self::Interrupted => [255, 160, 32, 255],
            Self::Shutdown => [255, 255, 255, 255],
        }
    }
}

#[derive(Clone)]
struct Thought {
    owner: Owner,
    epoch: u64,
    agent: AgentEntry,
    appearance: Appearance,
    shutdown_at_ms: Option<u64>,
    resting: Option<Point>,
    phase_slot: usize,
}

impl Thought {
    fn update(&mut self, agent: AgentEntry, now_ms: u64) {
        let appearance = Appearance::from_agent(&agent);
        if appearance != self.appearance {
            self.shutdown_at_ms = (appearance == Appearance::Shutdown)
                .then(|| now_ms.saturating_add(SHUTDOWN_DWELL_MS));
        }
        self.appearance = appearance;
        self.agent = agent;
    }

    fn visible(&self, now_ms: u64) -> bool {
        self.appearance != Appearance::Hidden
            && self.shutdown_at_ms.is_none_or(|deadline| now_ms < deadline)
    }
}

#[derive(Clone, Copy)]
struct Source {
    owner: Owner,
    epoch: u64,
    revision: u64,
    connected: bool,
}

struct Thoughts {
    sources: Vec<Source, SOURCE_CAPACITY>,
    agents: Vec<Thought, CAPACITY>,
}

impl Thoughts {
    const fn new() -> Self {
        Self {
            sources: Vec::new(),
            agents: Vec::new(),
        }
    }

    fn ingest(&mut self, owner: Owner, snapshot: AgentSnapshot, now_ms: u64) -> Result<(), i32> {
        if usize::from(owner.vm) >= SOURCE_CAPACITY {
            return Err(ERROR_INVALID);
        }
        snapshot.validate().map_err(|error| match error {
            v::spirit_agents::SnapshotError::Invalid => ERROR_INVALID,
            v::spirit_agents::SnapshotError::Capacity => ERROR_CAPACITY,
        })?;
        if snapshot.agents.len() > CAPACITY
            || snapshot
                .agents
                .iter()
                .any(|agent| agent.id.is_empty() || agent.parent_id.is_none())
            || snapshot.agents.iter().enumerate().any(|(index, agent)| {
                snapshot.agents[..index]
                    .iter()
                    .any(|previous| previous.id == agent.id)
            })
        {
            return Err(ERROR_INVALID);
        }
        // Exactly one source per VM slot, independently of the global glyph
        // budget. An empty publisher must not consume an agent's visual slot.
        let previous = self
            .sources
            .iter()
            .position(|source| source.owner.vm == owner.vm);
        if let Some(index) = previous {
            let source = self.sources[index];
            if owner.run < source.owner.run {
                return Err(ERROR_STALE);
            }
            if owner != source.owner && !snapshot.complete {
                return Err(ERROR_INVALID);
            }
            if owner == source.owner
                && (snapshot.epoch < source.epoch
                    || (snapshot.epoch == source.epoch && snapshot.revision < source.revision))
            {
                return Err(ERROR_STALE);
            }
            if owner == source.owner
                && snapshot.epoch == source.epoch
                && snapshot.revision == source.revision
            {
                return Ok(());
            }
            if owner == source.owner && snapshot.epoch != source.epoch && !snapshot.complete {
                return Err(ERROR_INVALID);
            }
        } else if self.sources.is_full() {
            return Err(ERROR_CAPACITY);
        } else if !snapshot.complete {
            return Err(ERROR_INVALID);
        }

        // Validate the complete resulting cardinality before changing anything.
        // An incomplete snapshot merges only what it knows; omission is not death.
        let retained = self
            .agents
            .iter()
            .filter(|thought| {
                thought.owner.vm != owner.vm
                    || (thought.owner == owner
                        && thought.epoch == snapshot.epoch
                        && !snapshot.complete
                        && !snapshot
                            .agents
                            .iter()
                            .any(|agent| agent.id == thought.agent.id))
            })
            .count();
        if retained.saturating_add(snapshot.agents.len()) > CAPACITY {
            return Err(ERROR_CAPACITY);
        }

        self.agents.retain(|thought| {
            thought.owner.vm != owner.vm
                || (thought.owner == owner
                    && thought.epoch == snapshot.epoch
                    && (!snapshot.complete
                        || snapshot
                            .agents
                            .iter()
                            .any(|agent| agent.id == thought.agent.id)))
        });
        let source = Source {
            owner,
            epoch: snapshot.epoch,
            revision: snapshot.revision,
            connected: snapshot.connected,
        };
        if let Some(index) = previous {
            self.sources[index] = source;
        } else {
            self.sources.push(source).map_err(|_| ERROR_CAPACITY)?;
        }
        for agent in snapshot.agents {
            if let Some(thought) = self.agents.iter_mut().find(|thought| {
                thought.owner == owner
                    && thought.epoch == snapshot.epoch
                    && thought.agent.id == agent.id
            }) {
                thought.update(agent, now_ms);
            } else {
                let appearance = Appearance::from_agent(&agent);
                let phase_slot = (0..CAPACITY)
                    .find(|slot| {
                        self.agents
                            .iter()
                            .all(|thought| thought.phase_slot != *slot)
                    })
                    .ok_or(ERROR_CAPACITY)?;
                self.agents
                    .push(Thought {
                        owner,
                        epoch: snapshot.epoch,
                        agent,
                        appearance,
                        shutdown_at_ms: (appearance == Appearance::Shutdown)
                            .then(|| now_ms.saturating_add(SHUTDOWN_DWELL_MS)),
                        resting: None,
                        phase_slot,
                    })
                    .map_err(|_| ERROR_CAPACITY)?;
            }
        }
        Ok(())
    }

    fn release(&mut self, owner: Owner) {
        self.agents.retain(|thought| thought.owner != owner);
        self.sources.retain(|source| source.owner != owner);
    }

    fn connected(&self, thought: &Thought) -> bool {
        self.sources.iter().any(|source| {
            source.owner == thought.owner && source.epoch == thought.epoch && source.connected
        })
    }

    fn plan(&mut self, geometry: Geometry, now_ms: u64) -> Vec<Placement, CAPACITY> {
        let mut occupied = Vec::<Point, CAPACITY>::new();
        let mut slots = [None; CAPACITY];
        // Keep still-valid resting positions. New agents do not reshuffle old ones.
        for (index, thought) in self.agents.iter().enumerate() {
            if self.connected(thought)
                && thought.visible(now_ms)
                && let Some(point) = thought.resting
                && geometry.valid_resting(point)
                && occupied.iter().all(|other| !markers_overlap(point, *other))
            {
                let _ = occupied.push(point);
                slots[index] = Some(point);
            }
        }
        for (index, thought) in self.agents.iter().enumerate() {
            if slots[index].is_none()
                && self.connected(thought)
                && thought.visible(now_ms)
                && let Some(point) = geometry.find_resting(&occupied)
            {
                let _ = occupied.push(point);
                slots[index] = Some(point);
            }
        }
        let running_requested = self
            .agents
            .iter()
            .enumerate()
            .filter(|(index, thought)| {
                slots[*index].is_some() && thought.appearance == Appearance::Running
            })
            .count();
        let track = if running_requested == 0 {
            None
        } else {
            let outer_ring = slots
                .iter()
                .flatten()
                .filter_map(|point| geometry.resting_ring(*point))
                .max()
                .unwrap_or(0);
            geometry.track(outer_ring)
        };
        let running = running_requested.min(track.map_or(0, Track::capacity));
        let mut running_slot = 0;
        let mut placements = Vec::new();
        for (index, thought) in self.agents.iter_mut().enumerate() {
            thought.resting = slots[index];
            let Some(resting) = thought.resting else {
                continue;
            };
            let orbit = if thought.appearance == Appearance::Running && running_slot < running {
                let slot = running_slot;
                running_slot += 1;
                track.map(|track| Motion {
                    track,
                    slot,
                    count: running,
                    phase_slot: thought.phase_slot,
                })
            } else {
                None
            };
            let _ = placements.push(Placement {
                owner: thought.owner,
                epoch: thought.epoch,
                id: thought.agent.id.clone(),
                color: thought.appearance.rgba(),
                resting,
                orbit,
            });
        }
        placements
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Geometry {
    screen_width: u32,
    screen_height: u32,
    lilly_x: i32,
    lilly_y: i32,
    lilly_width: u32,
    lilly_height: u32,
}

impl Geometry {
    fn screen_contains(self, point: Point) -> bool {
        if self.screen_width > i32::MAX as u32 || self.screen_height > i32::MAX as u32 {
            return false;
        }
        let half = i64::from((MARKER_SIDE + 1) / 2);
        i64::from(point.x) - half >= 0
            && i64::from(point.y) - half >= 0
            && i64::from(point.x) + half <= i64::from(self.screen_width)
            && i64::from(point.y) + half <= i64::from(self.screen_height)
    }

    fn valid_resting(self, point: Point) -> bool {
        self.resting_ring(point).is_some()
    }

    fn resting_ring(self, point: Point) -> Option<usize> {
        if !self.screen_contains(point) {
            return None;
        }
        let x = i64::from(point.x);
        let y = i64::from(point.y);
        let left = i64::from(self.lilly_x);
        let top = i64::from(self.lilly_y);
        let right = left + i64::from(self.lilly_width);
        let bottom = top + i64::from(self.lilly_height);
        (0..CAPACITY).find(|ring| {
            let half = i64::from((MARKER_SIDE + 1) / 2 + MARKER_GAP)
                + *ring as i64 * i64::from(MARKER_SIDE + MARKER_GAP);
            let on_side =
                (x == left - half || x == right + half) && y >= top - half && y <= bottom + half;
            let on_edge =
                (y == top - half || y == bottom + half) && x >= left - half && x <= right + half;
            on_side || on_edge
        })
    }

    fn find_resting(self, occupied: &Vec<Point, CAPACITY>) -> Option<Point> {
        let left = i64::from(self.lilly_x);
        let top = i64::from(self.lilly_y);
        let right = left + i64::from(self.lilly_width);
        let bottom = top + i64::from(self.lilly_height);
        let step = i64::from(MARKER_SIDE + MARKER_GAP);
        // At most 32 candidates per side: bounded even for malformed huge geometry.
        // Start around the side centers so small groups surround Lilly naturally.
        for ring in 0..CAPACITY {
            let half = i64::from((MARKER_SIDE + 1) / 2 + MARKER_GAP) + ring as i64 * step;
            for distance in 0..CAPACITY {
                for sign in [1_i64, -1] {
                    if distance == 0 && sign == -1 {
                        continue;
                    }
                    let offset = distance as i64 * step * sign;
                    let center_x = left + i64::from(self.lilly_width) / 2 + offset;
                    let center_y = top + i64::from(self.lilly_height) / 2 + offset;
                    for (x, y) in [
                        (center_x, top - half),
                        (right + half, center_y),
                        (center_x, bottom + half),
                        (left - half, center_y),
                    ] {
                        let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) else {
                            continue;
                        };
                        let point = Point { x, y };
                        if self.resting_ring(point) == Some(ring)
                            && occupied.iter().all(|other| !markers_overlap(point, *other))
                        {
                            return Some(point);
                        }
                    }
                }
            }
        }
        None
    }

    fn track(self, outer_ring: usize) -> Option<Track> {
        let center = Point {
            x: i32::try_from(i64::from(self.lilly_x) + i64::from(self.lilly_width) / 2).ok()?,
            y: i32::try_from(i64::from(self.lilly_y) + i64::from(self.lilly_height) / 2).ok()?,
        };
        let half = f64::from((MARKER_SIDE + 1) / 2 + MARKER_GAP)
            + outer_ring as f64 * f64::from(MARKER_SIDE + MARKER_GAP);
        // Clear the entire resting rectangle, including its corners. The orbit
        // never crosses Lilly or a stationary request/result marker.
        let rx = f64::from(self.lilly_width) / 2.0 + half;
        let ry = f64::from(self.lilly_height) / 2.0 + half;
        let radius = libm::ceil(
            (libm::sqrt(rx * rx + ry * ry) + f64::from(MARKER_SIDE + MARKER_GAP)).max(
                core::f64::consts::SQRT_2 * f64::from(MARKER_SIDE + MARKER_GAP)
                    / (2.0 * libm::sin(core::f64::consts::PI / CAPACITY as f64)),
            ),
        ) as i32;
        let orbit = Orbit { center, radius };
        // Sampling identifies the longest contiguous visible part of the same
        // circle. At a display edge the markers share this arc and reverse
        // together; they never squeeze Lilly or cross one another head-on.
        let mut visible = [false; TRACK_STEPS];
        for (step, valid) in visible.iter_mut().enumerate() {
            *valid = self.screen_contains(orbit.point(step as f64 * TRACK_STEP_ANGLE));
        }
        if visible.iter().all(|valid| *valid) {
            return Some(Track {
                orbit,
                start_step: 0,
                span_steps: TRACK_STEPS as u16,
            });
        }
        let first_hidden = visible.iter().position(|valid| !valid)?;
        let mut run = 0;
        let mut best = (0, 0);
        for offset in 1..=TRACK_STEPS {
            let step = (first_hidden + offset) % TRACK_STEPS;
            if visible[step] {
                run += 1;
                if run > best.1 {
                    best = ((step + TRACK_STEPS + 1 - run) % TRACK_STEPS, run);
                }
            } else {
                run = 0;
            }
        }
        // Keep one sample of clearance at either endpoint for rounded cubic
        // controls and the glyph's centered pixel envelope.
        if best.1 < 5 {
            return None;
        }
        Some(Track {
            orbit,
            start_step: ((best.0 + 1) % TRACK_STEPS) as u16,
            span_steps: (best.1 - 3) as u16,
        })
    }
}

fn markers_overlap(left: Point, right: Point) -> bool {
    let spacing = i64::from(MARKER_SIDE + MARKER_GAP);
    (i64::from(left.x) - i64::from(right.x)).abs() < spacing
        && (i64::from(left.y) - i64::from(right.y)).abs() < spacing
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Orbit {
    center: Point,
    radius: i32,
}

impl Orbit {
    fn point(self, angle: f64) -> Point {
        Point {
            x: (f64::from(self.center.x) + libm::round(f64::from(self.radius) * libm::cos(angle)))
                as i32,
            y: (f64::from(self.center.y) + libm::round(f64::from(self.radius) * libm::sin(angle)))
                as i32,
        }
    }
}

const TRACK_STEPS: usize = 256;
const TRACK_STEP_ANGLE: f64 = core::f64::consts::TAU / TRACK_STEPS as f64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Track {
    orbit: Orbit,
    start_step: u16,
    span_steps: u16,
}

impl Track {
    fn full(self) -> bool {
        self.span_steps as usize == TRACK_STEPS
    }

    fn spacing(self) -> f64 {
        // Chord distance is conservative for the square marker envelope.
        2.0 * libm::asin(
            core::f64::consts::SQRT_2 * f64::from(MARKER_SIDE + MARKER_GAP + 2)
                / (2.0 * f64::from(self.orbit.radius)),
        )
    }

    fn capacity(self) -> usize {
        let span = f64::from(self.span_steps) * TRACK_STEP_ANGLE;
        let available = if self.full() {
            span / self.spacing()
        } else {
            1.0 + span / self.spacing()
        };
        (available as usize).min(CAPACITY)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Motion {
    track: Track,
    slot: usize,
    count: usize,
    phase_slot: usize,
}

impl Motion {
    /// Current angle and the next common turnaround. The partial-arc train
    /// reverses as one group, preserving every neighbor's spacing.
    fn segment(self, now_ms: u64) -> (f64, f64, u32) {
        if self.track.full() {
            let angle = core::f64::consts::TAU
                * ((now_ms % ORBIT_PERIOD_MS) as f64 / ORBIT_PERIOD_MS as f64
                    + self.phase_slot as f64 / CAPACITY as f64);
            return (
                angle,
                angle + core::f64::consts::FRAC_PI_2,
                (ORBIT_PERIOD_MS / 4) as u32,
            );
        }
        let start = f64::from(self.track.start_step) * TRACK_STEP_ANGLE
            + self.slot as f64 * self.track.spacing();
        let travel = (f64::from(self.track.span_steps) * TRACK_STEP_ANGLE
            - self.count.saturating_sub(1) as f64 * self.track.spacing())
        .max(0.0);
        let half = ORBIT_PERIOD_MS / 2;
        let phase = now_ms % ORBIT_PERIOD_MS;
        let (fraction, target, remaining) = if phase < half {
            (phase as f64 / half as f64, 1.0, half - phase)
        } else {
            (
                (ORBIT_PERIOD_MS - phase) as f64 / half as f64,
                0.0,
                ORBIT_PERIOD_MS - phase,
            )
        };
        (
            start + travel * fraction,
            start + travel * target,
            remaining.max(8) as u32,
        )
    }

    fn curves(self, now_ms: u64) -> (Point, Vec<Curve, 4>) {
        let (angle, end_angle, duration_ms) = self.segment(now_ms);
        let count = (libm::ceil((end_angle - angle).abs() / core::f64::consts::FRAC_PI_2) as usize)
            .clamp(1, 4);
        let step = (end_angle - angle) / count as f64;
        let mut curves = Vec::new();
        for index in 0..count {
            let from_angle = angle + index as f64 * step;
            let to_angle = from_angle + step;
            let from = self.track.orbit.point(from_angle);
            let to = self.track.orbit.point(to_angle);
            let tangent = f64::from(self.track.orbit.radius) * (4.0 / 3.0) * libm::tan(step / 4.0);
            let _ = curves.push(Curve {
                from,
                to,
                duration_ms: (duration_ms / count as u32).max(8),
                control1: Point {
                    x: from.x - libm::round(tangent * libm::sin(from_angle)) as i32,
                    y: from.y + libm::round(tangent * libm::cos(from_angle)) as i32,
                },
                control2: Point {
                    x: to.x + libm::round(tangent * libm::sin(to_angle)) as i32,
                    y: to.y - libm::round(tangent * libm::cos(to_angle)) as i32,
                },
            });
        }
        (self.track.orbit.point(angle), curves)
    }
}

#[derive(Clone, Copy)]
struct Curve {
    from: Point,
    to: Point,
    control1: Point,
    control2: Point,
    duration_ms: u32,
}

#[derive(Clone)]
struct Placement {
    owner: Owner,
    epoch: u64,
    id: String,
    color: [u8; 4],
    resting: Point,
    orbit: Option<Motion>,
}

#[cfg(not(test))]
mod service {
    use super::*;
    use crate::graphics::primitives::Rgba8;
    use crate::r::services::mouse_motion_service::{
        self as motion, MOUSE_CONTROL_EASING_LINEAR, MOUSE_CONTROL_FLAG_CLEAR_QUEUE,
        MOUSE_CONTROL_OPCODE_STROKE, MOUSE_CONTROL_OPCODE_TELEPORT, MOUSE_CONTROL_PATH_CUBIC,
        MouseControlCommand, MouseControlCursor, MouseControlPrincipal,
    };
    use spin::Mutex;
    use trueos_time::{Duration, Instant, Timer};

    const PRINCIPAL: MouseControlPrincipal = MouseControlPrincipal::KernelApp(0x53);
    static THOUGHTS: Mutex<Thoughts> = Mutex::new(Thoughts::new());

    pub(crate) fn ingest(vm: u8, run: u64, snapshot: AgentSnapshot) -> Result<(), i32> {
        THOUGHTS
            .lock()
            .ingest(Owner { vm, run }, snapshot, Instant::now().as_millis())
    }

    fn release_owner(vm: u8, run: u64) {
        THOUGHTS.lock().release(Owner { vm, run });
    }

    fn prune_dead_owners() {
        // Query HV without holding the presentation lock. A reused VM slot
        // cannot inherit its predecessor's source or glyph capabilities.
        let sources = THOUGHTS.lock().sources.clone();
        for source in sources {
            let state = crate::hv::vm_state(source.owner.vm);
            if !state.supported
                || !state.running
                || state.stop_requested
                || crate::hv::vm_run_generation(source.owner.vm) != Some(source.owner.run)
                || crate::hv::blueprint_instance_identity(source.owner.vm).is_none()
            {
                release_owner(source.owner.vm, source.owner.run);
            }
        }
    }

    struct Cursor {
        owner: Owner,
        epoch: u64,
        id: String,
        cursor: MouseControlCursor,
        resting: Point,
        orbit: Option<Motion>,
    }

    fn same(cursor: &Cursor, placement: &Placement) -> bool {
        cursor.owner == placement.owner
            && cursor.epoch == placement.epoch
            && cursor.id == placement.id
    }

    fn teleport(point: Point) -> MouseControlCommand {
        MouseControlCommand {
            opcode: MOUSE_CONTROL_OPCODE_TELEPORT,
            flags: MOUSE_CONTROL_FLAG_CLEAR_QUEUE,
            x: point.x,
            y: point.y,
            ..MouseControlCommand::default()
        }
    }

    fn orbit_program(motion: Motion, now_ms: u64) -> Vec<MouseControlCommand, 5> {
        let (from, curves) = motion.curves(now_ms);
        let mut program = Vec::new();
        let _ = program.push(teleport(from));
        for curve in curves {
            let _ = program.push(MouseControlCommand {
                opcode: MOUSE_CONTROL_OPCODE_STROKE,
                path: MOUSE_CONTROL_PATH_CUBIC,
                easing: MOUSE_CONTROL_EASING_LINEAR,
                duration_ms: curve.duration_ms,
                x: curve.to.x,
                y: curve.to.y,
                control1_x: curve.control1.x,
                control1_y: curve.control1.y,
                control2_x: curve.control2.x,
                control2_y: curve.control2.y,
                ..MouseControlCommand::default()
            });
        }
        program
    }

    #[trueos_executor::task(pool_size = 1)]
    pub(crate) async fn spirit_thoughts_task() {
        let mut cursors = Vec::<Cursor, CAPACITY>::new();
        let mut hidden_count = 0;
        loop {
            prune_dead_owners();
            let now_ms = Instant::now().as_millis();
            let placements = if let Some((
                screen_width,
                screen_height,
                lilly_x,
                lilly_y,
                lilly_width,
                lilly_height,
            )) = super::super::marker_anchor()
            {
                THOUGHTS.lock().plan(
                    Geometry {
                        screen_width,
                        screen_height,
                        lilly_x,
                        lilly_y,
                        lilly_width,
                        lilly_height,
                    },
                    now_ms,
                )
            } else {
                Vec::new()
            };

            // Capability release happens outside the ingress/model lock.
            let mut index = 0;
            while index < cursors.len() {
                if placements
                    .iter()
                    .any(|placement| same(&cursors[index], placement))
                {
                    index += 1;
                } else {
                    let cursor = cursors.remove(index);
                    let _ = motion::release_cursor(PRINCIPAL, cursor.cursor.handle);
                }
            }
            for placement in &placements {
                let cursor_index = match cursors.iter().position(|cursor| same(cursor, placement)) {
                    Some(index) => index,
                    None => {
                        let [r, g, b, a] = placement.color;
                        let Ok(cursor) = motion::request_glyph_cursor(
                            PRINCIPAL,
                            "Spirit/agent",
                            '§',
                            MARKER_SIDE as u16,
                            Rgba8::new(r, g, b, a),
                        ) else {
                            continue;
                        };
                        if cursors
                            .push(Cursor {
                                owner: placement.owner,
                                epoch: placement.epoch,
                                id: placement.id.clone(),
                                cursor,
                                resting: Point {
                                    x: i32::MIN,
                                    y: i32::MIN,
                                },
                                orbit: None,
                            })
                            .is_err()
                        {
                            let _ = motion::release_cursor(PRINCIPAL, cursor.handle);
                            continue;
                        }
                        cursors.len() - 1
                    }
                };
                let [r, g, b, a] = placement.color;
                if motion::set_cursor_visual_color(
                    PRINCIPAL,
                    cursors[cursor_index].cursor.handle,
                    Rgba8::new(r, g, b, a),
                )
                .is_err()
                {
                    // An external principal teardown may already have retired
                    // this capability. Recreate through the station next tick.
                    let stale = cursors.remove(cursor_index);
                    let _ = motion::release_cursor(PRINCIPAL, stale.cursor.handle);
                    continue;
                }
                let cursor = &mut cursors[cursor_index];
                let changed =
                    cursor.resting != placement.resting || cursor.orbit != placement.orbit;
                let result = if let Some(orbit) = placement.orbit {
                    if changed
                        || motion::cursor_is_idle(PRINCIPAL, cursor.cursor.handle).unwrap_or(false)
                    {
                        motion::submit_program(
                            PRINCIPAL,
                            cursor.cursor.handle,
                            &orbit_program(orbit, now_ms),
                        )
                    } else {
                        Ok(())
                    }
                } else if changed {
                    motion::submit_command(
                        PRINCIPAL,
                        cursor.cursor.handle,
                        teleport(placement.resting),
                    )
                } else {
                    Ok(())
                };
                if result.is_ok() {
                    cursor.resting = placement.resting;
                    cursor.orbit = placement.orbit;
                }
            }
            let suppressed = {
                let thoughts = THOUGHTS.lock();
                thoughts
                    .agents
                    .iter()
                    .filter(|thought| thoughts.connected(thought) && thought.visible(now_ms))
                    .count()
                    .saturating_sub(placements.len())
            };
            if suppressed != hidden_count {
                hidden_count = suppressed;
                crate::log_info!(target: "gfx";
                    "trueos-spirit: agent markers visible={} space_deferred={} capacity={} glyph=section owner=spirit presentation=ui4-slot4 input=none\n",
                    cursors.len(), suppressed, CAPACITY);
            }
            Timer::after(Duration::from_millis(100)).await;
        }
    }
}

#[cfg(not(test))]
pub(crate) use service::{ingest, spirit_thoughts_task};

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, vec, vec::Vec as AllocVec};
    use v::spirit_agents::SNAPSHOT_VERSION;

    const OWNER: Owner = Owner { vm: 2, run: 7 };

    fn agent(id: &str, lifecycle: AgentLifecycle) -> AgentEntry {
        AgentEntry {
            id: id.into(),
            parent_id: Some("root".into()),
            name: None,
            role: None,
            lifecycle: Some(lifecycle),
            runtime: None,
            waiting_approval: false,
            waiting_input: false,
            loaded: true,
        }
    }

    fn snapshot(revision: u64, agents: AllocVec<AgentEntry>) -> AgentSnapshot {
        AgentSnapshot {
            version: SNAPSHOT_VERSION,
            epoch: 1,
            revision,
            complete: true,
            connected: true,
            agents,
        }
    }

    fn geometry(x: i32, y: i32) -> Geometry {
        Geometry {
            screen_width: 1920,
            screen_height: 1080,
            lilly_x: x,
            lilly_y: y,
            lilly_width: 256,
            lilly_height: 256,
        }
    }

    fn point_for(placement: &Placement, now_ms: u64) -> Point {
        placement.orbit.map_or(placement.resting, |motion| {
            motion.track.orbit.point(motion.segment(now_ms).0)
        })
    }

    fn check_layout(placements: &Vec<Placement, CAPACITY>, geometry: Geometry, now_ms: u64) {
        let mut points = AllocVec::new();
        for placement in placements {
            let point = point_for(placement, now_ms);
            assert!(
                geometry.screen_contains(point),
                "outside screen: {point:?} at {geometry:?}"
            );
            let half = MARKER_SIDE / 2;
            assert!(
                point.x + half <= geometry.lilly_x
                    || point.y + half <= geometry.lilly_y
                    || point.x - half >= geometry.lilly_x + geometry.lilly_width as i32
                    || point.y - half >= geometry.lilly_y + geometry.lilly_height as i32,
                "overlaps Lilly: {point:?} at {geometry:?}"
            );
            assert!(
                points.iter().all(|other| !markers_overlap(point, *other)),
                "overlapping markers at {point:?} at {geometry:?} time={now_ms}"
            );
            points.push(point);
        }
    }

    fn curve_point(curve: Curve, fraction: f64) -> Point {
        let inverse = 1.0 - fraction;
        let coordinate = |from: i32, control1: i32, control2: i32, to: i32| {
            libm::round(
                inverse * inverse * inverse * f64::from(from)
                    + 3.0 * inverse * inverse * fraction * f64::from(control1)
                    + 3.0 * inverse * fraction * fraction * f64::from(control2)
                    + fraction * fraction * fraction * f64::from(to),
            ) as i32
        };
        Point {
            x: coordinate(curve.from.x, curve.control1.x, curve.control2.x, curve.to.x),
            y: coordinate(curve.from.y, curve.control1.y, curve.control2.y, curve.to.y),
        }
    }

    #[test]
    fn preserves_runtime_attention_and_terminal_result_meanings() {
        for (lifecycle, appearance, color) in [
            (
                AgentLifecycle::PendingInit,
                Appearance::Initializing,
                [0, 0, 0, 255],
            ),
            (AgentLifecycle::Running, Appearance::Running, [0, 0, 0, 255]),
            (
                AgentLifecycle::Completed,
                Appearance::Complete,
                [32, 208, 112, 255],
            ),
            (
                AgentLifecycle::Errored,
                Appearance::Error,
                [255, 64, 64, 255],
            ),
            (
                AgentLifecycle::Interrupted,
                Appearance::Interrupted,
                [255, 160, 32, 255],
            ),
            (
                AgentLifecycle::Shutdown,
                Appearance::Shutdown,
                [255, 255, 255, 255],
            ),
            (AgentLifecycle::NotFound, Appearance::Hidden, [0, 0, 0, 255]),
        ] {
            let mut entry = agent("child", lifecycle);
            assert_eq!(Appearance::from_agent(&entry), appearance);
            assert_eq!(appearance.rgba(), color);
            entry.waiting_approval = true;
            assert_eq!(Appearance::from_agent(&entry), Appearance::Waiting);
            entry.waiting_approval = false;
            entry.waiting_input = true;
            assert_eq!(Appearance::from_agent(&entry), Appearance::Waiting);
        }
        let mut entry = agent("child", AgentLifecycle::Completed);
        entry.runtime = Some(AgentRuntime::Active);
        assert_eq!(Appearance::from_agent(&entry), Appearance::Running);
        entry.runtime = Some(AgentRuntime::SystemError);
        assert_eq!(Appearance::from_agent(&entry), Appearance::Error);
        entry.lifecycle = None;
        for runtime in [
            None,
            Some(AgentRuntime::Idle),
            Some(AgentRuntime::NotLoaded),
        ] {
            entry.runtime = runtime;
            assert_eq!(Appearance::from_agent(&entry), Appearance::Hidden);
        }
        entry.lifecycle = Some(AgentLifecycle::Running);
        entry.loaded = false;
        assert_eq!(Appearance::from_agent(&entry), Appearance::Hidden);
    }

    #[test]
    fn identity_owner_and_monotonic_snapshot_boundaries_are_enforced() {
        let mut thoughts = Thoughts::new();
        thoughts
            .ingest(
                OWNER,
                snapshot(2, vec![agent("same", AgentLifecycle::Running)]),
                0,
            )
            .unwrap();
        let other = Owner {
            vm: OWNER.vm + 1,
            run: OWNER.run + 1,
        };
        thoughts
            .ingest(
                other,
                snapshot(1, vec![agent("same", AgentLifecycle::Completed)]),
                0,
            )
            .unwrap();
        assert_eq!(thoughts.agents.len(), 2);
        assert_ne!(thoughts.agents[0].phase_slot, thoughts.agents[1].phase_slot);
        let plan = thoughts.plan(geometry(700, 400), 0);
        assert!(
            plan.iter()
                .any(|placement| placement.owner == other && placement.epoch == 1)
        );
        assert_eq!(
            thoughts.ingest(OWNER, snapshot(1, vec![]), 1),
            Err(ERROR_STALE)
        );
        thoughts.ingest(OWNER, snapshot(2, vec![]), 1).unwrap();
        assert_eq!(
            thoughts.agents.len(),
            2,
            "equal revisions are harmless retries"
        );
        let mut next = snapshot(1, vec![agent("replacement", AgentLifecycle::PendingInit)]);
        next.epoch = 2;
        next.complete = false;
        assert_eq!(thoughts.ingest(OWNER, next.clone(), 1), Err(ERROR_INVALID));
        next.complete = true;
        thoughts.ingest(OWNER, next, 1).unwrap();
        assert_eq!(thoughts.agents.len(), 2);
        assert!(
            thoughts
                .agents
                .iter()
                .any(|thought| thought.owner == other && thought.agent.id == "same")
        );
        assert_eq!(
            thoughts.ingest(OWNER, snapshot(100, vec![]), 2),
            Err(ERROR_STALE)
        );
        thoughts.release(other);
        assert_eq!(thoughts.agents.len(), 1);
        assert!(thoughts.agents.iter().all(|thought| thought.owner == OWNER));
    }

    #[test]
    fn partial_omission_merges_complete_omission_releases_and_capacity_is_atomic() {
        let mut thoughts = Thoughts::new();
        let entries = (0..CAPACITY)
            .map(|index| agent(&format!("child-{index}"), AgentLifecycle::Running))
            .collect();
        thoughts.ingest(OWNER, snapshot(1, entries), 0).unwrap();
        let mut partial = snapshot(2, vec![agent("child-0", AgentLifecycle::Completed)]);
        partial.complete = false;
        thoughts.ingest(OWNER, partial, 1).unwrap();
        assert_eq!(thoughts.agents.len(), CAPACITY);
        let other = Owner { vm: 4, run: 1 };
        assert_eq!(
            thoughts.ingest(
                other,
                snapshot(1, vec![agent("overflow", AgentLifecycle::Running)]),
                1
            ),
            Err(ERROR_CAPACITY)
        );
        assert_eq!(
            thoughts.sources.len(),
            1,
            "rejected snapshot must not register a source"
        );
        assert_eq!(thoughts.agents.len(), CAPACITY);
        thoughts
            .ingest(
                OWNER,
                snapshot(3, vec![agent("child-0", AgentLifecycle::Completed)]),
                2,
            )
            .unwrap();
        assert_eq!(thoughts.agents.len(), 1);
    }

    #[test]
    fn empty_live_publishers_do_not_consume_glyph_capacity() {
        let mut thoughts = Thoughts::new();
        for vm in 0..SOURCE_CAPACITY {
            thoughts
                .ingest(
                    Owner {
                        vm: vm as u8,
                        run: 1,
                    },
                    snapshot(1, vec![]),
                    0,
                )
                .unwrap();
        }
        assert_eq!(thoughts.sources.len(), SOURCE_CAPACITY);
        assert!(thoughts.agents.is_empty());
        thoughts
            .ingest(
                Owner { vm: 1, run: 1 },
                snapshot(2, vec![agent("real-agent", AgentLifecycle::Running)]),
                1,
            )
            .unwrap();
        assert_eq!(thoughts.sources.len(), SOURCE_CAPACITY);
        assert_eq!(thoughts.agents.len(), 1);
    }

    #[test]
    fn reused_vm_admission_replaces_old_run_atomically_and_rejects_delayed_old_run() {
        let mut thoughts = Thoughts::new();
        let entries = (0..CAPACITY)
            .map(|index| agent(&format!("old-{index}"), AgentLifecycle::Running))
            .collect();
        thoughts.ingest(OWNER, snapshot(1, entries), 0).unwrap();
        let next = Owner {
            vm: OWNER.vm,
            run: OWNER.run + 1,
        };
        let mut partial = snapshot(1, vec![agent("new", AgentLifecycle::Running)]);
        partial.complete = false;
        assert_eq!(
            thoughts.ingest(next, partial.clone(), 1),
            Err(ERROR_INVALID)
        );
        assert_eq!(thoughts.agents.len(), CAPACITY);
        partial.complete = true;
        thoughts.ingest(next, partial, 1).unwrap();
        assert_eq!(thoughts.sources.len(), 1);
        assert_eq!(thoughts.agents.len(), 1);
        assert_eq!(thoughts.agents[0].owner, next);
        assert_eq!(
            thoughts.ingest(OWNER, snapshot(9, vec![]), 2),
            Err(ERROR_STALE)
        );
        thoughts.release(OWNER);
        assert_eq!(
            thoughts.agents.len(),
            1,
            "late cleanup must preserve the new VM run"
        );
    }

    #[test]
    fn disconnected_owner_contributes_no_visible_or_invented_terminal_state() {
        let mut thoughts = Thoughts::new();
        thoughts
            .ingest(
                OWNER,
                snapshot(1, vec![agent("child", AgentLifecycle::Running)]),
                0,
            )
            .unwrap();
        let mut disconnected = snapshot(2, vec![]);
        disconnected.connected = false;
        disconnected.complete = false;
        thoughts.ingest(OWNER, disconnected, 100).unwrap();
        assert!(thoughts.plan(geometry(700, 400), 100).is_empty());
        assert_eq!(thoughts.agents[0].appearance, Appearance::Running);
        let mut reconnected = snapshot(3, vec![]);
        reconnected.complete = false;
        thoughts.ingest(OWNER, reconnected, 200).unwrap();
        assert_eq!(thoughts.plan(geometry(700, 400), 200).len(), 1);
        thoughts.release(OWNER);
        assert!(thoughts.plan(geometry(700, 400), 200).is_empty());
    }

    #[test]
    fn shutdown_is_brief_once_without_refresh_rearming_while_results_persist() {
        let mut thoughts = Thoughts::new();
        thoughts
            .ingest(
                OWNER,
                snapshot(
                    1,
                    vec![
                        agent("bye", AgentLifecycle::Shutdown),
                        agent("done", AgentLifecycle::Completed),
                    ],
                ),
                100,
            )
            .unwrap();
        assert_eq!(thoughts.plan(geometry(700, 400), 100).len(), 2);
        thoughts
            .ingest(
                OWNER,
                snapshot(
                    2,
                    vec![
                        agent("bye", AgentLifecycle::Shutdown),
                        agent("done", AgentLifecycle::Completed),
                    ],
                ),
                800,
            )
            .unwrap();
        assert_eq!(thoughts.plan(geometry(700, 400), 1_299).len(), 2);
        let remaining = thoughts.plan(geometry(700, 400), 1_300);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "done");
        thoughts
            .ingest(
                OWNER,
                snapshot(3, vec![agent("bye", AgentLifecycle::Running)]),
                2_000,
            )
            .unwrap();
        assert_eq!(thoughts.plan(geometry(700, 400), 2_000).len(), 1);
    }

    #[test]
    fn resting_positions_are_stable_and_waiting_really_stops_orbiting() {
        let mut thoughts = Thoughts::new();
        thoughts
            .ingest(
                OWNER,
                snapshot(1, vec![agent("first", AgentLifecycle::Running)]),
                0,
            )
            .unwrap();
        let first = thoughts.plan(geometry(700, 400), 0)[0].resting;
        thoughts
            .ingest(
                OWNER,
                snapshot(
                    2,
                    vec![
                        agent("first", AgentLifecycle::Running),
                        agent("new", AgentLifecycle::PendingInit),
                    ],
                ),
                100,
            )
            .unwrap();
        assert_eq!(thoughts.plan(geometry(700, 400), 100)[0].resting, first);
        let mut waiting = agent("first", AgentLifecycle::Running);
        waiting.waiting_input = true;
        thoughts
            .ingest(OWNER, snapshot(3, vec![waiting]), 200)
            .unwrap();
        for now in [200, 5_000, 80_000] {
            let plan = thoughts.plan(geometry(700, 400), now);
            assert_eq!(plan[0].resting, first);
            assert!(plan[0].orbit.is_none());
            assert_eq!(plan[0].color, [32, 128, 255, 255]);
        }
    }

    #[test]
    fn full_groups_fit_center_edges_and_corners_without_any_overlap() {
        for (x, y) in [
            (700, 400),
            (0, 0),
            (1664, 0),
            (0, 824),
            (1664, 824),
            (0, 400),
            (1664, 400),
            (700, 0),
            (700, 824),
        ] {
            let mut thoughts = Thoughts::new();
            let entries = (0..CAPACITY)
                .map(|index| {
                    agent(
                        &format!("child-{index}"),
                        if index % 3 == 0 {
                            AgentLifecycle::Completed
                        } else {
                            AgentLifecycle::Running
                        },
                    )
                })
                .collect();
            thoughts.ingest(OWNER, snapshot(1, entries), 0).unwrap();
            let layout = geometry(x, y);
            for now in (0..=24_000).step_by(125) {
                let plan = thoughts.plan(layout, now);
                assert_eq!(
                    plan.len(),
                    CAPACITY,
                    "normal displays must fit all markers at {layout:?}"
                );
                check_layout(&plan, layout, now);
                assert!(
                    plan.iter().any(|placement| placement.orbit.is_some()),
                    "running should still use a visible arc at {layout:?}"
                );
            }
        }
    }

    #[test]
    fn small_displays_defer_only_the_markers_that_cannot_fit() {
        let mut thoughts = Thoughts::new();
        let entries = (0..CAPACITY)
            .map(|index| agent(&format!("child-{index}"), AgentLifecycle::Running))
            .collect();
        thoughts.ingest(OWNER, snapshot(1, entries), 0).unwrap();
        let cramped = Geometry {
            screen_width: 280,
            screen_height: 280,
            ..geometry(0, 0)
        };
        let plan = thoughts.plan(cramped, 0);
        assert!(!plan.is_empty());
        assert!(plan.len() < CAPACITY);
        check_layout(&plan, cramped, 0);
        for (width, height) in [(256, 256), (128, 128), (1, 1), (0, 0), (u32::MAX, u32::MAX)] {
            let tiny = Geometry {
                screen_width: width,
                screen_height: height,
                ..geometry(0, 0)
            };
            assert!(thoughts.plan(tiny, 0).is_empty());
        }
        assert_eq!(
            thoughts.agents.len(),
            CAPACITY,
            "display pressure must not delete the facts"
        );
    }

    #[test]
    fn queued_cubic_paths_fit_actual_screen_and_leave_static_ring_clear() {
        for (x, y) in [
            (700, 400),
            (0, 0),
            (1664, 0),
            (0, 824),
            (1664, 824),
            (0, 400),
            (1664, 400),
            (700, 0),
            (700, 824),
        ] {
            let mut thoughts = Thoughts::new();
            let entries = (0..CAPACITY)
                .map(|index| {
                    agent(
                        &format!("child-{index}"),
                        if index % 4 == 0 {
                            AgentLifecycle::Completed
                        } else {
                            AgentLifecycle::Running
                        },
                    )
                })
                .collect();
            thoughts.ingest(OWNER, snapshot(1, entries), 0).unwrap();
            let layout = geometry(x, y);
            for now in [0, 3_225, 11_900, 12_000, 18_500, 23_900] {
                let plan = thoughts.plan(layout, now);
                let programs: AllocVec<_> = plan
                    .iter()
                    .map(|placement| placement.orbit.map(|motion| motion.curves(now)))
                    .collect();
                for elapsed in (0..6_000).step_by(50) {
                    let mut points = AllocVec::new();
                    for (placement, program) in plan.iter().zip(&programs) {
                        let mut position = placement.resting;
                        if let Some((from, curves)) = program {
                            position = *from;
                            let mut remaining = elapsed;
                            for curve in curves {
                                position = curve_point(
                                    *curve,
                                    (remaining as f64 / f64::from(curve.duration_ms)).min(1.0),
                                );
                                if remaining < curve.duration_ms {
                                    break;
                                }
                                remaining -= curve.duration_ms;
                            }
                        }
                        assert!(
                            layout.screen_contains(position),
                            "cubic outside {position:?}, {layout:?}"
                        );
                        assert!(
                            points
                                .iter()
                                .all(|other| !markers_overlap(position, *other)),
                            "cubic overlap {position:?}, {layout:?}, now={now}, elapsed={elapsed}"
                        );
                        points.push(position);
                    }
                }
            }
        }
    }

    #[test]
    fn export_model_preview_when_requested() {
        let Ok(path) = std::env::var("TRUEOS_SPIRIT_THOUGHTS_PREVIEW") else {
            return;
        };
        let mut frames = AllocVec::new();
        for (label, x, y) in [
            ("center", 700, 400),
            ("top left", 0, 0),
            ("top right", 1664, 0),
            ("bottom left", 0, 824),
            ("bottom right", 1664, 824),
            ("left edge", 0, 400),
            ("right edge", 1664, 400),
            ("top edge", 700, 0),
            ("bottom edge", 700, 824),
        ] {
            let mut thoughts = Thoughts::new();
            let entries = (0..CAPACITY)
                .map(|index| {
                    let mut entry = agent(
                        &format!("agent-{index}"),
                        match index % 8 {
                            0 => AgentLifecycle::Completed,
                            1 => AgentLifecycle::Errored,
                            2 => AgentLifecycle::Interrupted,
                            3 => AgentLifecycle::PendingInit,
                            _ => AgentLifecycle::Running,
                        },
                    );
                    entry.waiting_input = index % 8 == 4;
                    entry
                })
                .collect();
            thoughts.ingest(OWNER, snapshot(1, entries), 0).unwrap();
            let geometry = geometry(x, y);
            let placements = thoughts.plan(geometry, 4_000);
            check_layout(&placements, geometry, 4_000);
            let markers: AllocVec<_> = placements
                .iter()
                .map(|placement| {
                    let point = point_for(placement, 4_000);
                    let path: AllocVec<_> = placement
                        .orbit
                        .map(|motion| {
                            let (_, curves) = motion.curves(4_000);
                            curves
                                .into_iter()
                                .flat_map(|curve| {
                                    (0..=24).map(move |step| {
                                        let point = curve_point(curve, step as f64 / 24.0);
                                        [point.x, point.y]
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    serde_json::json!({ "id": placement.id, "position": [point.x, point.y],
                    "resting": [placement.resting.x, placement.resting.y],
                    "color": placement.color, "path": path })
                })
                .collect();
            frames.push(serde_json::json!({ "label": label, "screen": [1920, 1080],
                "lilly": [x, y, 256, 256], "marker_side": MARKER_SIDE, "markers": markers }));
        }
        std::fs::write(path, serde_json::to_vec_pretty(&frames).unwrap()).unwrap();
    }
}
