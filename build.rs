use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fmt::Write as _, fs, path::Path};

const PORTAL_CABI_LOCK_PATH: &str = "abi/portal-cabi-v2.sha256";

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
    let lock_path = manifest_dir.join(PORTAL_CABI_LOCK_PATH);
    println!("cargo:rerun-if-changed={}", lock_path.display());

    let contracts = parse_declared_cabi_contracts(&bp_abi_path)?;
    verify_portal_cabi_lock(&lock_path, &contracts)?;
    let import_names = contracts.keys().cloned().collect::<Vec<_>>();
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

fn parse_declared_cabi_contracts(bp_abi_path: &Path) -> Result<BTreeMap<String, String>, String> {
    let source = fs::read_to_string(bp_abi_path)
        .map_err(|err| format!("failed to read {}: {err}", bp_abi_path.display()))?;
    parse_cabi_contracts(&source).map_err(|err| format!("{}: {err}", bp_abi_path.display()))
}

fn verify_portal_cabi_lock(
    lock_path: &Path,
    contracts: &BTreeMap<String, String>,
) -> Result<(), String> {
    let actual = aggregate_cabi_contract_digest(contracts);
    let lock = fs::read_to_string(lock_path).map_err(|err| {
        format!("failed to read CABI contract lock {}: {err}", lock_path.display())
    })?;
    let expected = lock
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .ok_or_else(|| format!("CABI contract lock {} is empty", lock_path.display()))?;
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "CABI contract lock {} must contain one SHA-256 value; found `{expected}`",
            lock_path.display(),
        ));
    }
    if !expected.eq_ignore_ascii_case(&actual) {
        return Err(format!(
            "kernel CABI contract changed: locked_sha256={expected} actual_sha256={actual}; existing symbols are immutable. Restore the old signature and add a versioned symbol, or update {} only after a strictly additive-contract review",
            lock_path.display(),
        ));
    }
    Ok(())
}

fn parse_cabi_contracts(source: &str) -> Result<BTreeMap<String, String>, String> {
    const PREFIX: &str = "pub fn trueos_cabi_";
    let mut contracts = BTreeMap::new();
    let mut cursor = 0usize;
    while let Some(relative_start) = source[cursor..].find(PREFIX) {
        let start = cursor + relative_start;
        let name_start = start + "pub fn ".len();
        let open = source[name_start..]
            .find('(')
            .map(|offset| name_start + offset)
            .ok_or("CABI declaration is missing an argument list")?;
        let name = source[name_start..open].trim();
        if !name
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
        {
            return Err(format!("invalid CABI symbol name `{name}`"));
        }
        let close = matching_paren(source, open)
            .ok_or_else(|| format!("unclosed argument list for {name}"))?;
        let semicolon = source[close + 1..]
            .find(';')
            .map(|offset| close + 1 + offset)
            .ok_or_else(|| format!("missing semicolon after {name}"))?;

        let mut argument_types = Vec::new();
        for argument in split_top_level(&source[open + 1..close], ',') {
            let argument = argument.trim();
            if argument.is_empty() {
                continue;
            }
            let argument_type = top_level_colon(argument)
                .map(|colon| &argument[colon + 1..])
                .unwrap_or(argument);
            argument_types.push(normalize_tokens(argument_type));
        }
        let return_source = source[close + 1..semicolon].trim();
        let return_type = return_source
            .strip_prefix("->")
            .map(normalize_tokens)
            .unwrap_or_else(|| "()".to_string());
        let canonical = format!("fn {name}({})->{return_type}", argument_types.join(","));
        if contracts.insert(name.to_string(), canonical).is_some() {
            return Err(format!("duplicate CABI declaration for {name}"));
        }
        cursor = semicolon + 1;
    }
    Ok(contracts)
}

fn matching_paren(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, character) in source[open..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level(source: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, character) in source.char_indices() {
        match character {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
            _ if character == separator && depth == 0 => {
                parts.push(&source[start..index]);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&source[start..]);
    parts
}

fn top_level_colon(source: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, character) in source.char_indices() {
        match character {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn normalize_tokens(source: &str) -> String {
    source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn aggregate_cabi_contract_digest(contracts: &BTreeMap<String, String>) -> String {
    let mut hasher = Sha256::new();
    for (symbol, contract) in contracts {
        hasher.update((symbol.len() as u64).to_le_bytes());
        hasher.update(symbol.as_bytes());
        hasher.update((contract.len() as u64).to_le_bytes());
        hasher.update(contract.as_bytes());
    }
    let digest = hasher.finalize();
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(output, "{byte:02x}").expect("writing SHA-256 to String cannot fail");
    }
    output
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
