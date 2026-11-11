//! Finger-reader diagnostics for Cry. These inspect USB owner snapshots only;
//! they never enroll a print, send chip commands, or authorize a Cry session.

use crate::usb2::{TlbUsbDevice, TlbUsbSnapshot};
use alloc::{format, string::String, vec, vec::Vec};

const VENDOR_ID: u16 = 0x06cb;
const PRODUCT_ID: u16 = 0x00e9;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Scenario {
    UsbStatus,
    UsbReadiness,
}

pub(crate) const SCENARIOS: &[Scenario] = &[Scenario::UsbStatus, Scenario::UsbReadiness];

impl Scenario {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::UsbStatus => "USB status",
            Self::UsbReadiness => "USB readiness",
        }
    }

    pub(crate) fn run(self) -> Vec<String> {
        let snapshot = crate::usb2::tlb_usb_snapshot();
        let (readers, ready) = counts(&snapshot);
        crate::log_info!(target: "cry";
            "cry-finger: scenario={} readers={} usb_descriptor_ready={} source=cached-usb\n",
            self.label(), readers, ready);
        render(&snapshot, self)
    }
}

/// Called while painting the menu. It performs no transfers and emits no logs.
pub(crate) fn summary() -> Vec<String> {
    render(&crate::usb2::tlb_usb_snapshot(), Scenario::UsbStatus)
}

fn is_reader(device: &TlbUsbDevice) -> bool {
    device.vendor_id == VENDOR_ID && device.product_id == PRODUCT_ID
}

/// The known reader's vendor interface has a bulk request/reply pair and an
/// interrupt event endpoint. All three must belong to the same alternate
/// setting/configuration; descriptors alone do not prove chip communication.
fn usb_descriptors_ready(device: &TlbUsbDevice) -> bool {
    device.configurations.iter().any(|configuration| {
        configuration.configuration_value != 0
            && configuration.interfaces.iter().any(|interface| {
                interface.interface_number == 0
                    && interface.alternate_setting == 0
                    && interface.class == 0xff
                    && [(0x01, "bulk", 1), (0x81, "bulk", 1), (0x83, "interrupt", 8)]
                        .iter()
                        .all(|&(address, transfer_type, minimum_packet)| {
                            interface.endpoints.iter().any(|endpoint| {
                                endpoint.address == address
                                    && endpoint.transfer_type == transfer_type
                                    && endpoint.max_packet_size >= minimum_packet
                            })
                        })
            })
    })
}

fn counts(snapshot: &TlbUsbSnapshot) -> (usize, usize) {
    snapshot
        .devices
        .iter()
        .filter(|device| is_reader(device))
        .fold((0, 0), |(readers, ready), device| {
            (
                readers + 1,
                ready + usize::from(usb_descriptors_ready(device)),
            )
        })
}

