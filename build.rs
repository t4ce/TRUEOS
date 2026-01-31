use std::env;
use std::path::PathBuf;

use trueos_limloader::ensure_limine_from_manifest_dir;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    ensure_limine_from_manifest_dir(&manifest_dir);
}
