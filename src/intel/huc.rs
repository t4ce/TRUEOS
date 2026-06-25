use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

const HUC_MODULE_STRING: &[u8] = b"trueos.fw.huc.tgl";
const DMA_ADDR_0_LOW: usize = 0x0000_C300;
const DMA_ADDR_0_HIGH: usize = 0x0000_C304;
const DMA_ADDR_1_LOW: usize = 0x0000_C308;
const DMA_ADDR_1_HIGH: usize = 0x0000_C30C;
const DMA_COPY_SIZE: usize = 0x0000_C310;
const DMA_CTRL: usize = 0x0000_C314;
const DMA_ADDRESS_SPACE_WOPCM: u32 = 7 << 16;
const DMA_ADDRESS_SPACE_GGTT: u32 = 8 << 16;
const START_DMA: u32 = 1 << 0;
const HUC_UKERNEL: u32 = 1 << 9;
const GUC_ACTION_AUTHENTICATE_HUC: u32 = 0x4000;
const GEN11_HUC_KERNEL_LOAD_INFO: usize = 0x0000_C1DC;
const HUC_LOAD_SUCCESSFUL: u32 = 1 << 0;
const HUC_DMA_POLL_ITERS: usize = 20_000;
const HUC_AUTH_POLL_ITERS: usize = 50_000;

static PRESENT: AtomicBool = AtomicBool::new(false);
static AUTHENTICATED: AtomicBool = AtomicBool::new(false);
static LAST_STATUS2: AtomicU32 = AtomicU32::new(0);
static RSA_DATA: Mutex<Option<RsaData>> = Mutex::new(None);

#[derive(Copy, Clone)]
struct RsaData {
    phys: u64,
    virt: *mut u8,
    len: usize,
    gpu: u64,
    bytes: usize,
}

