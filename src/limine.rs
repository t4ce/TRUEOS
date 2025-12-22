#[repr(C)]
pub struct LimineSmpRequest {
    _id: [u64; 4],
    _revision: u64,
    pub response: *const LimineSmpResponse,
    pub flags: u64,
}

unsafe impl Sync for LimineSmpRequest {}

#[repr(C)]
pub struct LimineHhdmRequest {
    _id: [u64; 4],
    _revision: u64,
    pub response: *const LimineHhdmResponse,
}

unsafe impl Sync for LimineHhdmRequest {}

#[repr(C)]
pub struct LimineHhdmResponse {
    pub revision: u64,
    pub offset: u64,
}

#[repr(C)]
pub struct LimineMemmapRequest {
    _id: [u64; 4],
    _revision: u64,
    pub response: *const LimineMemmapResponse,
}

unsafe impl Sync for LimineMemmapRequest {}

#[repr(C)]
pub struct LimineMemmapResponse {
    pub revision: u64,
    pub entry_count: u64,
    pub entries: *const *const LimineMemmapEntry,
}

#[repr(C)]
pub struct LimineMemmapEntry {
    pub base: u64,
    pub length: u64,
    pub typ: u64,
}

pub fn memmap_entries() -> Option<&'static [*const LimineMemmapEntry]> {
    let resp_ptr = LIMINE_MEMMAP_REQUEST.response;
    if resp_ptr.is_null() {
        return None;
    }
    let resp = unsafe { &*resp_ptr };
    let entries = resp.entries;
    let count = resp.entry_count as usize;
    if entries.is_null() || count == 0 {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(entries, count) })
}

#[repr(C)]
pub struct LimineSmpResponse {
    pub revision: u64,
    pub flags: u32,
    pub bsp_lapic_id: u32,
    pub cpu_count: u64,
    pub cpus: *const *mut LimineSmpCpu,
}

#[repr(C)]
pub struct LimineSmpCpu {
    pub processor_id: u32,
    pub lapic_id: u32,
    pub reserved: u64,
    pub goto_address: extern "C" fn(*mut LimineSmpCpu),
    pub extra_argument: u64,
}

#[used]
#[link_section = ".limine_requests"]
static LIMINE_BASE_REVISION: [u64; 3] = [
    0xf9562b2d5c95a6c8,
    0x6a7b384944536bdc,
    0,
];

#[used]
#[link_section = ".limine_requests"]
pub static LIMINE_SMP_REQUEST: LimineSmpRequest = LimineSmpRequest {
    _id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x95a67b819a1b857e,
        0xa0b61b723b6a73e0,
    ],
    _revision: 0,
    response: core::ptr::null(),
    flags: 0,
};

#[used]
#[link_section = ".limine_requests"]
pub static LIMINE_HHDM_REQUEST: LimineHhdmRequest = LimineHhdmRequest {
    _id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x48dcf1cb8ad2b852,
        0x63984e959a98244b,
    ],
    _revision: 0,
    response: core::ptr::null(),
};

#[used]
#[link_section = ".limine_requests"]
pub static LIMINE_MEMMAP_REQUEST: LimineMemmapRequest = LimineMemmapRequest {
    _id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x67cf3d9d378a806f,
        0xe304acdfc50c3c62,
    ],
    _revision: 0,
    response: core::ptr::null(),
};

pub fn hhdm_offset() -> Option<u64> {
    let resp_ptr = LIMINE_HHDM_REQUEST.response;
    if resp_ptr.is_null() {
        return None;
    }
    let resp = unsafe { &*resp_ptr };
    Some(resp.offset)
}