extern crate alloc;
extern crate self as v;

mod allcaps {
    pub mod hv {
        pub const VM_ID_LIMIT: usize = @VM_ID_LIMIT@;
    }
}

#[path = "@TRUEOS_ROOT@/crates/trueos-v/src/spirit_agents.rs"]
pub mod spirit_agents;

#[path = "@TRUEOS_ROOT@/src/spirit/spirit_thoughts.rs"]
mod spirit_thoughts;
