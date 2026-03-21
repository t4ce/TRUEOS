// honor to Fabrice Bellard (same legend behind FFmpeg, QEMU, etc.).
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run(cmd: &mut Command) {
    let status = cmd
        .status()
        .unwrap_or_else(|e| panic!("spawn {:?} failed: {e}", cmd));
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
    let repo = env::var("TRUEOS_QJS_QUICKJS_REPO")
        .unwrap_or_else(|_| "https://github.com/bellard/quickjs".to_string());
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
    run(Command::new("git")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg("--branch")
        .arg(&reference)
        .arg(&repo)
        .arg(&checkout_dir));

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
    let gen_c = out_dir.join("qjs_bytecode_gen.c");
    // Minimal module-only bytecode generator.
    // Usage: qjs_bytecode_gen <app_root> <module_name> <input.mjs> <output.qjsc>
    let src = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "quickjs.h"

static const char *g_app_root = NULL;

static int starts_with(const char *s, const char *pfx) {
    size_t n = strlen(pfx);
    return strncmp(s, pfx, n) == 0;
}

static char *strdup_or_null(const char *s) {
    if (!s) return NULL;
    size_t n = strlen(s);
    char *out = (char*)malloc(n + 1);
    if (!out) return NULL;
    memcpy(out, s, n);
    out[n] = 0;
    return out;
}

static char *path_dirname(const char *path) {
    const char *slash = strrchr(path, '/');
    if (!slash) {
        return strdup_or_null("");
    }
    size_t n = (size_t)(slash - path);
    char *out = (char*)malloc(n + 1);
    if (!out) return NULL;
    memcpy(out, path, n);
    out[n] = 0;
    return out;
}

static char *join2(const char *a, const char *b) {
    size_t na = a ? strlen(a) : 0;
    size_t nb = b ? strlen(b) : 0;
    int need_slash = 0;
    if (na && nb) {
        need_slash = (a[na - 1] != '/' && b[0] != '/');
    }
    size_t n = na + nb + (need_slash ? 1 : 0);
    char *out = (char*)malloc(n + 1);
    if (!out) return NULL;
    size_t i = 0;
    if (na) { memcpy(out + i, a, na); i += na; }
    if (need_slash) out[i++] = '/';
    if (nb) { memcpy(out + i, b, nb); i += nb; }
    out[i] = 0;
    return out;
}

static char *normalize_path(const char *path) {
    // Very small normalizer for paths with '/' separators.
    // - keeps leading '/'
    // - resolves '.' and '..'
    int is_abs = (path && path[0] == '/');
    // Tokenize by '/'.
    const char *p = path;
    const char *segs[512];
    int seg_is_dotdot[512];
    int nseg = 0;
    while (p && *p) {
        while (*p == '/') p++;
        if (!*p) break;
        const char *start = p;
        while (*p && *p != '/') p++;
        size_t len = (size_t)(p - start);
        if (len == 1 && start[0] == '.') {
            continue;
        }
        if (len == 2 && start[0] == '.' && start[1] == '.') {
            if (nseg > 0 && !seg_is_dotdot[nseg - 1]) {
                nseg--;
                continue;
            }
            if (is_abs) {
                continue;
            }
            segs[nseg] = start;
            seg_is_dotdot[nseg] = 1;
            nseg++;
            continue;
        }
        segs[nseg] = start;
        seg_is_dotdot[nseg] = 0;
        nseg++;
        if (nseg >= 512) break;
    }

    // Compute output length.
    size_t out_len = is_abs ? 1 : 0;
    for (int i = 0; i < nseg; i++) {
        if (i != 0) out_len++;
        const char *s = segs[i];
        const char *e = s;
        while (*e && *e != '/') e++;
        out_len += (size_t)(e - s);
    }
    char *out = (char*)malloc(out_len + 1);
    if (!out) return NULL;
    size_t oi = 0;
    if (is_abs) out[oi++] = '/';
    for (int i = 0; i < nseg; i++) {
        if (i != 0) out[oi++] = '/';
        const char *s = segs[i];
        const char *e = s;
        while (*e && *e != '/') e++;
        size_t len = (size_t)(e - s);
        memcpy(out + oi, s, len);
        oi += len;
    }
    out[oi] = 0;
    return out;
}

static char *resolve_spec(const char *base, const char *spec) {
    if (!spec) return NULL;
    if (starts_with(spec, "/")) {
        return normalize_path(spec);
    }
    if (starts_with(spec, "./") || starts_with(spec, "../")) {
        if (!base) {
            return normalize_path(spec);
        }
        char *dir = path_dirname(base);
        if (!dir) return NULL;
        char *tmp = join2(dir, spec);
        free(dir);
        if (!tmp) return NULL;
        char *norm = normalize_path(tmp);
        free(tmp);
        return norm;
    }
    // Bare specifiers not supported in the host precompiler.
    return NULL;
}

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

static JSModuleDef *compile_module_from_buf(JSContext *ctx, const char *module_name, const uint8_t *buf, size_t len) {
    int flags = JS_EVAL_TYPE_MODULE | JS_EVAL_FLAG_COMPILE_ONLY;
    JSValue v = JS_Eval(ctx, (const char*)buf, len, module_name, flags);
    if (JS_IsException(v)) {
        return NULL;
    }
    if (JS_VALUE_GET_TAG(v) != JS_TAG_MODULE) {
        JS_FreeValue(ctx, v);
        return NULL;
    }
    JSModuleDef *m = (JSModuleDef*)JS_VALUE_GET_PTR(v);
    JS_FreeValue(ctx, v);
    return m;
}

static char *qjs_normalize(JSContext *ctx, const char *base_name, const char *name, void *opaque) {
    (void)ctx; (void)opaque;
    char *norm = resolve_spec(base_name, name);
    if (!norm) return NULL;
    return norm; // QuickJS frees with js_free_rt
}

static JSModuleDef *qjs_loader(JSContext *ctx, const char *module_name, void *opaque) {
    (void)opaque;
    if (!module_name || !g_app_root) return NULL;
    if (!starts_with(module_name, "/qjs/")) {
        return NULL;
    }
    const char *rel = module_name + 5; // strip "/qjs/"
    char *fs_path = join2(g_app_root, rel);
    if (!fs_path) return NULL;

    size_t len = 0;
    unsigned char *src = read_file_nul(fs_path, &len);
    free(fs_path);
    if (!src) return NULL;
    JSModuleDef *m = compile_module_from_buf(ctx, module_name, src, len);
    free(src);
    return m;
}

static int write_file(const char *path, const unsigned char *buf, size_t len) {
    FILE *f = fopen(path, "wb");
    if (!f) return 0;
    size_t got = fwrite(buf, 1, len, f);
    fclose(f);
    return got == len;
}

int main(int argc, char **argv) {
    if (argc != 5) {
        fprintf(stderr, "usage: %s <app_root> <module_name> <input.mjs> <output.qjsc>\n", argv[0]);
        return 2;
    }
    g_app_root = argv[1];
    const char *module_name = argv[2];
    const char *in_path = argv[3];
    const char *out_path = argv[4];

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

    JS_SetModuleLoaderFunc(rt, qjs_normalize, qjs_loader, NULL);

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

fn collect_mjs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if let Ok(ft) = ent.file_type() {
            if ft.is_dir() {
                collect_mjs_files(&p, out);
                continue;
            }
        }
        if p.extension().and_then(|s| s.to_str()) == Some("mjs") {
            out.push(p);
        }
    }
}

