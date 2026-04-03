use core::str::SplitWhitespace;

use alloc::string::String;
use alloc::vec::Vec;

use super::super::{ShellBackend2, print_shell_line};
use super::tlb_helper::print_table;
use crate::disc::block::{DeviceKind, PciAddress};
use crate::shell2::shell2_cmd::ParseOutcome;

const PROBE_MENU_HEADERS: [&str; 2] = ["Subcommand", "Description"];
const PROBE_MENU_ROWS: [[&str; 2]; 10] = [
    ["usb", "Show live USB controller runtime state"],
    ["usb snapshot [controller]", "Show richer USB runtime and recovery state"],
    ["usb kick [controller]", "Request a live USB reprobe"],
    ["usb rebind [controller]", "Force a live USB host rebind"],
    ["usb recover [controller]", "Run a prebaked USB recovery flow"],
    ["usb fix [controller]", "Run an aggressive USB fix flow"],
    ["usb mysterybox [controller]", "Run the garlic-to-the-witch USB fix flow"],
    ["nvme", "Show live NVMe controller state"],
    [
        "nvme probe",
        "Run a live NVMe probe when no NVMe device is registered",
    ],
    [
        "nvme flr <bb:dd.f>",
        "Issue PCIe FLR to an unclaimed NVMe controller",
    ],
];

fn line(io: &'static dyn ShellBackend2, text: &str) {
    print_shell_line(io, text);
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_table(io, &PROBE_MENU_HEADERS, &PROBE_MENU_ROWS);
}

fn emit_lines(io: &'static dyn ShellBackend2, lines: Vec<String>) {
    for line_text in lines.into_iter().rev() {
        line(io, line_text.as_str());
    }
}

fn parse_controller_id(raw: &str) -> Option<usize> {
    raw.trim().parse::<usize>().ok()
}

fn parse_bdf(raw: &str) -> Option<PciAddress> {
    let trimmed = raw.trim();
    let (bus_s, rest) = trimmed.split_once(':')?;
    let (slot_s, func_s) = rest.split_once('.')?;
    let bus = u8::from_str_radix(bus_s.trim(), 16).ok()?;
    let slot = u8::from_str_radix(slot_s.trim(), 16).ok()?;
    let function = func_s
        .trim()
        .parse::<u8>()
        .ok()
        .or_else(|| u8::from_str_radix(func_s.trim(), 16).ok())?;
    Some(PciAddress::new(bus, slot, function))
}

fn pci_matches(a: &PciAddress, b: &PciAddress) -> bool {
    a.bus == b.bus && a.slot == b.slot && a.function == b.function
}

fn registered_nvme_pci() -> Vec<PciAddress> {
    crate::disc::block::devices()
        .into_iter()
        .filter(|info| info.kind == DeviceKind::Nvme)
        .filter_map(|info| info.pci)
        .collect()
}

