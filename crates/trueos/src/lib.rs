#![no_std]

pub use trueos_sys as sys;

pub mod vcabi {
    pub use trueos_sys::vcabi::*;
}

pub mod vclock;
pub mod vfetch;
pub mod vfs;
pub mod vgfx;
pub mod vinput;
pub mod vnet;
pub mod vshell;
pub mod vsys;
pub mod ui2;

pub mod vled {
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct Rgb8 {
        pub r: u8,
        pub g: u8,
        pub b: u8,
    }

    impl Rgb8 {
        pub const fn new(r: u8, g: u8, b: u8) -> Self {
            Self { r, g, b }
        }
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub enum Effect {
        Solid,
        Breathing,
        Rainbow,
        Off,
    }
}

pub mod prelude {
    pub use crate::vclock;
    pub use crate::vfetch;
    pub use crate::vfs;
    pub use crate::vgfx;
    pub use crate::vinput;
    pub use crate::vnet;
    pub use crate::vshell;
    pub use crate::vsys;
}
