#![allow(dead_code)]

// Translated from compute-runtime/shared/source/helpers/driver_model_type.h.
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum DriverModelType {
    Unknown = 0,
    Wddm = 1,
    Drm = 2,
}

impl DriverModelType {
    pub(crate) const fn raw(self) -> u32 {
        self as u32
    }
}

// Translated from compute-runtime/shared/source/helpers/map_operation_type.h.
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MapOperationType {
    Map = 0,
    Unmap = 1,
}

impl MapOperationType {
    pub(crate) const fn raw(self) -> u32 {
        self as u32
    }
}

// Translated from compute-runtime/shared/source/helpers/heap_base_address_model.h.
#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum HeapAddressModel {
    PrivateHeaps = 0,
    GlobalStateless = 1,
    GlobalBindless = 2,
    GlobalBindful = 3,
}

impl HeapAddressModel {
    pub(crate) const fn raw(self) -> u32 {
        self as u32
    }
}

// Translated from compute-runtime/shared/source/helpers/bit_helpers.h.
pub(crate) const fn is_bit_set(field: u64, bit_position: u64) -> bool {
    if bit_position >= u64::BITS as u64 {
        return false;
    }

    (field & (1u64 << bit_position)) != 0
}

pub(crate) const fn is_any_bit_set(field: u64, checked_bits: u64) -> bool {
    (field & checked_bits) != 0
}

pub(crate) const fn is_value_set(field: u64, value: u64) -> bool {
    (field & value) == value
}

pub(crate) const fn is_field_valid(field: u64, accepted_bits: u64) -> bool {
    (field & !accepted_bits) == 0
}

pub(crate) const fn set_bits(field: u64, new_value: bool, bits_to_modify: u64) -> u64 {
    if new_value {
        field | bits_to_modify
    } else {
        field & !bits_to_modify
    }
}

pub(crate) const fn shift_left_by(bit_position: u64) -> u64 {
    if bit_position >= u64::BITS as u64 {
        return 0;
    }

    1u64 << bit_position
}

pub(crate) const fn get_most_significant_set_bit_index(mut field: u64) -> u32 {
    let mut index = 0;

    while field >> 1 != 0 {
        field >>= 1;
        index += 1;
    }

    index
}

pub(crate) const fn make_bit_mask<const N: usize>(bits: [u32; N]) -> u64 {
    let mut mask = 0;
    let mut index = 0;

    while index < N {
        mask |= shift_left_by(bits[index] as u64);
        index += 1;
    }

    mask
}