pub(crate) fn cmd_usb_status(io: &'static dyn ShellBackend2) {
    let snapshot = crate::usb2::tlb_snapshot();
    if snapshot.controllers.is_empty() {
        line(io, "probe usb: no xhci controllers found");
        return;
    }

    let mut lines = Vec::new();
    for ctrl in snapshot.controllers.iter() {
        let cached = snapshot
            .devices
            .iter()
            .filter(|dev| dev.controller_index == ctrl.index)
            .count();
        let runtime = crate::usb2::runtime_diag(ctrl.index);
        let progress = crab_usb::debug_usb_probe_progress();
        let bdf = alloc::format!("{:02X}:{:02X}.{}", ctrl.bus, ctrl.slot, ctrl.function);
        if let Some(diag) = runtime {
            lines.push(alloc::format!(
                "probe usb: ctrl={} bdf={} phase={} life={} ready={} pending={} rp={} empty={} fail={} early={} last={} last_count={} q={} qms={} skip={} settle={} quiet={} cached={} stage={} root_port={} port={} slot={} detail={} mmio=0x{:X}",
                ctrl.index,
                bdf,
                diag.controller_phase,
                diag.root_hub_lifecycle,
                diag.event_handler_ready as u8,
                diag.probe_requested as u8,
                diag.root_port_change_seen as u8,
                diag.empty_probe_streak,
                diag.probe_fail_streak,
                diag.early_fatal_rebind_streak,
                diag.last_probe_state,
                diag.last_probe_device_count,
                diag.recovery_quiescent_before_bind as u8,
                diag.recovery_quiescent_ms,
                diag.recovery_skip_delayed_event_handler as u8,
                diag.recovery_initial_settle_ms,
                diag.recovery_probe_quiet_ms,
                cached,
                crab_usb::debug_usb_probe_stage_name(progress.stage),
                progress.root_port,
                progress.port,
                progress.slot,
                progress.detail,
                ctrl.mmio_base.as_ptr() as usize,
            ));
        } else {
            lines.push(alloc::format!(
                "probe usb: ctrl={} bdf={} cached={} stage={} root_port={} port={} slot={} detail={} mmio=0x{:X}",
                ctrl.index,
                bdf,
                cached,
                crab_usb::debug_usb_probe_stage_name(progress.stage),
                progress.root_port,
                progress.port,
                progress.slot,
                progress.detail,
                ctrl.mmio_base.as_ptr() as usize,
            ));
        }
    }

    emit_lines(io, lines);
}

fn cmd_usb_snapshot(io: &'static dyn ShellBackend2, controller: Option<usize>) {
    let snapshot = crate::usb2::tlb_snapshot();
    if snapshot.controllers.is_empty() {
        line(io, "probe usb: no xhci controllers found");
        return;
    }

    let mut lines = Vec::new();
    for ctrl in snapshot.controllers.iter() {
        if controller.is_some() && controller != Some(ctrl.index) {
            continue;
        }
        let Some(diag) = crate::usb2::runtime_diag(ctrl.index) else {
            continue;
        };
        let bdf = alloc::format!("{:02X}:{:02X}.{}", ctrl.bus, ctrl.slot, ctrl.function);
        lines.push(alloc::format!(
            "probe usb snapshot: ctrl={} bdf={} phase={} life={} ready={} pending={} rp={} empty={} fail={} last={} count={} early={} q={} qms={} skip={} settle={} quiet={} mmio=0x{:X}",
            ctrl.index,
            bdf,
            diag.controller_phase,
            diag.root_hub_lifecycle,
            diag.event_handler_ready as u8,
            diag.probe_requested as u8,
            diag.root_port_change_seen as u8,
            diag.empty_probe_streak,
            diag.probe_fail_streak,
            diag.last_probe_state,
            diag.last_probe_device_count,
            diag.early_fatal_rebind_streak,
            diag.recovery_quiescent_before_bind as u8,
            diag.recovery_quiescent_ms,
            diag.recovery_skip_delayed_event_handler as u8,
            diag.recovery_initial_settle_ms,
            diag.recovery_probe_quiet_ms,
            ctrl.mmio_base.as_ptr() as usize,
        ));
    }

    if lines.is_empty() {
        line(io, "probe usb: no matching controller found");
        return;
    }
    emit_lines(io, lines);
}

fn cmd_usb_request(io: &'static dyn ShellBackend2, controller: Option<usize>, rebind: bool) {
    let controllers = crate::usb2::pci_usb_controllers();
    if controllers.is_empty() {
        line(io, "probe usb: no xhci controllers found");
        return;
    }

    let target_ids: Vec<usize> = if let Some(controller_id) = controller {
        alloc::vec![controller_id]
    } else {
        controllers.iter().map(|ctrl| ctrl.index).collect()
    };

    let mut did_any = false;
    for controller_id in target_ids {
        let result = if rebind {
            crate::usb2::request_rebind(controller_id)
        } else {
            crate::usb2::request_probe(controller_id)
        };
        match result {
            Ok(()) => {
                did_any = true;
                if rebind {
                    line(
                        io,
                        alloc::format!("probe usb: controller {} rebind requested", controller_id)
                            .as_str(),
                    );
                } else {
                    line(
                        io,
                        alloc::format!("probe usb: controller {} reprobe requested", controller_id)
                            .as_str(),
                    );
                }
            }
            Err(err) => {
                line(
                    io,
                    alloc::format!(
                        "probe usb: controller {} request failed ({})",
                        controller_id,
                        err
                    )
                    .as_str(),
                );
            }
        }
    }

    if !did_any {
        line(io, "probe usb: no controller accepted the request");
    }
}

