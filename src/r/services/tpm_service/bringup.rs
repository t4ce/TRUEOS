//! Disk round-trip for a disposable, hardware-bound TPM sealed object.
use alloc::{format, string::String};
use zeroize::Zeroizing;

use crate::{disc::block::DeviceHandle, efi::acpi::tpm2::Device, r::fs::trueosfs};

const DIR: &str = "testTpm";
const PATH: &str = "testTpm/bringupUser";
type Result<T> = core::result::Result<T, String>;

fn failure(stage: &str, error: impl core::fmt::Debug) -> String {
    format!("{stage}: {error:?}")
}

async fn round_trip(device: &Device, disk: DeviceHandle) -> Result<()> {
    let mut first = Zeroizing::new([0; 32]);
    let mut second = Zeroizing::new([0; 32]);
    if !crate::tyche::fill_bytes(&mut *first)
        || !crate::tyche::fill_bytes(&mut *second)
        || first == second
    {
        return Err("could not generate two distinct dummy keys".into());
    }
    for expected in [&first, &second] {
        let blob = super::sealed::seal(device, expected).await?;
        if !trueosfs::file_write_all_async(disk, PATH, &blob)
            .await
            .map_err(|e| failure("write sealed file", e))?
        {
            return Err("sealed file allocation failed".into());
        }
        let saved = trueosfs::file_out_async(disk, PATH)
            .await
            .map_err(|e| failure("read sealed file", e))?
            .ok_or_else(|| String::from("sealed file missing"))?;
        if super::sealed::unseal(device, &saved).await? != *expected {
            return Err("unsealed payload mismatch".into());
        }
    }
    Ok(())
}

pub(super) async fn run(device: &Device) -> Result<()> {
    let disk = trueosfs::primary_root_handle()
        .filter(|disk| !disk.info().is_read_only())
        .ok_or_else(|| String::from("writable primary TRUEOSFS root required"))?;
    if trueosfs::file_out_async(disk, PATH)
        .await
        .map_err(|e| failure("check test path", e))?
        .is_some()
    {
        return Err("testTpm/bringupUser already exists".into());
    }
    if !trueosfs::dir_create_all_async(disk, DIR)
        .await
        .map_err(|e| failure("create test directory", e))?
    {
        return Err("could not create testTpm".into());
    }
    let work = round_trip(device, disk).await;
    let deleted = trueosfs::file_delete_async(disk, PATH)
        .await
        .map_err(|e| failure("delete sealed file", e))?;
    work?;
    if !deleted
        || trueosfs::file_out_async(disk, PATH)
            .await
            .map_err(|e| failure("verify deletion", e))?
            .is_some()
    {
        return Err("sealed file deletion verification failed".into());
    }
    Ok(())
}
