#!/usr/bin/env python3
"""Exercise pinned XERJ C providers with the native allocator and text boundary.

Actual Zstd, Bash, Python and Perl execute with host std and native-selected C
build scripts. Every pinned grammar scanner is independently compiled and its
imports checked against current kernel literal bindings. A core+alloc target
then links the actual codec/parser archives without Unix allocation or FILE I/O.
This is provider evidence; it does not boot an engine or exercise native files.
"""
import argparse
import importlib.util
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from test_native_mio import ROOT, TOOLCHAIN, environment, info, manifest
from test_native_rama_tcp import STAGE_DRIVER

SDK = ROOT.parent / "TRUEOS-Blueprints"
TARGET = ROOT.parent / "TRUEOS-Buildins/tools/trueos-object-native.json"

BEHAVIOR = r'''
extern crate alloc;
#[path="@ALLOC@"] mod native_alloc;
use std::{io::Write,ops::ControlFlow};
#[unsafe(no_mangle)]unsafe extern "C" fn trueos_cabi_write(stream:u32,bytes:*const u8,len:usize){
 let bytes=unsafe{std::slice::from_raw_parts(bytes,len)};
 match stream{1=>std::io::stdout().write_all(bytes).unwrap(),2=>std::io::stderr().write_all(bytes).unwrap(),_=>panic!("unexpected native stream")}
}
fn main(){
 let bytes=b"XERJ native compressed stage and AST source evidence.\n".repeat(2000);
 let packed=zstd::stream::encode_all(&bytes[..],3).unwrap();
 assert_eq!(zstd::stream::decode_all(&packed[..]).unwrap(),bytes);
 println!("ok Zstd2.1 native allocator roundtrip");
 let mut parser=tree_sitter::Parser::new();
 for (language,source,kind) in [
  (tree_sitter_bash::LANGUAGE.into(),"cat <<'EOF'\nnative heredoc\nEOF\nfor x in a b; do echo \"$x\"; done\n","program"),
  (tree_sitter_python::LANGUAGE.into(),"def native(value):\n    if value:\n        return value + 7\n    return 0\n","module"),
  (tree_sitter_perl::LANGUAGE.into(),"my $text = <<'END';\nnative heredoc\nEND\nprint $text;\n","source_file"),
 ] {
  parser.set_language(&language).unwrap();
  let tree=parser.parse(source,None).unwrap();assert!(!tree.root_node().has_error(),"{}",tree.root_node().to_sexp());
  assert_eq!(tree.root_node().kind(),kind);
  let sexp=tree.root_node().to_sexp();assert!(sexp.starts_with(&format!("({kind}")));
  let again=parser.parse(source,Some(&tree)).unwrap();assert_eq!(sexp,again.root_node().to_sexp());
  println!("ok {kind} scanner, native text and incremental parse");
 }
 parser.set_language(&tree_sitter_python::LANGUAGE.into()).unwrap();
 let long="value = native(1)\n".repeat(20000);let mut cancelled=|_:&tree_sitter::ParseState|ControlFlow::Break(());
 let parsed=parser.parse_with_options(&mut |offset,_| &long.as_bytes()[offset..],None,Some(tree_sitter::ParseOptions::new().progress_callback(&mut cancelled)));
 assert!(parsed.is_none());parser.reset();
 assert!(!parser.parse("value = 1\n",None).unwrap().root_node().has_error());
 println!("ok progress cancellation and reset after cancellation");
}
'''

