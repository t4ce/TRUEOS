use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::Path,
};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS");
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").expect("CARGO_CFG_TARGET_ARCH");

    generate_ring_runtime_imports(
        Path::new(&manifest_dir),
        target_os.as_str(),
        target_arch.as_str(),
    )
    .expect("generate Ring runtime imports");

    generate_portal_imports(Path::new(&manifest_dir)).expect("generate portal imports");
}

fn generate_ring_runtime_imports(
    manifest_dir: &Path,
    target_os: &str,
    target_arch: &str,
) -> Result<(), String> {
    let symbols_path = manifest_dir.join("vendor/ring-0.17.14/runtime-loader-symbols-x86_64.txt");
    println!("cargo:rerun-if-changed={}", symbols_path.display());

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR");
    let generated_path = Path::new(&out_dir).join("generated_ring_runtime_imports.rs");
    if !matches!(target_os, "trueos" | "zkvm") || target_arch != "x86_64" {
        return fs::write(
            &generated_path,
            "fn resolve_ring_runtime_import(_name: &str) -> Option<usize> { None }\n",
        )
        .map_err(|err| format!("failed to write {}: {err}", generated_path.display()));
    }

    let contents = fs::read_to_string(&symbols_path)
        .map_err(|err| format!("failed to read {}: {err}", symbols_path.display()))?;
    let symbols = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    let mut generated = String::from("unsafe extern \"C\" {\n");
    for (index, symbol) in symbols.iter().enumerate() {
        if !(symbol.starts_with("ring_core_0_17_14__")
            && symbol
                .bytes()
                .all(|byte| byte == b'_' || byte.is_ascii_alphanumeric()))
        {
            return Err(format!("invalid Ring runtime export: {symbol}"));
        }
        generated.push_str(&format!(
            "    #[link_name = \"{symbol}\"]\n    static RING_RUNTIME_EXPORT_{index}: u8;\n"
        ));
        // Keep a link-time root as well as the resolver's address reference.
        println!("cargo:rustc-link-arg=--undefined={symbol}");
    }
    generated.push_str(
        "}\n\nfn resolve_ring_runtime_import(name: &str) -> Option<usize> {\n    match name {\n",
    );
    for (index, symbol) in symbols.iter().enumerate() {
        generated.push_str(&format!(
            "        \"{symbol}\" => Some(core::ptr::addr_of!(RING_RUNTIME_EXPORT_{index}) as usize),\n"
        ));
    }
    generated.push_str("        _ => None,\n    }\n}\n");

    fs::write(&generated_path, generated)
        .map_err(|err| format!("failed to write {}: {err}", generated_path.display()))
}

fn generate_portal_imports(manifest_dir: &Path) -> Result<(), String> {
    let bp_abi_path = manifest_dir.join("crates/trueos-v/src/bp_abi.rs");
    println!("cargo:rerun-if-changed={}", bp_abi_path.display());

    let import_names = parse_declared_cabi_imports(&bp_abi_path)?;
    let defined_exports = collect_defined_cabi_exports(manifest_dir)?;

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR");
    let generated_path = Path::new(&out_dir).join("generated_portal_imports.rs");

    let mut generated =
        String::from("fn resolve_cabi_import(name: &str) -> Option<usize> {\n    match name {\n");
    for name in import_names {
        let Some(symbol_path) = defined_exports.get(&name) else {
            println!(
                "cargo:warning=declared CABI symbol {name} has no kernel export and will stay unresolved"
            );
            continue;
        };
        generated.push_str("        \"");
        generated.push_str(&name);
        generated.push_str("\" => Some(");
        generated.push_str(symbol_path);
        generated.push_str(" as *const () as usize),\n");
    }
    generated.push_str("        _ => None,\n    }\n}\n");

    fs::write(&generated_path, generated)
        .map_err(|err| format!("failed to write {}: {err}", generated_path.display()))
}

fn parse_declared_cabi_imports(bp_abi_path: &Path) -> Result<Vec<String>, String> {
    let source = fs::read_to_string(bp_abi_path)
        .map_err(|err| format!("failed to read {}: {err}", bp_abi_path.display()))?;
    let mut import_names = BTreeSet::new();

    for line in source.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("pub fn trueos_cabi_") else {
            continue;
        };
        let Some(name_end) = rest.find('(') else {
            continue;
        };
        import_names.insert(format!("trueos_cabi_{}", &rest[..name_end]));
    }

    Ok(import_names.into_iter().collect())
}

fn collect_defined_cabi_exports(manifest_dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let src_dir = manifest_dir.join("src");
    let mut exports = BTreeMap::new();
    collect_defined_cabi_exports_in_dir(manifest_dir, &src_dir, &mut exports)?;
    Ok(exports)
}

fn collect_defined_cabi_exports_in_dir(
    manifest_dir: &Path,
    dir: &Path,
    exports: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    for entry in
        fs::read_dir(dir).map_err(|err| format!("failed to read {}: {err}", dir.display()))?
    {
        let entry = entry.map_err(|err| format!("failed to walk {}: {err}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_defined_cabi_exports_in_dir(manifest_dir, &path, exports)?;
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        collect_defined_cabi_exports_in_file(manifest_dir, &path, exports)?;
    }
    Ok(())
}

fn collect_defined_cabi_exports_in_file(
    manifest_dir: &Path,
    path: &Path,
    exports: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    let source = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let module_path = module_path_for_source(manifest_dir, path)?;

    for line in source.lines() {
        if !line.contains("fn trueos_cabi_") {
            continue;
        }
        let Some(rest) = line.split("fn ").nth(1) else {
            continue;
        };
        let Some(name_end) = rest.find('(') else {
            continue;
        };
        let name = &rest[..name_end];
        if !name.starts_with("trueos_cabi_") {
            continue;
        }
        let symbol_path = format!("{}::{}", module_path, name);
        exports.insert(name.to_string(), symbol_path);
    }

    Ok(())
}

fn module_path_for_source(manifest_dir: &Path, path: &Path) -> Result<String, String> {
    let rel = path
        .strip_prefix(manifest_dir.join("src"))
        .map_err(|_| format!("{} is not under src/", path.display()))?;

    if rel == Path::new("r/io.rs") || rel == Path::new("r/io_cursor.rs") {
        return Ok(String::from("crate::r::io::cabi"));
    }

    let mut parts = rel
        .iter()
        .map(|part| {
            part.to_str()
                .ok_or_else(|| format!("non-utf8 source path: {}", path.display()))
                .map(String::from)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let Some(last) = parts.pop() else {
        return Err(format!("bad source path: {}", path.display()));
    };
    if last != "mod.rs" {
        let stem = last
            .strip_suffix(".rs")
            .ok_or_else(|| format!("bad rust source path: {}", path.display()))?;
        parts.push(stem.to_string());
    }

    let mut module_path = String::from("crate");
    for part in parts {
        module_path.push_str("::");
        module_path.push_str(&part);
    }
    Ok(module_path)
}
