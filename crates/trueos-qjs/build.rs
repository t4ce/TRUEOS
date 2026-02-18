// honor to Fabrice Bellard (same legend behind FFmpeg, QEMU, etc.).
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

fn build_host_qjs_bytecode_gen(quickjs_dir: &Path, out_dir: &Path) -> PathBuf {
    let exe = out_dir.join("qjs_bytecode_gen");
    if exe.is_file() {
        return exe;
    }

    let gen_c = out_dir.join("qjs_bytecode_gen.c");
    // Minimal module-only bytecode generator.
    // Usage: qjs_bytecode_gen <module_name> <input.mjs> <output.qjsc>
    let src = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "quickjs.h"

static unsigned char *read_file_nul(const char *path, size_t *out_len) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    if (fseek(f, 0, SEEK_END) != 0) { fclose(f); return NULL; }
    long sz = ftell(f);
    if (sz < 0) { fclose(f); return NULL; }
    if (fseek(f, 0, SEEK_SET) != 0) { fclose(f); return NULL; }
    size_t len = (size_t)sz;
    unsigned char *buf = (unsigned char*)malloc(len + 1);
    if (!buf) { fclose(f); return NULL; }
    size_t got = fread(buf, 1, len, f);
    fclose(f);
    if (got != len) { free(buf); return NULL; }
    buf[len] = 0;
    *out_len = len;
    return buf;
}

static int write_file(const char *path, const unsigned char *buf, size_t len) {
    FILE *f = fopen(path, "wb");
    if (!f) return 0;
    size_t got = fwrite(buf, 1, len, f);
    fclose(f);
    return got == len;
}

int main(int argc, char **argv) {
    if (argc != 4) {
        fprintf(stderr, "usage: %s <module_name> <input.mjs> <output.qjsc>\n", argv[0]);
        return 2;
    }
    const char *module_name = argv[1];
    const char *in_path = argv[2];
    const char *out_path = argv[3];

    size_t src_len = 0;
    unsigned char *src = read_file_nul(in_path, &src_len);
    if (!src) {
        fprintf(stderr, "read failed: %s\n", in_path);
        return 3;
    }

    JSRuntime *rt = JS_NewRuntime();
    if (!rt) {
        free(src);
        return 4;
    }
    JSContext *ctx = JS_NewContext(rt);
    if (!ctx) {
        JS_FreeRuntime(rt);
        free(src);
        return 5;
    }

    int flags = JS_EVAL_TYPE_MODULE | JS_EVAL_FLAG_COMPILE_ONLY;
    JSValue v = JS_Eval(ctx, (const char*)src, src_len, module_name, flags);
    if (JS_IsException(v)) {
        JSValue exc = JS_GetException(ctx);
        const char *s = JS_ToCString(ctx, exc);
        if (s) {
            fprintf(stderr, "compile exception: %s\n", s);
            JS_FreeCString(ctx, s);
        } else {
            fprintf(stderr, "compile exception\n");
        }
        JS_FreeValue(ctx, exc);
        JS_FreeValue(ctx, v);
        JS_FreeContext(ctx);
        JS_FreeRuntime(rt);
        free(src);
        return 6;
    }

    size_t bc_len = 0;
    uint8_t *bc = JS_WriteObject(ctx, &bc_len, v, JS_WRITE_OBJ_BYTECODE);
    JS_FreeValue(ctx, v);
    if (!bc || bc_len == 0) {
        JS_FreeContext(ctx);
        JS_FreeRuntime(rt);
        free(src);
        return 7;
    }

    int ok = write_file(out_path, bc, bc_len);
    js_free(ctx, bc);

    JS_FreeContext(ctx);
    JS_FreeRuntime(rt);
    free(src);

    return ok ? 0 : 8;
}
"#;
    std::fs::write(&gen_c, src).expect("write qjs_bytecode_gen.c");

    let sources = [
        "quickjs.c",
        "libregexp.c",
        "libunicode.c",
        "cutils.c",
        "dtoa.c",
    ];

    // Build the generator as a host executable (never freestanding).
    let host_cc = env::var("HOST_CC").ok().unwrap_or_else(|| "cc".to_string());
    let mut cmd = Command::new(host_cc);
    cmd.arg("-O2")
        .arg("-I")
        .arg(quickjs_dir)
        .arg("-DCONFIG_VERSION=\"TRUEOS\"")
        .arg("-o")
        .arg(&exe)
        .arg(&gen_c);
    for s in &sources {
        cmd.arg(quickjs_dir.join(s));
    }
    cmd.arg("-lm").arg("-ldl").arg("-pthread");
    run(&mut cmd);

    exe
}

fn gen_embedded_bytecode(quickjs_dir: &Path, manifest_dir: &Path, out_dir: &Path) {
    let app_util = manifest_dir.join("app").join("util.mjs");
    println!("cargo:rerun-if-changed={}", app_util.display());

    let out_embedded = out_dir.join("embedded_qjs");
    std::fs::create_dir_all(&out_embedded).expect("create OUT_DIR/embedded_qjs");

    let gen_exe = build_host_qjs_bytecode_gen(quickjs_dir, out_dir);
    let out_qjsc = out_embedded.join("util.qjsc");

    run(
        Command::new(gen_exe)
            .arg("/qjs/util.mjs")
            .arg(&app_util)
            .arg(&out_qjsc),
    );
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
        .flag("-w")
        // Prevent QuickJS from enabling pthread-backed Atomics and stack checking.
        .define("EMSCRIPTEN", None)
        .define("CONFIG_VERSION", Some("\"TRUEOS\""))
        .compile("quickjs");

    // Build-time embedded module bytecode blobs (tiny, deterministic, no runtime compilation).
    gen_embedded_bytecode(&quickjs_dir, &manifest_dir, &out_dir);
}