NATIVE_ENTRY = r'''
#![no_std]
#![no_main]
use core::ffi::{c_int,c_uint,c_void,c_char};
unsafe extern "C"{
 fn ZSTD_compress(dst:*mut c_void,cap:usize,src:*const c_void,len:usize,level:c_int)->usize;
 fn ts_parser_new()->*mut c_void;fn ts_parser_delete(p:*mut c_void);fn ts_parser_set_language(p:*mut c_void,l:*const c_void)->bool;
 fn tree_sitter_bash()->*const c_void;fn tree_sitter_python()->*const c_void;fn tree_sitter_perl()->*const c_void;
 fn ts_parser_parse_string(p:*mut c_void,old:*const c_void,s:*const c_char,len:c_uint)->*mut c_void;fn ts_tree_delete(p:*mut c_void);
}
#[unsafe(no_mangle)]pub extern "C" fn _start()->!{
 unsafe{
  let p=trueos::native_c_alloc::trueos_blueprint_c_malloc(32);trueos::native_c_alloc::trueos_blueprint_c_free(p);
  let p=trueos::native_c_alloc::trueos_blueprint_c_calloc(7,9);let p=trueos::native_c_alloc::trueos_blueprint_c_realloc(p,128);trueos::native_c_alloc::trueos_blueprint_c_free(p);
  let mut out=[0u8;128];let input=b"value = 1\n";core::hint::black_box(ZSTD_compress(out.as_mut_ptr().cast(),out.len(),input.as_ptr().cast(),input.len(),1));
  let p=ts_parser_new();
  for language in [tree_sitter_bash(),tree_sitter_python(),tree_sitter_perl()]{
   core::hint::black_box(ts_parser_set_language(p,language));
   let tree=ts_parser_parse_string(p,core::ptr::null(),input.as_ptr().cast(),input.len()as c_uint);ts_tree_delete(tree);
  }
  ts_parser_delete(p);
 }
 loop{core::hint::spin_loop()}
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    checks = []
    source = (SDK / "src/codex_native_c_overlay.rs").read_text()
    additional = source.split("const ADDITIONAL_PROVIDERS:", 1)[1].split("];", 1)[0]
    providers = re.findall(r'\("([^"]+)", "([^"]+)"\)', additional)
    assert len(providers) == 37
    registry = Path.home() / ".cargo/registry/src"
    with tempfile.TemporaryDirectory(prefix="trueos-xerj-native-c-") as temporary:
        work = Path(temporary)

        def run(label, command, env=None):
            log = output / (label + ".log")
            with log.open("w") as stream:
                result = subprocess.run(command, cwd=work, env=env, stdout=stream,
                                        stderr=subprocess.STDOUT, timeout=600)
            checks.append({"label": label, "exit": result.returncode, "log": str(log)})
            if result.returncode:
                raise RuntimeError(f"{label}:\n{log.read_text()[-12000:]}")
            return log.read_text()

        driver = STAGE_DRIVER.replace("@MODULE@", json.dumps(str(SDK / "src/codex_native_c_overlay.rs")))
        driver = driver.replace("overlay::stage(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]))",
                                "overlay::stage(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]),&args[3],&args[4])")
        (work / "stage.rs").write_text(driver)
        run("compile-production-stage", ["rustup", "run", TOOLCHAIN, "rustc", "--edition=2024",
                                         str(work / "stage.rs"), "-o", str(work / "stage")])
        contract = work / "grammar-contract.c"
        contract.write_text(r'''
