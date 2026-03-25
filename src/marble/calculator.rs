//! Calculator marble concepts are split by concern.
//!
//! Main path:
//! - `ccw` models the manually designed collapsed calculator world
//! - `harness` keeps the older lane/package sandbox
//!
//! Sidequest:
//! - `sidequest` contains the collapse, waver, and etcher experiments
//!   which are intentionally separate from the current calculator path

pub(crate) use super::{
    Marble, MarbleEmpty, MarbleGadget, MarbleGather, MarblePackage, MarbleTraceField,
    MarbleTransform,
};
pub(crate) use crate::widget_kind::WidgetKind;

#[path = "calculator/ccw.rs"]
mod ccw;
#[path = "calculator/graphcalc.rs"]
mod graphcalc;
#[path = "calculator/harness.rs"]
mod harness;
#[path = "calculator/sidequest.rs"]
mod sidequest;

pub use ccw::*;
pub use graphcalc::*;
pub use harness::*;
pub use sidequest::*;
