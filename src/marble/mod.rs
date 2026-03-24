//! Contract-only marble vocabulary.
//!
//! This module intentionally defines abstract ports and gadget shapes without
//! committing to storage, scheduling, locking, routing, or execution strategy.

pub mod calculator;
pub mod instruction_ram;
pub mod park;

/// Marker contract for a pipeline artifact that can move through a marble track.
pub trait Marble {
    fn kind(&self) -> &'static str;
}

/// Lightweight marble-side tag state.
pub trait MarbleTag {
    fn tag(&self) -> &'static str;
}

/// Lightweight marble-side color state.
pub trait MarbleColor {
    fn color(&self) -> &'static str;
}

/// Lightweight marble-side pattern state.
pub trait MarblePattern {
    fn pattern(&self) -> &'static str;
}

/// Lightweight marble-side mass state.
pub trait MarbleMass {
    fn mass(&self) -> u64;
}

/// Lightweight marble-side grip state.
pub trait MarbleGrip {
    fn grip(&self) -> u64;
}

/// Lightweight marble-side material state.
pub trait MarbleMaterial {
    fn material(&self) -> &'static str;
}

/// Abstract storage or reserve associated with a marble flow.
pub trait MarbleTresor<M: Marble> {
    type Error;

    fn store(&mut self, marble: M) -> Result<(), Self::Error>;

    fn retrieve(&mut self) -> Result<Option<M>, Self::Error>;
}

/// Input side of a gadget.
pub trait MarbleIn<M: Marble> {
    type Error;

    fn try_put(&mut self, marble: M) -> Result<(), Self::Error>;
}

/// Output side of a gadget.
pub trait MarbleOut<M: Marble> {
    type Error;

    fn try_take(&mut self) -> Result<Option<M>, Self::Error>;
}

/// Base contract shared by all marble gadgets.
pub trait MarbleGadget {
    fn name(&self) -> &'static str;
}

/// A straight-through gadget with one input and one output.
pub trait MarbleTrack<M: Marble>: MarbleGadget {
    type In: MarbleIn<M>;
    type Out: MarbleOut<M>;

    fn input(&mut self) -> &mut Self::In;

    fn output(&mut self) -> &mut Self::Out;
}

/// A static T-shaped gadget with one input and two outputs.
pub trait MarbleT<M: Marble>: MarbleGadget {
    type In: MarbleIn<M>;
    type OutA: MarbleOut<M>;
    type OutB: MarbleOut<M>;

    fn input(&mut self) -> &mut Self::In;

    fn out_a(&mut self) -> &mut Self::OutA;

    fn out_b(&mut self) -> &mut Self::OutB;
}

/// A gadget that consumes one marble and emits a marble of another kind.
pub trait MarbleTransform<I: Marble, O: Marble>: MarbleGadget {
    type Error;

    fn transform(&mut self, marble: I) -> Result<O, Self::Error>;
}

/// A passive observer attached to a marble gadget.
pub trait MarbleTap<M: Marble>: MarbleGadget {
    fn observe_in(&mut self, marble: &M);

    fn observe_out(&mut self, marble: &M);
}

/// A lightweight wakeup contract between marble boundaries.
pub trait MarbleSignal: MarbleGadget {
    fn signal(&self);
}

/// Contract for a gadget that acts as a bounded or unbounded marble pool.
pub trait MarblePool<M: Marble>: MarbleGadget {
    type Error;

    fn push(&mut self, marble: M) -> Result<(), Self::Error>;

    fn pop(&mut self) -> Result<Option<M>, Self::Error>;
}

/// Contract for a gadget that receives marbles rejected from a pool.
pub trait MarblePoolOverflow<M: Marble>: MarbleGadget {
    type Error;

    fn overflow(&mut self, marble: M) -> Result<(), Self::Error>;
}

/// Contract for a gadget that drains marbles from a pool or held store.
pub trait MarblePoolDrain<M: Marble>: MarbleGadget {
    type Error;

    fn drain(&mut self) -> Result<(), Self::Error>;

    fn take_drained(&mut self) -> Result<Option<M>, Self::Error>;
}

