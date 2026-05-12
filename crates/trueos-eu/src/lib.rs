#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod gfx12;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EuIsa {
    Gfx12,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EuArtifactKind {
    ThreadSpawnerEot,
    ThreadSpawnerEotSend1,
    GatewayEot,
    IllegalInstructionTrap,
    Hdc1BtiStoreThenThreadSpawnerEot,
    StaticDp4aThenHdc1StoreThenThreadSpawnerEot,
    LiveXStaticDp4aRequirementThenHdc1StoreThenThreadSpawnerEot,
    T5StoreOnlyArenaOffsetThenHdc1StoreThenThreadSpawnerEot,
    T5SmallLive4Bf16DotThenHdc1StoreThenThreadSpawnerEot,
    T6SmallLive8Bf16DotThenHdc1StoreThenThreadSpawnerEot,
    T61Live16Bf16DotThenHdc1StoreThenThreadSpawnerEot,
    T62RowIndexedLive16Bf16DotThenHdc1StoreThenThreadSpawnerEot,
    T63LaneIndexedLive32Bf16DotThenHdc1StoreThenThreadSpawnerEot,
    T63Accum16HiLive32Bf16DotThenHdc1StoreThenThreadSpawnerEot,
    PrimaryScanoutMandelbrot8ThenHdc1StoreThenThreadSpawnerEot,
    PrimaryScanoutGroupidLine320ScalarBwThenHdc1StoreThenThreadSpawnerEot,
    PrimaryScanoutRow2560Simd8BwThenHdc1StoreThenThreadSpawnerEot,
    PrimaryScanoutLine320ScalarBwThenHdc1StoreThenThreadSpawnerEot,
    PrimaryScanoutLine8Scalar8BwThenHdc1StoreThenThreadSpawnerEot,
    PrimaryScanoutLine8Simd8BwThenHdc1StoreThenThreadSpawnerEot,
    PrimaryScanoutMandelbrot8Simd8CoordColorThenHdc1StoreThenThreadSpawnerEot,
    PrimaryScanoutMandelbrot8Simd8Q12EscapeThenHdc1StoreThenThreadSpawnerEot,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct EuArtifact {
    pub name: &'static str,
    pub isa: EuIsa,
    pub kind: EuArtifactKind,
    pub words: &'static [u32],
    pub expects_store: bool,
}

impl EuArtifact {
    pub const fn bytes_len(self) -> usize {
        self.words.len() * core::mem::size_of::<u32>()
    }

    pub const fn is_empty(self) -> bool {
        self.words.is_empty()
    }
}
