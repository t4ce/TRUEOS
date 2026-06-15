#![allow(dead_code)]

//! Small Intel NEO dispatch definition leaves translated from compute-runtime.

// Source: opencl/source/helpers/dispatch_info_builder.h
pub(crate) mod split_dispatch {
    #[repr(u32)]
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum Dim {
        D1 = 0,
        D2 = 1,
        D3 = 2,
    }

    #[repr(u32)]
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum SplitMode {
        NoSplit = 0,
        WalkerSplit = 1,
        KernelSplit = 2,
    }

    #[repr(u32)]
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum RegionCoordX {
        Left = 0,
        Middle = 1,
        Right = 2,
    }

    #[repr(u32)]
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum RegionCoordY {
        Top = 0,
        Middle = 1,
        Bottom = 2,
    }

    #[repr(u32)]
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum RegionCoordZ {
        Front = 0,
        Middle = 1,
        Back = 2,
    }
}

// Source: opencl/source/helpers/dispatch_info_builder.h
pub(crate) const fn pow_const(base: u32, exp: u32) -> u32 {
    if exp == 0 {
        1
    } else {
        base * pow_const(base, exp - 1)
    }
}

// Source: level_zero/core/source/kernel/sampler_patch_values.h
#[repr(transparent)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SamplerPatchValue(u32);

impl SamplerPatchValue {
    pub(crate) const ADDRESS_NONE: Self = Self(0x00);
    pub(crate) const ADDRESS_CLAMP_TO_BORDER: Self = Self(0x01);
    pub(crate) const ADDRESS_CLAMP_TO_EDGE: Self = Self(0x02);
    pub(crate) const ADDRESS_REPEAT: Self = Self(0x03);
    pub(crate) const ADDRESS_MIRRORED_REPEAT: Self = Self(0x04);
    pub(crate) const ADDRESS_MIRRORED_REPEAT_101: Self = Self(0x05);
    pub(crate) const NORMALIZED_COORDS_FALSE: Self = Self(0x00);
    pub(crate) const NORMALIZED_COORDS_TRUE: Self = Self(0x08);

    pub(crate) const fn raw(self) -> u32 {
        self.0
    }
}

// Source: shared/source/helpers/hw_walk_order.h
pub(crate) mod hw_walk_order {
    pub(crate) const WALK_ORDER_POSSIBILITIES: usize = 6;
    pub(crate) const X: u8 = 0;
    pub(crate) const Y: u8 = 1;
    pub(crate) const Z: u8 = 2;

    pub(crate) const LINEAR_WALK: [u8; 3] = [X, Y, Z];
    pub(crate) const Y_ORDER_WALK: [u8; 3] = [Y, X, Z];
    pub(crate) const SINGLE_DIM_WALK: [u8; 3] = [Y, Z, X];

    pub(crate) const COMPATIBLE_DIMENSION_ORDERS: [[u8; 3]; WALK_ORDER_POSSIBILITIES] = [
        LINEAR_WALK,
        [X, Z, Y],
        Y_ORDER_WALK,
        [Z, X, Y],
        SINGLE_DIM_WALK,
        [Z, Y, X],
    ];

    pub(crate) const LINEAR_WALK_INDEX: usize = 0;
    pub(crate) const SINGLE_DIM_WALK_INDEX: usize = 4;
}
