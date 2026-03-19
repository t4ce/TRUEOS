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
pub mod runtime;
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

#[macro_export]
macro_rules! app_entry {
    ($main:path) => {
        #[global_allocator]
        static TRUEOS_GLOBAL_ALLOCATOR: $crate::runtime::TrueosAllocator =
            $crate::runtime::TrueosAllocator;

        #[panic_handler]
        fn trueos_panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
            $crate::runtime::panic_handler(info)
        }

        mod __trueos_app_entry {
            #[unsafe(no_mangle)]
            pub extern "C" fn main(
                argc: usize,
                argv: *const *const core::ffi::c_char,
            ) {
                let args = unsafe { $crate::runtime::args_from_abi(argc, argv) };
                super::$main(args)
            }
        }
    };
    ($body:block) => {
        #[global_allocator]
        static TRUEOS_GLOBAL_ALLOCATOR: $crate::runtime::TrueosAllocator =
            $crate::runtime::TrueosAllocator;

        #[panic_handler]
        fn trueos_panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
            $crate::runtime::panic_handler(info)
        }

        mod __trueos_app_entry {
            #[unsafe(no_mangle)]
            pub extern "C" fn main(
                _argc: usize,
                _argv: *const *const core::ffi::c_char,
            ) {
                $body
            }
        }
    };
}
