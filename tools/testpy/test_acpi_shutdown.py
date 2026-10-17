#!/usr/bin/env python3
"""Run production ACPI power sequencing against simulated registers, never hardware."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = (ROOT / 'src/efi/acpi/facp.rs').read_text()
# Substitute table discovery only; run the real sequencing and fallback code.
source = source[:source.index('fn with_fadt<T>')]
source += '''fn with_fadt<T>(f: impl FnOnce(&Fadt) -> FacpResult<T>) -> FacpResult<T> {
    f(&crate::FADT)
}
'''
harness = r'''
extern crate self as acpi;
use std::sync::Mutex;
#[derive(Debug)] pub struct AcpiError;
#[macro_export] macro_rules! log { ($($arg:tt)*) => {{ let _ = format_args!($($arg)*); }} }
struct State { values: [u64; 2], writes: Vec<(u64, u64)>, ack: bool, fallback: usize, waits: Vec<u64> }
static STATE: Mutex<State> = Mutex::new(State { values: [1, 1], writes: Vec::new(), ack: true, fallback: 0, waits: Vec::new() });
pub mod address {
    use super::*;
    #[derive(Clone, Copy)] pub struct GenericAddress { pub address: u64 }
    pub struct MappedGas<H> { address: u64, _handler: std::marker::PhantomData<H> }
    impl<H> MappedGas<H> {
        pub unsafe fn map_gas(gas: GenericAddress, _: &H) -> Result<Self, AcpiError> {
            Ok(Self { address: gas.address, _handler: std::marker::PhantomData })
        }
        pub fn read(&self) -> Result<u64, AcpiError> { Ok(STATE.lock().unwrap().values[self.address as usize]) }
        pub fn write(&self, value: u64) -> Result<(), AcpiError> {
            let mut s = STATE.lock().unwrap(); s.values[self.address as usize] = value;
            s.writes.push((self.address, value)); Ok(())
        }
    }
}
pub mod sdt { pub mod fadt {
    use crate::{AcpiError, address::GenericAddress};
    pub struct FixedFeatureFlags;
    impl FixedFeatureFlags { pub fn supports_system_reset_via_fadt(&self) -> bool { false } }
    pub struct Fadt { pub pm1_control_length: u8, pub smi_cmd_port: u32, pub acpi_enable: u8, pub flags: FixedFeatureFlags, pub reset_value: u8 }
    impl Fadt {
        pub fn pm1a_control_block(&self) -> Result<GenericAddress, AcpiError> { Ok(GenericAddress { address: 0 }) }
        pub fn pm1b_control_block(&self) -> Result<Option<GenericAddress>, AcpiError> { Ok(Some(GenericAddress { address: 1 })) }
        pub fn reset_register(&self) -> Result<GenericAddress, AcpiError> { Err(AcpiError) }
    }
}}
static FADT: sdt::fadt::Fadt = sdt::fadt::Fadt { pm1_control_length: 2, smi_cmd_port: 0xb2, acpi_enable: 0xa1, flags: sdt::fadt::FixedFeatureFlags, reset_value: 0 };
pub unsafe fn outb(port: u16, value: u8) {
    assert_eq!((port, value), (0xb2, 0xa1));
    let mut s = STATE.lock().unwrap(); s.writes.push((port as u64, value as u64));
    if s.ack { s.values[0] |= 1; }
}
mod wait {
    pub fn spin_until_timeout_no_exec(ms: u64, mut condition: impl FnMut() -> bool) -> bool {
        crate::STATE.lock().unwrap().waits.push(ms); condition()
    }
}
mod efi {
    pub enum EfiResetType { Shutdown, Cold }
    pub unsafe fn runtime_services_reset(_: EfiResetType) { crate::STATE.lock().unwrap().fallback += 1; }
    pub mod acpi {
        pub struct AcpiIdentityHandler;
        pub fn ensure_tables() {}
        pub mod sleep {
            pub struct SleepType { pub pm1a: u8, pub pm1b: Option<u8> }
            pub fn sleep_type_for_state(_: u8) -> Option<SleepType> { Some(SleepType { pm1a: 5, pm1b: Some(6) }) }
        }
        pub mod facp { __SOURCE__ }
    }
}
fn reset(values: [u64; 2], ack: bool) {
    *STATE.lock().unwrap() = State { values, writes: Vec::new(), ack, fallback: 0, waits: Vec::new() };
}
fn main() {
    use efi::acpi::facp::*;
    // Both type fields must be prepared before either sleep-enable write.
    // Preserve unrelated bits and replace stale sleep type/enable bits.
    reset([0xfc03, 0xec03], true);
    enter_s_state(5, Some(6)).unwrap();
    assert_eq!(STATE.lock().unwrap().writes, [(0, 0xd403), (1, 0xd803), (0, 0xf403), (1, 0xf803)]);
    // Legacy firmware must acknowledge ACPI_ENABLE before any sleep writes.
    reset([0, 0], true);
    enter_s_state(5, None).unwrap();
    assert_eq!(STATE.lock().unwrap().writes, [(0xb2, 0xa1), (0, 0x1401), (1, 0x1400), (0, 0x3401), (1, 0x3400)]);
    reset([0, 0], false);
    assert!(matches!(enter_s_state(5, None), Err(FacpError::AcpiEnableFailed)));
    assert_eq!(STATE.lock().unwrap().writes, [(0xb2, 0xa1)]);
    // A returned S5 request must reach fallback and report failure, never Ok.
    reset([1, 1], true);
    assert!(matches!(enter_named_sleep_state(5), Err(FacpError::ShutdownReturned)));
    { let s = STATE.lock().unwrap(); assert_eq!(s.fallback, 1); assert_eq!(s.waits, [1000]); }
    reset([0, 0], false);
    assert!(matches!(enter_named_sleep_state(5), Err(FacpError::ShutdownReturned)));
    assert_eq!(STATE.lock().unwrap().fallback, 1);
    // Resumable states keep their existing return behavior; invalid inputs do no I/O.
    reset([1, 1], true);
    enter_named_sleep_state(3).unwrap();
    assert_eq!(STATE.lock().unwrap().fallback, 0);
    reset([1, 1], true);
    for state in [0, 6, 255] { assert!(matches!(enter_named_sleep_state(state), Err(FacpError::InvalidSleepState))); }
    assert!(matches!(enter_s_state(8, None), Err(FacpError::InvalidSleepState)));
    assert!(matches!(enter_s_state(5, Some(8)), Err(FacpError::InvalidSleepState)));
    assert!(STATE.lock().unwrap().writes.is_empty());
}
'''
with tempfile.TemporaryDirectory(prefix='trueos-acpi-power-') as tmp:
    path = Path(tmp)
    (path / 'test.rs').write_text(harness.replace('__SOURCE__', source))
    subprocess.run(['rustc', '--edition=2024', '-Awarnings', str(path / 'test.rs'), '-o', str(path / 'test')], check=True)
    subprocess.run([str(path / 'test')], check=True)
print('ACPI shutdown sequencing: passed')
