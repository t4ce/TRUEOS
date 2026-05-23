// Copyright 2015-2017 Brian Smith.
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

//! ECDH key agreement using the P-256 and P-384 curves.

use super::{ops::*, private_key::*, public_key::*};
use crate::{agreement, cpu, ec, error};

/// A key agreement algorithm.
macro_rules! ecdh {
    ( $NAME:ident, $curve:expr, $name_str:expr, $private_key_ops:expr,
      $public_key_ops:expr, $ecdh:ident ) => {
        #[doc = "ECDH using the NSA Suite B"]
        #[doc=$name_str]
        #[doc = "curve."]
        ///
        /// Public keys are encoding in uncompressed form using the
        /// Octet-String-to-Elliptic-Curve-Point algorithm in
        /// [SEC 1: Elliptic Curve Cryptography, Version 2.0]. Public keys are
        /// validated during key agreement according to
        /// [NIST Special Publication 800-56A, revision 2] and Appendix B.3 of
        /// the NSA's [Suite B Implementer's Guide to NIST SP 800-56A].
        ///
        /// [SEC 1: Elliptic Curve Cryptography, Version 2.0]:
        ///     http://www.secg.org/sec1-v2.pdf
        /// [NIST Special Publication 800-56A, revision 2]:
        ///     http://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-56Ar2.pdf
        /// [Suite B Implementer's Guide to NIST SP 800-56A]:
        ///     https://github.com/briansmith/ring/blob/main/doc/ecdh.pdf
        pub static $NAME: agreement::Algorithm = agreement::Algorithm {
            curve: $curve,
            ecdh: $ecdh,
        };

        fn $ecdh(
            out: &mut [u8],
            my_private_key: &ec::Seed,
            peer_public_key: untrusted::Input,
            cpu: cpu::Features,
        ) -> Result<(), error::Unspecified> {
            ecdh(
                $private_key_ops,
                $public_key_ops,
                out,
                my_private_key,
                peer_public_key,
                cpu,
            )
        }
    };
}

ecdh!(
    ECDH_P256,
    &ec::suite_b::curve::P256,
    "P-256 (secp256r1)",
    &p256::PRIVATE_KEY_OPS,
    &p256::PUBLIC_KEY_OPS,
    p256_ecdh
);

ecdh!(
    ECDH_P384,
    &ec::suite_b::curve::P384,
    "P-384 (secp384r1)",
    &p384::PRIVATE_KEY_OPS,
    &p384::PUBLIC_KEY_OPS,
    p384_ecdh
);

fn ecdh(
    private_key_ops: &PrivateKeyOps,
    public_key_ops: &PublicKeyOps,
    out: &mut [u8],
    my_private_key: &ec::Seed,
    peer_public_key: untrusted::Input,
    cpu: cpu::Features,
) -> Result<(), error::Unspecified> {
    // The NIST SP 800-56Ar2 steps are from section 5.7.1.2 Elliptic Curve
    // Cryptography Cofactor Diffie-Hellman (ECC CDH) Primitive.
    //
    // The "NSA Guide" steps are from section 3.1 of the NSA guide, "Ephemeral
    // Unified Model."

    let q = &public_key_ops.common.elem_modulus(cpu);

    // NSA Guide Step 1 is handled separately.

    // NIST SP 800-56Ar2 5.6.2.2.2.
    // NSA Guide Step 2.
    //
    // `parse_uncompressed_point` verifies that the point is not at infinity
    // and that it is on the curve, using the Partial Public-Key Validation
    // Routine.
    let peer_public_key = parse_uncompressed_point(public_key_ops, q, peer_public_key)?;

    // NIST SP 800-56Ar2 Step 1.
    // NSA Guide Step 3 (except point at infinity check).
    //
    // Note that the cofactor (h) is one since we only support prime-order
    // curves, so we can safely ignore the cofactor.
    //
    // It is impossible for the result to be the point at infinity because our
    // private key is in the range [1, n) and the curve has prime order and
    // `parse_uncompressed_point` verified that the peer public key is on the
    // curve and not at infinity. However, since the standards require the
    // check, we do it using `assert!`.
    //
    // NIST SP 800-56Ar2 defines "Destroy" thusly: "In this Recommendation, to
    // destroy is an action applied to a key or a piece of secret data. After
    // a key or a piece of secret data is destroyed, no information about its
    // value can be recovered." We interpret "destroy" somewhat liberally: we
    // assume that since we throw away the values to be destroyed, no
    // information about their values can be recovered. This doesn't meet the
    // NSA guide's explicit requirement to "zeroize" them though.
    // TODO: this only needs common scalar ops
    let n = &private_key_ops.common.scalar_modulus(cpu);
    let my_private_key = private_key_as_scalar(n, my_private_key);
    let product = private_key_ops.point_mul(&my_private_key, &peer_public_key, cpu);

    // NIST SP 800-56Ar2 Steps 2, 3, 4, and 5.
    // NSA Guide Steps 3 (point at infinity check) and 4.
    //
    // Again, we have a pretty liberal interpretation of the NIST's spec's
    // "Destroy" that doesn't meet the NSA requirement to "zeroize."
    // `big_endian_affine_from_jacobian` verifies that the result is not at
    // infinity and also does an extra check to verify that the point is on
    // the curve.
    big_endian_affine_from_jacobian(private_key_ops, q, out, None, &product)

    // NSA Guide Step 5 & 6 are deferred to the caller. Again, we have a
    // pretty liberal interpretation of the NIST's spec's "Destroy" that
    // doesn't meet the NSA requirement to "zeroize."
}
