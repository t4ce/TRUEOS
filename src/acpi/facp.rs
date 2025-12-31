use acpi::address::GenericAddress;
use acpi::sdt::fadt::{Fadt, FixedFeatureFlags, IaPcBootArchFlags};
use core::ptr::{addr_of, read_unaligned};
use spin::Once;

use crate::debugconf;

use super::ensure_tables;

static LOG_ONCE: Once<()> = Once::new();

pub fn log_once() {
    LOG_ONCE.call_once(|| {
        let Some(tables) = ensure_tables() else {
            return;
        };

        let Some(fadt) = tables.find_table::<Fadt>() else {
            debugconf!("FADT/FACP missing\n");
            return;
        };

        let fadt = unsafe { fadt.virtual_start.as_ref() };

        if let Err(err) = fadt.validate() {
            debugconf!("FADT invalid: {:?}\n", err);
            return;
        }

        log_addresses(fadt);
        log_flags(fadt);
        log_reset(fadt);
    });
}

fn log_addresses(fadt: &Fadt) {
    match (fadt.dsdt_address(), fadt.facs_address()) {
        (Ok(dsdt), Ok(facs)) => debugconf!("FADT: DSDT=0x{:X} FACS=0x{:X}\n", dsdt, facs),
        (Ok(dsdt), Err(e)) => debugconf!("FADT: DSDT=0x{:X} FACS=<err:{:?}>\n", dsdt, e),
        (Err(e), Ok(facs)) => debugconf!("FADT: DSDT=<err:{:?}> FACS=0x{:X}\n", e, facs),
        (Err(ed), Err(ef)) => debugconf!("FADT: DSDT/FACS errors dsdt={:?} facs={:?}\n", ed, ef),
    }

    let sci = unsafe { read_unaligned(addr_of!(fadt.sci_interrupt)) };
    let smi = unsafe { read_unaligned(addr_of!(fadt.smi_cmd_port)) };

    debugconf!(
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
    debugconf!(
        "FADT: iAPC flags legacy={} 8042={} no_vga_probe={} no_msi={} no_aspm={} rtc_via_ns={}\n",
        arch.legacy_devices_are_accessible(),
        arch.motherboard_implements_8042(),
        arch.dont_probe_vga(),
        arch.dont_enable_msi(),
        arch.dont_enable_pcie_aspm(),
        arch.use_time_and_alarm_namespace_for_rtc(),
    );

    let flags: FixedFeatureFlags = unsafe { read_unaligned(addr_of!(fadt.flags)) };
    debugconf!(
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
            debugconf!(
                "FADT: reset_reg {} reset_value=0x{:02X}\n",
                format_gas(&reg),
                fadt.reset_value
            );
        }
        Ok(_) => {
            // No reset register.
        }
        Err(err) => debugconf!("FADT: reset_reg err {:?}\n", err),
    }

    if let Ok(Some(sleep_ctrl)) = fadt.sleep_control_register() {
        log_gas("SLEEP_CTRL", &sleep_ctrl);
    }
    if let Ok(Some(sleep_status)) = fadt.sleep_status_register() {
        log_gas("SLEEP_STAT", &sleep_status);
    }
}

fn log_gas(label: &str, gas: &GenericAddress) {
    debugconf!(
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