/// Contract for a gadget that takes one marble and yields multiple duplicates.
///
/// This is a shape contract only. Implementations decide whether duplication is
/// eager, lazy, copy-on-write, or reference-counted.
pub trait MarbleDuplicator<M: Marble>: MarbleGadget {
    type Error;
    type Out: MarbleOut<M>;

    fn duplicate(&mut self, marble: M, copies: usize) -> Result<(), Self::Error>;

    fn output(&mut self, index: usize) -> Option<&mut Self::Out>;
}

/// Contract for a gadget that terminates a marble's journey.
pub trait MarbleDestroyer<M: Marble>: MarbleGadget {
    type Error;

    fn destroy(&mut self, marble: M) -> Result<(), Self::Error>;
}

/// Contract for a gadget that boxes or encapsulates a marble.
pub trait MarbleBox<I: Marble, O: Marble>: MarbleGadget {
    type Error;

    fn box_marble(&mut self, marble: I) -> Result<O, Self::Error>;
}

/// Contract for a gadget that stores boxed marbles.
pub trait MarbleBoxStore<M: Marble>: MarbleGadget {
    type Error;

    fn store_box(&mut self, marble: M) -> Result<(), Self::Error>;

    fn take_box(&mut self) -> Result<Option<M>, Self::Error>;
}

/// Contract for a gadget that speeds up or prioritizes marble flow.
///
/// This does not prescribe what "faster" means. It may be urgency, scheduling
/// priority, lane preference, or reduced latency through a boundary.
pub trait MarbleAccelerator<M: Marble>: MarbleGadget {
    type Error;

    fn accelerate(&mut self, marble: M) -> Result<M, Self::Error>;
}

/// Contract for a gadget that deliberately slows or backs off marble flow.
pub trait MarbleDecelerator<M: Marble>: MarbleGadget {
    type Error;

    fn decelerate(&mut self, marble: M) -> Result<M, Self::Error>;
}

/// Contract for a gadget that constrains ingress, egress, or in-flight marbles.
pub trait MarbleChoke<M: Marble>: MarbleGadget {
    type Error;

    fn choke(&mut self, marble: M) -> Result<M, Self::Error>;
}

/// Contract for a gadget that routes one input marble toward multiple outputs.
///
/// Unlike a duplicator, a fanout may route to one, many, or all outputs based on
/// policy chosen by the implementation.
pub trait MarbleFanout<M: Marble>: MarbleGadget {
    type Error;
    type Out: MarbleOut<M>;

    fn fanout(&mut self, marble: M) -> Result<(), Self::Error>;

    fn output(&mut self, index: usize) -> Option<&mut Self::Out>;
}

/// Contract for a gadget that reduces or compacts a marble into a smaller kind.
pub trait MarbleShrink<I: Marble, O: Marble>: MarbleGadget {
    type Error;

    fn shrink(&mut self, marble: I) -> Result<O, Self::Error>;
}

/// Contract for a gadget that expands a marble into a richer or larger kind.
pub trait MarbleExpand<I: Marble, O: Marble>: MarbleGadget {
    type Error;

    fn expand(&mut self, marble: I) -> Result<O, Self::Error>;
}

/// Contract for a gadget that freezes a marble into a stable held form.
pub trait MarbleFreeze<M: Marble>: MarbleGadget {
    type Error;

    fn freeze(&mut self, marble: M) -> Result<M, Self::Error>;
}

/// Contract for a gadget that allows a marble to pass through without taking the
/// usual visible or blocking route.
///
/// "Ghostwalk" is left deliberately abstract. It can mean bypass, hidden flow,
/// side-lane motion, or non-interfering passage depending on the system.
pub trait MarbleGhostwalk<M: Marble>: MarbleGadget {
    type Error;

    fn ghostwalk(&mut self, marble: M) -> Result<M, Self::Error>;
}

/// Contract for a gadget where lane-fed marbles compete for one downstream path.
///
/// The intended policy is priority ordered by lane index: lane 0 is the highest
/// priority, and a newly non-empty higher-priority lane may preempt the current
/// flow on the next take.
pub trait MarbleRace<M: Marble>: MarbleGadget {
    type Error;

