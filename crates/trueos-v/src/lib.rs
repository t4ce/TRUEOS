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

pub mod alsa_compat;
pub mod borrow;
pub mod bp_abi;
pub mod calculator_base;
pub mod env;
pub mod ffi;
#[doc(hidden)]
pub mod legacy_fs_abi;
pub mod qjs_abi;
pub mod sync;
pub mod varchive;
pub mod vaudio;
pub mod vcabi;
pub mod vclock;
pub mod vfetch;
#[doc(hidden)]
pub mod vfs;
pub mod vfs_async;
pub mod vgpu;
pub mod vhttp_srv;
pub mod vinput;
#[doc(hidden)]
pub mod vio;
pub mod vmail;
pub mod vmedia;
pub mod vnet;
pub mod vnetfs;
pub mod vshell;
pub mod vsys;

#[macro_export]
macro_rules! shell_line {
    ($($arg:tt)*) => {
        $crate::shell::linef(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! shell_ok {
    ($($arg:tt)*) => {
        $crate::shell::styled_linef(format_args!($($arg)*), $crate::shell::color::OK, false)
    };
}

#[macro_export]
macro_rules! shell_warn {
    ($($arg:tt)*) => {
        $crate::shell::styled_linef(format_args!($($arg)*), $crate::shell::color::WARN, true)
    };
}

#[macro_export]
macro_rules! shell_error {
    ($($arg:tt)*) => {
        $crate::shell::styled_linef(format_args!($($arg)*), $crate::shell::color::ERROR, true)
    };
}
