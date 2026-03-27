use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
};

use trueos_limloader::ensure_limine_from_manifest_dir;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    generate_portal_imports(&manifest_dir).expect("generate portal imports");
    generate_imbafont_registry(&manifest_dir).expect("generate imbafont registry");

    // Ensure Cargo reruns this build script if the Limine toolchain outputs were deleted.
    // These paths are generated into `bld/` (which is not tracked by git), so without explicit
    // `rerun-if-changed` directives Cargo may skip the build script and `make iso` will fail.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=bld/limine-build/.installed");
    println!("cargo:rerun-if-changed=bld/limine-build/.config_args");
    println!("cargo:rerun-if-changed=bld/limine-prefix/share/limine/BOOTX64.EFI");
    println!("cargo:rerun-if-changed=bld/limine-prefix/share/limine/limine-uefi-cd.bin");

    ensure_limine_from_manifest_dir(&manifest_dir);
}

struct FontFaceGen {
    folder: String,
    variant: String,
    assets: Vec<FontAssetGen>,
    metrics_rel_path: Option<String>,
}

struct FontAssetGen {
    ch: char,
    rel_path: String,
}

fn generate_imbafont_registry(manifest_dir: &Path) -> Result<(), String> {
    let font_root = manifest_dir.join("src/gfx/loadscreen");
    println!("cargo:rerun-if-changed={}", font_root.display());

    let mut faces = Vec::new();
    let entries: Vec<_> = fs::read_dir(&font_root)
        .map_err(|err| format!("failed to read {}: {err}", font_root.display()))?
        .collect();
    let mut has_subdirs = false;
    for entry in &entries {
        let entry =
            entry.as_ref().map_err(|err| format!("failed to walk {}: {err}", font_root.display()))?;
        if entry.path().is_dir() {
            has_subdirs = true;
            break;
        }
    }

    if has_subdirs {
        for entry in entries {
            let entry =
                entry.map_err(|err| format!("failed to walk {}: {err}", font_root.display()))?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let folder = entry
                .file_name()
                .into_string()
                .map_err(|_| format!("non-utf8 font directory: {}", path.display()))?;
            let variant = font_enum_variant(&folder)?;
            println!("cargo:rerun-if-changed={}", path.display());

            let metrics_rel_path = optional_metrics_rel_path(manifest_dir, &path)?;
            let assets = collect_font_assets(manifest_dir, &path)?;
            faces.push(FontFaceGen {
                folder,
                variant,
                assets,
                metrics_rel_path,
            });
        }
    } else {
        let assets = collect_font_assets(manifest_dir, &font_root)?;
        faces.push(FontFaceGen {
            folder: String::from("font"),
            variant: String::from("Font"),
            assets,
            metrics_rel_path: optional_metrics_rel_path(manifest_dir, &font_root)?,
        });
    }

    faces.sort_by(|a, b| a.folder.cmp(&b.folder));
    if faces.is_empty() {
        return Err(format!("no font directories found in {}", font_root.display()));
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let generated_path = out_dir.join("generated_imbafont_fonts.rs");
    let generated = build_imbafont_registry_source(&faces);
    fs::write(&generated_path, generated)
        .map_err(|err| format!("failed to write {}: {err}", generated_path.display()))
}

fn optional_metrics_rel_path(manifest_dir: &Path, path: &Path) -> Result<Option<String>, String> {
    let metrics_path = path.join("metrics.txt");
    if !metrics_path.exists() {
        return Ok(None);
    }
    println!("cargo:rerun-if-changed={}", metrics_path.display());
    let rel_path = metrics_path
        .strip_prefix(manifest_dir)
        .map_err(|_| format!("{} is not under {}", metrics_path.display(), manifest_dir.display()))?
        .to_str()
        .ok_or_else(|| format!("non-utf8 metrics path: {}", metrics_path.display()))?
        .replace('\\', "/");
    Ok(Some(rel_path))
}

fn collect_font_assets(manifest_dir: &Path, path: &Path) -> Result<Vec<FontAssetGen>, String> {
    let mut assets = Vec::new();
    for svg_entry in
        fs::read_dir(path).map_err(|err| format!("failed to read {}: {err}", path.display()))?
    {
        let svg_entry =
            svg_entry.map_err(|err| format!("failed to walk {}: {err}", path.display()))?;
        let svg_path = svg_entry.path();
        if svg_path.extension() != Some(OsStr::new("svg")) {
            continue;
        }

        println!("cargo:rerun-if-changed={}", svg_path.display());

        let stem = svg_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| format!("bad svg filename: {}", svg_path.display()))?;
        let ch = if stem.chars().count() == 1 {
            stem.chars().next().unwrap()
        } else {
            let code = u32::from_str_radix(stem, 16)
                .map_err(|err| format!("bad hex glyph filename {}: {err}", svg_path.display()))?;
            let Some(ch) = char::from_u32(code) else {
                return Err(format!(
                    "glyph filename {} maps to invalid Rust char U+{:X}",
                    svg_path.display(),
                    code
                ));
            };
            ch
        };

        let rel_path = svg_path
            .strip_prefix(manifest_dir)
            .map_err(|_| format!("{} is not under {}", svg_path.display(), manifest_dir.display()))?
            .to_str()
            .ok_or_else(|| format!("non-utf8 svg path: {}", svg_path.display()))?
            .replace('\\', "/");

        assets.push(FontAssetGen { ch, rel_path });
    }

    assets.sort_by_key(|asset| asset.ch as u32);
    Ok(assets)
}

