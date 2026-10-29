//! Small, owner-bound agent presences around the one resident Lilly.
//!
//! Codex supplies facts, not cursor programs. This module retains their current
//! presentation, lays out nonoverlapping resting places, and lends visual-only
//! glyph cursors to the existing mouse-motion station. It never focuses a frame,
//! sends a key, or changes Lilly's animation or VFX policy.

extern crate alloc;

use alloc::{format, string::String, sync::Arc};
use heapless::Vec;
use v::spirit_agents::{AgentEntry, AgentLifecycle, AgentRuntime, AgentSnapshot};

const CAPACITY: usize = v::spirit_agents::MAX_AGENTS;
const SOURCE_CAPACITY: usize = crate::allcaps::hv::VM_ID_LIMIT;
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
    label: Arc<str>,
    appearance: Appearance,
    shutdown_at_ms: Option<u64>,
    resting: Option<Point>,
    track_cache: Option<(Geometry, u16, Option<Track>)>,
    phase_slot: usize,
}

impl Thought {
    fn update(&mut self, agent: AgentEntry, now_ms: u64) {
        let appearance = Appearance::from_agent(&agent);
        if appearance != self.appearance {
            self.shutdown_at_ms = (appearance == Appearance::Shutdown)
                .then(|| now_ms.saturating_add(SHUTDOWN_DWELL_MS));
        }
        if self.agent.nickname != agent.nickname {
            self.label = Self::label_for_agent(&agent);
        }
        self.appearance = appearance;
        self.agent = agent;
    }

