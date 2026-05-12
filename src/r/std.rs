extern crate alloc;

/// Shared prelude exports for no_std compatibility wiring.
pub mod prelude {}

/// Re-export alloc types commonly expected from std.
pub mod alloc_types {}

/// Platform-backed I/O facade.
pub mod io {
    pub use trueos_io::*;
}

/// Surface-backed path facade.
pub mod path {}
