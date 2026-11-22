//! Hardware-bound 32-byte sealed objects with a transient storage parent.
use alloc::{format, string::String, vec::Vec};
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

use crate::efi::acpi::tpm2::{Device, MAX_FRAME_BYTES};

const MAGIC: &[u8] = b"TRUEOS-TPM-KEY1";
const OWNER: u32 = 0x4000_0001;
const PASSWORD_SESSION: u32 = 0x4000_0009;
type Result<T> = core::result::Result<T, String>;

fn failure(stage: &str, error: impl core::fmt::Debug) -> String {
    format!("{stage}: {error:?}")
}

async fn primary(device: &Device, auth: &[TpmsAuthCommand]) -> Result<TpmHandle> {
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
    let response: TpmCreatePrimaryResponse = device
        .execute(&request, auth)
        .await
        .map_err(|e| failure("CreatePrimary", e))?;
    Ok(response.handles[0])
}

async fn flush(device: &Device, handle: TpmHandle) -> Result<()> {
    let _: TpmFlushContextResponse = device
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

async fn create(
    device: &Device,
    auth: &[TpmsAuthCommand],
    parent: TpmHandle,
    data: &[u8; 32],
) -> Result<Vec<u8>> {
    let request = TpmCreateCommand {
        handles: [parent],
        in_sensitive: TpmsSensitiveCreate {
            user_auth: Default::default(),
            data: data
                .as_slice()
                .try_into()
                .map_err(|e| failure("sensitive data", e))?,
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
    let response: TpmCreateResponse = device
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
    Ok(writer.as_bytes().to_vec())
}

async fn load(
    device: &Device,
    auth: &[TpmsAuthCommand],
    parent: TpmHandle,
    blob: &[u8],
) -> Result<Zeroizing<[u8; 32]>> {
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
    let response: TpmLoadResponse = device
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
    let unsealed = device
        .execute::<TpmUnsealResponse>(&TpmUnsealCommand { handles: [handle] }, auth)
        .await;
    let cleanup = flush(device, handle).await;
    let response = unsealed.map_err(|e| failure("Unseal", e))?;
    cleanup?;
    let recovered = Zeroizing::new(
        <[u8; 32]>::try_from(response.out_data.as_ref())
            .map_err(|_| String::from("unexpected unsealed payload length"))?,
    );
    Ok(recovered)
}

fn password_session() -> [TpmsAuthCommand; 1] {
    [TpmsAuthCommand {
        session_handle: PASSWORD_SESSION.into(),
        ..Default::default()
    }]
}

pub(super) async fn seal(device: &Device, data: &[u8; 32]) -> Result<Vec<u8>> {
    let auth = password_session();
    let parent = primary(device, &auth).await?;
    let result = create(device, &auth, parent, data).await;
    let cleanup = flush(device, parent).await;
    let blob = result?;
    cleanup?;
    Ok(blob)
}

pub(super) async fn unseal(device: &Device, blob: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let auth = password_session();
    let parent = primary(device, &auth).await?;
    let result = load(device, &auth, parent, blob).await;
    let cleanup = flush(device, parent).await;
    let key = result?;
    cleanup?;
    Ok(key)
}
