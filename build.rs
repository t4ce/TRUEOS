use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"TAPPDB1\0";

fn manifest_names(text: &str) -> Result<Vec<String>, String> {
    let mut quoted = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('"') {
        rest = &rest[start + 1..];
        let end = rest
            .find('"')
            .ok_or_else(|| String::from("unterminated string in buildins.json"))?;
        quoted.push(rest[..end].to_string());
        rest = &rest[end + 1..];
    }
    if quoted.first().map(String::as_str) != Some("buildins") {
        return Err(String::from(
            "buildins.json must contain one top-level `buildins` string array",
        ));
    }
    let names = quoted.into_iter().skip(1).collect::<Vec<_>>();
    if names.is_empty() {
        return Err(String::from("buildins.json contains no apps"));
    }
    for name in &names {
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(format!("invalid build-in app name `{name}`"));
        }
    }
    Ok(names)
}

fn write_bundle(out: &Path, entries: &[(String, Vec<u8>)]) -> Result<(), String> {
    let mut bundle = Vec::new();
    bundle.extend_from_slice(MAGIC);
    bundle.extend_from_slice(
        &u32::try_from(entries.len())
            .map_err(|_| String::from("too many build-in apps"))?
            .to_le_bytes(),
    );
    for (name, bytes) in entries {
        let archive = format!("{name}.bp");
        let name_len = u16::try_from(archive.len())
            .map_err(|_| format!("build-in archive name is too long: {archive}"))?;
        let data_len = u64::try_from(bytes.len())
            .map_err(|_| format!("build-in archive is too large: {archive}"))?;
        bundle.extend_from_slice(&name_len.to_le_bytes());
        bundle.extend_from_slice(&data_len.to_le_bytes());
        bundle.extend_from_slice(archive.as_bytes());
        bundle.extend_from_slice(bytes);
    }
    fs::write(out, bundle).map_err(|err| format!("write {}: {err}", out.display()))
}

fn main() {
    println!("cargo:rerun-if-env-changed=TRUEOS_BLUEPRINTS_DIR");
    println!("cargo:rerun-if-env-changed=TRUEOS_REQUIRE_BUILDINS");

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let blueprints_dir = env::var_os("TRUEOS_BLUEPRINTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.join("../TRUEOS-Blueprints"));
    let manifest = blueprints_dir.join("buildins.json");
    let required = env::var_os("TRUEOS_REQUIRE_BUILDINS").is_some_and(|value| value == "1");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("app-buildins.bin");

    if !manifest.is_file() {
        if required {
            panic!(
                "required Blueprint build-in manifest is missing: {}",
                manifest.display()
            );
        }
        println!(
            "cargo:warning=Blueprint build-ins unavailable at {}; embedding an empty app.db seed",
            manifest.display()
        );
        write_bundle(&out, &[]).unwrap();
        return;
    }

    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = fs::read_to_string(&manifest)
        .unwrap_or_else(|err| panic!("read {}: {err}", manifest.display()));
    let names = manifest_names(&text).unwrap_or_else(|err| panic!("{}: {err}", manifest.display()));
    let mut entries = Vec::with_capacity(names.len());
    for name in names {
        let path = blueprints_dir.join("dist").join(format!("{name}.bp"));
        println!("cargo:rerun-if-changed={}", path.display());
        match fs::read(&path) {
            Ok(bytes) => entries.push((name, bytes)),
            Err(err) if !required => println!(
                "cargo:warning=skipping unavailable Blueprint build-in {}: {err}",
                path.display()
            ),
            Err(err) => panic!("required Blueprint build-in {}: {err}", path.display()),
        }
    }
    write_bundle(&out, &entries).unwrap();
    println!(
        "cargo:warning=embedding {} Blueprint build-in app(s) into app.db seed",
        entries.len()
    );
}