    fn lanes(&self) -> usize;

    fn enter_race(&mut self, lane: usize, marble: M) -> Result<(), Self::Error>;

    fn active_lane(&self) -> Option<usize>;

    fn finish_race(&mut self) -> Result<Option<M>, Self::Error>;
}

/// Contract for a gadget that reflects or rebounds a marble.
pub trait MarbleBounce<M: Marble>: MarbleGadget {
    type Error;

    fn bounce(&mut self, marble: M) -> Result<M, Self::Error>;
}

/// Contract for a gadget that attracts, captures, or redirects marbles.
pub trait MarbleMagnet<M: Marble>: MarbleGadget {
    type Error;

    fn pull(&mut self, marble: M) -> Result<M, Self::Error>;
}

/// Contract for a guided flowing path of marbles through a system.
pub trait MarbleRiver<M: Marble>: MarbleGadget {
    type Error;

    fn flow(&mut self, marble: M) -> Result<(), Self::Error>;

    fn next(&mut self) -> Result<Option<M>, Self::Error>;
}

/// Contract for a gateway that carries marbles across a boundary as discrete hops.
pub trait MarblePortal<M: Marble>: MarbleGadget {
    type Error;

    fn send(&mut self, marble: M) -> Result<(), Self::Error>;

    fn receive(&mut self) -> Result<Option<M>, Self::Error>;
}

/// A normalized direction carried by a marble-side trajectory.
///
/// The contract is semantic rather than enforced here: implementations are
/// expected to provide a unit-length direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarbleUnitVector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl MarbleUnitVector3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

/// Minimal trajectory payload that can be transferred between marbles.
///
/// `direction` says where the motion points. `amount` is the scalar intensity
/// carried along that direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarbleImpulse {
    pub direction: MarbleUnitVector3,
    pub amount: f32,
}

impl MarbleImpulse {
    pub const fn new(direction: MarbleUnitVector3, amount: f32) -> Self {
        Self { direction, amount }
    }
}

/// Contract for a marble that currently carries a trajectory.
pub trait MarbleTrajectory: Marble {
    fn trajectory(&self) -> MarbleImpulse;
}

/// Contract for a marble that can have a trajectory written onto it.
pub trait MarbleTrajectorySink: Marble {
    type Error;

    fn receive_trajectory(&mut self, impulse: MarbleImpulse) -> Result<(), Self::Error>;
}

/// Contract for the one direct marble-to-marble interaction: impact transfer.
///
/// A source marble does not need to know where the target goes next. It only
/// provides a directed amount, and the target decides how to absorb it.
pub trait MarbleImpact<Target: MarbleTrajectorySink>: MarbleTrajectory {
    type Error;

    fn hit(&self, target: &mut Target) -> Result<(), Self::Error>;
}

/// Contract for a marble that represents the deliberate absence of payload.
///
/// This is the "leading zero" marble: it occupies a lane slot meaningfully
/// enough to keep flow going, while carrying no domain payload.
pub trait MarbleEmpty: Marble {
    fn is_empty(&self) -> bool;
}

/// Contract for a package built from one marble position per lane.
pub trait MarblePackage<M: Marble>: Marble {
    fn width(&self) -> usize;

    fn lane(&self, index: usize) -> Option<&M>;
}

/// Contract for an N-lane marble trace field backed by per-lane slots.
///
/// The intended shape is a mapped area where each lane advances independently,
/// but package release observes one exact slot at the end of every lane.
pub trait MarbleTraceField<M: Marble>: MarbleGadget {
    type Error;

    fn lanes(&self) -> usize;

    fn try_put_lane(&mut self, lane: usize, marble: M) -> Result<(), Self::Error>;

    fn lane_ready(&self, lane: usize) -> bool;
}

/// Contract for a gadget that gathers one marble from each lane into a package.
///
/// Implementations choose whether an incomplete set halts release, or whether
/// they inject empty marbles to complete the package.
pub trait MarbleGather<M: Marble, P: MarblePackage<M>>: MarbleGadget {
    type Error;

    fn gather(&mut self) -> Result<Option<P>, Self::Error>;
}