    fn label_for_agent(agent: &AgentEntry) -> Arc<str> {
        match agent
            .nickname
            .as_deref()
            .filter(|name| !name.trim().is_empty() && !name.chars().any(char::is_control))
        {
            Some(nickname) => Arc::from(format!("§{nickname}")),
            None => Arc::from("§"),
        }
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
                        label: Thought::label_for_agent(&agent),
                        agent,
                        appearance,
                        shutdown_at_ms: (appearance == Appearance::Shutdown)
                            .then(|| now_ms.saturating_add(SHUTDOWN_DWELL_MS)),
                        resting: None,
                        track_cache: None,
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
        let mut occupied = Vec::<PlacedMarker, CAPACITY>::new();
        let mut slots = [None; CAPACITY];
        let sizes: [Option<MarkerSize>; CAPACITY] = core::array::from_fn(|index| {
            self.agents
                .get(index)
                .and_then(|thought| geometry.marker.for_label(&thought.label))
        });
        // Keep each complete label's still-valid resting rectangle. Changing a
        // color never rebuilds the label or reshuffles an unrelated agent.
        for (index, thought) in self.agents.iter().enumerate() {
            if self.connected(thought)
                && thought.visible(now_ms)
                && let Some(size) = sizes[index]
                && let Some(point) = thought.resting
                && geometry.with_marker(size).valid_resting(point)
                && occupied
                    .iter()
                    .all(|other| !size.overlaps_other(point, *other))
            {
                let _ = occupied.push(PlacedMarker { point, size });
                slots[index] = Some(point);
            }
        }
        for (index, thought) in self.agents.iter().enumerate() {
            if slots[index].is_none()
                && self.connected(thought)
                && thought.visible(now_ms)
                && let Some(size) = sizes[index]
                && let Some(point) = geometry.with_marker(size).find_resting(&occupied)
            {
                let _ = occupied.push(PlacedMarker { point, size });
                slots[index] = Some(point);
            }
        }
        let running = self
            .agents
            .iter()
            .enumerate()
            .filter(|(index, thought)| {
                slots[*index].is_some() && thought.appearance == Appearance::Running
            })
            .count();
        let mut running_slot = 0;
        let mut tracks = Vec::<(Geometry, Option<Track>), CAPACITY>::new();
        let mut placements = Vec::new();
        for (index, thought) in self.agents.iter_mut().enumerate() {
            thought.resting = slots[index];
            let (Some(resting), Some(marker)) = (thought.resting, sizes[index]) else {
                continue;
            };
            let orbit = if thought.appearance == Appearance::Running {
                let slot = running_slot;
                running_slot += 1;
                let bounds = geometry.with_marker(marker);
                let track = if let Some((previous, prefix_width, track)) = thought.track_cache
                    && previous == bounds
                    && prefix_width == geometry.marker.width
                {
                    track
                } else {
                    // Common label widths share this bounded calculation;
                    // each thought then retains it until geometry changes.
                    let track = tracks
                        .iter()
                        .find(|(previous, _)| *previous == bounds)
                        .map(|(_, track)| *track)
                        .unwrap_or_else(|| bounds.track(geometry.marker.width));
                    let _ = tracks.push((bounds, track));
                    thought.track_cache = Some((bounds, geometry.marker.width, track));
                    track
                };
                track.map(|track| Motion {
                    track,
                    slot,
                    count: running,
                    wave_phase: thought.phase_slot,
                })
            } else {
                None
            };
            let _ = placements.push(Placement {
                owner: thought.owner,
                epoch: thought.epoch,
                id: thought.agent.id.clone(),
                label: thought.label.clone(),
                color: thought.appearance.rgba(),
                marker,
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

/// The centered rectangle of a complete label, including transparent em
/// margins. Each character uses Paper's already-warmed Shell3 atlas cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MarkerSize {
    width: u16,
    height: u16,
}

impl MarkerSize {
    fn new(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        Some(Self {
            width: width.try_into().ok()?,
            height: height.try_into().ok()?,
        })
    }

    fn for_label(self, label: &str) -> Option<Self> {
        Self::new(
            u32::from(self.width).checked_mul(label.chars().count().try_into().ok()?)?,
            u32::from(self.height),
        )
    }

    fn half_width(self) -> i64 {
        i64::from(self.width.div_ceil(2))
    }

    fn half_height(self) -> i64 {
        i64::from(self.height.div_ceil(2))
    }

    fn spacing_x(self) -> i64 {
        i64::from(self.width) + i64::from(MARKER_GAP)
    }

    fn spacing_y(self) -> i64 {
        i64::from(self.height) + i64::from(MARKER_GAP)
    }

    fn overlaps_other(self, point: Point, other: PlacedMarker) -> bool {
        (i64::from(point.x) - i64::from(other.point.x)).abs()
            < self.half_width() + other.size.half_width() + i64::from(MARKER_GAP)
            && (i64::from(point.y) - i64::from(other.point.y)).abs()
                < self.half_height() + other.size.half_height() + i64::from(MARKER_GAP)
    }
}

#[derive(Clone, Copy, Debug)]
struct PlacedMarker {
    point: Point,
    size: MarkerSize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Geometry {
    screen_width: u32,
    screen_height: u32,
    lilly_x: i32,
    lilly_y: i32,
    lilly_width: u32,
    lilly_height: u32,
    marker: MarkerSize,
}

impl Geometry {
    fn with_marker(self, marker: MarkerSize) -> Self {
        Self { marker, ..self }
    }

    fn screen_contains(self, point: Point) -> bool {
        if self.screen_width > i32::MAX as u32 || self.screen_height > i32::MAX as u32 {
            return false;
        }
        i64::from(point.x) - self.marker.half_width() >= 0
            && i64::from(point.y) - self.marker.half_height() >= 0
            && i64::from(point.x) + self.marker.half_width() <= i64::from(self.screen_width)
            && i64::from(point.y) + self.marker.half_height() <= i64::from(self.screen_height)
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
            let half_x = self.marker.half_width()
                + i64::from(MARKER_GAP)
                + *ring as i64 * self.marker.spacing_x();
            let half_y = self.marker.half_height()
                + i64::from(MARKER_GAP)
                + *ring as i64 * self.marker.spacing_y();
            let on_side = (x == left - half_x || x == right + half_x)
                && y >= top - half_y
                && y <= bottom + half_y;
            let on_edge = (y == top - half_y || y == bottom + half_y)
                && x >= left - half_x
                && x <= right + half_x;
            on_side || on_edge
        })
    }

    fn find_resting(self, occupied: &Vec<PlacedMarker, CAPACITY>) -> Option<Point> {
        let left = i64::from(self.lilly_x);
        let top = i64::from(self.lilly_y);
        let right = left + i64::from(self.lilly_width);
        let bottom = top + i64::from(self.lilly_height);
        // At most 32 candidates per side: bounded even for malformed huge geometry.
        // Start around the side centers so small groups surround Lilly naturally.
        for ring in 0..CAPACITY {
            let half_x = self.marker.half_width()
                + i64::from(MARKER_GAP)
                + ring as i64 * self.marker.spacing_x();
            let half_y = self.marker.half_height()
                + i64::from(MARKER_GAP)
                + ring as i64 * self.marker.spacing_y();
            for distance in 0..CAPACITY {
                for sign in [1_i64, -1] {
                    if distance == 0 && sign == -1 {
                        continue;
                    }
                    let center_x = left
                        + i64::from(self.lilly_width) / 2
                        + distance as i64 * self.marker.spacing_x() * sign;
                    let center_y = top
                        + i64::from(self.lilly_height) / 2
                        + distance as i64 * self.marker.spacing_y() * sign;
                    for (x, y) in [
                        (center_x, top - half_y),
                        (right + half_x, center_y),
                        (center_x, bottom + half_y),
                        (left - half_x, center_y),
                    ] {
                        let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) else {
                            continue;
                        };
                        let point = Point { x, y };
                        if self.resting_ring(point) == Some(ring)
                            && occupied
                                .iter()
                                .all(|other| !self.marker.overlaps_other(point, *other))
                        {
                            return Some(point);
                        }
                    }
                }
            }
        }
        None
    }

    fn track(self, prefix_width: u16) -> Option<Track> {
        let center = Point {
            x: i32::try_from(i64::from(self.lilly_x) + i64::from(self.lilly_width) / 2).ok()?,
            y: i32::try_from(i64::from(self.lilly_y) + i64::from(self.lilly_height) / 2).ok()?,
        };
        // The leading §, rather than the complete text's center, follows the
        // circle that just circumscribes Lilly. A longer nickname or another
        // resting agent must not push this path farther away from her.
        let rx = f64::from(self.lilly_width) / 2.0;
        let ry = f64::from(self.lilly_height) / 2.0;
        let radius = libm::ceil(libm::sqrt(rx * rx + ry * ry)) as i32;
        let orbit = Orbit { center, radius };
        let center_offset_x = i32::from(self.marker.width / 2) - i32::from(prefix_width / 2);
        let label_center = |point: Point| Point {
            x: point.x.saturating_add(center_offset_x),
            y: point.y,
        };
        // Reserve the entire outward wave when finding the visible arc. Both
        // ends of its radial envelope must fit even for a very long label.
        // Moving paths may cross other agents; settled rectangles stay separate.
        let outer = Orbit {
            radius: radius + ORBIT_WAVE_PX,
            ..orbit
        };
        let mut visible = [false; TRACK_STEPS];
        for (step, valid) in visible.iter_mut().enumerate() {
            let angle = step as f64 * TRACK_STEP_ANGLE;
            *valid = self.screen_contains(label_center(orbit.point(angle)))
                && self.screen_contains(label_center(outer.point(angle)));
        }
        let make_track = |start_step, span_steps| Track {
            orbit,
            start_step,
            span_steps,
            center_offset_x,
            // The cursor station interpolates the submitted cubic. Keeping
            // both controls inside these bounds also protects the whole curve
            // at an edge without introducing per-frame geometry checks.
            min: Point {
                x: self.marker.half_width() as i32,
                y: self.marker.half_height() as i32,
            },
            max: Point {
                x: self.screen_width as i32 - self.marker.half_width() as i32,
                y: self.screen_height as i32 - self.marker.half_height() as i32,
            },
        };
        if visible.iter().all(|valid| *valid) {
            return Some(make_track(0, TRACK_STEPS as u16));
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
        if best.1 < 5 {
            return None;
        }
        Some(make_track(
            ((best.0 + 1) % TRACK_STEPS) as u16,
            (best.1 - 3) as u16,
        ))
    }
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

const ORBIT_WAVE_PX: i32 = 8;
const TRACK_STEPS: usize = 256;
const TRACK_STEP_ANGLE: f64 = core::f64::consts::TAU / TRACK_STEPS as f64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Track {
    orbit: Orbit,
    start_step: u16,
    span_steps: u16,
    center_offset_x: i32,
    min: Point,
    max: Point,
}

impl Track {
    fn full(self) -> bool {
        self.span_steps as usize == TRACK_STEPS
    }

    fn label_center(self, point: Point) -> Point {
        Point {
            x: point.x + self.center_offset_x,
            y: point.y,
        }
    }

    fn clamp(self, point: Point) -> Point {
        Point {
            x: point.x.clamp(self.min.x, self.max.x),
            y: point.y.clamp(self.min.y, self.max.y),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Motion {
    track: Track,
    slot: usize,
    count: usize,
    wave_phase: usize,
}

impl Motion {
    /// Current angle and next turnaround. At a screen edge each agent uses
    /// the visible part of its own label's circle, with a stable phase offset.
    fn segment(self, now_ms: u64) -> (f64, f64, u32) {
        let phase_ms = (now_ms % ORBIT_PERIOD_MS
            + self.slot as u64 * ORBIT_PERIOD_MS / self.count.max(1) as u64)
            % ORBIT_PERIOD_MS;
        if self.track.full() {
            let angle = core::f64::consts::TAU * phase_ms as f64 / ORBIT_PERIOD_MS as f64;
            return (
                angle,
                angle + core::f64::consts::FRAC_PI_2,
                (ORBIT_PERIOD_MS / 4) as u32,
            );
        }
        let start = f64::from(self.track.start_step) * TRACK_STEP_ANGLE;
        let travel = f64::from(self.track.span_steps) * TRACK_STEP_ANGLE;
        let half = ORBIT_PERIOD_MS / 2;
        let (fraction, target, remaining) = if phase_ms < half {
            (phase_ms as f64 / half as f64, 1.0, half - phase_ms)
        } else {
            (
                (ORBIT_PERIOD_MS - phase_ms) as f64 / half as f64,
                0.0,
                ORBIT_PERIOD_MS - phase_ms,
            )
        };
        (
            start + travel * fraction,
            start + travel * target,
            remaining.max(8) as u32,
        )
    }

    fn radius(self, angle: f64) -> f64 {
        // A small triangular ripple needs no second sine/cosine evaluation.
        // This runs only while queueing a curve, never on every cursor frame.
        // Each stable agent slot offsets its three gentle waves per circuit.
        let phase = angle * (3.0 / core::f64::consts::TAU)
            + self.wave_phase as f64 * 0.381_966_011_250_105_1;
        let fraction = phase - libm::floor(phase);
        let wave = 1.0 - (2.0 * fraction - 1.0).abs();
        f64::from(self.track.orbit.radius) + f64::from(ORBIT_WAVE_PX) * wave
    }

    fn prefix_point(self, angle: f64) -> Point {
        Orbit {
            radius: libm::round(self.radius(angle)) as i32,
            ..self.track.orbit
        }
        .point(angle)
    }

    fn point(self, angle: f64) -> Point {
        self.track.label_center(self.prefix_point(angle))
    }

    fn curves(self, now_ms: u64) -> (Point, Vec<Curve, 4>) {
        let (angle, end_angle, remaining_ms) = self.segment(now_ms);
        // Four short queued cubics expose the ripple without adding a renderer
        // or a frame-time task. Partial arcs queue at most the next quarter.
        let travel = (end_angle - angle).abs().min(core::f64::consts::FRAC_PI_2);
        let direction = (end_angle - angle).signum();
        let duration_ms = if (end_angle - angle).abs() > f64::EPSILON {
            libm::round(f64::from(remaining_ms) * travel / (end_angle - angle).abs()) as u32
        } else {
            remaining_ms
        };
        let count = (libm::ceil(travel / (core::f64::consts::PI / 8.0)) as usize).clamp(1, 4);
        let step = direction * travel / count as f64;
        let mut curves = Vec::new();
        for index in 0..count {
            let from_angle = angle + index as f64 * step;
            let to_angle = from_angle + step;
            let from = self.point(from_angle);
            let to = self.point(to_angle);
            let tangent = (4.0 / 3.0) * libm::tan(step / 4.0);
            let tangent_from = tangent * self.radius(from_angle);
            let tangent_to = tangent * self.radius(to_angle);
            let _ = curves.push(Curve {
                from,
                to,
                duration_ms: (duration_ms / count as u32).max(8),
                control1: self.track.clamp(Point {
                    x: from.x - libm::round(tangent_from * libm::sin(from_angle)) as i32,
                    y: from.y + libm::round(tangent_from * libm::cos(from_angle)) as i32,
                }),
                control2: self.track.clamp(Point {
                    x: to.x + libm::round(tangent_to * libm::sin(to_angle)) as i32,
                    y: to.y - libm::round(tangent_to * libm::cos(to_angle)) as i32,
                }),
            });
        }
        (self.point(angle), curves)
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
    label: Arc<str>,
    color: [u8; 4],
    marker: MarkerSize,
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
        label: Arc<str>,
        cursor: MouseControlCursor,
        marker: MarkerSize,
        resting: Point,
        orbit: Option<Motion>,
    }

    fn same(cursor: &Cursor, placement: &Placement) -> bool {
        cursor.owner == placement.owner
            && cursor.epoch == placement.epoch
            && cursor.id == placement.id
            && cursor.label == placement.label
            && cursor.marker == placement.marker
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
            // Use Paper's physical-size grid cell, not an independent pixel
            // size. Missing display metrics or atlas readiness defer glyphs;
            // the next tick retries after the normal shared-font warmup.
            let marker = crate::r::services::shell3_font_service::grid_cell_extent(
                super::super::paper_demo::GREETING_GRID_SCALE,
            )
            .and_then(|(width, height)| MarkerSize::new(width, height));
            let placements = if let (
                Some(marker),
                Some((screen_width, screen_height, lilly_x, lilly_y, lilly_width, lilly_height)),
            ) = (marker, super::super::marker_anchor())
            {
                THOUGHTS.lock().plan(
                    Geometry {
                        screen_width,
                        screen_height,
                        lilly_x,
                        lilly_y,
                        lilly_width,
                        lilly_height,
                        marker,
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
                        let Ok(cursor) = motion::request_text_cursor(
                            PRINCIPAL,
                            "Spirit/agent",
                            &placement.label,
                            placement.marker.height,
                            1,
                            Rgba8::new(r, g, b, a),
                            Rgba8::new(255, 255, 255, 255),
                        ) else {
                            continue;
                        };
                        if cursors
                            .push(Cursor {
                                owner: placement.owner,
                                epoch: placement.epoch,
                                id: placement.id.clone(),
                                label: placement.label.clone(),
                                cursor,
                                marker: placement.marker,
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
            nickname: None,
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
            marker: MarkerSize::new(16, 16).unwrap(),
        }
    }

    fn paper_geometry(x: i32, y: i32, height: u32) -> Geometry {
        Geometry {
            // This is Shell3's content-cell aspect ratio, not a square proxy.
            marker: MarkerSize::new((height * 6 + 5) / 11, height).unwrap(),
            ..geometry(x, y)
        }
    }

    fn point_for(placement: &Placement, now_ms: u64) -> Point {
        placement.orbit.map_or(placement.resting, |motion| {
            motion.point(motion.segment(now_ms).0)
        })
    }

    fn check_layout(placements: &Vec<Placement, CAPACITY>, geometry: Geometry, now_ms: u64) {
        let mut resting = AllocVec::new();
        for placement in placements {
            let point = point_for(placement, now_ms);
            assert!(
                geometry
                    .with_marker(placement.marker)
                    .screen_contains(point),
                "outside screen: {point:?} at {geometry:?}, label={}",
                placement.label
            );
            assert!(
                geometry
                    .with_marker(placement.marker)
                    .valid_resting(placement.resting)
            );
            assert!(
                resting
                    .iter()
                    .all(|other| !placement.marker.overlaps_other(placement.resting, *other)),
                "overlapping resting labels at {:?} at {geometry:?}",
                placement.resting
            );
            resting.push(PlacedMarker {
                point: placement.resting,
                size: placement.marker,
            });
            if let Some(motion) = placement.orbit {
                let prefix = motion.prefix_point(motion.segment(now_ms).0);
                let dx = f64::from(prefix.x - motion.track.orbit.center.x);
                let dy = f64::from(prefix.y - motion.track.orbit.center.y);
                let distance = libm::sqrt(dx * dx + dy * dy);
                assert!(
                    (distance - f64::from(motion.track.orbit.radius)).abs()
                        <= f64::from(ORBIT_WAVE_PX + 1)
                );
            }
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

    fn named_agent(id: &str, nickname: &str, lifecycle: AgentLifecycle) -> AgentEntry {
        let mut entry = agent(id, lifecycle);
        entry.name = Some(format!("/root/{id}"));
        entry.nickname = Some(nickname.into());
        entry
    }

    fn check_programs(placements: &Vec<Placement, CAPACITY>, geometry: Geometry, now_ms: u64) {
        let programs: AllocVec<_> = placements
            .iter()
            .map(|placement| placement.orbit.map(|motion| motion.curves(now_ms)))
            .collect();
        for elapsed in (0..=6_000).step_by(50) {
            for (placement, program) in placements.iter().zip(&programs) {
                let mut point = placement.resting;
                if let Some((from, curves)) = program {
                    point = *from;
                    let mut remaining = elapsed;
                    for curve in curves {
                        point = curve_point(
                            *curve,
                            (f64::from(remaining) / f64::from(curve.duration_ms)).min(1.0),
                        );
                        if remaining < curve.duration_ms {
                            break;
                        }
                        remaining -= curve.duration_ms;
                    }
                }
                let bounds = geometry.with_marker(placement.marker);
                assert!(
                    bounds.screen_contains(point),
                    "queued label clips: {} {point:?} {bounds:?}",
                    placement.label
                );
            }
        }
    }

    #[test]
    fn nickname_is_one_static_identity_label_while_state_and_orbit_change() {
        let mut thoughts = Thoughts::new();
        let layout = paper_geometry(700, 400, 48);
        let mut identity = None::<Arc<str>>;
        for (revision, lifecycle, waiting) in [
            (1, AgentLifecycle::PendingInit, false),
            (2, AgentLifecycle::Running, false),
            (3, AgentLifecycle::Running, true),
            (4, AgentLifecycle::Completed, false),
        ] {
            let mut entry = named_agent("task-not-display-name", "Joker", lifecycle);
            entry.waiting_approval = waiting;
            thoughts
                .ingest(OWNER, snapshot(revision, vec![entry]), 0)
                .unwrap();
            let plan = thoughts.plan(layout, 3_000);
            assert_eq!(plan.len(), 1);
            assert_eq!(plan[0].label.as_ref(), "§Joker");
            if let Some(original) = identity.as_ref() {
                assert!(
                    Arc::ptr_eq(original, &plan[0].label),
                    "status/color updates must reuse the cached static text"
                );
            } else {
                identity = Some(plan[0].label.clone());
            }
            assert_eq!(plan[0].marker.width, layout.marker.width * 6);
            assert_eq!(plan[0].marker.height, layout.marker.height);
            assert_eq!(
                plan[0].orbit.is_some(),
                lifecycle == AgentLifecycle::Running && !waiting
            );
            check_layout(&plan, layout, 3_000);
        }
        thoughts
            .ingest(
                OWNER,
                snapshot(
                    5,
                    vec![named_agent(
                        "task-not-display-name",
                        "Quinn",
                        AgentLifecycle::Completed,
                    )],
                ),
                0,
            )
            .unwrap();
        let renamed = thoughts.plan(layout, 3_000);
        assert_eq!(renamed[0].label.as_ref(), "§Quinn");
        assert!(!Arc::ptr_eq(identity.as_ref().unwrap(), &renamed[0].label));
        assert_eq!(renamed[0].id, "task-not-display-name");
    }

    #[test]
    fn missing_blank_or_control_nicknames_fall_back_to_section_not_task_path() {
        let mut thoughts = Thoughts::new();
        let mut entries = vec![agent("missing", AgentLifecycle::Completed)];
        entries[0].name = Some("/root/do-not-show-this".into());
        for (index, nickname) in ["", " ", "\t", "Ada\nBlue", "\0"].iter().enumerate() {
            entries.push(named_agent(
                &format!("fallback-{index}"),
                nickname,
                AgentLifecycle::Completed,
            ));
        }
        thoughts.ingest(OWNER, snapshot(1, entries), 0).unwrap();
        let layout = paper_geometry(700, 400, 48);
        let plan = thoughts.plan(layout, 0);
        assert_eq!(plan.len(), 6);
        assert!(
            plan.iter()
                .all(|placement| placement.label.as_ref() == "§"
                    && placement.marker == layout.marker)
        );
        assert_eq!(
            thoughts.agents[4].agent.nickname.as_deref(),
            Some("Ada\nBlue"),
            "display fallback must preserve canonical source metadata"
        );
        check_layout(&plan, layout, 0);
    }

    #[test]
    fn mixed_nickname_widths_use_complete_nonoverlapping_resting_rectangles() {
        for (x, y) in [(700, 400), (0, 0), (1664, 0), (0, 824), (1664, 824)] {
            let mut thoughts = Thoughts::new();
            let names = [
                String::from("A"),
                String::from("Joker"),
                String::from("Scout"),
                String::from("Ada Lovelace"),
                "Z".repeat(96),
                "é".repeat(48),
            ];
            let entries = names
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    named_agent(&format!("named-{index}"), name, AgentLifecycle::Completed)
                })
                .collect();
            thoughts.ingest(OWNER, snapshot(1, entries), 0).unwrap();
            let layout = paper_geometry(x, y, 16);
            let plan = thoughts.plan(layout, 0);
            assert_eq!(
                plan.len(),
                names.len(),
                "complete labels should fit: {layout:?}"
            );
            for placement in &plan {
                assert_eq!(
                    usize::from(placement.marker.width),
                    usize::from(layout.marker.width) * placement.label.chars().count()
                );
                assert_eq!(placement.marker.height, layout.marker.height);
            }
            check_layout(&plan, layout, 0);
            let repeat = thoughts.plan(layout, 100);
            assert!(
                plan.iter()
                    .zip(&repeat)
                    .all(|(before, after)| before.id == after.id
                        && before.label == after.label
                        && before.resting == after.resting)
            );
        }
    }

    #[test]
    fn mixed_nickname_running_labels_queue_whole_text_bounds_at_center_and_edges() {
        for height in [16, 32, 48] {
            for (x, y) in [
                (700, 400),
                (0, 0),
                (1664, 0),
                (0, 824),
                (1664, 824),
                (0, 400),
                (700, 0),
            ] {
                let mut thoughts = Thoughts::new();
                let entries = ["A", "Joker", "Scout"]
                    .iter()
                    .enumerate()
                    .map(|(index, nickname)| {
                        named_agent(
                            &format!("runner-{index}"),
                            nickname,
                            AgentLifecycle::Running,
                        )
                    })
                    .collect();
                thoughts.ingest(OWNER, snapshot(1, entries), 0).unwrap();
                let layout = paper_geometry(x, y, height);
                for now in [0, 3_225, 11_900, 12_000, 18_500, 23_900] {
                    let plan = thoughts.plan(layout, now);
                    assert_eq!(plan.len(), 3);
                    assert!(
                        plan.iter().any(|placement| placement.orbit.is_some()),
                        "a nickname group should retain visible movement: {layout:?}"
                    );
                    check_layout(&plan, layout, now);
                    check_programs(&plan, layout, now);
                }
            }
        }
    }

    #[test]
    fn section_prefix_uses_lilly_half_diagonal_with_small_stable_agent_waves() {
        let mut thoughts = Thoughts::new();
        thoughts
            .ingest(
                OWNER,
                snapshot(
                    1,
                    vec![
                        named_agent("short", "A", AgentLifecycle::Running),
                        named_agent("long", "A much longer nickname", AgentLifecycle::Running),
                        named_agent("waiting", "A stationary name", AgentLifecycle::Completed),
                    ],
                ),
                0,
            )
            .unwrap();
        let layout = paper_geometry(700, 400, 16);
        let plan = thoughts.plan(layout, 0);
        let first = plan[0].orbit.unwrap();
        let second = plan[1].orbit.unwrap();
        assert_eq!(first.track.orbit.radius, 182);
        assert_eq!(
            second.track.orbit.radius, 182,
            "nickname width cannot enlarge §'s orbit"
        );
        assert!(second.track.center_offset_x > first.track.center_offset_x);
        let mut minimum = f64::INFINITY;
        let mut maximum = f64::NEG_INFINITY;
        let mut different_waves = false;
        for step in 0..=256 {
            let angle = step as f64 * TRACK_STEP_ANGLE;
            let radius = first.radius(angle);
            minimum = minimum.min(radius);
            maximum = maximum.max(radius);
            different_waves |= (radius - second.radius(angle)).abs() > 1.0;
            assert!((182.0..=190.0).contains(&radius));
            let point = first.point(angle);
            let prefix = first.prefix_point(angle);
            assert_eq!(point.x - prefix.x, first.track.center_offset_x);
            assert_eq!(point.y, prefix.y);
        }
        assert!(
            maximum - minimum > 7.0,
            "the running path must actually ripple"
        );
        assert!(
            different_waves,
            "each stable agent has its own ripple phase"
        );
        assert!((first.radius(0.0) - first.radius(core::f64::consts::TAU)).abs() < 0.000_001);
        for now in (0..ORBIT_PERIOD_MS).step_by(250) {
            check_layout(&plan, layout, now);
            check_programs(&plan, layout, now);
            for motion in [first, second] {
                let (_, curves) = motion.curves(now);
                for curve in curves {
                    for step in 0..=32 {
                        let point = curve_point(curve, step as f64 / 32.0);
                        let dx = f64::from(
                            point.x - motion.track.center_offset_x - motion.track.orbit.center.x,
                        );
                        let dy = f64::from(point.y - motion.track.orbit.center.y);
                        let distance = libm::sqrt(dx * dx + dy * dy);
                        assert!(
                            (180.0..=192.0).contains(&distance),
                            "the actual queued cubic must keep § close to Lilly: {distance}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn edge_arc_reverses_without_changing_the_tight_prefix_radius() {
        let mut thoughts = Thoughts::new();
        thoughts
            .ingest(
                OWNER,
                snapshot(
                    1,
                    vec![named_agent("runner", "Joker", AgentLifecycle::Running)],
                ),
                0,
            )
            .unwrap();
        let layout = paper_geometry(1664, 0, 48);
        let plan = thoughts.plan(layout, 0);
        let motion = plan[0].orbit.unwrap();
        assert!(!motion.track.full());
        assert_eq!(motion.track.orbit.radius, 182);
        let start = motion.point(motion.segment(0).0);
        let turnaround = motion.point(motion.segment(ORBIT_PERIOD_MS / 2).0);
        assert_ne!(
            start, turnaround,
            "a screen edge must retain visible motion"
        );
        assert_eq!(start, motion.point(motion.segment(ORBIT_PERIOD_MS).0));
        for now in (0..ORBIT_PERIOD_MS).step_by(125) {
            check_layout(&plan, layout, now);
            check_programs(&plan, layout, now);
        }
    }

    #[test]
    fn full_nickname_is_deferred_until_it_fits_and_relayouts_after_font_resize() {
        let mut thoughts = Thoughts::new();
        thoughts
            .ingest(
                OWNER,
                snapshot(
                    1,
                    vec![
                        named_agent("long", &"Z".repeat(96), AgentLifecycle::Completed),
                        named_agent("short", "Ada", AgentLifecycle::Completed),
                    ],
                ),
                0,
            )
            .unwrap();
        let layout = paper_geometry(700, 400, 48);
        let plan = thoughts.plan(layout, 0);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].label.as_ref(), "§Ada");
        assert_eq!(thoughts.agents.len(), 2);
        let expanded = Geometry {
            screen_width: 6000,
            ..layout
        };
        let plan = thoughts.plan(expanded, 100);
        assert_eq!(plan.len(), 2);
        assert!(
            plan.iter()
                .any(|placement| placement.label.as_ref() == format!("§{}", "Z".repeat(96)))
        );
        check_layout(&plan, expanded, 100);
        let resized = paper_geometry(700, 400, 16);
        let plan = thoughts.plan(resized, 200);
        assert_eq!(plan.len(), 2);
        check_layout(&plan, resized, 200);
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
    fn full_groups_keep_resting_layout_separate_while_orbiting_center_edges_and_corners() {
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
    fn physical_paper_cells_relayout_full_groups_at_edges_without_overlap() {
        for height in [1, 5, 10, 16, 32, 48, 64, 80, 128] {
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
                    .map(|index| agent(&format!("child-{index}"), AgentLifecycle::Completed))
                    .collect();
                thoughts.ingest(OWNER, snapshot(1, entries), 0).unwrap();
                let initial = paper_geometry(x, y, 16);
                thoughts.plan(initial, 0);
                let resized = paper_geometry(x, y, height);
                let placements = thoughts.plan(resized, 100);
                assert_eq!(
                    placements.len(),
                    CAPACITY,
                    "32 physical-size markers must fit at {resized:?}"
                );
                assert!(placements.iter().all(|placement| {
                    placement.marker == resized.marker && placement.orbit.is_none()
                }));
                check_layout(&placements, resized, 100);
                let repeated = thoughts.plan(resized, 200);
                for (left, right) in placements.iter().zip(repeated.iter()) {
                    assert_eq!(left.id, right.id);
                    assert_eq!(left.resting, right.resting);
                    assert_eq!(left.marker, right.marker);
                }
            }
        }
    }

    #[test]
    fn physical_paper_orbits_and_their_cubic_paths_clear_screen_edges() {
        for height in [1, 5, 10, 16, 32, 48, 80, 128] {
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
                thoughts
                    .ingest(
                        OWNER,
                        snapshot(1, vec![agent("runner", AgentLifecycle::Running)]),
                        0,
                    )
                    .unwrap();
                let layout = paper_geometry(x, y, height);
                for now in [0, 3_225, 11_900, 12_000, 18_500, 23_900] {
                    let placements = thoughts.plan(layout, now);
                    assert_eq!(placements.len(), 1);
                    let motion = placements[0]
                        .orbit
                        .expect("a single physical-size marker has room for a visible arc");
                    check_layout(&placements, layout, now);
                    let (_, curves) = motion.curves(now);
                    for curve in curves {
                        for step in 0..=100 {
                            let point = curve_point(curve, f64::from(step) / 100.0);
                            assert!(
                                layout.screen_contains(point),
                                "physical-size cubic clips: {point:?} {layout:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn physical_paper_running_groups_keep_full_text_on_screen() {
        for height in [32, 48, 80, 128] {
            for (x, y) in [(700, 400), (0, 0), (1664, 0), (0, 824), (1664, 824)] {
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
                let layout = paper_geometry(x, y, height);
                for now in [0, 3_225, 11_900, 12_000, 18_500, 23_900] {
                    let placements = thoughts.plan(layout, now);
                    assert_eq!(placements.len(), CAPACITY);
                    check_layout(&placements, layout, now);
                    check_programs(&placements, layout, now);
                }
            }
        }
    }

    #[test]
    fn large_physical_cells_defer_display_pressure_without_losing_agents() {
        let mut thoughts = Thoughts::new();
        let entries = (0..CAPACITY)
            .map(|index| agent(&format!("child-{index}"), AgentLifecycle::Completed))
            .collect();
        thoughts.ingest(OWNER, snapshot(1, entries), 0).unwrap();
        let layout = Geometry {
            screen_width: 340,
            screen_height: 340,
            ..paper_geometry(0, 0, 128)
        };
        let placements = thoughts.plan(layout, 0);
        assert!(placements.len() < CAPACITY);
        check_layout(&placements, layout, 0);
        assert_eq!(thoughts.agents.len(), CAPACITY);
        assert_eq!(
            thoughts.plan(paper_geometry(0, 0, 128), 100).len(),
            CAPACITY,
            "restoring display space restores deferred presences"
        );
        assert!(MarkerSize::new(0, 40).is_none());
        assert!(MarkerSize::new(30, 0).is_none());
        assert!(MarkerSize::new(30, u32::MAX).is_none());
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
    fn queued_cubic_paths_fit_actual_screen_with_mixed_stationary_agents() {
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
                check_programs(&plan, layout, now);
            }
        }
    }

    #[test]
    fn export_model_preview_when_requested() {
        let Ok(path) = std::env::var("TRUEOS_SPIRIT_THOUGHTS_PREVIEW") else {
            return;
        };
        let mut frames = AllocVec::new();
        for height in [32, 48, 80, 128] {
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
                let names = [
                    "Ada",
                    "Joker",
                    "Scout",
                    "Ada Lovelace",
                    "Quinn",
                    "Devon",
                    "River",
                    "Skye",
                ];
                let entries = names
                    .iter()
                    .enumerate()
                    .map(|(index, nickname)| {
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
                        entry.nickname = Some((*nickname).into());
                        entry.waiting_input = index % 8 == 4;
                        entry
                    })
                    .collect();
                thoughts.ingest(OWNER, snapshot(1, entries), 0).unwrap();
                let geometry = paper_geometry(x, y, height);
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
                        serde_json::json!({ "id": placement.id, "label": placement.label.as_ref(),
                    "marker_size": [placement.marker.width, placement.marker.height],
                    "position": [point.x, point.y], "resting": [placement.resting.x, placement.resting.y],
                    "color": placement.color, "path": path })
                    })
                    .collect();
                frames.push(serde_json::json!({ "label": format!("{label}; Paper height {height}px"), "screen": [1920, 1080],
                "lilly": [x, y, 256, 256], "marker_size": [geometry.marker.width, geometry.marker.height], "markers": markers }));
            }
        }
        std::fs::write(path, serde_json::to_vec_pretty(&frames).unwrap()).unwrap();
    }
}
