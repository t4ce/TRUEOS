use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let quickjs_dir = manifest_dir.join("..").join("..").join("quickjs");

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
    println!("cargo:rerun-if-changed={}", quickjs_dir.join("quickjs.h").display());
    println!("cargo:rerun-if-changed={}", quickjs_dir.join("libregexp.h").display());
    println!("cargo:rerun-if-changed={}", quickjs_dir.join("libunicode.h").display());

    let target = env::var("TARGET").unwrap_or_default();
    let mut build = cc::Build::new();
    build.include(&quickjs_dir);
    for src in &sources {
        build.file(quickjs_dir.join(src));
    }

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
        .flag("-mno-red-zone")
        .flag("-msse2")
        .flag("-mcmodel=kernel")
        .define("CONFIG_VERSION", Some("\"TRUEOS\""))
        .compile("quickjs");
}
