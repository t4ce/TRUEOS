//! Disk round-trip for a disposable, hardware-bound TPM sealed object.
use alloc::format;
use alloc::string::String;
use tpm2_protocol::{
    TpmMarshal, TpmUnmarshal, TpmWriter,
    basic::TpmHandle,
    data::{
        Tpm2bPrivate, Tpm2bPublic, TpmAlgId, TpmEccCurve, TpmaObject, TpmsAuthCommand,
        TpmsEccParms, TpmsKeyedhashParms, TpmsSensitiveCreate, TpmtPublic, TpmtSymDef,
        TpmuPublicId, TpmuPublicParms, TpmuSymKeyBits, TpmuSymMode,
    },
    frame::{
        TpmCreateCommand, TpmCreatePrimaryCommand, TpmCreatePrimaryResponse, TpmCreateResponse,
        TpmFlushContextCommand, TpmFlushContextResponse, TpmLoadCommand, TpmLoadResponse,
        TpmUnsealCommand, TpmUnsealResponse,
    },
};
use zeroize::Zeroizing;

use crate::{
    disc::block::DeviceHandle,
    efi::acpi::tpm2::{Crb, MAX_FRAME_BYTES},
    r::fs::trueosfs,
};

const DIR: &str = "testTpm";
const PATH: &str = "testTpm/bringupUser";
const MAGIC: &[u8] = b"TRUEOS-TPM-TEST1";
const OWNER: u32 = 0x4000_0001;
const PASSWORD_SESSION: u32 = 0x4000_0009;
type Result<T> = core::result::Result<T, String>;

fn failure(stage: &str, error: impl core::fmt::Debug) -> String {
    format!("{stage}: {error:?}")
}

