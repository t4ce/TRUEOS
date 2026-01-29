use acpi::address::{GenericAddress, MappedGas};
use acpi::sdt::fadt::{Fadt, FixedFeatureFlags, IaPcBootArchFlags};
use acpi::AcpiError;
use core::ptr::{addr_of, read_unaligned};
use spin::Once;

use super::{ensure_tables, AcpiIdentityHandler};

const PM1_SLEEP_TYP_SHIFT: u64 = 10;
const PM1_SLEEP_TYP_MASK: u64 = 0b111 << PM1_SLEEP_TYP_SHIFT;
const PM1_SLEEP_ENABLE_BIT: u64 = 1 << 13;

pub type FacpResult<T> = core::result::Result<T, FacpError>;

#[derive(Debug)]
pub enum FacpError {
    TablesMissing,
    FadtMissing,
    ResetUnsupported,
    SleepUnsupported,
    Acpi,
}

impl From<AcpiError> for FacpError {
    fn from(_value: AcpiError) -> Self {
        FacpError::Acpi
    }
}

static LOG_ONCE: Once<()> = Once::new();

pub fn log_once() {
    LOG_ONCE.call_once(|| {
        let Some(tables) = ensure_tables() else {
            return;
        };

        let Some(fadt) = tables.find_table::<Fadt>() else {
            crate::log!("FADT/FACP missing\n");
            return;
        };

        let fadt = unsafe { fadt.virtual_start.as_ref() };

        if let Err(err) = fadt.validate() {
            crate::log!("FADT invalid: {:?}\n", err);
            return;
        }

        log_addresses(fadt);
        log_flags(fadt);
        log_reset(fadt);
    });
}

fn log_addresses(fadt: &Fadt) {
    match (fadt.dsdt_address(), fadt.facs_address()) {
        (Ok(dsdt), Ok(facs)) => crate::log!("FADT: DSDT=0x{:X} FACS=0x{:X}\n", dsdt, facs),
        (Ok(dsdt), Err(e)) => crate::log!("FADT: DSDT=0x{:X} FACS=<err:{:?}>\n", dsdt, e),
        (Err(e), Ok(facs)) => crate::log!("FADT: DSDT=<err:{:?}> FACS=0x{:X}\n", e, facs),
        (Err(ed), Err(ef)) => crate::log!("FADT: DSDT/FACS errors dsdt={:?} facs={:?}\n", ed, ef),
    }

    let sci = unsafe { read_unaligned(addr_of!(fadt.sci_interrupt)) };
    let smi = unsafe { read_unaligned(addr_of!(fadt.smi_cmd_port)) };

    crate::log!(
        "FADT: profile={:?} sci_irq={} smi_cmd=0x{:X} pm_timer_len={} pm1_len={} pm1_ctrl_len={} pm2_ctrl_len={} gpe0_len={} gpe1_len={} gpe1_base={}\n",
        fadt.power_profile(),
        sci,
        smi,
        fadt.pm_timer_length,
        fadt.pm1_event_length,
        fadt.pm1_control_length,
        fadt.pm2_control_length,
        fadt.gpe0_block_length,
        fadt.gpe1_block_length,
        fadt.gpe1_base,
    );

    if let Ok(Some(tmr)) = fadt.pm_timer_block() {
        log_gas("PMTMR", &tmr);
    }
    if let Ok(gpe0) = fadt.gpe0_block() {
        if let Some(g) = gpe0 {
            log_gas("GPE0", &g);
        }
    }
    if let Ok(gpe1) = fadt.gpe1_block() {
        if let Some(g) = gpe1 {
            log_gas("GPE1", &g);
        }
    }
}

fn log_flags(fadt: &Fadt) {
    let arch: IaPcBootArchFlags = unsafe { read_unaligned(addr_of!(fadt.iapc_boot_arch)) };
    crate::log!(
        "FADT: iAPC flags legacy={} 8042={} no_vga_probe={} no_msi={} no_aspm={} rtc_via_ns={}\n",
        arch.legacy_devices_are_accessible(),
        arch.motherboard_implements_8042(),
        arch.dont_probe_vga(),
        arch.dont_enable_msi(),
        arch.dont_enable_pcie_aspm(),
        arch.use_time_and_alarm_namespace_for_rtc(),
    );

    let flags: FixedFeatureFlags = unsafe { read_unaligned(addr_of!(fadt.flags)) };
    crate::log!(
        "FADT: fixed-features wbinvd={} wbinvd_flushes={} c1={} c2_mp={} pwrbtn_cm={} slpbtn_cm={} pm_tmr_32={} reset_cap={} headless={} hw_reduced={} no_s3_benefit={} pciexp_wake={} pm/hpet={} rtc_s4={} gpe_s5={}\n",
        flags.supports_equivalent_to_wbinvd(),
        flags.wbinvd_flushes_all_caches(),
        flags.all_procs_support_c1_power_state(),
        flags.c2_configured_for_mp_system(),
        flags.power_button_is_control_method(),
        flags.sleep_button_is_control_method(),
        flags.pm_timer_is_32_bit(),
        flags.supports_system_reset_via_fadt(),
        flags.system_is_headless(),
        flags.system_is_hw_reduced_acpi(),
        flags.no_benefit_to_s3(),
        flags.supports_pciexp_wake_in_pm1(),
        flags.use_pm_or_hpet_for_monotonically_decreasing_timers(),
        flags.rtc_sts_is_valid_after_wakeup_from_s4(),
        flags.ospm_may_leave_gpe_wake_events_armed_before_s5(),
    );
}

fn log_reset(fadt: &Fadt) {
    match fadt.reset_register() {
        Ok(reg) if reg.address != 0 => {
            crate::log!(
                "FADT: reset_reg {} reset_value=0x{:02X}\n",
                format_gas(&reg),
                fadt.reset_value
            );
        }
        Ok(_) => {
            // No reset register.
        }
        Err(err) => crate::log!("FADT: reset_reg err {:?}\n", err),
    }

    if let Ok(Some(sleep_ctrl)) = fadt.sleep_control_register() {
        log_gas("SLEEP_CTRL", &sleep_ctrl);
    }
    if let Ok(Some(sleep_status)) = fadt.sleep_status_register() {
        log_gas("SLEEP_STAT", &sleep_status);
    }
}

fn log_gas(label: &str, gas: &GenericAddress) {
    crate::log!(
        "FADT: {} space={:?} addr=0x{:X} width={} offset={} access={}\n",
        label,
        gas.address_space,
        gas.address,
        gas.bit_width,
        gas.bit_offset,
        gas.access_size
    );
}

fn format_gas(gas: &GenericAddress) -> heapless::String<96> {
    use core::fmt::Write;
    let mut s: heapless::String<96> = heapless::String::new();
    let _ = write!(
        &mut s,
        "space={:?} addr=0x{:X} width={} offset={} access={}",
        gas.address_space, gas.address, gas.bit_width, gas.bit_offset, gas.access_size
    );
    s
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

pub fn enter_s5(pm1a_slp_typ: u8, pm1b_slp_typ: Option<u8>) -> FacpResult<()> {
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