fn render(snapshot: &TlbUsbSnapshot, scenario: Scenario) -> Vec<String> {
    let (readers, ready) = counts(snapshot);
    let census = if readers == 0 {
        String::from("USB cache: reader not enumerated.")
    } else {
        format!("USB cache: {readers} reader(s) enumerated.")
    };
    let detail = match scenario {
        Scenario::UsbStatus => {
            if let Some(reader) = snapshot.devices.iter().find(|device| is_reader(device)) {
                format!(
                    "First reader: ctrl {} root {} slot {}.",
                    reader.controller_index, reader.root_port_id, reader.slot_id
                )
            } else if snapshot.probe_error.is_some() {
                // Enumeration errors are controller-wide and have no VID/PID.
                // Do not claim they identify this reader, or that it is absent.
                String::from("USB enumeration reports errors.")
            } else {
                format!("USB controllers: {}.", snapshot.controllers.len())
            }
        }
        Scenario::UsbReadiness => {
            format!("USB descriptor sets ready: {ready}/{readers}.")
        }
    };
    vec![
        String::from("Target: Synaptics 06cb:00e9."),
        census,
        detail,
        String::from("Biometric protocol is not connected."),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb2::{TlbUsbConfiguration, TlbUsbEndpoint, TlbUsbInterface};

    fn reader() -> TlbUsbDevice {
        TlbUsbDevice {
            vendor_id: VENDOR_ID,
            product_id: PRODUCT_ID,
            controller_index: 1,
            root_port_id: 7,
            slot_id: 3,
            configurations: vec![TlbUsbConfiguration {
                configuration_value: 1,
                interfaces: vec![TlbUsbInterface {
                    class: 0xff,
                    endpoints: [
                        (0x01, "bulk", 64),
                        (0x81, "bulk", 64),
                        (0x83, "interrupt", 8),
                    ]
                    .into_iter()
                    .map(|(address, transfer_type, max_packet_size)| TlbUsbEndpoint {
                        address,
                        transfer_type,
                        max_packet_size,
                        ..Default::default()
                    })
                    .collect(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn enumeration_failure_does_not_mean_a_physically_absent_reader() {
        let snapshot = TlbUsbSnapshot {
            probe_error: Some("unrelated configuration descriptor failure"),
            ..Default::default()
        };
        let lines = render(&snapshot, Scenario::UsbStatus);
        assert!(
            lines
                .iter()
                .any(|line| line == "USB cache: reader not enumerated.")
        );
        assert!(
            lines
                .iter()
                .any(|line| line == "USB enumeration reports errors.")
        );
        assert!(!lines.join(" ").contains("unrelated configuration"));
        assert!(!lines.join(" ").contains("absent"));
    }

    #[test]
    fn only_the_known_reader_counts_and_usb_readiness_is_not_a_biometric_proof() {
        let mut other = reader();
        other.product_id = 0x00e8;
        let mut incomplete = reader();
        incomplete.configurations.clear();
        let snapshot = TlbUsbSnapshot {
            devices: vec![other, reader(), incomplete],
            ..Default::default()
        };
        assert_eq!(counts(&snapshot), (2, 1));
        let lines = render(&snapshot, Scenario::UsbReadiness);
        assert!(
            lines
                .iter()
                .any(|line| line == "USB descriptor sets ready: 1/2.")
        );
        assert!(
            lines
                .iter()
                .any(|line| line == "Biometric protocol is not connected.")
        );
    }

    #[test]
    fn endpoints_cannot_be_combined_across_alternate_settings() {
        let mut device = reader();
        let interface = &mut device.configurations[0].interfaces[0];
        let mut other = interface.clone();
        other.alternate_setting = 1;
        other.endpoints = interface.endpoints.split_off(1);
        device.configurations[0].interfaces.push(other);
        assert!(!usb_descriptors_ready(&device));
    }

    #[test]
    fn readiness_rejects_missing_wrong_direction_and_invalid_packet_endpoints() {
        for index in 0..3 {
            let mut device = reader();
            device.configurations[0].interfaces[0]
                .endpoints
                .remove(index);
            assert!(!usb_descriptors_ready(&device));
        }
        let mut device = reader();
        device.configurations[0].interfaces[0].endpoints[1].address = 0x01;
        assert!(!usb_descriptors_ready(&device));
        let mut device = reader();
        device.configurations[0].interfaces[0].endpoints[2].max_packet_size = 0;
        assert!(!usb_descriptors_ready(&device));
    }

    #[test]
    fn results_are_bounded_and_never_display_device_strings() {
        let mut device = reader();
        device.serial = Some(String::from("private-serial"));
        device.product = Some(String::from("untrusted\u{1b}[2J"));
        let snapshot = TlbUsbSnapshot {
            devices: vec![device],
            ..Default::default()
        };
        for scenario in SCENARIOS {
            let lines = render(&snapshot, *scenario);
            assert!(lines.len() <= 4);
            assert!(lines.iter().all(|line| line.len() <= 64));
            let text = lines.join(" ");
            assert!(!text.contains("private-serial"));
            assert!(!text.contains('\u{1b}'));
        }
    }
}
