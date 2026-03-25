use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const SKIP_DIRS: &[&str] = &["vendor", "target", "tgt", "bld", ".git"];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Severity {
    Error,
    Warning,
}

impl Severity {
    fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "ERROR",
            Severity::Warning => "WARNING",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct Finding {
    severity: Severity,
    path: PathBuf,
    line: usize,
    kind: &'static str,
    message: String,
}

#[derive(Clone, Debug)]
struct ConstInfo {
    name: String,
    value: u64,
    #[allow(dead_code)]
    line: usize,
}

#[derive(Clone, Debug)]
struct ArrayInfo {
    name: String,
    len: usize,
    line: usize,
}

#[derive(Clone, Debug)]
struct MaskInfo {
    name: String,
    ty: String,
    bits: u32,
    line: usize,
    origin: &'static str,
}

#[derive(Clone, Debug)]
struct ShiftInfo {
    base_ty: String,
    bits: u32,
    line: usize,
    expr_text: String,
}

#[derive(Clone, Debug)]
struct FunctionInfo {
    name: String,
    line: usize,
    mask_param: Option<(String, u32)>,
    return_mask: Option<(String, u32)>,
}

#[derive(Clone, Debug, Default)]
struct FileFacts {
    consts: Vec<ConstInfo>,
    arrays: Vec<ArrayInfo>,
    masks: Vec<MaskInfo>,
    shifts: Vec<ShiftInfo>,
    functions: Vec<FunctionInfo>,
}

#[derive(Clone, Debug)]
struct ParsedFile {
    path: PathBuf,
    facts: FileFacts,
    lines: Vec<String>,
}

fn main() {
    let root = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let root = root.canonicalize().unwrap_or(root);
    let files = scan_repo(&root);
    let findings = analyze(&root, &files);
    if findings.is_empty() {
        println!("no findings");
        return;
    }
    let errors = findings
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .count();
    let warnings = findings.len().saturating_sub(errors);
    for finding in &findings {
        let rel = finding.path.strip_prefix(&root).unwrap_or(&finding.path);
        println!(
            "{:<7} {}:{} {}: {}",
            finding.severity.as_str(),
            rel.display(),
            finding.line,
            finding.kind,
            finding.message
        );
    }
    println!(
        "\nsummary: {} error(s), {} warning(s), {} total",
        errors,
        warnings,
        findings.len()
    );
}

fn scan_repo(root: &Path) -> Vec<ParsedFile> {
    let mut out = Vec::new();
    walk_rs(root, &mut out);
    out.sort_by(|left, right| left.path.cmp(&right.path));
    out
}

fn walk_rs(path: &Path, out: &mut Vec<ParsedFile>) {
    let Ok(read_dir) = fs::read_dir(path) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| SKIP_DIRS.contains(&name))
            {
                continue;
            }
            walk_rs(&path, out);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let lines = source.lines().map(|line| line.to_string()).collect();
        out.push(ParsedFile {
            path,
            facts: collect_facts(&source),
            lines,
        });
    }
}

fn collect_facts(source: &str) -> FileFacts {
    let mut facts = FileFacts::default();
    for (index, line) in source.lines().enumerate() {
        let line_no = index + 1;
        let trimmed = line.trim();
        if let Some(item) = parse_const(trimmed, line_no) {
            facts.consts.push(item);
        }
        if let Some(item) = parse_array_const(trimmed, line_no) {
            facts.arrays.push(item);
        }
        if let Some(item) = parse_mask_decl(trimmed, line_no) {
            facts.masks.push(item);
        }
        if let Some(item) = parse_function(trimmed, line_no) {
            facts.functions.push(item);
        }
        if let Some(item) = parse_shift(trimmed, line_no) {
            facts.shifts.push(item);
        }
    }
    facts
}

fn strip_vis(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix("pub(crate) ") { return rest; }
    if let Some(rest) = line.strip_prefix("pub(super) ") { return rest; }
    if let Some(rest) = line.strip_prefix("pub(self) ") { return rest; }
    if let Some(rest) = line.strip_prefix("pub ") { return rest; }
    line
}

