use super::*;

use sha2::{Digest, Sha256};
use std::{fs, io::ErrorKind, path::PathBuf, sync::OnceLock, vec, vec::Vec};

const FIRST_FILE_NAME: &[u8] = b"af_alloy.npy";
const FIRST_EXTRA_OFFSET: usize = LOCAL_FIXED_BYTES + FIRST_FILE_NAME.len();
const FIRST_NPY_OFFSET: usize = FIRST_EXTRA_OFFSET + LOCAL_ZIP64_EXTRA_BYTES;
const FIRST_PAYLOAD_OFFSET: usize = FIRST_NPY_OFFSET + NPY_HEADER_BYTES;
const FIRST_CENTRAL_NAME_OFFSET: usize = PINNED_DIRECTORY_OFFSET + CENTRAL_FIXED_BYTES;
const SECOND_CENTRAL_OFFSET: usize = FIRST_CENTRAL_NAME_OFFSET + FIRST_FILE_NAME.len();
const SECOND_CENTRAL_NAME_OFFSET: usize = SECOND_CENTRAL_OFFSET + CENTRAL_FIXED_BYTES;

fn pinned_archive_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ttstt/.ttstt/models/kokoro/voices-v1.0.bin")
}

fn pinned_archive_bytes() -> Option<&'static [u8]> {
    static ARCHIVE: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    ARCHIVE
        .get_or_init(|| {
            let path = pinned_archive_path();
            match fs::read(&path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    std::eprintln!(
                        "skipping resident-archive fixture test; {} is absent",
                        path.display()
                    );
                    None
                }
                Err(error) => panic!("failed to read {}: {error}", path.display()),
            }
        })
        .as_deref()
}

