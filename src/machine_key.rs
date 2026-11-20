//! TPM-only persistence for the Cry recovery key.
use crate::{
    disc::block::DeviceHandle,
    r::{fs::trueosfs, services::tpm_service},
};
use alloc::{format, string::String, vec::Vec};
use zeroize::Zeroizing;

pub(crate) const PATH: &str = "trueos/cry-key.tpm";
pub(crate) const LEGACY_PATH: &str = "trueos/uncrypted.blob";
pub(crate) const SAVED_FILENAME: &str = "cry-key.tpm";
const MAGIC: &[u8; 8] = b"TRTPMKEY";

/// Keep account history even if an account's files are later removed. Commit
/// before any credential write; interrupted enrollment must not reopen SSH.
pub(crate) async fn mark_account_history(disk: DeviceHandle) -> Result<(), String> {
    const HISTORY: &str = "trueos/users-ever-persisted";
    if !trueosfs::dir_create_all_async(disk, "trueos")
        .await
        .map_err(|e| alloc::format!("account history directory: {e:?}"))?
    {
        return Err(String::from("account history directory allocation failed"));
    }
    if !trueosfs::file_write_all_async(disk, HISTORY, b"1\n")
        .await
        .map_err(|e| alloc::format!("account history write: {e:?}"))?
    {
        return Err(String::from("account history allocation failed"));
    }
    if trueosfs::file_out_async(disk, HISTORY)
        .await
        .map_err(|e| alloc::format!("account history readback: {e:?}"))?
        .as_deref()
        != Some(b"1\n")
    {
        return Err(String::from("account history readback failed"));
    }
    Ok(())
}

pub(crate) async fn prepare(username: &str, key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let name = crate::crypt::canonical_username(username)
        .map_err(|_| String::from("invalid machine-key account"))?;
    let sealed = tpm_service::seal(key).await?;
    let mut blob = Vec::with_capacity(MAGIC.len() + 1 + name.len() + sealed.len());
    blob.extend_from_slice(MAGIC);
    blob.push(name.len() as u8);
    blob.extend_from_slice(name.as_bytes());
    blob.extend_from_slice(&sealed);
    Ok(blob)
}

pub(crate) async fn save_boot(disk: DeviceHandle, blob: &[u8]) -> Result<(), String> {
    save(disk, PATH, LEGACY_PATH, blob).await
}

pub(crate) async fn save_account(
    disk: DeviceHandle,
    username: &str,
    blob: &[u8],
) -> Result<(), String> {
    let directory = format!("users/{username}/secrets");
    save(
        disk,
        &format!("{directory}/{SAVED_FILENAME}"),
        &format!("{directory}/uncrypted.blob"),
        blob,
    )
    .await
}

async fn save(disk: DeviceHandle, path: &str, legacy: &str, blob: &[u8]) -> Result<(), String> {
    if !trueosfs::file_write_all_async(disk, path, blob)
        .await
        .map_err(|e| format!("sealed key write: {e:?}"))?
    {
        return Err("sealed key allocation failed".into());
    }
    let readback = trueosfs::file_out_async(disk, path)
        .await
        .map_err(|e| format!("sealed key readback: {e:?}"))?;
    if readback.as_deref() != Some(blob) {
        return Err("sealed key readback mismatch".into());
    }
    // A TPM replacement must not leave the old plaintext pathname live.
    trueosfs::file_delete_async(disk, legacy)
        .await
        .map_err(|e| format!("legacy key removal: {e:?}"))?;
    Ok(())
}

async fn unseal(disk: DeviceHandle) -> Result<Option<(String, Zeroizing<[u8; 32]>)>, String> {
    let Some(blob) = trueosfs::file_out_async(disk, PATH)
        .await
        .map_err(|e| format!("sealed key read: {e:?}"))?
    else {
        return Ok(None);
    };
    let payload = blob
        .strip_prefix(MAGIC)
        .ok_or("invalid machine-key blob header")?;
    let (&len, payload) = payload
        .split_first()
        .ok_or("missing machine-key account length")?;
    let (name, sealed) = payload
        .split_at_checked(usize::from(len))
        .ok_or("invalid machine-key account length")?;
    let name = core::str::from_utf8(name).map_err(|_| "invalid machine-key account")?;
    let username =
        crate::crypt::canonical_username(name).map_err(|_| "invalid machine-key account")?;
    if username != name {
        return Err("noncanonical machine-key account".into());
    }
    let key = tpm_service::unseal(sealed.to_vec()).await?;
    Ok(Some((username, key)))
}

pub(crate) async fn restore_account(disk: DeviceHandle) {
    if crate::crypt::status().configured {
        return;
    }
    let result = async {
        let Some((username, key)) = unseal(disk).await? else {
            return Ok(None);
        };
        let path = alloc::format!("users/{username}/secrets/cry.v1.aes256gcm");
        let envelope = Zeroizing::new(
            trueosfs::file_out_async(disk, &path)
                .await
                .map_err(|e| alloc::format!("credential read: {e:?}"))?
                .ok_or_else(|| String::from("credential missing"))?,
        );
        crate::crypt::unlock_persisted(&username, &key, &envelope)
            .map(Some)
            .map_err(|_| String::from("credential unlock rejected"))
    }
    .await;
    match result {
        Ok(Some(report)) => crate::log_important!(target: "storage";
            "cry-boot: provider=tpm2 status=unlocked username={} generation={} session=locked\n",
            report.username, report.generation),
        Ok(None) => {}
        Err(error) => crate::log_warn!(target: "storage";
            "cry-boot: provider=tpm2 status=failed reason={}\n", error),
    }
}