#include <assert.h>
#include <stdint.h>
#include <string.h>
#include <wctype.h>
static int (*original_upper)(wint_t) = iswupper;
static int (*original_lower)(wint_t) = iswlower;
static int (*original_blank)(wint_t) = iswblank;
static int (*original_xdigit)(wint_t) = iswxdigit;
static wint_t (*original_toupper)(wint_t) = towupper;
static wint_t (*original_tolower)(wint_t) = towlower;
static char *(*original_copy)(char *,const char *,size_t) = strncpy;
static char *(*original_cat)(char *,const char *,size_t) = strncat;
#include "@GRAMMAR@"
int main(void) {
 for (wint_t value=0; value<=0x10ffff; value++) {
  assert(!!iswupper(value)==!!original_upper(value));
  assert(!!iswlower(value)==!!original_lower(value));
  assert(!!iswblank(value)==!!original_blank(value));
  assert(!!iswxdigit(value)==!!original_xdigit(value));
  assert(towupper(value)==original_toupper(value));
  assert(towlower(value)==original_tolower(value));
 }
 assert(!iswupper(WEOF));assert(towupper(WEOF)==WEOF);
 for (size_t count=0;count<=12;count++) {
  char actual[32],expected[32];memset(actual,'!',sizeof(actual));memcpy(expected,actual,sizeof(actual));
  assert(strncpy(actual,"native",count)==actual);original_copy(expected,"native",count);
  assert(!memcmp(actual,expected,sizeof(actual)));
  memcpy(actual,"native\0",7);memcpy(expected,actual,sizeof(actual));
  assert(strncat(actual," text",count)==actual);original_cat(expected," text",count);
  assert(!memcmp(actual,expected,sizeof(actual)));
 }
}
'''.replace("@GRAMMAR@", str(SDK / "src/native_c_grammar.h")))
        run("grammar-contract-build", ["cc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                                        str(contract), "-o", str(work / "grammar-contract")])
        run("grammar-contract", [str(work / "grammar-contract")])
        staged = {}
        scanner_imports = {}
        loader = (ROOT / "src/hv/blueprint/blueprint.rs").read_text()
        unix = (ROOT / "src/unix_compat.rs").read_text()
        known = set(re.findall(r'"([^"\n]+)"\s*(?:\||=>)', loader + unix.split("pub(crate) fn resolve_import", 1)[1]))
        for name, version in providers:
            original, = registry.glob(f"*/{name}-{version}")
            relative = "build.rs" if name == "zstd-sys" else "binding_rust/build.rs" if name == "tree-sitter" else "bindings/rust/build.rs"
            before = info(original / relative)
            path = Path(run("stage-" + name, [str(work / "stage"), str(work / "staged"), str(original), name, version]).strip())
            assert info(original / relative) == before
            staged[name] = path
            if name.startswith("tree-sitter-"):
                for i, scanner in enumerate(sorted(path.rglob("scanner.c"))):
                    obj = work / f"{name}-{i}.o"
                    cmd = ["cc", "-std=c11", "-O2", "-fno-stack-protector", "-I", str(scanner.parent),
                           "-include", str(path / "trueos_native_grammar.h")]
                    cmd += [f"-D{function}=trueos_blueprint_c_{function}" for function in ["malloc", "calloc", "realloc", "free"]]
                    run(f"scanner-{name}-{i}", cmd + ["-c", str(scanner), "-o", str(obj)])
                    imports = set(re.findall(r"\bU (\S+)", run(f"imports-{name}-{i}", ["nm", "-u", str(obj)])))
                    unexpected = imports - known - {"_GLOBAL_OFFSET_TABLE_", "ts_current_malloc", "ts_current_calloc", "ts_current_realloc", "ts_current_free"}
                    unexpected = {symbol for symbol in unexpected if not symbol.startswith("trueos_blueprint_c_")}
                    assert not unexpected, (name, unexpected)
                    assert not imports.intersection({"malloc", "calloc", "realloc", "free", "iswblank", "iswupper", "iswlower", "iswxdigit"}), (name, imports)
                    scanner_imports[f"{name}/{i}"] = sorted(imports)
        selected = ["zstd-sys", "tree-sitter", "tree-sitter-bash", "tree-sitter-python", "tree-sitter-perl"]
        for name in selected:
            relative = "build.rs" if name == "zstd-sys" else "binding_rust/build.rs" if name == "tree-sitter" else "bindings/rust/build.rs"
            build = staged[name] / relative
            build.with_name("__native_host_original.rs").write_bytes(build.read_bytes())
            build.write_text('mod original {include!("__native_host_original.rs");pub fn invoke(){main();}}\nfn main(){unsafe{std::env::set_var("CARGO_CFG_TARGET_OS","trueos");}original::invoke();}\n')
        patches = "\n".join(f"{name}={{path={json.dumps(str(staged[name]))}}}" for name in selected)
        manifest(work, f'''[package]
name="native-xerj-c-behavior"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
zstd="=0.14.3"
tree-sitter="=0.26.12"
tree-sitter-bash="=0.23.3"
tree-sitter-python="=0.23.6"
tree-sitter-perl="=1.1.2"
[patch.crates-io]
{patches}
[[bin]]
name="native-xerj-c-behavior"
path="behavior.rs"
''')
        (work / "behavior.rs").write_text(BEHAVIOR.replace("@ALLOC@", str(SDK / "crates/trueos-v/src/native_c_alloc.rs")))
        cargo = ["rustup", "run", TOOLCHAIN, "cargo"]
        env = environment(output / "host-target")
        env["CARGO_BUILD_JOBS"] = "2"
        env["CFLAGS"] = "-fno-stack-protector"
        counts = {}
        for mode, flags in [("debug", []), ("release", ["--release"])]:
            text = run("behavior-" + mode, cargo + ["run", "--offline", *flags, "--target", "x86_64-unknown-linux-gnu"], env)
            counts[mode] = len(re.findall(r"^ok ", text, re.M))
            assert counts[mode] == 5
        archives = []
        base = output / "host-target/x86_64-unknown-linux-gnu/release/build"
        for name in selected:
            archive_name = "libzstd.a" if name == "zstd-sys" else f"lib{name}.a"
            archive, = base.glob(f"{name}-*/out/{archive_name}")
            imports = run("archive-imports-" + name, ["nm", "-u", str(archive)])
            assert not re.search(r"\bU (malloc|calloc|realloc|free|fdopen|fclose|dup|clock_gettime|ZSTD_trace_\w+)\b", imports), imports
            archives.append(archive)
        native = work / "native"
        manifest(native, f'''[package]
