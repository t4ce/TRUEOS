use x86_64::registers::model_specific::Msr;

const MSR_IA32_MISC_ENABLE: u32 = 0x1A0;
const TURBO_DISABLE_BIT: u64 = 1 << 38;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurboState {
    Turbo,
    NoTurbo,
}

pub fn local_state() -> TurboState {
    let value = unsafe { Msr::new(MSR_IA32_MISC_ENABLE).read() };
    if (value & TURBO_DISABLE_BIT) != 0 {
        TurboState::NoTurbo
    } else {
        TurboState::Turbo
    }
}
