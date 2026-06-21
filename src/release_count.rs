const RELEASE_COUNT_BYTES: &[u8] = include_bytes!("../tools/cnt");

#[allow(dead_code)]
pub(crate) const RELEASE_COUNT: u64 = parse_release_count(RELEASE_COUNT_BYTES);

#[used]
pub(crate) static RELEASE_COUNT_EMBED: u64 = RELEASE_COUNT;

const fn parse_release_count(bytes: &[u8]) -> u64 {
    let mut value = 0u64;
    let mut idx = 0usize;

    while idx < bytes.len() {
        let byte = bytes[idx];
        if byte >= b'0' && byte <= b'9' {
            value = value
                .saturating_mul(10)
                .saturating_add((byte - b'0') as u64);
        }
        idx += 1;
    }

    value
}
