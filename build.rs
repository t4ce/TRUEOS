use std::env;
use std::path::PathBuf;

use trueos_limloader::ensure_limine_from_manifest_dir;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));

    // Keep these as explicit rerun triggers even when Limine is already built.
    // (The helper only prints rerun directives when it actually rebuilds.)
    println!("cargo:rerun-if-env-changed=TRUEOS_LIMINE_REPO");
    println!("cargo:rerun-if-env-changed=TRUEOS_LIMINE_REF");
    println!("cargo:rerun-if-env-changed=TRUEOS_LIMINE_CONFIG_ARGS");
    println!("cargo:rerun-if-env-changed=CARGO_NET_OFFLINE");

    // Ensure Limine is present/built for both ISO assembly and installer payload embedding.
    ensure_limine_from_manifest_dir(&manifest_dir);
    println!("cargo:rerun-if-changed=build.rs");
}
