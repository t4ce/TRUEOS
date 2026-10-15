#!/usr/bin/env python3
"""Check production ctype against C-locale libc and real Bash parsing."""
from pathlib import Path
import subprocess
import tempfile
import re

ROOT = Path(__file__).resolve().parents[2]
REGISTRY = Path.home() / '.cargo/registry/src'
C_TEST = r'''
#include <ctype.h>
#include <wctype.h>
#include <locale.h>
#include <stdint.h>
int compare_c_locale(const unsigned short **(*get)(void), int (*space)(uint32_t), int (*alpha)(uint32_t), int (*digit)(uint32_t), int (*alnum)(uint32_t)) {
    if (!setlocale(LC_CTYPE, "C")) return 1;
    const unsigned short *ours = *get(), *host = *__ctype_b_loc();
    if (!ours || get() != get()) return 2;
    for (int c = -128; c <= 255; ++c) if (ours[c] != host[c]) return 1000+c;
    for (uint32_t c = 0; c <= 0x10ffff; ++c)
        if (!!space(c) != !!iswspace(c) || !!alpha(c) != !!iswalpha(c)
            || !!digit(c) != !!iswdigit(c) || !!alnum(c) != !!iswalnum(c)) return 2000+c;
    if (space(WEOF) || alpha(WEOF) || digit(WEOF) || alnum(WEOF)) return 3;
    return 0;
}
'''
PARSER_TEST = r'''
#include <tree_sitter/api.h>
#include <string.h>
extern const TSLanguage *tree_sitter_bash(void);
int real_bash_parser_contract(void) {
    const char *scripts[] = { "echo hello\n", "FOO=bar printf '%s\\n' value\n", "for x in one two; do echo \"$x\"; done\n", "cat <<EOF\nhello\nEOF\n" };
    TSParser *parser = ts_parser_new();
    if (!parser || !ts_parser_set_language(parser, tree_sitter_bash())) return 1;
    for (unsigned i=0;i<sizeof(scripts)/sizeof(*scripts);i++) {
        TSTree *tree = ts_parser_parse_string(parser, NULL, scripts[i], strlen(scripts[i]));
        if (!tree) return 2+i;
        TSNode root = ts_tree_root_node(tree);
        int bad = ts_node_has_error(root) || ts_node_named_child_count(root)==0 || strcmp(ts_node_type(root), "program");
        ts_tree_delete(tree);
        if (bad) return 10+i;
    }
    ts_parser_delete(parser); return 0;
}
'''
RUST = r'''
#![allow(dead_code)]
#[path="CTYPE"] mod ctype;
use core::ffi::{c_void,c_int};
use core::sync::atomic::Ordering;
mod std_abi_shim {
    pub const TRUEOS_ENOSYS:i32=38;
    pub static TRUEOS_ERRNO:core::sync::atomic::AtomicI32=core::sync::atomic::AtomicI32::new(0);
}
use std_abi_shim::TRUEOS_ERRNO;
STATVFS
#[unsafe(no_mangle)] unsafe extern "C" fn trueos_ctype_b_loc()->*const *const u16 {unsafe{ctype::__ctype_b_loc()}}
#[unsafe(no_mangle)] extern "C" fn trueos_iswspace(c:u32)->i32 {ctype::iswspace(c)}
#[unsafe(no_mangle)] extern "C" fn trueos_iswalpha(c:u32)->i32 {ctype::iswalpha(c)}
#[unsafe(no_mangle)] extern "C" fn trueos_iswdigit(c:u32)->i32 {ctype::iswdigit(c)}
#[unsafe(no_mangle)] extern "C" fn trueos_iswalnum(c:u32)->i32 {ctype::iswalnum(c)}
unsafe extern "C" {
 fn compare_c_locale(get:unsafe extern "C" fn()->*const *const u16,space:extern "C" fn(u32)->i32,alpha:extern "C" fn(u32)->i32,digit:extern "C" fn(u32)->i32,alnum:extern "C" fn(u32)->i32)->i32;
 fn real_bash_parser_contract()->i32;
}
#[test] fn actual_c_header_table_all_bytes_eof_signed_and_wide_locale() {
 assert_eq!(unsafe { compare_c_locale(ctype::__ctype_b_loc,ctype::iswspace,ctype::iswalpha,ctype::iswdigit,ctype::iswalnum) },0);
}
#[test] fn parser_uses_real_provider_for_words_heredoc_variables_and_loops() {
 assert_eq!(unsafe {real_bash_parser_contract()},0);
}
#[test] fn accessor_is_stable_across_threads() {
 let initial=unsafe{ctype::__ctype_b_loc()} as usize;
 for _ in 0..8 {assert_eq!(std::thread::spawn(||unsafe{ctype::__ctype_b_loc()}as usize).join().unwrap(),initial);}
}
#[test] fn unsupported_statvfs_never_claims_output_initialization() {
 let mut bytes=[0xa5u8;256];
 assert_eq!(unsafe{statvfs(c"/fixture".as_ptr(),bytes.as_mut_ptr().cast())},-1);
 assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),38);
 assert_eq!(bytes,[0xa5;256]);
 assert_eq!(unsafe{statvfs(core::ptr::null(),core::ptr::null_mut())},-1);
}
'''

def main():
    core = next(REGISTRY.glob('*/tree-sitter-0.25.10'))
    bash = next(REGISTRY.glob('*/tree-sitter-bash-0.25.1'))
    source = (ROOT / 'src/unix_abi_shim.rs').read_text()
    statvfs = re.search(r'^pub unsafe extern "C" fn statvfs\b.*?^}', source, re.MULTILINE|re.DOTALL).group(0)
    with tempfile.TemporaryDirectory(prefix='trueos-ctype-') as folder:
        work = Path(folder)
        (work/'compare.c').write_text(C_TEST);(work/'parser.c').write_text(PARSER_TEST)
        objects=[]
        for i,path in enumerate([work/'compare.c',work/'parser.c',core/'src/lib.c',bash/'src/parser.c',bash/'src/scanner.c']):
            obj=work/f'{i}.o';objects.append(obj)
            flags=[] if i==0 else ['-D__ctype_b_loc=trueos_ctype_b_loc','-Diswspace=trueos_iswspace','-Diswalpha=trueos_iswalpha','-Diswdigit=trueos_iswdigit','-Diswalnum=trueos_iswalnum']
            subprocess.run(['cc','-std=c11','-D_DEFAULT_SOURCE','-O1','-I',str(core/'include'),'-I',str(core/'src'),'-I',str(bash/'src'),*flags,'-c',str(path),'-o',str(obj)],check=True,timeout=120)
        rust=work/'test.rs';rust.write_text(RUST.replace('CTYPE',str(ROOT/'src/blueprint_shims/ctype.rs')).replace('STATVFS',statvfs))
        binary=work/'tests'
        subprocess.run(['rustc','--edition=2024','--test',str(rust),'-o',str(binary),*[f'-Clink-arg={p}' for p in objects]],check=True,timeout=120)
        subprocess.run([str(binary),'--test-threads=1'],check=True,timeout=30)

if __name__=='__main__':main()