fn cmd_usb_recover(io: &'static dyn ShellBackend2, controller: Option<usize>) {
    let controllers = crate::usb2::pci_usb_controllers();
    if controllers.is_empty() {
        line(io, "probe usb: no xhci controllers found");
        return;
    }

    let target_ids: Vec<usize> = if let Some(controller_id) = controller {
        alloc::vec![controller_id]
    } else {
        controllers.iter().map(|ctrl| ctrl.index).collect()
    };

    let mut did_any = false;
    for controller_id in target_ids {
        let Some(diag) = crate::usb2::runtime_diag(controller_id) else {
            line(
                io,
                alloc::format!("probe usb recover: controller {} diag unavailable", controller_id)
                    .as_str(),
            );
            continue;
        };

        let use_rebind = diag.early_fatal_rebind_streak > 0
            || diag.probe_fail_streak > 0
            || matches!(diag.last_probe_state, "error" | "timeout")
            || matches!(diag.controller_phase, "quarantined" | "recoverable")
            || !diag.event_handler_ready;

        let result = if use_rebind {
            crate::usb2::request_rebind(controller_id)
        } else {
            crate::usb2::request_probe(controller_id)
        };

        match result {
            Ok(()) => {
                did_any = true;
                line(
                    io,
                    alloc::format!(
                        "probe usb recover: controller {} action={} phase={} life={} early={} fail={} last={} q={} skip={} settle={} quiet={}",
                        controller_id,
                        if use_rebind { "rebind" } else { "kick" },
                        diag.controller_phase,
                        diag.root_hub_lifecycle,
                        diag.early_fatal_rebind_streak,
                        diag.probe_fail_streak,
                        diag.last_probe_state,
                        diag.recovery_quiescent_before_bind as u8,
                        diag.recovery_skip_delayed_event_handler as u8,
                        diag.recovery_initial_settle_ms,
                        diag.recovery_probe_quiet_ms
                    )
                    .as_str(),
                );
            }
            Err(err) => line(
                io,
                alloc::format!(
                    "probe usb recover: controller {} action={} failed ({})",
                    controller_id,
                    if use_rebind { "rebind" } else { "kick" },
                    err
                )
                .as_str(),
            ),
        }
    }

    if !did_any {
        line(io, "probe usb recover: no controller accepted the request");
    }
}