fn collect_dirs(dir: &Path, out: &mut Vec<PathBuf>) {
    out.push(dir.to_path_buf());
    let rd = match std::fs::read_dir(dir) {
        Ok(v) => v,
        Err(_) => return,
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if let Ok(ft) = ent.file_type() {
            if ft.is_dir() {
                collect_dirs(&p, out);
            }
        }
    }
}

fn to_qjs_specifier(app_root: &Path, file: &Path) -> String {
    let rel = file
        .strip_prefix(app_root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/");
    format!("/qjs/{rel}")
}

fn out_qjsc_path(out_embedded: &Path, app_root: &Path, file: &Path) -> PathBuf {
    let rel = file.strip_prefix(app_root).unwrap_or(file);
    let mut out = out_embedded.join(rel);
    out.set_extension("qjsc");
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    out
}

fn write_embedded_table(out_embedded: &Path, entries: &[(String, PathBuf, Option<PathBuf>)]) {
    // Generates Rust code that populates the embedded registry.
    // Note: this is included by `src/trueos_module_loader/embedded.rs`.
    let mut s = String::new();
    s.push_str("// @generated by crates/trueos-qjs/build.rs\n");
    s.push_str("// Embedded ES modules from crates/trueos-qjs/app/**/*.mjs\n\n");
    s.push_str("static EMBEDDED: &[EmbeddedModule] = &[\n");
    for (spec, src_path, qjsc_path) in entries {
        let spec_bytes = format!("b\"{}\"", spec.replace('"', "\\\""));
        // Use absolute (build-time) paths for include_bytes!.
        let src_lit = src_path.to_string_lossy().replace('\\', "/");
        s.push_str("    EmbeddedModule {\n");
        s.push_str(&format!("        path: {spec_bytes},\n"));
        s.push_str(&format!("        src: include_bytes!(\"{src_lit}\"),\n"));
        if let Some(qjsc_path) = qjsc_path {
            let qjsc_lit = qjsc_path.to_string_lossy().replace('\\', "/");
            s.push_str(&format!(
                "        bytecode: include_bytes!(\"{qjsc_lit}\"),\n"
            ));
        } else {
            s.push_str("        bytecode: b\"\",\n");
        }
        s.push_str("    },\n");
    }
    s.push_str("];\n");

    let out_rs = out_embedded.join("embedded_modules.rs");
    std::fs::write(&out_rs, s).expect("write embedded_modules.rs");
}

fn gen_embedded_modules(quickjs_dir: &Path, manifest_dir: &Path, out_dir: &Path) {
    let app_root = manifest_dir.join("app");
    let out_embedded = out_dir.join("embedded_qjs");
    std::fs::create_dir_all(&out_embedded).expect("create OUT_DIR/embedded_qjs");

    // Important: Cargo only reruns build scripts when watched paths change.
    // If we only watch the current file list, adding a new .mjs won't trigger a rebuild.
    // Watching directories keeps the embedded table in sync when new modules are added.
    let mut dirs: Vec<PathBuf> = Vec::new();
    collect_dirs(&app_root, &mut dirs);
    dirs.sort();
    dirs.dedup();
    for d in &dirs {
        println!("cargo:rerun-if-changed={}", d.display());
    }

    let mut files: Vec<PathBuf> = Vec::new();
    collect_mjs_files(&app_root, &mut files);
    files.sort();

    // Track the whole directory tree for incremental builds by listing files explicitly.
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
    }

    let want_bytecode = env::var("TRUEOS_QJS_EMBED_BYTECODE")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes"))
        .unwrap_or(false);

    let gen_exe = if want_bytecode {
        Some(build_host_qjs_bytecode_gen(quickjs_dir, out_dir))
    } else {
        None
    };

    let mut entries: Vec<(String, PathBuf, Option<PathBuf>)> = Vec::new();
    for f in &files {
        let spec = to_qjs_specifier(&app_root, f);

        if let Some(gen_exe) = &gen_exe {
            let out_qjsc = out_qjsc_path(&out_embedded, &app_root, f);
            let status = Command::new(gen_exe)
                .arg(&app_root)
                .arg(&spec)
                .arg(f)
                .arg(&out_qjsc)
                .status();
            match status {
                Ok(st) if st.success() => {
                    entries.push((spec, f.clone(), Some(out_qjsc)));
                }
                Ok(st) => {
                    let _ = std::fs::remove_file(&out_qjsc);
                    println!(
                        "cargo:warning=embedded precompile skipped (exit={}) for {} (will embed source only)",
                        st.code().unwrap_or(-1),
                        f.display()
                    );
                    entries.push((spec, f.clone(), None));
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&out_qjsc);
                    println!(
                        "cargo:warning=embedded precompile skipped (spawn failed: {}) for {} (will embed source only)",
                        e,
                        f.display()
                    );
                    entries.push((spec, f.clone(), None));
                }
            }
        } else {
            entries.push((spec, f.clone(), None));
        }
    }

    // Always write the table (even if empty) so the include! path exists.
    write_embedded_table(&out_embedded, &entries);
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    println!("cargo:rerun-if-env-changed=TRUEOS_QJS_QUICKJS_DIR");
    println!("cargo:rerun-if-env-changed=TRUEOS_QJS_QUICKJS_REPO");
    println!("cargo:rerun-if-env-changed=TRUEOS_QJS_QUICKJS_REF");
    println!("cargo:rerun-if-env-changed=TRUEOS_QJS_EMBED_BYTECODE");
    println!("cargo:rerun-if-env-changed=CARGO_NET_OFFLINE");

    let quickjs_dir = ensure_quickjs_checkout(&out_dir);

    // Freestanding C ABI stubs for printf/vsnprintf/etc.
    // Kept in the kernel's v layer so both C and Rust can share the same routing.
    let trueos_stdio = manifest_dir
        .join("..")
        .join("..")
        .join("src")
        .join("r")
        .join("stdio.c");
    let yoga_cabi = manifest_dir.join("src").join("yoga").join("yoga_cabi.c");
    let enable_yoga_native = env::var_os("CARGO_FEATURE_YOGA_NATIVE").is_some();

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
    if enable_yoga_native {
        println!("cargo:rerun-if-changed={}", yoga_cabi.display());
    }
    println!(
        "cargo:rerun-if-changed={}",
        quickjs_dir.join("quickjs.h").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        quickjs_dir.join("libregexp.h").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        quickjs_dir.join("libunicode.h").display()
    );

    let target = env::var("TARGET").unwrap_or_default();
    let mut build = cc::Build::new();
    build.include(&quickjs_dir);
    for src in &sources {
        build.file(quickjs_dir.join(src));
    }
    build.file(&trueos_stdio);
    if enable_yoga_native {
        build.file(&yoga_cabi);
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

    // Embedded module registry; bytecode blobs are opt-in via TRUEOS_QJS_EMBED_BYTECODE=1.
    gen_embedded_modules(&quickjs_dir, &manifest_dir, &out_dir);
}