unsafe impl Send for RsaData {}

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
    let rsa_gpu = stage_rsa_only(blob, css.rsa_offset, css.rsa_size);
    let (major, minor, patch) = crate::intel::uc_fw::version_triplet(css.sw_version);
    crate::log!(
        "intel/huc: firmware staged phys=0x{:X} gpu=0x{:X} len=0x{:X} xfer=0x{:X} rsa_gpu=0x{:X} rsa_mode=separate-ggtt-page css_type={} vendor=0x{:04X} sw={}.{}.{} raw=0x{:08X} ggtt_slot=i915-huc-type1-2m\n",
        phys,
        crate::intel::GPU_VA_HUC_FW_BASE,
        len,
        css.xfer_len,
        rsa_gpu,
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

pub(crate) fn map_rsa(dev: crate::intel::Dev) -> bool {
    let Some(rsa) = *RSA_DATA.lock() else {
        crate::log!(
            "intel/huc: rsa-map skipped reason=rsa-stage-missing fallback=firmware-inline-rsa\n"
        );
        return true;
    };
    let mapped = crate::intel::map_ggtt(dev, rsa.phys, rsa.len, rsa.gpu);
    crate::log!(
        "intel/huc: rsa-map mapped={} phys=0x{:X} virt=0x{:X} gpu=0x{:X} len=0x{:X} bytes=0x{:X} source=i915-rsa-data-vma-model\n",
        mapped as u8,
        rsa.phys,
        rsa.virt as usize,
        rsa.gpu,
        rsa.len,
        rsa.bytes
    );
    mapped
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
    let rsa_gpu = rsa_gpu().unwrap_or_else(|| fw.gpu.saturating_add(fw.rsa_offset as u64) as u32);
    let before = huc_auth_status(dev);
    if crate::intel::guc_ctb::enabled() {
        let result =
            crate::intel::guc_ctb::send_hxg_action(dev, GUC_ACTION_AUTHENTICATE_HUC, &[rsa_gpu]);
        if result.accepted {
            let mut status = before;
            let mut poll_iters = 0usize;
            while poll_iters < HUC_AUTH_POLL_ITERS {
                status = huc_auth_status(dev);
                if huc_auth_verified(status) {
                    break;
                }
                poll_iters += 1;
                core::hint::spin_loop();
            }
            LAST_STATUS2.store(status, Ordering::Release);
            let verified = huc_auth_verified(status);
            AUTHENTICATED.store(verified, Ordering::Release);
            crate::log!(
                "intel/huc: auth-via-ctb accepted=1 rsa_gpu=0x{:X} status_reg=GEN11_HUC_KERNEL_LOAD_INFO status_before=0x{:08X} status=0x{:08X} verified={} response=0x{:08X} response_type={} error={} h2g_poll_iters={} g2h_poll_iters={} status_poll_iters={} does_not_prove=media-codec-path\n",
                rsa_gpu,
                before,
                status,
                verified as u8,
                result.response,
                result.response_type,
                result.error,
                result.h2g_poll_iters,
                result.g2h_poll_iters,
                poll_iters
            );
            return verified;
        }
        let status = huc_auth_status(dev);
        LAST_STATUS2.store(status, Ordering::Release);
        AUTHENTICATED.store(false, Ordering::Release);
        crate::log!(
            "intel/huc: auth-via-ctb accepted=0 rsa_gpu=0x{:X} status_reg=GEN11_HUC_KERNEL_LOAD_INFO status_before=0x{:08X} status=0x{:08X} verified=0 response=0x{:08X} response_type={} error={} h2g_poll_iters={} g2h_poll_iters={} fallback=mmio does_not_prove=media-codec-path\n",
            rsa_gpu,
            before,
            status,
            result.response,
            result.response_type,
            result.error,
            result.h2g_poll_iters,
            result.g2h_poll_iters
        );
    }
    let result =
        crate::intel::guc::send_h2g_mmio_action(dev, GUC_ACTION_AUTHENTICATE_HUC, &[rsa_gpu]);
    if !result.accepted {
        let status = huc_auth_status(dev);
        LAST_STATUS2.store(status, Ordering::Release);
        AUTHENTICATED.store(false, Ordering::Release);
        crate::log!(
            "intel/huc: auth-via-guc accepted=0 rsa_gpu=0x{:X} status_reg=GEN11_HUC_KERNEL_LOAD_INFO status_before=0x{:08X} status=0x{:08X} verified=0 response=0x{:08X} response_type={} error={} h2g_poll_iters={} status_poll_iters=0 blocker=ctb-required-or-auth-rejected next=guc-ctb-register does_not_prove=media-codec-path\n",
            rsa_gpu,
            before,
            status,
            result.response,
            result.response_type,
            result.error,
            result.poll_iters
        );
        return false;
    }
    let mut status = before;
    let mut poll_iters = 0usize;
    while poll_iters < HUC_AUTH_POLL_ITERS {
        status = huc_auth_status(dev);
        if huc_auth_verified(status) {
            break;
        }
        poll_iters += 1;
        core::hint::spin_loop();
    }
    LAST_STATUS2.store(status, Ordering::Release);
    let verified = huc_auth_verified(status);
    AUTHENTICATED.store(verified, Ordering::Release);
    crate::log!(
        "intel/huc: auth-via-guc accepted={} rsa_gpu=0x{:X} status_reg=GEN11_HUC_KERNEL_LOAD_INFO status_before=0x{:08X} status=0x{:08X} verified={} response=0x{:08X} response_type={} error={} h2g_poll_iters={} status_poll_iters={} does_not_prove=media-codec-path\n",
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

fn huc_auth_status(dev: crate::intel::Dev) -> u32 {
    crate::intel::mmio_read(dev, GEN11_HUC_KERNEL_LOAD_INFO)
}

fn huc_auth_verified(status: u32) -> bool {
    (status & HUC_LOAD_SUCCESSFUL) == HUC_LOAD_SUCCESSFUL
}

fn stage_rsa_only(blob: &[u8], rsa_offset: usize, rsa_size: usize) -> u64 {
    if rsa_size == 0
        || rsa_offset
            .checked_add(rsa_size)
            .map_or(true, |end| end > blob.len())
    {
        *RSA_DATA.lock() = None;
        return crate::intel::GPU_VA_HUC_FW_BASE + rsa_offset as u64;
    }
    let len = crate::intel::WARM_ALIGN;
    let Some((phys, virt)) = crate::dma::alloc(len, crate::intel::WARM_ALIGN) else {
        *RSA_DATA.lock() = None;
        return crate::intel::GPU_VA_HUC_FW_BASE + rsa_offset as u64;
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, len);
        core::ptr::copy_nonoverlapping(blob.as_ptr().add(rsa_offset), virt, rsa_size.min(len));
    }
    crate::intel::dma_flush(virt, len);
    let rsa = RsaData {
        phys,
        virt,
        len,
        gpu: crate::intel::GPU_VA_HUC_RSA_BASE,
        bytes: rsa_size.min(len),
    };
    *RSA_DATA.lock() = Some(rsa);
    rsa.gpu
}

fn rsa_gpu() -> Option<u32> {
    RSA_DATA.lock().map(|rsa| rsa.gpu as u32)
}

pub(crate) fn upload_via_dma(dev: crate::intel::Dev, fw: crate::intel::Buf) -> bool {
    if fw.len == 0 {
        crate::log!("intel/huc: dma-upload skipped reason=firmware-missing-or-invalid\n");
        return false;
    }
    let src = fw.gpu + fw.css_offset as u64;
    crate::intel::mmio_write(dev, DMA_ADDR_0_LOW, src as u32);
    crate::intel::mmio_write(dev, DMA_ADDR_0_HIGH, ((src >> 32) as u32) | DMA_ADDRESS_SPACE_GGTT);
    crate::intel::mmio_write(dev, DMA_ADDR_1_LOW, 0);
    crate::intel::mmio_write(dev, DMA_ADDR_1_HIGH, DMA_ADDRESS_SPACE_WOPCM);
    crate::intel::mmio_write(dev, DMA_COPY_SIZE, fw.xfer_len as u32);
    crate::intel::mmio_write(dev, DMA_CTRL, crate::intel::mask_en(HUC_UKERNEL | START_DMA));

    let mut poll_iters = 0usize;
    while poll_iters < HUC_DMA_POLL_ITERS {
        if (crate::intel::mmio_read(dev, DMA_CTRL) & START_DMA) == 0 {
            break;
        }
        poll_iters += 1;
        core::hint::spin_loop();
    }
    let ctrl_after = crate::intel::mmio_read(dev, DMA_CTRL);
    crate::intel::mmio_write(dev, DMA_CTRL, crate::intel::mask_dis(HUC_UKERNEL));
    let done = (ctrl_after & START_DMA) == 0;
    crate::log!(
        "intel/huc: dma-upload done={} src_gpu=0x{:X} dst_wopcm=0x0 xfer=0x{:X} ctrl_after=0x{:08X} poll_iters={} next=guc-authenticate-huc\n",
        done as u8,
        src,
        fw.xfer_len,
        ctrl_after,
        poll_iters
    );
    done
}
