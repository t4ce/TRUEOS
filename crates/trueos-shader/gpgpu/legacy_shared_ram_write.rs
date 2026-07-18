// Legacy diagnostic dataport probe. This hand-written EU blob is not the final
// Burn/matmul kernel path; it is only a bounded oscilloscope for the current
// phase: if the dispatched EU thread can write shared RAM, then we know it
// decoded enough instructions to issue a dataport side effect before/around EOT.
static GPU_PROGRAM_SHARED_RAM_WRITE_CODE: [u32; 12] = [
    0xA07E0061,
    0x00010000,
    0xA0780061,
    GPU_PROGRAM_SHARED_RAM_WRITE_EXPECTED,
    0xA07A0061,
    0x3F810000,
    0xA07C0061,
    0x3F810000,
    0x00040132,
    0x00000004,
    0x50007E14,
    0x00C47834,
];