async fn primary(crb: &Crb, auth: &[TpmsAuthCommand]) -> Result<TpmHandle> {
    let request = TpmCreatePrimaryCommand {
        handles: [OWNER.into()],
        in_public: TpmtPublic {
            object_type: TpmAlgId::Ecc,
            name_alg: TpmAlgId::Sha256,
            object_attributes: TpmaObject::FIXED_TPM
                | TpmaObject::FIXED_PARENT
                | TpmaObject::SENSITIVE_DATA_ORIGIN
                | TpmaObject::USER_WITH_AUTH
                | TpmaObject::NO_DA
                | TpmaObject::RESTRICTED
                | TpmaObject::DECRYPT,
            parameters: TpmuPublicParms::Ecc(TpmsEccParms {
                symmetric: TpmtSymDef {
                    algorithm: TpmAlgId::Aes,
                    key_bits: TpmuSymKeyBits::Aes(128u16.into()),
                    mode: TpmuSymMode::Aes(TpmAlgId::Cfb),
                },
                curve_id: TpmEccCurve::NistP256,
                ..Default::default()
            }),
            unique: TpmuPublicId::Ecc(Default::default()),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    };
    let response: TpmCreatePrimaryResponse = crb
        .execute(&request, auth)
        .await
        .map_err(|e| failure("CreatePrimary", e))?;
    Ok(response.handles[0])
}

async fn flush(crb: &Crb, handle: TpmHandle) -> Result<()> {
    let _: TpmFlushContextResponse = crb
        .execute(
            &TpmFlushContextCommand {
                handles: [],
                flush_handle: handle,
            },
            &[],
        )
        .await
        .map_err(|e| failure("FlushContext", e))?;
    Ok(())
}

async fn store(
    crb: &Crb,
    auth: &[TpmsAuthCommand],
    disk: DeviceHandle,
    parent: TpmHandle,
    data: &[u8],
) -> Result<()> {
    let request = TpmCreateCommand {
        handles: [parent],
        in_sensitive: TpmsSensitiveCreate {
            user_auth: Default::default(),
            data: data.try_into().map_err(|e| failure("sensitive data", e))?,
        }
        .into(),
        in_public: TpmtPublic {
            object_type: TpmAlgId::KeyedHash,
            name_alg: TpmAlgId::Sha256,
            object_attributes: TpmaObject::FIXED_TPM
                | TpmaObject::FIXED_PARENT
                | TpmaObject::USER_WITH_AUTH
                | TpmaObject::NO_DA,
            parameters: TpmuPublicParms::KeyedHash(TpmsKeyedhashParms::default()),
            unique: TpmuPublicId::KeyedHash(Default::default()),
            ..Default::default()
        }
        .into(),
        outside_info: Default::default(),
        creation_pcr: Default::default(),
    };
    let response: TpmCreateResponse = crb
        .execute(&request, auth)
        .await
        .map_err(|e| failure("Create", e))?;
    let mut blob = [0; MAX_FRAME_BYTES];
    let mut writer = TpmWriter::new(&mut blob);
    writer
        .write_bytes(MAGIC)
        .and_then(|()| response.out_private.marshal(&mut writer))
        .and_then(|()| response.out_public.marshal(&mut writer))
        .map_err(|e| failure("encode sealed object", e))?;
    if !trueosfs::file_write_all_async(disk, PATH, writer.as_bytes())
        .await
        .map_err(|e| failure("write sealed file", e))?
    {
        return Err("sealed file allocation failed".into());
    }
    Ok(())
}

async fn read_matches(
    crb: &Crb,
    auth: &[TpmsAuthCommand],
    disk: DeviceHandle,
    parent: TpmHandle,
    expected: &[u8],
) -> Result<()> {
    let blob = trueosfs::file_out_async(disk, PATH)
        .await
        .map_err(|e| failure("read sealed file", e))?
        .ok_or_else(|| String::from("sealed file missing"))?;
    let bytes = blob
        .strip_prefix(MAGIC)
        .ok_or_else(|| String::from("sealed file header"))?;
    let (in_private, bytes) =
        Tpm2bPrivate::unmarshal(bytes).map_err(|e| failure("decode private", e))?;
    let (in_public, rest) =
        Tpm2bPublic::unmarshal(bytes).map_err(|e| failure("decode public", e))?;
    if !rest.is_empty() {
        return Err("unexpected data after sealed object".into());
    }
    let response: TpmLoadResponse = crb
        .execute(
            &TpmLoadCommand {
                handles: [parent],
                in_private,
                in_public,
            },
            auth,
        )
        .await
        .map_err(|e| failure("Load", e))?;
    let handle = response.handles[0];
    let unsealed = crb
        .execute::<TpmUnsealResponse>(&TpmUnsealCommand { handles: [handle] }, auth)
        .await;
    let cleanup = flush(crb, handle).await;
    let response = unsealed.map_err(|e| failure("Unseal", e))?;
    cleanup?;
    let recovered = Zeroizing::new(
        <[u8; 32]>::try_from(response.out_data.as_ref())
            .map_err(|_| String::from("unexpected unsealed payload length"))?,
    );
    if &recovered[..] != expected {
        return Err("unsealed payload mismatch".into());
    }
    Ok(())
}

async fn round_trip(crb: &Crb, auth: &[TpmsAuthCommand], disk: DeviceHandle) -> Result<()> {
    let mut first = Zeroizing::new([0; 32]);
    let mut second = Zeroizing::new([0; 32]);
    if !crate::tyche::fill_bytes(&mut *first)
        || !crate::tyche::fill_bytes(&mut *second)
        || first == second
    {
        return Err("could not generate two distinct dummy keys".into());
    }
    let parent = primary(crb, auth).await?;
    let work = async {
        store(crb, auth, disk, parent, &first[..]).await?;
        read_matches(crb, auth, disk, parent, &first[..]).await?;
        store(crb, auth, disk, parent, &second[..]).await?;
        read_matches(crb, auth, disk, parent, &second[..]).await
    }
    .await;
    let cleanup = flush(crb, parent).await;
    work?;
    cleanup
}

pub(super) async fn run(crb: &Crb) -> Result<()> {
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
    let auth = [TpmsAuthCommand {
        session_handle: PASSWORD_SESSION.into(),
        ..Default::default()
    }];
    let work = round_trip(crb, &auth, disk).await;
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
