use super::{Duration, Instant};

/// Blocks for at least `duration`.
pub fn block_for(duration: Duration) {
    let expires_at = Instant::now() + duration;
    while Instant::now() < expires_at {}
}

/// TRUEOS-local delay marker.
///
/// Upstream `embassy-time` implements `embedded-hal` delay traits for this type.
/// TRUEOS does not use those traits, so this patched crate intentionally keeps the
/// concrete type without pulling both `embedded-hal` generations into the graph.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Delay;
