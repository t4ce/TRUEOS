use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use x86_64::registers::model_specific::Msr;

const MSR_IA32_MISC_ENABLE: u32 = 0x1A0;
const TURBO_DISABLE_BIT: u64 = 1 << 38;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurboState {
    Turbo,
    NoTurbo,
}

impl TurboState {
    #[inline]
    const fn to_u8(self) -> u8 {
        match self {
            TurboState::Turbo => 0,
            TurboState::NoTurbo => 1,
        }
    }

    #[inline]
    const fn from_u8(v: u8) -> Self {
        match v {
            1 => TurboState::NoTurbo,
            _ => TurboState::Turbo,
        }
    }
}

static DESIRED: AtomicU8 = AtomicU8::new(TurboState::Turbo.to_u8());
static LOGGED_ERROR: AtomicBool = AtomicBool::new(false);

pub fn set_desired(state: TurboState) -> TurboState {
    let prev = DESIRED.swap(state.to_u8(), Ordering::AcqRel);
    TurboState::from_u8(prev)
}

pub fn desired_state() -> TurboState {
    TurboState::from_u8(DESIRED.load(Ordering::Acquire))
}

pub fn apply_local(state: TurboState) -> Result<(), &'static str> {
    let msr = Msr::new(MSR_IA32_MISC_ENABLE);
    let mut value = unsafe { msr.read() };
    let want_disable = state == TurboState::NoTurbo;
    let has_disable = (value & TURBO_DISABLE_BIT) != 0;
    if want_disable == has_disable {
        return Ok(());
    }
    if want_disable {
        value |= TURBO_DISABLE_BIT;
    } else {
        value &= !TURBO_DISABLE_BIT;
    }
    unsafe { msr.write(value) };
    Ok(())
}