#[test]
fn pinned_archive_and_af_heart_style_16_match_reference() {
    let Some(bytes) = pinned_archive_bytes() else {
        return;
    };

    let digest: [u8; 32] = Sha256::digest(bytes).into();
    assert_eq!(digest, PINNED_ARCHIVE_SHA256);

    let archive = VoiceArchive::parse(bytes).expect("pinned resident archive must validate");
    assert_eq!(archive.len(), PINNED_ENTRY_COUNT);
    assert!(!archive.is_empty());

    let mut voices = archive.voices();
    assert_eq!(voices.len(), PINNED_ENTRY_COUNT);
    for expected_name in PINNED_VOICE_NAMES {
        let voice = voices
            .next()
            .expect("all pinned voices must be present")
            .expect("validated iteration cannot fail");
        assert_eq!(voice.name(), expected_name);
        assert_eq!(voice.npy_bytes().len(), NPY_FILE_BYTES);
    }
    assert!(voices.next().is_none());

    assert_eq!(archive.lookup("AF_heart").unwrap_err(), Error::InvalidName);
    assert_eq!(archive.lookup("../af_heart").unwrap_err(), Error::InvalidName);
    assert_eq!(archive.lookup("af_heart.npy").unwrap_err(), Error::InvalidName);
    assert_eq!(archive.lookup("af_missing").unwrap_err(), Error::VoiceNotFound);

    let heart = archive.lookup("af_heart").expect("af_heart must exist");
    assert_eq!(heart.crc32(), 0xe396_2fc2);
    let mut style = [0.0f32; STYLE_WIDTH];
    assert_eq!(heart.decode_style(16, &mut style), Ok(16));

    let mut style_hasher = Sha256::new();
    let mut fnv64 = 0xcbf2_9ce4_8422_2325u64;
    for value in style {
        let encoded = value.to_le_bytes();
        style_hasher.update(encoded);
        for byte in encoded {
            fnv64 ^= u64::from(byte);
            fnv64 = fnv64.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    let style_digest: [u8; 32] = style_hasher.finalize().into();
    assert_eq!(
        style_digest,
        [
            0xe5, 0xae, 0x5f, 0x98, 0x13, 0x7e, 0x45, 0xe2, 0xc9, 0x44, 0x52, 0xd4, 0x82, 0xf2,
            0xe4, 0x3f, 0xb1, 0x4b, 0x71, 0x07, 0x98, 0x30, 0xe5, 0x02, 0x26, 0xdc, 0x20, 0x76,
            0xd3, 0xd0, 0xdc, 0xe4,
        ]
    );
    assert_eq!(fnv64, 0x6127_2f4f_5b80_bbf1);
    for (index, expected_bits) in [
        (0, 0xbe6c_e5ff),
        (1, 0x3e41_884d),
        (2, 0xbbac_70ad),
        (127, 0xbe30_5739),
        (128, 0xbd21_5a0a),
        (254, 0xbe70_8c91),
        (255, 0x3d95_d325),
    ] {
        assert_eq!(style[index].to_bits(), expected_bits, "style[{index}]");
    }

    assert_eq!(heart.decode_style(usize::MAX, &mut style), Ok(509));
}

#[test]
fn parser_rejects_every_untrusted_archive_surface() {
    let Some(bytes) = pinned_archive_bytes() else {
        return;
    };

    reject_mutation(bytes, "local encryption flag", Error::EncryptedEntry, |bad| {
        put_u16(bad, 6, 1);
    });
    reject_mutation(bytes, "local descriptor flag", Error::DataDescriptor, |bad| {
        put_u16(bad, 6, 1 << 3);
    });
    reject_mutation(bytes, "unknown local flag", Error::UnsupportedFlags, |bad| {
        put_u16(bad, 6, 1 << 1);
    });
    reject_mutation(bytes, "local method", Error::UnsupportedMethod, |bad| {
        put_u16(bad, 8, 8);
    });
    reject_mutation(bytes, "local ZIP64 placeholder", Error::MalformedZip64Extra, |bad| {
        put_u32(bad, 18, NPY_FILE_BYTES as u32)
    });
    reject_mutation(bytes, "local ZIP64 extra ID", Error::MalformedZip64Extra, |bad| {
        put_u16(bad, FIRST_EXTRA_OFFSET, 2)
    });
    reject_mutation(bytes, "central method", Error::UnsupportedMethod, |bad| {
        put_u16(bad, PINNED_DIRECTORY_OFFSET + 10, 8)
    });
    reject_mutation(bytes, "central size", Error::SizeMismatch, |bad| {
        put_u32(bad, PINNED_DIRECTORY_OFFSET + 20, (NPY_FILE_BYTES - 1) as u32);
    });
    reject_mutation(bytes, "central bounds", Error::DirectoryBounds, |bad| {
        put_u16(bad, PINNED_DIRECTORY_OFFSET + 28, u16::MAX);
    });
    reject_mutation(bytes, "archive path", Error::InvalidName, |bad| {
        bad[FIRST_CENTRAL_NAME_OFFSET] = b'/';
    });
    reject_mutation(bytes, "duplicate voice", Error::DuplicateOrUnsortedName, |bad| {
        bad.copy_within(
            FIRST_CENTRAL_NAME_OFFSET..FIRST_CENTRAL_NAME_OFFSET + FIRST_FILE_NAME.len(),
            SECOND_CENTRAL_NAME_OFFSET,
        );
    });
    reject_mutation(bytes, "NPY dtype", Error::InvalidNpy, |bad| {
        replace_first_npy_header(bad, b"<f4", b">f4");
    });
    reject_mutation(bytes, "NPY ordering", Error::InvalidNpy, |bad| {
        replace_first_npy_header(bad, b"False", b"True ");
    });
    reject_mutation(bytes, "NPY shape", Error::InvalidNpy, |bad| {
        replace_first_npy_header(bad, b"(510, 1, 256)", b"(510, 2, 256)");
    });
    reject_mutation(bytes, "non-finite style", Error::NonFiniteStyle, |bad| {
        bad[FIRST_PAYLOAD_OFFSET..FIRST_PAYLOAD_OFFSET + 4]
            .copy_from_slice(&f32::NAN.to_le_bytes());
    });
    reject_mutation(bytes, "payload CRC", Error::CrcMismatch, |bad| {
        bad[FIRST_PAYLOAD_OFFSET] ^= 1;
    });
    reject_mutation(
        bytes,
        "otherwise valid archive substitution",
        Error::ArchiveDigestMismatch,
        |bad| bad[PINNED_DIRECTORY_OFFSET + 12] ^= 1,
    );
}

#[test]
fn style_selection_clamps_and_decode_is_transactional() {
    assert_eq!(style_index(0), 0);
    assert_eq!(style_index(16), 16);
    assert_eq!(style_index(509), 509);
    assert_eq!(style_index(510), 509);
    assert_eq!(style_index(usize::MAX), 509);

    let mut payload = vec![0u8; NPY_PAYLOAD_BYTES];
    let selected = 23;
    let nan_offset = selected * STYLE_BYTES;
    payload[nan_offset..nan_offset + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    let voice = Voice {
        name: "test_voice",
        npy: &[],
        payload: &payload,
        crc32: 0,
    };
    let mut output = [7.0f32; STYLE_WIDTH];
    assert_eq!(voice.decode_style(selected, &mut output), Err(Error::NonFiniteStyle));
    assert_eq!(output, [7.0; STYLE_WIDTH]);

    let truncated = Voice {
        payload: &payload[..STYLE_BYTES - 1],
        ..voice
    };
    assert_eq!(truncated.decode_style(0, &mut output), Err(Error::InvalidNpy));
    assert_eq!(output, [7.0; STYLE_WIDTH]);
}

fn reject_mutation(source: &[u8], label: &str, expected: Error, mutate: impl FnOnce(&mut [u8])) {
    let mut bad = source.to_vec();
    mutate(&mut bad);
    let actual = VoiceArchive::parse(&bad).expect_err(label);
    assert_eq!(actual, expected, "{label}");
}

fn replace_first_npy_header(bytes: &mut [u8], needle: &[u8], replacement: &[u8]) {
    assert_eq!(needle.len(), replacement.len());
    let header = &mut bytes[FIRST_NPY_OFFSET..FIRST_NPY_OFFSET + NPY_HEADER_BYTES];
    let offset = header
        .windows(needle.len())
        .position(|candidate| candidate == needle)
        .expect("test token must occur in the pinned NPY header");
    header[offset..offset + replacement.len()].copy_from_slice(replacement);
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
