// Copyright 2015-2016 Brian Smith.
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
// SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
// OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
// CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

//! X25519 Key agreement.

use super::{ops, scalar::SCALAR_LEN};
use crate::{agreement, bb, cpu, ec, error, rand};
use core::ffi::c_int;

static CURVE25519: ec::Curve = ec::Curve {
    public_key_len: PUBLIC_KEY_LEN,
    elem_scalar_seed_len: ELEM_AND_SCALAR_LEN,
    id: ec::CurveID::Curve25519,
    check_private_key_bytes: x25519_check_private_key_bytes,
    generate_private_key: x25519_generate_private_key,
    public_from_private: x25519_public_from_private,
};

/// X25519 (ECDH using Curve25519) as described in [RFC 7748].
///
/// Everything is as described in RFC 7748. Key agreement will fail if the
/// result of the X25519 operation is zero; see the notes on the
/// "all-zero value" in [RFC 7748 section 6.1].
///
/// [RFC 7748]: https://tools.ietf.org/html/rfc7748
/// [RFC 7748 section 6.1]: https://tools.ietf.org/html/rfc7748#section-6.1
pub static X25519: agreement::Algorithm = agreement::Algorithm {
    curve: &CURVE25519,
    ecdh: x25519_ecdh,
};

#[allow(clippy::unnecessary_wraps)]
fn x25519_check_private_key_bytes(
    bytes: &[u8],
    _: cpu::Features,
) -> Result<(), error::Unspecified> {
    debug_assert_eq!(bytes.len(), PRIVATE_KEY_LEN);
    Ok(())
}

fn x25519_generate_private_key(
    rng: &dyn rand::SecureRandom,
    out: &mut [u8],
    _: cpu::Features,
) -> Result<(), error::Unspecified> {
    rng.fill(out)
}

fn x25519_public_from_private(
    public_out: &mut [u8],
    private_key: &ec::Seed,
    cpu_features: cpu::Features,
) -> Result<(), error::Unspecified> {
    let public_out = public_out.try_into()?;

    let private_key: &[u8; SCALAR_LEN] = private_key.bytes_less_safe().try_into()?;
    let private_key = ops::MaskedScalar::from_bytes_masked(*private_key);

    #[cfg(all(
        all(target_arch = "arm", target_endian = "little"),
        any(target_os = "android", target_os = "linux")
    ))]
    if let Some(cpu) = <cpu::Features as cpu::GetFeature<_>>::get_feature(&cpu_features) {
        static MONTGOMERY_BASE_POINT: [u8; 32] = [
            9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];
        x25519_neon(public_out, &private_key, &MONTGOMERY_BASE_POINT, cpu);
        return Ok(());
    }

    prefixed_extern! {
        fn x25519_public_from_private_generic_masked(
            public_key_out: &mut PublicKey,
            private_key: &PrivateKey,
            use_adx: c_int,
        );
    }
    unsafe {
        x25519_public_from_private_generic_masked(
            public_out,
            &private_key,
            ops::has_fe25519_adx(cpu_features).into(),
        );
    }

    Ok(())
}

fn x25519_ecdh(
    out: &mut [u8],
    my_private_key: &ec::Seed,
    peer_public_key: untrusted::Input,
    cpu_features: cpu::Features,
) -> Result<(), error::Unspecified> {
    let my_private_key: &[u8; SCALAR_LEN] = my_private_key.bytes_less_safe().try_into()?;
    let my_private_key = ops::MaskedScalar::from_bytes_masked(*my_private_key);
    let peer_public_key: &[u8; PUBLIC_KEY_LEN] = peer_public_key.as_slice_less_safe().try_into()?;

    fn scalar_mult(
        out: &mut ops::EncodedPoint,
        scalar: &ops::MaskedScalar,
        point: &ops::EncodedPoint,
        #[allow(unused_variables)] cpu_features: cpu::Features,
    ) {
        #[cfg(all(
            all(target_arch = "arm", target_endian = "little"),
            any(target_os = "android", target_os = "linux")
        ))]
        if let Some(cpu) = <cpu::Features as cpu::GetFeature<_>>::get_feature(&cpu_features) {
            return x25519_neon(out, scalar, point, cpu);
        }

        #[cfg(all(target_arch = "x86_64", not(target_os = "windows")))]
        {
            if ops::has_fe25519_adx(cpu_features) {
                prefixed_extern! {
                    fn x25519_scalar_mult_adx(
                        out: &mut ops::EncodedPoint,
                        scalar: &ops::MaskedScalar,
                        point: &ops::EncodedPoint,
                    );
                }
                return unsafe { x25519_scalar_mult_adx(out, scalar, point) };
            }
        }

        prefixed_extern! {
            fn x25519_scalar_mult_generic_masked(
                out: &mut ops::EncodedPoint,
                scalar: &ops::MaskedScalar,
                point: &ops::EncodedPoint,
            );
        }
        unsafe {
            x25519_scalar_mult_generic_masked(out, scalar, point);
        }
    }

    scalar_mult(
        out.try_into()?,
        &my_private_key,
        peer_public_key,
        cpu_features,
    );

    let zeros: SharedSecret = [0; SHARED_SECRET_LEN];
    if bb::verify_slices_are_equal(out, &zeros).is_ok() {
        // All-zero output results when the input is a point of small order.
        return Err(error::Unspecified);
    }

    Ok(())
}

// BoringSSL uses `!defined(OPENSSL_APPLE)`.
#[cfg(all(
    all(target_arch = "arm", target_endian = "little"),
    any(target_os = "android", target_os = "linux")
))]
fn x25519_neon(
    out: &mut ops::EncodedPoint,
    scalar: &ops::MaskedScalar,
    point: &ops::EncodedPoint,
    _cpu: cpu::arm::Neon,
) {
    prefixed_extern! {
        fn x25519_NEON(
            out: &mut ops::EncodedPoint,
            scalar: &ops::MaskedScalar,
            point: &ops::EncodedPoint,
        );
    }
    unsafe { x25519_NEON(out, scalar, point) }
}

const ELEM_AND_SCALAR_LEN: usize = ops::ELEM_LEN;

type PrivateKey = ops::MaskedScalar;
const PRIVATE_KEY_LEN: usize = ELEM_AND_SCALAR_LEN;

// An X25519 public key as an encoded Curve25519 point.
type PublicKey = [u8; PUBLIC_KEY_LEN];
const PUBLIC_KEY_LEN: usize = ELEM_AND_SCALAR_LEN;

// An X25519 shared secret as an encoded Curve25519 point.
type SharedSecret = [u8; SHARED_SECRET_LEN];
const SHARED_SECRET_LEN: usize = ELEM_AND_SCALAR_LEN;
