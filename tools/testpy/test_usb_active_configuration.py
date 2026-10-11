#!/usr/bin/env python3
"""Exercise the production xHCI lookup methods without controller hardware."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'vendor/CrabUSB/usb-host/src/backend/kmod/xhci/device.rs'


def method(source, name):
    start = source.index('    fn ' + name + '(')
    return source[start:source.index('\n    }', start) + len('\n    }')]


def main():
    source = SOURCE.read_text()
    methods = '\n'.join(method(source, name) for name in
                        ['find_interface_endpoints', 'current_config_desc'])
    harness = r'''
#[derive(Debug, PartialEq)] enum USBError { ConfigurationNotSet, NotFound }
type Result<T> = std::result::Result<T, USBError>;
#[derive(Debug, PartialEq)] struct EndpointDescriptor { address: u8, packet: u16 }
struct Alternate { alternate_setting: u8, endpoints: Vec<EndpointDescriptor> }
struct Interface { interface_number: u8, alt_settings: Vec<Alternate> }
struct ConfigurationDescriptor { configuration_value: u8, interfaces: Vec<Interface> }
struct Device { current_config_value: Option<u8>, config_desc: Vec<ConfigurationDescriptor> }
impl Device { __METHODS__ }
fn config(value: u8, alternate: u8, address: u8, packet: u16) -> ConfigurationDescriptor {
    ConfigurationDescriptor { configuration_value: value, interfaces: vec![Interface {
        interface_number: 0, alt_settings: vec![Alternate { alternate_setting: alternate,
            endpoints: vec![EndpointDescriptor { address, packet }] }] }] }
}
fn device(selected: Option<u8>) -> Device {
    Device { current_config_value: selected, config_desc: vec![
        config(1, 0, 0x83, 8), config(2, 0, 0x83, 16)] }
}
#[test] fn ecm_uses_its_own_endpoint_layout() {
    let mut d = device(Some(2));
    assert_eq!(d.find_interface_endpoints(0, 0).unwrap()[0].packet, 16);
    d.current_config_value = Some(1);
    assert_eq!(d.find_interface_endpoints(0, 0).unwrap()[0].packet, 8);
}
#[test] fn absent_interface_or_alternate_does_not_fall_back_to_vendor_config() {
    let mut d = device(Some(2));
    d.config_desc[0].interfaces[0].alt_settings[0].alternate_setting = 1;
    assert_eq!(d.find_interface_endpoints(0, 1), Err(USBError::NotFound));
    d.config_desc[1].interfaces[0].interface_number = 1;
    assert_eq!(d.find_interface_endpoints(0, 0), Err(USBError::NotFound));
}
#[test] fn unconfigured_or_unknown_config_cannot_supply_endpoints() {
    assert_eq!(device(None).find_interface_endpoints(0, 0), Err(USBError::ConfigurationNotSet));
    assert_eq!(device(Some(3)).find_interface_endpoints(0, 0), Err(USBError::NotFound));
}
'''.replace('__METHODS__', methods)
    with tempfile.TemporaryDirectory(prefix='trueos-usb-config-') as temp:
        test = Path(temp) / 'lookup.rs'
        binary = Path(temp) / 'lookup-tests'
        test.write_text(harness)
        subprocess.run(['rustc', '+stable', '--edition=2024', '--test', str(test),
                        '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
