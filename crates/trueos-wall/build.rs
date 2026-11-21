use serde_json::Value;
use std::{collections::BTreeSet, env, fs, io, path::Path};

fn write_catalog(generated: &mut String, path: &Path, key: &str, constant: &str, optional: bool) {
    println!("cargo:rerun-if-changed={}", path.display());
    let mut names = BTreeSet::new();
    match fs::read(path) {
        Ok(bytes) => {
            let catalog: Value = serde_json::from_slice(&bytes)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let entries = catalog[key]
                .as_array()
                .unwrap_or_else(|| panic!("{}: expected an array named {key}", path.display()));
            for entry in entries {
                let name = if key == "apps" && entry.is_object() {
                    entry["name"].as_str()
                } else {
                    entry.as_str()
                };
                let name = name
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| panic!("{}: missing {key} entry name", path.display()));
                assert!(
                    names.insert(name.to_owned()),
                    "{}: duplicate {name}",
                    path.display()
                );
            }
        }
        Err(error) if optional && error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => panic!("{}: {error}", path.display()),
    }
    let names: Vec<_> = names.into_iter().collect();
    generated.push_str(&format!("pub const {constant}: &[&str] = &{names:?};\n"));
}

fn main() {
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR").unwrap();
    let kernel = Path::new(&manifest_dir).join("../..");
    let out = env::var_os("OUT_DIR").unwrap();
    println!("cargo:rerun-if-env-changed=TRUEOS_BUILDINS_SOURCE");
    let configured = env::var_os("TRUEOS_BUILDINS_SOURCE");
    let buildins = configured.as_ref().map_or_else(
        || kernel.join("../TRUEOS-Buildins"),
        |source| kernel.join(source),
    );
    let mut generated = String::new();
    write_catalog(
        &mut generated,
        &buildins.join("buildins.json"),
        "buildins",
        "BUILDINS",
        configured.is_none(),
    );
    write_catalog(
        &mut generated,
        &kernel.join("../TRUEOS-Blueprints/apps.json"),
        "apps",
        "BLUEPRINTS",
        false,
    );
    fs::write(Path::new(&out).join("catalogs.rs"), generated).expect("generate firewall catalogs");
}