name="native-xerj-c-link"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
trueos={{path={json.dumps(str(SDK / 'api'))},default-features=false,features=["native-c-alloc","default-global-allocator","default-panic-handler"]}}
[[bin]]
name="native-xerj-c-link"
path="main.rs"
[profile.release]
panic="abort"
''')
        (native / "main.rs").write_text(NATIVE_ENTRY)
        run("native-core-alloc-object", cargo + ["build", "--offline", "--release", "--manifest-path", str(native / "Cargo.toml"),
                                                "--target", str(TARGET), "-Zbuild-std=core,compiler_builtins,alloc",
                                                "-Zbuild-std-features=compiler-builtins-mem", "-Zjson-target-spec"], environment(output / "native-target"))
        native_object = output / "native-target" / TARGET.stem / "release/native-xerj-c-link.o"
        sysroot = Path(subprocess.check_output(["rustup", "run", TOOLCHAIN, "rustc", "--print", "sysroot"], text=True).strip())
        linker = sysroot / "lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld"
        linked = output / "native-xerj-c.o"
        run("native-link", [str(linker), "-flavor", "gnu", "-r", "--gc-sections", "--undefined=_start", "-o", str(linked),
                            str(native_object), "--start-group", *map(str, archives), "--end-group"])
        imports = sorted(set(re.findall(r"\bU (\S+)", run("native-imports", ["nm", "-u", str(linked)]))))
        assert not {symbol for symbol in set(imports) - known if not symbol.startswith("trueos_cabi_")}, imports
        assert not any(symbol.startswith("trueos_blueprint_c_") for symbol in imports), imports
        # Run the same production admission verifier used by buildin builds.
        tools = ROOT.parent / "TRUEOS-Buildins/tools"
        sys.path.insert(0, str(tools))
        spec = importlib.util.spec_from_file_location("native_c_build_guard", tools / "build.py")
        guard = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = guard
        spec.loader.exec_module(guard)
        guard_object = output / "objects/native-xerj-c/module.stripped.o"
        guard_object.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(linked, guard_object)
        guard.verify_abi(ROOT, output, ["native-xerj-c"])
        disassembly = run("native-instructions", ["objdump", "-d", str(linked)])
        assert not re.search(r"%[fg]s:", disassembly), "native C retained host TLS"
        assert not list((output / "native-target" / TARGET.stem).rglob("libstd-*.rlib"))
        # The same staged sources must retain their original host selection.
        for name in selected:
            relative = "build.rs" if name == "zstd-sys" else "binding_rust/build.rs" if name == "tree-sitter" else "bindings/rust/build.rs"
            build = staged[name] / relative
            build.write_bytes(build.with_name("__native_host_original.rs").read_bytes())
        text = run("behavior-ordinary-host", cargo + ["run", "--offline", "--target", "x86_64-unknown-linux-gnu"], env)
        counts["ordinary-host"] = len(re.findall(r"^ok ", text, re.M))
        assert counts["ordinary-host"] == 5
    (output / "receipt.json").write_text(json.dumps({
        "scope": __doc__, "checks": checks, "behavior_cases": counts,
        "providers": providers, "scanner_imports": scanner_imports,
        "object": info(linked), "undefined": imports, "production_kernel_abi_guard": "passed",
        "sources": [info(SDK / "src" / path) for path in ["codex_native_c_overlay.rs", "codex_native_c_text.rs", "native_c_grammar.h", "native_c_dependency_overlays.rs"]],
    }, indent=2) + "\n")
    print(output / "receipt.json")


if __name__ == "__main__":
    main()
