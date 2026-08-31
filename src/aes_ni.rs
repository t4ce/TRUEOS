//! One-shot AES-NI and AES-GCM boot validation.
//!
//! This is deliberately a capability probe, not a filesystem cipher API.  It
//! proves that an AES-NI-advertising CPU can execute the base encrypt/decrypt
//! instructions and that the already-linked Ring provider passes a standard
//! AES-128-GCM known-answer test.  File-format policy, key ownership, and nonce
//! management remain above this hardware boundary.

use core::arch::x86_64::{_mm_aesdec_si128, _mm_aesenc_si128, _mm_setzero_si128, _mm_storeu_si128};

use raw_cpuid::CpuId;
use ring::aead::{AES_128_GCM, Aad, LessSafeKey, Nonce, Tag, UnboundKey};

static BOOT_PROBE: spin::Once<AesNiBootProbeReport> = spin::Once::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InstructionKat {
    Skipped,
    Passed,
    Failed,
}

impl InstructionKat {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Skipped => "skipped",
            Self::Passed => "pass",
            Self::Failed => "fail",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AesNiBootProbeReport {
    aes_ni: bool,
    pclmulqdq: bool,
    ssse3: bool,
    instruction_kat: InstructionKat,
    ring_aes_128_gcm_kat: bool,
}

impl AesNiBootProbeReport {
    fn status(self) -> &'static str {
        if self.instruction_kat == InstructionKat::Failed || !self.ring_aes_128_gcm_kat {
            "fail"
        } else if !self.aes_ni {
            "unsupported"
        } else {
            "pass"
        }
    }

    const fn ring_aes_gcm_path(self) -> &'static str {
        // Ring's x86-64 AES-GCM hardware path requires this combination.  If
        // any member is absent, Ring retains a constant-time fallback.
        if self.aes_ni && self.pclmulqdq && self.ssse3 {
            "hardware-eligible"
        } else {
            "fallback"
        }
    }
}

/// Run and log the AES boot probe exactly once, even if a later boot stage asks
/// for the report again.
pub(crate) fn boot_probe_once() -> &'static AesNiBootProbeReport {
    BOOT_PROBE.call_once(|| {
        let report = probe_current_cpu();
        crate::log_important!(
            target: "boot";
            "crypto-aes-ni: boot-probe status={} aesni={} aesenc-aesdec-kat={} pclmulqdq={} ssse3={} ring-aes128-gcm-kat={} ring-aes-gcm-path={}\n",
            report.status(),
            yes_no(report.aes_ni),
            report.instruction_kat.as_str(),
            yes_no(report.pclmulqdq),
            yes_no(report.ssse3),
            pass_fail(report.ring_aes_128_gcm_kat),
            report.ring_aes_gcm_path(),
        );
        report
    })
}

fn probe_current_cpu() -> AesNiBootProbeReport {
    let features = CpuId::new().get_feature_info();
    let aes_ni = features.as_ref().is_some_and(|info| info.has_aesni());
    let pclmulqdq = features.as_ref().is_some_and(|info| info.has_pclmulqdq());
    let ssse3 = features.as_ref().is_some_and(|info| info.has_ssse3());

    let instruction_kat = if aes_ni {
        // SAFETY: CPUID.01H:ECX.AESNI[bit 25] is checked immediately above.
        if unsafe { aesenc_aesdec_known_answer() } {
            InstructionKat::Passed
        } else {
            InstructionKat::Failed
        }
    } else {
        InstructionKat::Skipped
    };

    AesNiBootProbeReport {
        aes_ni,
        pclmulqdq,
        ssse3,
        instruction_kat,
        ring_aes_128_gcm_kat: ring_aes_128_gcm_known_answer(),
    }
}

/// Exercise the AES-NI opcodes themselves without relying on a provider's
/// runtime dispatch.  For an all-zero state and round key, one AESENC round is
/// sixteen `0x63` bytes and one AESDEC round is sixteen `0x52` bytes.
#[target_feature(enable = "aes")]
unsafe fn aesenc_aesdec_known_answer() -> bool {
    let zero = _mm_setzero_si128();
    let encrypted = _mm_aesenc_si128(zero, zero);
    let decrypted = _mm_aesdec_si128(zero, zero);
    let mut encrypted_bytes = [0u8; 16];
    let mut decrypted_bytes = [0u8; 16];

    // SAFETY: Both destinations are valid, writable 16-byte arrays and the
    // unaligned store intrinsic accepts their byte alignment.
    unsafe {
        _mm_storeu_si128(encrypted_bytes.as_mut_ptr().cast(), encrypted);
        _mm_storeu_si128(decrypted_bytes.as_mut_ptr().cast(), decrypted);
    }

    encrypted_bytes == [0x63; 16] && decrypted_bytes == [0x52; 16]
}

/// NIST AES-128-GCM known-answer test: zero key, zero 96-bit IV, one zero
/// plaintext block, and no associated data.
fn ring_aes_128_gcm_known_answer() -> bool {
    const KEY: [u8; 16] = [0; 16];
    const NONCE: [u8; 12] = [0; 12];
    const PLAINTEXT: [u8; 16] = [0; 16];
    const CIPHERTEXT: [u8; 16] = [
        0x03, 0x88, 0xda, 0xce, 0x60, 0xb6, 0xa3, 0x92, 0xf3, 0x28, 0xc2, 0xb9, 0x71, 0xb2, 0xfe,
        0x78,
    ];
    const AUTH_TAG: [u8; 16] = [
        0xab, 0x6e, 0x47, 0xd4, 0x2c, 0xec, 0x13, 0xbd, 0xf5, 0x3a, 0x67, 0xb2, 0x12, 0x57, 0xbd,
        0xdf,
    ];

    let Ok(sealing_key) = UnboundKey::new(&AES_128_GCM, &KEY) else {
        return false;
    };
    let sealing_key = LessSafeKey::new(sealing_key);
    let mut encrypted = PLAINTEXT;
    let Ok(tag) = sealing_key.seal_in_place_separate_tag(
        Nonce::assume_unique_for_key(NONCE),
        Aad::empty(),
        &mut encrypted,
    ) else {
        return false;
    };
    if encrypted != CIPHERTEXT || tag.as_ref() != AUTH_TAG {
        return false;
    }

    let Ok(opening_key) = UnboundKey::new(&AES_128_GCM, &KEY) else {
        return false;
    };
    let opening_key = LessSafeKey::new(opening_key);
    let mut decrypted = CIPHERTEXT;
    let Ok(opened) = opening_key.open_in_place_separate_tag(
        Nonce::assume_unique_for_key(NONCE),
        Aad::empty(),
        Tag::from(AUTH_TAG),
        &mut decrypted,
        0..,
    ) else {
        return false;
    };

    opened == PLAINTEXT
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

const fn pass_fail(value: bool) -> &'static str {
    if value { "pass" } else { "fail" }
}
