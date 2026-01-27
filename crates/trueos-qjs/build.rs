use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let quickjs_dir = manifest_dir.join("..").join("..").join("quickjs");

    let quickjs_probe = quickjs_dir.join("quickjs.c");
    if !quickjs_probe.is_file() {
        panic!(
            "Missing QuickJS sources at {}. Fetch deps first (e.g. run `make deps` or `./scripts/fetch-deps.sh`).",
            quickjs_probe.display()
        );
    }

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
