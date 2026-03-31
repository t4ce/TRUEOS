#![no_std]

extern crate alloc;

pub mod collections;
pub mod fmt;
pub mod iter;
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

pub mod borrow;
pub mod env;
pub mod ffi;
pub mod sync;
pub mod vcabi;
pub mod vclock;
pub mod vfetch;
pub mod vfs;
pub mod vgfx;
pub mod vhttp_srv;
pub mod vinput;
pub mod vio;
pub mod vnet;
pub mod vnetfs;
pub mod vshell;
pub mod vsys;
pub mod vui2;
