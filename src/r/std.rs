extern crate alloc;

/// Shared prelude exports for no_std compatibility wiring.
pub mod prelude {}

/// Re-export alloc types commonly expected from std.
pub mod alloc_types {}

/// Surface-backed path facade.
pub mod path {}
