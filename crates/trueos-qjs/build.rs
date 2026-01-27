use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run(cmd: &mut Command) {
    let status = cmd.status().unwrap_or_else(|e| panic!("spawn {:?} failed: {e}", cmd));
    if !status.success() {
        panic!("command {:?} failed with status {status}", cmd);
    }
}

fn ensure_quickjs_checkout(out_dir: &Path) -> PathBuf {
    // User override: point directly at a QuickJS source tree.
    if let Ok(p) = env::var("TRUEOS_QJS_QUICKJS_DIR") {
        return PathBuf::from(p);
    }

    // Dev convenience: if the workspace has a top-level quickjs/ checkout, use it.
    // This preserves the old behavior for contributors who already cloned it.
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let workspace_quickjs = manifest_dir.join("..").join("..").join("quickjs");
    if workspace_quickjs.join("quickjs.c").is_file() {
        return workspace_quickjs;
    }

    // Otherwise, fetch QuickJS into OUT_DIR so `cargo build` works without submodules
    // or pre-cloned dependencies in the repo.
    let repo = env::var("TRUEOS_QJS_QUICKJS_REPO").unwrap_or_else(|_| "https://github.com/bellard/quickjs".to_string());
    let reference = env::var("TRUEOS_QJS_QUICKJS_REF").unwrap_or_else(|_| "master".to_string());

    // If Cargo is in offline mode, don't try to hit the network.
    if env::var_os("CARGO_NET_OFFLINE").is_some() {
        panic!(
            "QuickJS sources not found in workspace and CARGO_NET_OFFLINE is set. \
Set TRUEOS_QJS_QUICKJS_DIR=/path/to/quickjs or run with network access."
        );
    }

    let checkout_dir = out_dir.join("quickjs-src");
    if checkout_dir.join("quickjs.c").is_file() {
        return checkout_dir;
    }

    // Fresh clone.
    // Note: This is intentionally *not pinned* by default (per user choice).
    // You can pin by setting TRUEOS_QJS_QUICKJS_REF=<commit-or-tag>.
    std::fs::create_dir_all(out_dir).expect("create OUT_DIR");
    if checkout_dir.exists() {
        let _ = std::fs::remove_dir_all(&checkout_dir);
    }

    // Prefer git because it's widely available and QuickJS doesn't always publish tarball tags.
    run(
        Command::new("git")
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg("--branch")
            .arg(&reference)
            .arg(&repo)
            .arg(&checkout_dir),
    );

    if !checkout_dir.join("quickjs.c").is_file() {
        panic!(
            "Fetched QuickJS but did not find quickjs.c at {}",
            checkout_dir.display()
        );
    }

    checkout_dir
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    println!("cargo:rerun-if-env-changed=TRUEOS_QJS_QUICKJS_DIR");
    println!("cargo:rerun-if-env-changed=TRUEOS_QJS_QUICKJS_REPO");
    println!("cargo:rerun-if-env-changed=TRUEOS_QJS_QUICKJS_REF");
    println!("cargo:rerun-if-env-changed=CARGO_NET_OFFLINE");

    let quickjs_dir = ensure_quickjs_checkout(&out_dir);

    // Freestanding C ABI stubs for printf/vsnprintf/etc.
    // Kept in the kernel's surface layer so both C and Rust can share the same routing.
    let trueos_stdio = manifest_dir
        .join("..")
        .join("..")
        .join("src")
        .join("surface")
        .join("stdio.c");

    let sources = [
        "quickjs.c",
        "libregexp.c",
        "libunicode.c",
        "cutils.c",
        "dtoa.c",
    ];

    for src in &sources {
        println!("cargo:rerun-if-changed={}", quickjs_dir.join(src).display());
    }
    println!("cargo:rerun-if-changed={}", trueos_stdio.display());
    println!("cargo:rerun-if-changed={}", quickjs_dir.join("quickjs.h").display());
    println!("cargo:rerun-if-changed={}", quickjs_dir.join("libregexp.h").display());
    println!("cargo:rerun-if-changed={}", quickjs_dir.join("libunicode.h").display());

    let target = env::var("TARGET").unwrap_or_default();
    let mut build = cc::Build::new();
    build.include(&quickjs_dir);
    for src in &sources {
        build.file(quickjs_dir.join(src));
    }
    build.file(&trueos_stdio);

    if !target.contains('-') {
        let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "x86_64".to_string());
        let fixed_target = format!("{}-unknown-none", arch);
        build.target(&fixed_target);
    } else {
        build.target(&target);
    }

    build
        .flag("-ffreestanding")
        .flag("-fno-builtin")
        .flag("-fno-stack-protector")
        .flag("-fno-pic")
        // Release builds often enable glibc "fortify" wrappers (snprintf -> __snprintf_chk)
        // when optimization is on. In a freestanding kernel link, those symbols don't exist.
        .flag("-U_FORTIFY_SOURCE")
        .define("_FORTIFY_SOURCE", Some("0"))
        .define("__NO_FORTIFY", Some("1"))
        .flag("-mno-red-zone")
        .flag("-msse2")
        .flag("-mcmodel=kernel")
        // Prevent QuickJS from enabling pthread-backed Atomics and stack checking.
        .define("EMSCRIPTEN", None)
        .define("CONFIG_VERSION", Some("\"TRUEOS\""))
        .compile("quickjs");
}
