use core::ops::Index;

use spin::Mutex;

/// Fixed-size static slot table for per-VM/per-CPU state.
///
/// Use this where the table shape is known at compile time and the storage must
/// not allocate or retain heap-backed indexing metadata.
pub struct StaticSlots<T, const N: usize> {
    slots: [Mutex<T>; N],
}

impl<T, const N: usize> StaticSlots<T, N> {
    pub const fn from_slots(slots: [Mutex<T>; N]) -> Self {
        Self { slots }
    }

    pub fn get(&self, index: usize) -> Option<&Mutex<T>> {
        self.slots.get(index)
    }

    pub fn get_u8(&self, index: u8) -> Option<&Mutex<T>> {
        self.get(index as usize)
    }

    pub fn checked_u8(&self, index: u8, err: &'static str) -> Result<&Mutex<T>, &'static str> {
        self.get_u8(index).ok_or(err)
    }

    pub fn iter(&self) -> core::slice::Iter<'_, Mutex<T>> {
        self.slots.iter()
    }
}

impl<T, const N: usize> Index<usize> for StaticSlots<T, N> {
    type Output = Mutex<T>;

    fn index(&self, index: usize) -> &Self::Output {
        &self.slots[index]
    }
}