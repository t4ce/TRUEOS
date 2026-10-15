//! glibc-compatible immutable C-locale character classification table.
//!
//! The imported accessor returns a pointer to a pointer, biased by 128 for
//! signed-char indices and EOF. ctype.h stores its twelve masks in network byte
//! order. No mutable locale API is currently exposed by this compatibility ABI.

const fn classification(byte: u16) -> u16 {
    let upper = byte >= b'A' as u16 && byte <= b'Z' as u16;
    let lower = byte >= b'a' as u16 && byte <= b'z' as u16;
    let digit = byte >= b'0' as u16 && byte <= b'9' as u16;
    let alpha = upper || lower;
    let print = byte >= 0x20 && byte <= 0x7e;
    let graph = byte >= 0x21 && byte <= 0x7e;
    let flags = (upper as u16)
        | ((lower as u16) << 1)
        | ((alpha as u16) << 2)
        | ((digit as u16) << 3)
        | (((digit
            || (byte >= b'A' as u16 && byte <= b'F' as u16)
            || (byte >= b'a' as u16 && byte <= b'f' as u16)) as u16)
            << 4)
        | (((byte == 0x20 || (byte >= 0x09 && byte <= 0x0d)) as u16) << 5)
        | ((print as u16) << 6)
        | ((graph as u16) << 7)
        | (((byte == 0x20 || byte == 0x09) as u16) << 8)
        | (((byte < 0x20 || byte == 0x7f) as u16) << 9)
        | (((graph && !alpha && !digit) as u16) << 10)
        | (((alpha || digit) as u16) << 11);
    flags.to_be()
}

const fn table() -> [u16; 384] {
    let mut table = [0; 384];
    let mut byte = 0;
    while byte < 128 {
        table[128 + byte] = classification(byte as u16);
        byte += 1;
    }
    table
}

static TABLE: [u16; 384] = table();
struct TablePointer(*const u16);
// Both this pointer slot and all table entries are immutable for process life.
unsafe impl Sync for TablePointer {}
static TABLE_POINTER: TablePointer = TablePointer(unsafe { TABLE.as_ptr().add(128) });

pub unsafe extern "C" fn __ctype_b_loc() -> *const *const u16 {
    &TABLE_POINTER.0
}

// Wide classification follows the same fixed C locale: ASCII categories only.
// WEOF and non-ASCII scalar values are unclassified; no Unicode/UTF-8 locale
// switch is implemented or silently inferred from environment variables.
fn wide_has(value: u32, bit: u32) -> core::ffi::c_int {
    if value > 0x7f {
        return 0;
    }
    i32::from(classification(value as u16) & (1u16 << bit).to_be() != 0)
}
pub extern "C" fn iswspace(value: u32) -> core::ffi::c_int {
    wide_has(value, 5)
}
pub extern "C" fn iswalpha(value: u32) -> core::ffi::c_int {
    wide_has(value, 2)
}
pub extern "C" fn iswdigit(value: u32) -> core::ffi::c_int {
    wide_has(value, 3)
}
pub extern "C" fn iswalnum(value: u32) -> core::ffi::c_int {
    wide_has(value, 11)
}
