use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::PathBuf;

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    result
}

fn main() {
    let mut arguments = env::args_os();
    let program = arguments
        .next()
        .unwrap_or_else(|| "lfm25-packed-seal".into());
    let Some(path) = arguments.next().map(PathBuf::from) else {
        eprintln!("usage: {} NATIVE_IMAGE", PathBuf::from(program).display());
        std::process::exit(2);
    };
    if arguments.next().is_some() {
        eprintln!("lfm25-packed-seal: exactly one native image is required");
        std::process::exit(2);
    }

    let mut image = fs::read(&path).unwrap_or_else(|error| {
        panic!("lfm25-packed-seal: cannot read {}: {error}", path.display())
    });
    let native_sha: [u8; 32] = Sha256::digest(&image).into();
    assert_eq!(
        image.len(),
        trueos_lfm25_model::lfm25::PINNED_NATIVE_IMAGE_BYTES as usize,
        "native image byte count"
    );
    assert_eq!(
        native_sha,
        trueos_lfm25_model::lfm25::PINNED_NATIVE_IMAGE_SHA256,
        "native image SHA-256"
    );

    let stats = trueos_lfm25_cpu::pack_q8x16_model_in_place(&mut image)
        .expect("fixed packed model conversion");
    let packed_sha: [u8; 32] = Sha256::digest(&image).into();
    assert_eq!(packed_sha, trueos_lfm25_cpu::PACKED_Q8X16_IMAGE_SHA256, "packed image SHA-256");
    println!(
        "lfm25-packed-seal: PASS bytes={} tensors={} block_tiles={} quantized_values={} \
         subnormal_scales={} native_sha256={} packed_sha256={}",
        image.len(),
        stats.tensor_count,
        stats.block_tiles,
        stats.quantized_values,
        stats.subnormal_scales,
        hex(&native_sha),
        hex(&packed_sha),
    );
}