fn cmd_usb_fix(io: &'static dyn ShellBackend2, controller: Option<usize>, mystery_box: bool) {
    let controllers = crate::usb2::pci_usb_controllers();
    if controllers.is_empty() {
        line(io, "probe usb: no xhci controllers found");
        return;
    }

    let target_ids: Vec<usize> = if let Some(controller_id) = controller {
        alloc::vec![controller_id]
    } else {
        controllers.iter().map(|ctrl| ctrl.index).collect()
    };

    let mut did_any = false;
    for controller_id in target_ids {
        let diag = crate::usb2::runtime_diag(controller_id);
        let rebind_res = crate::usb2::request_rebind(controller_id);
        let probe_res = crate::usb2::request_probe(controller_id);

        match (rebind_res, probe_res) {
            (Ok(()), Ok(())) => {
                did_any = true;
                if let Some(diag) = diag {
                    line(
                        io,
                        alloc::format!(
                            "probe usb {}: controller {} chained rebind+kick phase={} life={} early={} fail={} last={} q={} skip={} settle={} quiet={}",
                            if mystery_box { "mysterybox" } else { "fix" },
                            controller_id,
                            diag.controller_phase,
                            diag.root_hub_lifecycle,
                            diag.early_fatal_rebind_streak,
                            diag.probe_fail_streak,
                            diag.last_probe_state,
                            diag.recovery_quiescent_before_bind as u8,
                            diag.recovery_skip_delayed_event_handler as u8,
                            diag.recovery_initial_settle_ms,
                            diag.recovery_probe_quiet_ms
                        )
                        .as_str(),
                    );
                } else {
                    line(
                        io,
                        alloc::format!(
                            "probe usb {}: controller {} chained rebind+kick",
                            if mystery_box { "mysterybox" } else { "fix" },
                            controller_id
                        )
                        .as_str(),
                    );
                }
            }
            (rebind_err, probe_err) => {
                line(
                    io,
                    alloc::format!(
                        "probe usb {}: controller {} failed rebind={:?} kick={:?}",
                        if mystery_box { "mysterybox" } else { "fix" },
                        controller_id,
                        rebind_err.err(),
                        probe_err.err()
                    )
                    .as_str(),
                );
            }
        }
    }

    if !did_any {
        line(
            io,
            alloc::format!(
                "probe usb {}: no controller accepted the fix flow",
                if mystery_box { "mysterybox" } else { "fix" }
            )
            .as_str(),
        );
    }
}

fn cmd_nvme_status(io: &'static dyn ShellBackend2) {
    let controllers = crate::pci::nvme::diag_snapshot();
    if controllers.is_empty() {
        line(io, "probe nvme: no controllers found");
        return;
    }

    let mut lines = Vec::new();
    for ctrl in controllers {
        let regs = match (ctrl.cap, ctrl.vs, ctrl.cc, ctrl.csts) {
            (Some(cap), Some(vs), Some(cc), Some(csts)) => {
                alloc::format!(
                    "cap=0x{:016X} vs=0x{:08X} cc=0x{:08X} csts=0x{:08X}",
                    cap,
                    vs,
                    cc,
                    csts
                )
            }
            _ if ctrl.bar_assigned => String::from("regs=unmapped"),
            _ => String::from("bar=unassigned"),
        };
        lines.push(alloc::format!(
            "probe nvme: pci={} registered={} bar=0x{:X} {}",
            ctrl.pci,
            ctrl.registered as u8,
            ctrl.bar_base,
            regs,
        ));
    }
    emit_lines(io, lines);
}

fn cmd_nvme_probe(io: &'static dyn ShellBackend2) {
    let registered = registered_nvme_pci();
    if !registered.is_empty() {
        line(io, "probe nvme: refusing live reprobe while an NVMe block device is registered");
        return;
    }

    line(io, "probe nvme: starting live reprobe");
    crate::pci::nvme::probe_once();
    let registered_after = registered_nvme_pci().len();
    line(
        io,
        alloc::format!("probe nvme: live reprobe complete registered_devices={}", registered_after)
            .as_str(),
    );
}

