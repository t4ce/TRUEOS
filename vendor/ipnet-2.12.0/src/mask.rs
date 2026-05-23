use crate::PrefixLenError;
#[cfg(not(feature = "std"))]
use core::net::{IpAddr, Ipv4Addr, Ipv6Addr};
#[cfg(feature = "std")]
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Converts a `IpAddr` network mask into a prefix.
///
/// # Errors
/// If the mask is invalid this will return an `PrefixLenError`.
pub fn ip_mask_to_prefix(mask: IpAddr) -> Result<u8, PrefixLenError> {
    match mask {
        IpAddr::V4(mask) => ipv4_mask_to_prefix(mask),
        IpAddr::V6(mask) => ipv6_mask_to_prefix(mask),
    }
}

/// Converts a `Ipv4Addr` network mask into a prefix.
///
/// # Errors
/// If the mask is invalid this will return an `PrefixLenError`.
pub fn ipv4_mask_to_prefix(mask: Ipv4Addr) -> Result<u8, PrefixLenError> {
    let mask = u32::from(mask);

    let prefix = mask.leading_ones();
    if mask.checked_shl(prefix).unwrap_or(0) == 0 {
        Ok(prefix as u8)
    } else {
        Err(PrefixLenError)
    }
}

/// Converts a `Ipv6Addr` network mask into a prefix.
///
/// # Errors
/// If the mask is invalid this will return an `PrefixLenError`.
pub fn ipv6_mask_to_prefix(mask: Ipv6Addr) -> Result<u8, PrefixLenError> {
    let mask = u128::from(mask);

    let prefix = mask.leading_ones();
    if mask.checked_shl(prefix).unwrap_or(0) == 0 {
        Ok(prefix as u8)
    } else {
        Err(PrefixLenError)
    }
}
