use core::mem;

/// On-disk object kinds.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjKind {
    Super = 1,
    Checkpoint = 2,
    Dir = 3,
    Inode = 4,
    Data = 5,
    Alloc = 6,
}

/// Common object header.
///
/// Stored little-endian on disk.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ObjHeader {
    pub magic: [u8; 8],
    pub kind: u8,
    pub flags: u8,
    pub header_len: u16,
    pub payload_len: u32,
    pub checksum32: u32,
}

impl ObjHeader {
    pub const MAGIC: [u8; 8] = *b"TRUEOSFS";

    pub fn new(kind: ObjKind, payload_len: u32) -> Self {
        Self {
            magic: Self::MAGIC,
            kind: kind as u8,
            flags: 0,
            header_len: mem::size_of::<ObjHeader>() as u16,
            payload_len,
            checksum32: 0,
        }
    }
}

/// Two fixed superblock slots (classic A/B).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Superblock {
    pub hdr: ObjHeader,
    pub epoch: u64,
    pub root_lba: u64,
    pub root_len: u32,
    pub _pad: u32,
}

impl Superblock {
    pub fn new(epoch: u64, root_lba: u64, root_len: u32) -> Self {
        let mut sb = Self {
            hdr: ObjHeader::new(ObjKind::Super, (mem::size_of::<Superblock>() - mem::size_of::<ObjHeader>()) as u32),
            epoch,
            root_lba,
            root_len,
            _pad: 0,
        };
        sb
    }
}
