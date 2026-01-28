#![no_std]

pub const MAGIC: [u8; 8] = *b"TRUEOSFS";
pub const VERSION: u32 = 1;

pub fn write_blank_superblock(block0: &mut [u8]) {
    if block0.len() < 16 {
        return;
    }

    for b in block0.iter_mut() {
        *b = 0;
    }

    block0[0..8].copy_from_slice(&MAGIC);
    block0[8..12].copy_from_slice(&VERSION.to_le_bytes());
    // [12..16] reserved/flags = 0
}
