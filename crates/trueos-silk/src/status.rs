//! Shared artifact status words.
//!
//! Hardware does not fail with prose; it exposes small facts like carry,
//! bounds failure, empty queues, and invalid alignment. Silk keeps that flavor.
//! Every artifact returns a compact status so upper layers can compose machine
//! facts without parsing logs or inventing exception paths.

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SilkStatus {
    Ok = 0,
    Overflow = 1,
    OutOfBounds = 2,
    Exhausted = 3,
    BadAlign = 4,
    Full = 5,
    Empty = 6,
    Corrupt = 7,
}
