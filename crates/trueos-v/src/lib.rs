#![no_std]

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

pub mod vgeom {
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct Point2i {
        pub x: i16,
        pub y: i16,
    }

    impl Point2i {
        pub const fn new(x: i16, y: i16) -> Self {
            Self { x, y }
        }
    }
}

pub mod vnet;
