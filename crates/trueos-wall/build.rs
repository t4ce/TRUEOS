use std::{env, path::Path, process::Command};

fn main() {
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let crate_dir = Path::new(&manifest_dir);
    let kernel = crate_dir.join("../..");
    let out = env::var_os("OUT_DIR").expect("OUT_DIR");

    println!("cargo:rerun-if-env-changed=TRUEOS_BUILDINS_SOURCE");
    let configured = env::var_os("TRUEOS_BUILDINS_SOURCE");
    let buildins = configured.as_ref().map_or_else(
        || kernel.join("../TRUEOS-Buildins"),
        |source| kernel.join(source),
    );
    let buildins = buildins.join("buildins.json");
    let blueprints = kernel.join("../TRUEOS-Blueprints/apps.json");
    let generator = crate_dir.join("generate_catalogs.py");
    for path in [&buildins, &blueprints, &generator] {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let mut command = Command::new("python3");
    command
        .arg(generator)
        .arg(buildins)
        .arg(blueprints)
        .arg(Path::new(&out).join("catalogs.rs"));
    if configured.is_none() {
        command.arg("--optional-buildins");
    }
    let status = command
        .status()
        .expect("run the firewall catalog generator");
    assert!(status.success(), "firewall catalog generation failed");
}