fn parse_const(line: &str, line_no: usize) -> Option<ConstInfo> {
    let line = strip_vis(line);
    if !line.starts_with("const ") {
        return None;
    }
    let name = line.strip_prefix("const ")?.split(':').next()?.trim();
    let value_text = line.split('=').nth(1)?.trim().trim_end_matches(';').trim();
    let value = parse_integer(value_text)?;
    Some(ConstInfo {
        name: name.to_string(),
        value,
        line: line_no,
    })
}

fn parse_array_const(line: &str, line_no: usize) -> Option<ArrayInfo> {
    let stripped = strip_vis(line);
    if !stripped.starts_with("const ") || !stripped.contains('[') || !stripped.contains(']') {
        return None;
    }
    let name = stripped.strip_prefix("const ")?.split(':').next()?.trim();
    // type is [T; N] — the semicolon inside the brackets holds the length
    let bracket_content = stripped.split('[').nth(1)?.split(']').next()?;
    if !bracket_content.contains(';') {
        return None; // no length — not a fixed-size array const
    }
    let len_text = bracket_content.split(';').nth(1)?.trim();
    let len = parse_integer(len_text).and_then(|value| usize::try_from(value).ok())?;
    Some(ArrayInfo {
        name: name.to_string(),
        len,
        line: line_no,
    })
}

fn parse_mask_decl(line: &str, line_no: usize) -> Option<MaskInfo> {
    if !line.contains("mask") || !line.contains(':') {
        return None;
    }
    let left = line.split(':').next()?.trim();
    let name = left.split_whitespace().last()?.trim_matches(',');
    let ty = line
        .split(':')
        .nth(1)?
        .split(|ch: char| ch == ',' || ch == '=' || ch.is_whitespace())
        .find(|segment| !segment.is_empty())?
        .trim();
    let bits = int_width_bits(ty)?;
    let stripped = strip_vis(line);
    let origin = if stripped.starts_with("static ") {
        "static"
    } else if stripped.starts_with("fn ") || stripped.contains(" fn ") {
        "fn-sig"
    } else {
        "field"
    };
    Some(MaskInfo {
        name: name.to_string(),
        ty: ty.to_string(),
        bits,
        line: line_no,
        origin,
    })
}

fn parse_function(line: &str, line_no: usize) -> Option<FunctionInfo> {
    if !line.contains(" fn ") && !line.starts_with("fn ") {
        return None;
    }
    let fn_pos = line.find("fn ")?;
    let after_fn = &line[fn_pos + 3..];
    let name = after_fn.split('(').next()?.trim();
    let params_text = after_fn.split('(').nth(1)?.split(')').next()?.trim();
    let mut mask_param = None;
    for param in params_text.split(',') {
        let param = param.trim();
        if !param.contains(':') || !param.contains("mask") {
            continue;
        }
        let mut parts = param.split(':');
        let param_name = parts.next()?.trim().to_string();
        let ty = parts.next()?.trim();
        let bits = int_width_bits(ty)?;
        mask_param = Some((param_name, bits));
        break;
    }
    let return_mask = if let Some(return_text) = line.split("->").nth(1) {
        let cleaned = return_text
            .split('{')
            .next()
            .unwrap_or(return_text)
            .trim()
            .trim_end_matches(';')
            .trim();
        let inner = cleaned
            .strip_prefix("Option<")
            .and_then(|rest| rest.strip_suffix('>'))
            .unwrap_or(cleaned);
        int_width_bits(inner).map(|bits| (inner.to_string(), bits))
    } else {
        None
    };
    Some(FunctionInfo {
        name: name.to_string(),
        line: line_no,
        mask_param,
        return_mask,
    })
}

fn parse_shift(line: &str, line_no: usize) -> Option<ShiftInfo> {
    let marker = if line.contains("1u32 <<") {
        "1u32 <<"
    } else if line.contains("1u64 <<") {
        "1u64 <<"
    } else if line.contains("1usize <<") {
        "1usize <<"
    } else {
        return None;
    };
    let bits = int_width_bits(marker.split_whitespace().next()?.trim())?;
    let expr_text = line.split(marker).nth(1)?.trim().trim_end_matches(';').to_string();
    Some(ShiftInfo {
        base_ty: marker.split_whitespace().next()?.to_string(),
        bits,
        line: line_no,
        expr_text,
    })
}