fn font_enum_variant(folder: &str) -> Result<String, String> {
    let mut chars = folder.chars();
    let Some(first) = chars.next() else {
        return Err(String::from("empty font folder name"));
    };
    if !first.is_ascii_alphabetic() {
        return Err(format!(
            "font folder {folder} must start with an ASCII letter to become a Rust enum variant"
        ));
    }

    let mut variant = String::new();
    variant.push(first.to_ascii_uppercase());
    for ch in chars {
        if ch.is_ascii_alphanumeric() {
            variant.push(ch);
        } else {
            return Err(format!(
                "font folder {folder} contains unsupported character {ch:?}"
            ));
        }
    }
    Ok(variant)
}

fn char_literal(ch: char) -> String {
    format!("'\\u{{{:X}}}'", ch as u32)
}

fn build_imbafont_registry_source(faces: &[FontFaceGen]) -> String {
    let mut generated = String::new();
    generated.push_str("#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n");
    generated.push_str("pub enum ImbaFontFace {\n");
    for face in faces {
        generated.push_str("    ");
        generated.push_str(&face.variant);
        generated.push_str(",\n");
    }
    generated.push_str("}\n\n");

    for face in faces {
        let ident = face.variant.to_ascii_uppercase();
        generated.push_str("static IMBAFONT_");
        generated.push_str(&ident);
        generated.push_str("_ASSETS: &[SvgIconAsset] = &[\n");
        for asset in &face.assets {
            generated.push_str("    SvgIconAsset {\n");
            generated.push_str("        ch: ");
            generated.push_str(&char_literal(asset.ch));
            generated.push_str(",\n");
            generated.push_str("        bytes: include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/");
            generated.push_str(&asset.rel_path);
            generated.push_str("\")),\n");
            generated.push_str("    },\n");
        }
        generated.push_str("];\n");

        generated.push_str("static IMBAFONT_");
        generated.push_str(&ident);
        generated.push_str("_METRICS: Once<BTreeMap<char, SvgGlyphMetric>> = Once::new();\n");
        generated.push_str("static IMBAFONT_");
        generated.push_str(&ident);
        generated.push_str("_ICONS: Once<Vec<ImbaFontIcon>> = Once::new();\n");
        generated.push_str("static IMBAFONT_");
        generated.push_str(&ident);
        generated.push_str("_LAYOUT_METRICS: Once<Vec<ImbaFontLayoutMetric>> = Once::new();\n\n");
    }

    generated.push_str("fn metrics_bytes_for_face(face: ImbaFontFace) -> &'static [u8] {\n");
    generated.push_str("    match face {\n");
    for face in faces {
        generated.push_str("        ImbaFontFace::");
        generated.push_str(&face.variant);
        generated.push_str(" => ");
        if let Some(rel_path) = &face.metrics_rel_path {
            generated.push_str("include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/");
            generated.push_str(rel_path);
            generated.push_str("\")),\n");
        } else {
            generated.push_str("b\"\",\n");
        }
    }
    generated.push_str("    }\n}\n\n");

    generated.push_str(
        "fn parsed_metrics_for_face(face: ImbaFontFace) -> &'static BTreeMap<char, SvgGlyphMetric> {\n",
    );
    generated.push_str("    let build = || parse_metrics(metrics_bytes_for_face(face));\n");
    generated.push_str("    match face {\n");
    for face in faces {
        generated.push_str("        ImbaFontFace::");
        generated.push_str(&face.variant);
        generated.push_str(" => IMBAFONT_");
        generated.push_str(&face.variant.to_ascii_uppercase());
        generated.push_str("_METRICS.call_once(build),\n");
    }
    generated.push_str("    }\n}\n\n");

    generated.push_str("fn assets_for_face(face: ImbaFontFace) -> &'static [SvgIconAsset] {\n");
    generated.push_str("    match face {\n");
    for face in faces {
        generated.push_str("        ImbaFontFace::");
        generated.push_str(&face.variant);
        generated.push_str(" => IMBAFONT_");
        generated.push_str(&face.variant.to_ascii_uppercase());
        generated.push_str("_ASSETS,\n");
    }
    generated.push_str("    }\n}\n\n");

    generated.push_str("fn icons_for_face(face: ImbaFontFace) -> &'static Vec<ImbaFontIcon> {\n");
    generated.push_str("    let metrics = parsed_metrics_for_face(face);\n");
    generated.push_str("    let assets = assets_for_face(face);\n\n");
    generated.push_str("    let build = || {\n");
    generated.push_str("        let mut icons = Vec::with_capacity(assets.len());\n");
    generated.push_str("        for asset in assets {\n");
    generated.push_str("            let Some(mesh) = build_svg_mesh(asset.bytes) else {\n");
    generated.push_str("                continue;\n");
    generated.push_str("            };\n");
    generated.push_str("            let metric = metrics\n");
    generated.push_str("                .get(&asset.ch)\n");
    generated.push_str("                .copied()\n");
    generated.push_str("                .unwrap_or_else(|| default_metric_from_mesh(&mesh));\n");
    generated.push_str("            icons.push(ImbaFontIcon {\n");
    generated.push_str("                ch: asset.ch,\n");
    generated.push_str("                metric,\n");
    generated.push_str("                mesh,\n");
    generated.push_str("            });\n");
    generated.push_str("        }\n");
    generated.push_str("        icons\n");
    generated.push_str("    };\n\n");
    generated.push_str("    match face {\n");
    for face in faces {
        generated.push_str("        ImbaFontFace::");
        generated.push_str(&face.variant);
        generated.push_str(" => IMBAFONT_");
        generated.push_str(&face.variant.to_ascii_uppercase());
        generated.push_str("_ICONS.call_once(build),\n");
    }
    generated.push_str("    }\n}\n\n");

    generated.push_str(
        "fn layout_metrics_for_face(face: ImbaFontFace) -> &'static Vec<ImbaFontLayoutMetric> {\n",
    );
    generated.push_str("    let build = || {\n");
    generated.push_str("        let icons = icons_for_face(face);\n");
    generated.push_str("        let mut layout_metrics = Vec::with_capacity(icons.len());\n");
    generated.push_str("        for icon in icons {\n");
    generated.push_str("            layout_metrics.push(ImbaFontLayoutMetric { metric: icon.metric });\n");
    generated.push_str("        }\n");
    generated.push_str("        layout_metrics\n");
    generated.push_str("    };\n\n");
    generated.push_str("    match face {\n");
    for face in faces {
        generated.push_str("        ImbaFontFace::");
        generated.push_str(&face.variant);
        generated.push_str(" => IMBAFONT_");
        generated.push_str(&face.variant.to_ascii_uppercase());
        generated.push_str("_LAYOUT_METRICS.call_once(build),\n");
    }
    generated.push_str("    }\n}\n");

    generated
}

fn generate_portal_imports(manifest_dir: &Path) -> Result<(), String> {
    let vcabi_path = manifest_dir.join("crates/trueos-sys/src/vcabi.rs");
    println!("cargo:rerun-if-changed={}", vcabi_path.display());

    let import_names = parse_declared_cabi_imports(&vcabi_path)?;
    let defined_exports = collect_defined_cabi_exports(manifest_dir)?;

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let generated_path = out_dir.join("generated_portal_imports.rs");

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

fn parse_declared_cabi_imports(vcabi_path: &Path) -> Result<Vec<String>, String> {
    let source = fs::read_to_string(vcabi_path)
        .map_err(|err| format!("failed to read {}: {err}", vcabi_path.display()))?;
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
        if path.extension() != Some(OsStr::new("rs")) {
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
        if !line.contains("extern \"C\"") || !line.contains("fn trueos_cabi_") {
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

    if rel == Path::new("ui2/mod.rs") {
        return Ok(String::from("crate::r::ui2"));
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
