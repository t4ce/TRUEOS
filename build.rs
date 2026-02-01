use std::env;
use std::path::PathBuf;

use trueos_limloader::ensure_limine_from_manifest_dir;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));

    // Ensure Cargo reruns this build script if the Limine toolchain outputs were deleted.
    // These paths are generated into `bld/` (which is not tracked by git), so without explicit
    // `rerun-if-changed` directives Cargo may skip the build script and `make iso` will fail.
    println!("cargo:rerun-if-changed=bld/limine-build/.installed");
    println!("cargo:rerun-if-changed=bld/limine-build/.config_args");
    println!("cargo:rerun-if-changed=bld/limine-prefix/share/limine/BOOTX64.EFI");
    println!("cargo:rerun-if-changed=bld/limine-prefix/share/limine/limine-uefi-cd.bin");

    ensure_limine_from_manifest_dir(&manifest_dir);
}
