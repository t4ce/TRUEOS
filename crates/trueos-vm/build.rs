use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN_PATTERNS: &[(&str, &str)] = &[
    (
        "crate::r::net",
        "Hull guest code must not depend on host runtime networking (`crate::r::net`). Use vmcall/vlayer facades instead.",
    ),
    (
        "crate::net::",
        "Hull guest code must not depend on host kernel networking (`crate::net`). Use vmcall/vlayer facades instead.",
    ),
    (
        "use v::vnet",
        "Hull guest code must not import raw `v::vnet`. Route networking through approved guest facades only.",
    ),
    (
        "use v::vnetfs",
        "Hull guest code must not import raw `v::vnetfs`. Route networking through approved guest facades only.",
    ),
    (
        "pub use v::vnet",
        "Hull guest code must not re-export raw `v::vnet`. Keep the guest surface high-level and vm-safe.",
    ),
    (
        "pub use v::vnetfs",
        "Hull guest code must not re-export raw `v::vnetfs`. Keep the guest surface high-level and vm-safe.",
    ),
    (
        "use v::vfetch",
        "Hull guest code must not import raw `v::vfetch`. Go through the approved guest fetch-job facade instead.",
    ),
    (
        "pub use v::vfetch",
        "Hull guest code must not re-export raw `v::vfetch`. Keep fetch execution behind the guest-owned job facade.",
    ),
    (
        "v::vfetch::",
        "Hull guest code must not call raw `v::vfetch` directly. Route through the approved guest fetch-job facade instead.",
    ),
    (
        "crate::vfs::",
        "Hull guest code must not depend on raw filesystem plumbing (`crate::vfs`). Use approved guest I/O facades instead.",
    ),
    (
        "crate::vcabi",
        "Hull guest code must not depend on raw CABI (`crate::vcabi`). Go through the guest-safe vlayer facade instead.",
    ),
    (
        "use v::vfs",
        "Hull guest code must not import raw `v::vfs`. Route storage through `v::vio::kfs` instead.",
    ),
    (
        "pub use v::vfs",
        "Hull guest code must not re-export raw `v::vfs`. Keep the guest storage surface at the `vio::kfs` layer.",
    ),
    (
        "use v::vcabi",
        "Hull guest code must not import raw `v::vcabi`. Route through approved guest facades instead.",
    ),
    (
        "pub use v::vcabi",
        "Hull guest code must not re-export raw `v::vcabi`. Route through approved guest facades instead.",
    ),
    (
        "v::vcabi::",
        "Hull guest code must not call raw `v::vcabi` symbols directly. Route through approved guest facades instead.",
    ),
    (
        "use v::vio::cabi",
        "Hull guest code must not import `v::vio::cabi`. Route through approved guest facades instead.",
    ),
    (
        "v::vio::cabi::",
        "Hull guest code must not call `v::vio::cabi` directly. Route through approved guest facades instead.",
    ),
];

fn main() {
    let src_dir = Path::new("src");
    println!("cargo:rerun-if-changed={}", src_dir.display());

    let mut violations = Vec::new();
    visit_rs_files(src_dir, &mut |path| {
        let Ok(contents) = fs::read_to_string(path) else {
            return;
        };
        for (needle, message) in FORBIDDEN_PATTERNS {
            if path.ends_with("src/vfetch_job.rs") && needle.contains("v::vfetch") {
                continue;
            }
            if contents.contains(needle) {
                violations.push(format!(
                    "{}: found forbidden pattern `{}`. {}",
                    path.display(),
                    needle,
                    message
                ));
            }
        }
    });

    if !violations.is_empty() {
        panic!("trueos-vm guest boundary violation(s):\n{}", violations.join("\n"));
    }
}

fn visit_rs_files(dir: &Path, f: &mut impl FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        if path.is_dir() {
            visit_rs_files(&path, f);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            f(&path);
        }
    }
}
