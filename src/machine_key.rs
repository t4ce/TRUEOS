//! Temporary machine-key provider. The file backend is deliberately plaintext.
//! Callers seal/unseal through this boundary; a TPM backend can replace it later.
use crate::{disc::block::DeviceHandle, r::fs::trueosfs};
use alloc::{string::String, vec::Vec};
use zeroize::Zeroizing;

const PATH: &str = "trueos/uncrypted.blob";
const MAGIC: &[u8; 8] = b"TRUNKEY1";

fn encode(username: &str, key: &[u8; 32]) -> Zeroizing<Vec<u8>> {
    let mut blob = Zeroizing::new(Vec::new());
    blob.extend_from_slice(MAGIC);
    blob.push(username.len() as u8);
    blob.extend_from_slice(username.as_bytes());
    blob.extend_from_slice(key);
    blob
}

fn decode(blob: &[u8]) -> Result<(String, Zeroizing<[u8; 32]>), String> {
    if blob.len() < 9 || &blob[..8] != MAGIC {
        return Err(String::from("invalid machine-key blob header"));
    }
    let len = blob[8] as usize;
    if blob.len() != 9 + len + 32 {
        return Err(String::from("invalid machine-key blob length"));
    }
    let name = core::str::from_utf8(&blob[9..9 + len])
        .map_err(|_| String::from("invalid machine-key account"))?;
    let username = crate::crypt::canonical_username(name)
        .map_err(|_| String::from("invalid machine-key account"))?;
    if username != name {
        return Err(String::from("noncanonical machine-key account"));
    }
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&blob[9 + len..]);
    Ok((username, key))
}

pub(crate) async fn seal(disk: DeviceHandle, username: &str, key: &[u8; 32]) -> Result<(), String> {
    let canonical = crate::crypt::canonical_username(username)
        .map_err(|_| String::from("invalid machine-key account"))?;
    let blob = encode(&canonical, key);
    if !trueosfs::dir_create_all_async(disk, "trueos")
        .await
        .map_err(|e| alloc::format!("machine-key directory: {e:?}"))?
    {
        return Err(String::from("machine-key directory allocation failed"));
    }
    if !trueosfs::file_write_all_async(disk, PATH, &blob)
        .await
        .map_err(|e| alloc::format!("machine-key write: {e:?}"))?
    {
        return Err(String::from("machine-key allocation failed"));
    }
    let readback = Zeroizing::new(
        trueosfs::file_out_async(disk, PATH)
            .await
            .map_err(|e| alloc::format!("machine-key readback: {e:?}"))?
            .ok_or_else(|| String::from("machine-key readback missing"))?,
    );
    if *readback != *blob {
        return Err(String::from("machine-key readback mismatch"));
    }
    Ok(())
}

pub(crate) async fn unseal(
    disk: DeviceHandle,
) -> Result<Option<(String, Zeroizing<[u8; 32]>)>, String> {
    let Some(blob) = trueosfs::file_out_async(disk, PATH)
        .await
        .map_err(|e| alloc::format!("machine-key read: {e:?}"))?
    else {
        return Ok(None);
    };
    decode(&Zeroizing::new(blob)).map(Some)
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
            "cry-boot: provider=plaintext-file status=unlocked username={} generation={} session=locked\n",
            report.username, report.generation),
        Ok(None) => {}
        Err(error) => crate::log_warn!(target: "storage";
            "cry-boot: provider=plaintext-file status=failed reason={}\n", error),
    }
}
