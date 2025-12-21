#[repr(C)]
pub struct LimineSmpRequest {
    _id: [u64; 4],
    _revision: u64,
    pub response: *const LimineSmpResponse,
    pub flags: u64,
}

unsafe impl Sync for LimineSmpRequest {}

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
