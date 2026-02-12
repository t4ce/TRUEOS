use acpi::address::{GenericAddress, MappedGas};
use acpi::sdt::fadt::{Fadt, FixedFeatureFlags};
use acpi::AcpiError;
use core::ptr::{addr_of, read_unaligned};

use super::{ensure_tables, sleep, AcpiIdentityHandler};

const PM1_SLEEP_TYP_SHIFT: u64 = 10;
const PM1_SLEEP_TYP_MASK: u64 = 0b111 << PM1_SLEEP_TYP_SHIFT;
const PM1_SLEEP_ENABLE_BIT: u64 = 1 << 13;

pub type FacpResult<T> = core::result::Result<T, FacpError>;

#[derive(Debug)]
pub enum FacpError {
    TablesMissing,
    FadtMissing,
    ResetUnsupported,
    InvalidSleepState,
    SleepUnsupported,
    Acpi,
}

impl From<AcpiError> for FacpError {
    fn from(_value: AcpiError) -> Self {
        FacpError::Acpi
    }
}







pub fn enter_s_state(pm1a_slp_typ: u8, pm1b_slp_typ: Option<u8>) -> FacpResult<()> {
    with_fadt(|fadt| {
        if fadt.pm1_control_length < 2 {
            return Err(FacpError::SleepUnsupported);
        }

        let handler = AcpiIdentityHandler;
        let pm1a = unsafe { MappedGas::map_gas(fadt.pm1a_control_block()?, &handler)? };
        program_pm1_control(&pm1a, pm1a_slp_typ)?;

        if let Some(pm1b_addr) = fadt.pm1b_control_block()? {
            let slp_typ_b = pm1b_slp_typ.unwrap_or(pm1a_slp_typ);
            let pm1b = unsafe { MappedGas::map_gas(pm1b_addr, &handler)? };
            program_pm1_control(&pm1b, slp_typ_b)?;
        }

        Ok(())
    })
}

pub fn enter_named_sleep_state(state: u8) -> FacpResult<()> {
    if state == 0 || state > 5 {
        return Err(FacpError::InvalidSleepState);
    }
    let st = sleep::sleep_type_for_state(state).ok_or(FacpError::SleepUnsupported)?;
    enter_s_state(st.pm1a, st.pm1b)
}

pub fn reset_system() -> FacpResult<()> {
    with_fadt(|fadt| {
        let flags: FixedFeatureFlags = unsafe { read_unaligned(addr_of!(fadt.flags)) };
        if !flags.supports_system_reset_via_fadt() {
            return Err(FacpError::ResetUnsupported);
        }

        let reg = fadt.reset_register()?;
        if reg.address == 0 {
            return Err(FacpError::ResetUnsupported);
        }

        write_gas_u64(&reg, u64::from(fadt.reset_value))
    })
}

fn program_pm1_control(register: &MappedGas<AcpiIdentityHandler>, slp_typ: u8) -> FacpResult<()> {
    let mut value = register.read()?;
    value &= !(PM1_SLEEP_TYP_MASK | PM1_SLEEP_ENABLE_BIT);
    value |= ((slp_typ as u64) << PM1_SLEEP_TYP_SHIFT) & PM1_SLEEP_TYP_MASK;
    value |= PM1_SLEEP_ENABLE_BIT;
    register.write(value)?;
    Ok(())
}

fn write_gas_u64(gas: &GenericAddress, value: u64) -> FacpResult<()> {
    let handler = AcpiIdentityHandler;
    let mapped = unsafe { MappedGas::map_gas(*gas, &handler)? };
    mapped.write(value)?;
    Ok(())
}

fn with_fadt<T>(f: impl FnOnce(&Fadt) -> FacpResult<T>) -> FacpResult<T> {
    let tables = ensure_tables().ok_or(FacpError::TablesMissing)?;
    let mapping = tables
        .find_table::<Fadt>()
        .ok_or(FacpError::FadtMissing)?;
    let fadt_ref = unsafe { mapping.virtual_start.as_ref() };
    fadt_ref.validate()?;
    f(fadt_ref)
}
