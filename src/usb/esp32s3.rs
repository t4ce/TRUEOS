use super::cdc_acm::{self, AttachParams};
use crate::debugconf;
use crate::truelog;

const ESPRESSIF_VID: u16 = 0x303A;

pub async fn try_attach(dev_vid: u16, _dev_pid: u16, params: AttachParams<'_>) -> Result<(), ()> {
    if dev_vid != ESPRESSIF_VID {
        return Err(());
    }

    cdc_acm::attach_device(params).await?;

    if let Err(err) = truelog::promote_backend(cdc_acm::backend()) {
        debugconf!(
            "serial: failed to promote esp32s3 cdc backend err={:?}\n",
            err
        );
    }

    Ok(())
}
