use core::sync::atomic::{AtomicBool, Ordering};

const DMC_MODULE_STRING: &[u8] = b"trueos.fw.dmc";

static PRESENT: AtomicBool = AtomicBool::new(false);
static LOAD_PATH_WIRED: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug)]
struct DmcPackageInfo {
    package_type: u32,
    header_size_dw: u32,
    header_version: u32,
    package_size_dw: u32,
    version_raw: u32,
    major: u32,
    minor: u32,
}

pub(crate) fn wire_load_path(_dev: crate::intel::Dev) {
    let Some(bytes) = crate::limine::module_bytes_by_string(DMC_MODULE_STRING) else {
        PRESENT.store(false, Ordering::Release);
        LOAD_PATH_WIRED.store(false, Ordering::Release);
        crate::log!("intel/dmc: load-path present=0 action=skip reason=module-missing\n");
        return;
    };
    PRESENT.store(true, Ordering::Release);
    let Some(info) = parse_package(bytes) else {
        LOAD_PATH_WIRED.store(false, Ordering::Release);
        crate::log!(
            "intel/dmc: load-path present=1 action=skip reason=invalid-package len=0x{:X}\n",
            bytes.len()
        );
        return;
    };
    LOAD_PATH_WIRED.store(true, Ordering::Release);
    crate::log!(
        "intel/dmc: load-path wired package_type={} header_dw={} header_version=0x{:08X} len=0x{:X} package_size_dw=0x{:X} version={}.{} raw=0x{:08X} action=defer-mmio-program reason=program-table-parser-not-enabled does_not_prove=dmc-running\n",
        info.package_type,
        info.header_size_dw,
        info.header_version,
        bytes.len(),
        info.package_size_dw,
        info.major,
        info.minor,
        info.version_raw
    );
}

fn parse_package(bytes: &[u8]) -> Option<DmcPackageInfo> {
    let package_type = crate::intel::uc_fw::read_le_u32(bytes, 0);
    let header_size_dw = crate::intel::uc_fw::read_le_u32(bytes, 4);
    let header_version = crate::intel::uc_fw::read_le_u32(bytes, 8);
    let package_size_dw = crate::intel::uc_fw::read_le_u32(bytes, 24);
    let version_raw = crate::intel::uc_fw::read_le_u32(bytes, 0x58);
    if package_type != 9 || header_size_dw < 16 {
        return None;
    }
    if (package_size_dw as usize).checked_mul(4)? > bytes.len() {
        return None;
    }
    Some(DmcPackageInfo {
        package_type,
        header_size_dw,
        header_version,
        package_size_dw,
        version_raw,
        major: (version_raw >> 16) & 0xFFFF,
        minor: version_raw & 0xFFFF,
    })
}
