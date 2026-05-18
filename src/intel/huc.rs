use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const HUC_MODULE_STRING: &[u8] = b"trueos.fw.huc.candidate.tgl";
const GUC_ACTION_AUTHENTICATE_HUC: u32 = 0x4000;
const HUC_STATUS2: usize = 0x0000_D3B0;
const HUC_FW_VERIFIED: u32 = 1 << 7;
const HUC_AUTH_POLL_ITERS: usize = 50_000;

static PRESENT: AtomicBool = AtomicBool::new(false);
static AUTHENTICATED: AtomicBool = AtomicBool::new(false);
static LAST_STATUS2: AtomicU32 = AtomicU32::new(0);

pub(crate) fn present() -> bool {
    PRESENT.load(Ordering::Acquire)
}

pub(crate) fn authenticated() -> bool {
    AUTHENTICATED.load(Ordering::Acquire)
}

pub(crate) fn load_fw() -> crate::intel::Buf {
    let Some(blob) = crate::limine::module_bytes_by_string(HUC_MODULE_STRING) else {
        PRESENT.store(false, Ordering::Release);
        return crate::intel::empty();
    };
    PRESENT.store(true, Ordering::Release);
    let Some(css) = crate::intel::uc_fw::parse_css(blob) else {
        return crate::intel::empty();
    };
    let len = blob.len().div_ceil(crate::intel::WARM_ALIGN) * crate::intel::WARM_ALIGN;
    let Some((phys, virt)) = crate::dma::alloc(len, crate::intel::WARM_ALIGN) else {
        return crate::intel::empty();
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, len);
        core::ptr::copy_nonoverlapping(blob.as_ptr(), virt, blob.len());
    }
    crate::intel::dma_flush(virt, len);
    let (major, minor, patch) = crate::intel::uc_fw::version_triplet(css.sw_version);
    crate::log!(
        "intel/huc: firmware staged phys=0x{:X} gpu=0x{:X} len=0x{:X} xfer=0x{:X} rsa_gpu=0x{:X} css_type={} vendor=0x{:04X} sw={}.{}.{} raw=0x{:08X}\n",
        phys,
        crate::intel::GPU_VA_HUC_FW_BASE,
        len,
        css.xfer_len,
        crate::intel::GPU_VA_HUC_FW_BASE + css.rsa_offset as u64,
        css.module_type,
        css.vendor,
        major,
        minor,
        patch,
        css.sw_version
    );
    crate::intel::Buf {
        phys,
        virt,
        len,
        gpu: crate::intel::GPU_VA_HUC_FW_BASE,
        css_offset: css.offset,
        xfer_len: css.xfer_len,
        private_data_size: css.private_data_size as usize,
        rsa_offset: css.rsa_offset,
        rsa_size: css.rsa_size,
    }
}

pub(crate) fn authenticate_via_guc(dev: crate::intel::Dev, fw: crate::intel::Buf) -> bool {
    if fw.len == 0 {
        crate::log!("intel/huc: auth skipped reason=firmware-missing-or-invalid\n");
        return false;
    }
    if !crate::intel::guc_ready() {
        crate::log!("intel/huc: auth skipped reason=guc-not-ready\n");
        return false;
    }
    let rsa_gpu = fw.gpu.saturating_add(fw.rsa_offset as u64) as u32;
    let before = crate::intel::mmio_read(dev, HUC_STATUS2);
    let result =
        crate::intel::guc::send_h2g_mmio_action(dev, GUC_ACTION_AUTHENTICATE_HUC, &[rsa_gpu]);
    let mut status = before;
    let mut poll_iters = 0usize;
    while poll_iters < HUC_AUTH_POLL_ITERS {
        status = crate::intel::mmio_read(dev, HUC_STATUS2);
        if (status & HUC_FW_VERIFIED) != 0 {
            break;
        }
        poll_iters += 1;
        core::hint::spin_loop();
    }
    LAST_STATUS2.store(status, Ordering::Release);
    let verified = (status & HUC_FW_VERIFIED) != 0;
    AUTHENTICATED.store(verified, Ordering::Release);
    crate::log!(
        "intel/huc: auth-via-guc accepted={} rsa_gpu=0x{:X} status2_before=0x{:08X} status2=0x{:08X} verified={} response=0x{:08X} response_type={} error={} h2g_poll_iters={} status_poll_iters={} does_not_prove=media-codec-path\n",
        result.accepted as u8,
        rsa_gpu,
        before,
        status,
        verified as u8,
        result.response,
        result.response_type,
        result.error,
        result.poll_iters,
        poll_iters
    );
    verified
}

pub(crate) fn status2() -> u32 {
    LAST_STATUS2.load(Ordering::Acquire)
}