fn analyze(root: &Path, files: &[ParsedFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut max_consts: Vec<(&ParsedFile, &ConstInfo)> = Vec::new();
    let mut arrays: Vec<(&ParsedFile, &ArrayInfo)> = Vec::new();
    let mut masks: Vec<(&ParsedFile, &MaskInfo)> = Vec::new();
    let mut signal_fns: BTreeMap<String, (&ParsedFile, &FunctionInfo)> = BTreeMap::new();
    let mut take_fns: BTreeMap<String, (&ParsedFile, &FunctionInfo)> = BTreeMap::new();

    for file in files {
        for item in &file.facts.consts {
            if is_max_range_const(&item.name) {
                max_consts.push((file, item));
            }
        }
        for item in &file.facts.arrays {
            arrays.push((file, item));
        }
        for item in &file.facts.masks {
            masks.push((file, item));
        }
        for function in &file.facts.functions {
            if let Some(domain) = function.name.strip_prefix("signal_").and_then(|name| name.strip_suffix("_mask")) {
                signal_fns.insert(domain.to_string(), (file, function));
            }
            if let Some(domain) = function.name.strip_prefix("take_").and_then(|name| name.strip_suffix("_mask")) {
                take_fns.insert(domain.to_string(), (file, function));
            }
        }
    }

    for (file, mask) in &masks {
        for (_, max_const) in &max_consts {
            if max_const.value <= mask.bits as u64 {
                continue;
            }
            if !domains_match(&mask.name, &max_const.name) {
                continue;
            }
            findings.push(Finding {
                severity: Severity::Error,
                path: file.path.clone(),
                line: mask.line,
                kind: "mask-width",
                message: format!(
                    "{} {} uses {} ({} bits) but {}={} exceeds that width",
                    mask.origin, mask.name, mask.ty, mask.bits, max_const.name, max_const.value
                ),
            });
        }
    }

    for file in files {
        let local_max = file
            .facts
            .consts
            .iter()
            .filter(|item| is_max_range_const(&item.name))
            .max_by_key(|item| item.value);
        let Some(local_max) = local_max else {
            continue;
        };
        for shift in &file.facts.shifts {
            if local_max.value <= shift.bits as u64 {
                continue;
            }
            if !(shift.expr_text.contains("saturating_sub") || shift.expr_text.contains("- 1") || shift.expr_text.contains("-1")) {
                continue;
            }
            findings.push(Finding {
                severity: Severity::Error,
                path: file.path.clone(),
                line: shift.line,
                kind: "shift-width",
                message: format!(
                    "bit expression with {} base ({} bits) may overflow when {}={} (expr: {})",
                    shift.base_ty, shift.bits, local_max.name, local_max.value, shift.expr_text
                ),
            });
        }
    }

    for (file, array) in &arrays {
        if !array.name.ends_with("_IDS") {
            continue;
        }
        for (_, max_const) in &max_consts {
            if max_const.value <= array.len as u64 {
                continue;
            }
            if !domains_match(&array.name, &max_const.name) {
                continue;
            }
            findings.push(Finding {
                severity: Severity::Warning,
                path: file.path.clone(),
                line: array.line,
                kind: "subset-consumer",
                message: format!(
                    "{} only exposes {} ids, but {}={}; consumers may ignore valid runtime instances",
                    array.name, array.len, max_const.name, max_const.value
                ),
            });
        }
    }

    for (domain, (signal_file, signal_fn)) in &signal_fns {
        let Some((take_file, take_fn)) = take_fns.get(domain) else {
            continue;
        };
        let Some((_, signal_bits)) = &signal_fn.mask_param else {
            continue;
        };
        let Some((_, take_bits)) = &take_fn.return_mask else {
            continue;
        };
        if signal_bits == take_bits {
            continue;
        }
        let message = format!(
            "signal/take pair for `{}` mismatches widths: signal uses {} bits, take returns {} bits",
            domain, signal_bits, take_bits
        );
        findings.push(Finding {
            severity: Severity::Error,
            path: signal_file.path.clone(),
            line: signal_fn.line,
            kind: "signal-take-width",
            message: message.clone(),
        });
        findings.push(Finding {
            severity: Severity::Error,
            path: take_file.path.clone(),
            line: take_fn.line,
            kind: "signal-take-width",
            message,
        });
    }

    // Broader static checks over all scanned lines.
    for file in files {
        for (idx, raw_line) in file.lines.iter().enumerate() {
            let line_no = idx + 1;
            let line = raw_line.trim();
            if line.starts_with("//") {
                continue;
            }

            if line.contains(".unwrap()") || line.contains(".expect(") {
                findings.push(Finding {
                    severity: Severity::Warning,
                    path: file.path.clone(),
                    line: line_no,
                    kind: "panic-path",
                    message: "unwrap/expect in kernel path may panic unexpectedly".to_string(),
                });
            }

            if line.contains("todo!(") || line.contains("unimplemented!(") {
                findings.push(Finding {
                    severity: Severity::Warning,
                    path: file.path.clone(),
                    line: line_no,
                    kind: "unfinished-code",
                    message: "todo!/unimplemented! left in executable code path".to_string(),
                });
            }

            if line.contains("panic!(")
                && !line.contains("assert!")
                && !line.contains("debug_assert!")
            {
                findings.push(Finding {
                    severity: Severity::Warning,
                    path: file.path.clone(),
                    line: line_no,
                    kind: "explicit-panic",
                    message: "explicit panic! in kernel code path".to_string(),
                });
            }

            if line.contains("Ordering::Relaxed") {
                findings.push(Finding {
                    severity: Severity::Warning,
                    path: file.path.clone(),
                    line: line_no,
                    kind: "atomic-ordering",
                    message: "Ordering::Relaxed used; validate memory-ordering contract".to_string(),
                });
            }

            if (line.contains(" as u8") || line.contains(" as u16") || line.contains(" as u32"))
                && (line.contains("len")
                    || line.contains("count")
                    || line.contains("index")
                    || line.contains("_id")
                    || line.contains("size"))
            {
                findings.push(Finding {
                    severity: Severity::Warning,
                    path: file.path.clone(),
                    line: line_no,
                    kind: "narrow-cast",
                    message: "potentially narrowing integer cast on size/id/count value".to_string(),
                });
            }

            if line.contains('[')
                && line.contains(']')
                && (line.contains("_id") || line.contains("index"))
                && !line.contains(".get(")
                && !line.contains(".get_mut(")
            {
                findings.push(Finding {
                    severity: Severity::Warning,
                    path: file.path.clone(),
                    line: line_no,
                    kind: "unchecked-index",
                    message: "indexing by id/index detected without get()/get_mut() guard on this line"
                        .to_string(),
                });
            }
        }
    }

    let mut dedup = BTreeSet::new();
    findings.retain(|finding| {
        dedup.insert((
            finding.severity,
            finding.path.strip_prefix(root).unwrap_or(&finding.path).to_path_buf(),
            finding.line,
            finding.kind,
            finding.message.clone(),
        ))
    });
    findings.sort();
    findings
}

fn int_width_bits(ty: &str) -> Option<u32> {
    match ty {
        "u8" => Some(8),
        "u16" => Some(16),
        "u32" => Some(32),
        "u64" => Some(64),
        "u128" => Some(128),
        "usize" => Some(64),
        _ => None,
    }
}

fn parse_integer(text: &str) -> Option<u64> {
    let trimmed = text.trim();
    let digits = trimmed
        .trim_end_matches(|ch: char| ch.is_ascii_alphabetic() || ch.is_ascii_digit() && false)
        .trim_end_matches(|ch: char| ch.is_ascii_alphabetic());
    let sanitized = digits.replace('_', "");
    if let Some(hex) = sanitized.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).ok()
    } else {
        sanitized.parse::<u64>().ok()
    }
}

fn is_max_range_const(name: &str) -> bool {
    name.contains("MAX")
        && (name.contains("ID")
            || name.contains("INSTANCE")
            || name.contains("COUNT")
            || name.contains("SLOT")
            || name.contains("SIZE"))
}

fn domains_match(left: &str, right: &str) -> bool {
    let left_tokens = domain_tokens(left);
    let right_tokens = domain_tokens(right);
    let overlap = left_tokens.intersection(&right_tokens).count();
    overlap >= 2 || (overlap >= 1 && (left.contains("browser") || right.contains("browser")))
}

fn domain_tokens(name: &str) -> BTreeSet<String> {
    name.split('_')
        .filter_map(|part| {
            let lowered = part.to_ascii_lowercase();
            if lowered.len() <= 2 {
                return None;
            }
            match lowered.as_str() {
                "mask" | "latest" | "taken" | "signal" | "state" | "const" | "max" | "boot" => None,
                _ => Some(lowered),
            }
        })
        .collect()
}
