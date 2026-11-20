//! Owns the TPM transport and publishes the result of capability diagnostics.
mod bringup;

use alloc::{format, string::String};
use embassy_sync::signal::Signal;
use spin::Mutex;
use trueos_time::{Duration, Timer};

use crate::{
    efi::acpi::tpm2::{Crb, Error},
    wait::EmbassySpinRawMutex,
};

#[derive(Clone, Copy)]
enum Probe {
    Pending,
    Running,
    Complete(Result<(u64, [u8; 4]), Error>),
}

static PROBE: Mutex<Probe> = Mutex::new(Probe::Pending);
#[derive(Clone, Copy)]
enum Request {
    Capability,
    StorageTest,
}

static REQUEST: Signal<EmbassySpinRawMutex, Request> = Signal::new();
static STORAGE_TEST: Mutex<Option<Result<(), String>>> = Mutex::new(None);

pub(crate) fn request_storage_test() {
    let mut probe = PROBE.lock();
    if matches!(*probe, Probe::Complete(_)) {
        *probe = Probe::Pending;
        *STORAGE_TEST.lock() = None;
        REQUEST.signal(Request::StorageTest);
    }
}

pub(crate) fn storage_test_status() -> String {
    match &*STORAGE_TEST.lock() {
        Some(Ok(())) => "testTpm/bringupUser: store, read, update, delete OK".into(),
        Some(Err(error)) => format!("testTpm/bringupUser: {error}"),
        None => "Storage test uses a generated dummy key".into(),
    }
}

pub(crate) fn request_probe() {
    let mut probe = PROBE.lock();
    if matches!(*probe, Probe::Complete(_)) {
        *probe = Probe::Pending;
        REQUEST.signal(Request::Capability);
    }
}

pub(crate) fn status() -> String {
    format_status(*PROBE.lock())
}

fn format_status(probe: Probe) -> String {
    match probe {
        Probe::Pending => "TPM 2.0 capability probe pending".into(),
        Probe::Running => "TPM2_GetCapability running".into(),
        Probe::Complete(Ok((control, manufacturer))) => {
            let vendor = core::str::from_utf8(&manufacturer).unwrap_or("unknown");
            let vendor = vendor.trim_end_matches('\0');
            let raw = u32::from_be_bytes(manufacturer);
            format!(
                "TPM2_GetCapability OK · CRB 0x{control:08X} · manufacturer {vendor} (0x{raw:08X})"
            )
        }
        Probe::Complete(Err(Error::NoAcpiTable)) => "TPM2 ACPI table unavailable".into(),
        Probe::Complete(Err(Error::UnsupportedStartMethod(method))) => {
            format!("TPM2 ACPI start method {method} unsupported by the CRB probe")
        }
        Probe::Complete(Err(Error::TpmResponse(code))) => {
            format!("TPM2_GetCapability returned TPM error 0x{code:08X}")
        }
        Probe::Complete(Err(error)) => format!("TPM2_GetCapability failed: {error:?}"),
    }
}

#[trueos_executor::task]
pub(crate) async fn tpm_service_task() {
    Timer::after(Duration::from_secs(5)).await;
    let device = Crb::discover();
    let mut request = Request::Capability;
    loop {
        *PROBE.lock() = Probe::Running;
        let result = match &device {
            Ok(device) => device
                .get_manufacturer()
                .await
                .map(|vendor| (device.control_address(), vendor)),
            Err(error) => Err(*error),
        };
        if let Request::StorageTest = request {
            let test = match &device {
                Ok(device) if result.is_ok() => bringup::run(device).await,
                _ => Err(String::from("TPM capability check failed")),
            };
            *STORAGE_TEST.lock() = Some(test);
            crate::log_info!(target: "boot"; "tpm-service: {}\n", storage_test_status());
        }
        let complete = Probe::Complete(result);
        *PROBE.lock() = complete;
        crate::log_info!(target: "boot"; "tpm-service: {}\n", format_status(complete));
        request = REQUEST.wait().await;
    }
}