fn cmd_nvme_flr(io: &'static dyn ShellBackend2, pci: PciAddress) {
    if registered_nvme_pci()
        .iter()
        .any(|registered| pci_matches(registered, &pci))
    {
        line(io, "probe nvme: refusing FLR on a currently registered NVMe controller");
        return;
    }

    let ok = crate::pci::try_function_level_reset(pci.bus, pci.slot, pci.function);
    line(
        io,
        alloc::format!(
            "probe nvme: flr pci={} result={}",
            pci,
            if ok { "issued" } else { "unsupported" }
        )
        .as_str(),
    );
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(domain) = args.next() else {
        print_usage(io);
        return ParseOutcome::Handled;
    };

    match domain {
        "usb" => match args.next() {
            None => cmd_usb_status(io),
            Some("status") => {
                if args.next().is_some() {
                    print_usage(io);
                    return ParseOutcome::Handled;
                }
                cmd_usb_status(io);
            }
            Some("snapshot") => {
                let controller = match args.next() {
                    Some(raw) => {
                        let Some(controller_id) = parse_controller_id(raw) else {
                            print_usage(io);
                            return ParseOutcome::Handled;
                        };
                        if args.next().is_some() {
                            print_usage(io);
                            return ParseOutcome::Handled;
                        }
                        Some(controller_id)
                    }
                    None => None,
                };
                cmd_usb_snapshot(io, controller);
            }
            Some("kick") => {
                let controller = match args.next() {
                    Some(raw) => {
                        let Some(controller_id) = parse_controller_id(raw) else {
                            print_usage(io);
                            return ParseOutcome::Handled;
                        };
                        if args.next().is_some() {
                            print_usage(io);
                            return ParseOutcome::Handled;
                        }
                        Some(controller_id)
                    }
                    None => None,
                };
                cmd_usb_request(io, controller, false);
            }
            Some("rebind") => {
                let controller = match args.next() {
                    Some(raw) => {
                        let Some(controller_id) = parse_controller_id(raw) else {
                            print_usage(io);
                            return ParseOutcome::Handled;
                        };
                        if args.next().is_some() {
                            print_usage(io);
                            return ParseOutcome::Handled;
                        }
                        Some(controller_id)
                    }
                    None => None,
                };
                cmd_usb_request(io, controller, true);
            }
            Some("recover") => {
                let controller = match args.next() {
                    Some(raw) => {
                        let Some(controller_id) = parse_controller_id(raw) else {
                            print_usage(io);
                            return ParseOutcome::Handled;
                        };
                        if args.next().is_some() {
                            print_usage(io);
                            return ParseOutcome::Handled;
                        }
                        Some(controller_id)
                    }
                    None => None,
                };
                cmd_usb_recover(io, controller);
            }
            Some("fix") => {
                let controller = match args.next() {
                    Some(raw) => {
                        let Some(controller_id) = parse_controller_id(raw) else {
                            print_usage(io);
                            return ParseOutcome::Handled;
                        };
                        if args.next().is_some() {
                            print_usage(io);
                            return ParseOutcome::Handled;
                        }
                        Some(controller_id)
                    }
                    None => None,
                };
                cmd_usb_fix(io, controller, false);
            }
            Some("mysterybox") => {
                let controller = match args.next() {
                    Some(raw) => {
                        let Some(controller_id) = parse_controller_id(raw) else {
                            print_usage(io);
                            return ParseOutcome::Handled;
                        };
                        if args.next().is_some() {
                            print_usage(io);
                            return ParseOutcome::Handled;
                        }
                        Some(controller_id)
                    }
                    None => None,
                };
                cmd_usb_fix(io, controller, true);
            }
            Some(_) => print_usage(io),
        },
        "nvme" => match args.next() {
            None => cmd_nvme_status(io),
            Some("status") => {
                if args.next().is_some() {
                    print_usage(io);
                    return ParseOutcome::Handled;
                }
                cmd_nvme_status(io);
            }
            Some("probe") => {
                if args.next().is_some() {
                    print_usage(io);
                    return ParseOutcome::Handled;
                }
                cmd_nvme_probe(io);
            }
            Some("flr") => {
                let Some(raw_bdf) = args.next() else {
                    print_usage(io);
                    return ParseOutcome::Handled;
                };
                if args.next().is_some() {
                    print_usage(io);
                    return ParseOutcome::Handled;
                }
                let Some(pci) = parse_bdf(raw_bdf) else {
                    print_usage(io);
                    return ParseOutcome::Handled;
                };
                cmd_nvme_flr(io, pci);
            }
            Some(_) => print_usage(io),
        },
        _ => print_usage(io),
    }

    ParseOutcome::Handled
}
