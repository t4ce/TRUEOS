//! Contract-only marble vocabulary.
//!
//! This module intentionally defines abstract ports and gadget shapes without
//! committing to storage, scheduling, locking, routing, or execution strategy.

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

/// Contract for a gadget where marbles may compete for passage or priority.
pub trait MarbleRace<M: Marble>: MarbleGadget {
    type Error;

    fn enter_race(&mut self, marble: M) -> Result<(), Self::Error>;

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
