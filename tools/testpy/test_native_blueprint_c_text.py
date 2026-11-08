#!/usr/bin/env python3
"""Differential native parser/regex text and actual selected C provider proof."""
from pathlib import Path
import argparse
import json
import os
import resource
import subprocess
import sys
import tempfile
import test_native_blueprint_c_alloc as base

SDK = base.SDK
CONTRACT = r'''
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <limits.h>
#include <stdint.h>
#include <assert.h>
static int host_format(char *out, size_t cap, const char *format, va_list args) {
 return vsnprintf(out, cap, format, args);
}
#include "__HEADER__"
void trueos_cabi_write(uint32_t stream, const unsigned char *bytes, size_t len) {
 (void)stream; (void)bytes; (void)len;
}
static unsigned cases;
static void compare(const char *format, ...) {
 for (size_t cap = 0; cap < 130; cap++) {
  char expected[160], actual[160];
  memset(expected, 0x5a, sizeof(expected)); memset(actual, 0x5a, sizeof(actual));
  va_list args; va_start(args, format);
  int a = host_format(expected, cap, format, args); va_end(args);
  va_start(args, format);
  int b = trueos_c_text_vformat(actual, cap, format, args); va_end(args);
  assert(a == b); assert(memcmp(actual, expected, sizeof(actual)) == 0); cases++;
 }
}
static int reject_write(void *ctx, const char *bytes, size_t len) {
 (void)ctx; (void)bytes; (void)len; return -1;
}
static int capture_write(void *ctx, const char *bytes, size_t len) {
 char *out = (char *)ctx; memcpy(out, bytes, len); out[len] = 0; return (int)len;
}
int main(int argc, char **argv) {
 if (argc > 1) { char out[16]; trueos_c_text_format(out, sizeof(out), "%f", 1.0); return 1; }
 (void)argv;
 compare("literal %% newline\n");
 compare("%d %i %u %x %X %o", INT_MIN, INT_MAX, UINT_MAX, UINT_MAX, UINT_MAX, UINT_MAX);
 compare("%lld %llu %ld %lu %jd %ju %zd %zu %td", LLONG_MIN, ULLONG_MAX, LONG_MIN, ULONG_MAX, (intmax_t)INTMAX_MIN, (uintmax_t)UINTMAX_MAX, (ptrdiff_t)-91, SIZE_MAX, PTRDIFF_MIN);
 compare("%hhd %hhu %hd %hu", (int)SCHAR_MIN, (unsigned int)UCHAR_MAX, (int)SHRT_MIN, (unsigned int)USHRT_MAX);
 compare("%+09d|% 9d|%-9d|%09d", -11, 11, 11, 11);
 compare("%#08x|%#X|%#.0x|%#.0o|%#.4o", 0x34u, 0x34u, 0u, 0u, 7u);
 compare("%0*.*d|%*.*u|%.*s", 12, 6, -123, -12, 6, 123u, -1, "utf8-å雪");
 compare("%10.3s|%-10.3s|%.0s|%c|%c", "å雪value", "å雪value", "no", 0, 'Z');
 compare("%p|%16p|%-16p", (void *)0, (void *)(uintptr_t)0x12345, (void *)(uintptr_t)0x12345);
 compare("tree-sitter failed to allocate %zu bytes", (size_t)65537);
 compare("ONIG-MONITOR: %-4s %s at: %d [%d - %d] len: %d\n", "#1", "=>", 3, 0, 9, 9);
 compare("labeltooltip=\"error_cost: %u\\ndynamic_precedence: %d\"", UINT_MAX, INT_MIN);
 for (int i = -100; i <= 100; i++) {
  compare("%+08d %08x %8.5u", i, (unsigned)i, (unsigned)i);
  compare("%#.*o %.*s", i % 12, (unsigned)i, i % 16, "abcdefghijklmnopqrstuvwxyz");
 }
 char capture[4096]; TrueosCTextSink sink = {capture_write, capture};
 char large[2049]; memset(large, 'x', sizeof(large)-1); large[sizeof(large)-1] = 0;
 assert(trueos_c_text_printf(&sink, "%s-%u", large, 123u) == 2052);
 assert(strlen(capture) == 2052 && strcmp(capture + 2048, "-123") == 0);
 sink.write = reject_write; assert(trueos_c_text_printf(&sink, "%s", large) == -1);
 assert(trueos_c_text_putc('a', &sink) == -1); assert(trueos_c_text_puts("x", &sink) == -1);
 printf("comparisons:%u\n", cases);
 return cases > 50000 ? 0 : 1;
}
'''
ALLOCATION_FAILURE=r'''
#include <stdlib.h>
#include <unistd.h>
#include "__HEADER__"
void trueos_cabi_write(uint32_t stream,const unsigned char *bytes,size_t len) {
 if (stream != 2 || write(2,bytes,len)!=(ssize_t)len) _exit(91);
}
static void *failed_malloc(size_t size) {(void)size;return NULL;}
static void *failed_calloc(size_t count,size_t size) {(void)count;(void)size;return NULL;}
static void *failed_realloc(void *bytes,size_t size) {(void)bytes;(void)size;return NULL;}
static void failed_free(void *bytes) {(void)bytes;}
#define malloc failed_malloc
#define calloc failed_calloc
#define realloc failed_realloc
#define free failed_free
#include "__ALLOC_SOURCE__"
int main(int argc,char **argv) {
 if (argc!=2) return 92;
 if (argv[1][0]=='m') ts_current_malloc(123);
 else if (argv[1][0]=='c') ts_current_calloc(7,19);
 else if (argv[1][0]=='r') ts_current_realloc(NULL,456);
 return 93;
}
'''
BEHAVIOR = r'''
 let error=onig::Regex::new(r"(?<valid>a)\k<missing>").unwrap_err();
 assert!(error.to_string().contains("undefined name"), "{error}");
 let error=onig::Regex::new("(").unwrap_err();assert!(error.to_string().contains("end pattern"), "{error}");
 let regex=onig::Regex::new(r"(?<日本語>雪+)\k<日本語>").unwrap();assert!(regex.is_match("雪雪"));
 let events=std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));let captured=events.clone();
 parser.set_logger(Some(Box::new(move |kind,message|captured.lock().unwrap().push((kind,message.to_owned())))));
 let tree=parser.parse(program,None).unwrap();let sexp=tree.root_node().to_sexp();
 assert!(sexp.starts_with("(program (redirected_statement body: (command"),"{sexp}");assert!(sexp.contains("heredoc_body"),"{sexp}");
 assert!(sexp.contains("for_statement"),"{sexp}");
 let events=events.lock().unwrap();assert!(events.iter().any(|(kind,message)|*kind==tree_sitter::LogType::Lex&&message.contains("character:")));
 assert!(events.iter().any(|(kind,message)|*kind==tree_sitter::LogType::Parse&&message.contains("sym:")));
 drop(events);parser.set_logger(None);
 println!("ok Onig invalid-regex diagnostics and UTF8 named backreference; Tree-sitter logger and S-expression");
'''
NATIVE_DECLARATIONS=r'''
 static OnigEncodingUTF8:u8;static OnigSyntaxRuby:u8;
 fn onig_new(regex:*mut *mut c_void,start:*const u8,end:*const u8,options:u32,encoding:*const c_void,syntax:*const c_void,error:*mut c_void)->c_int;
 fn onig_search(regex:*mut c_void,string:*const u8,end:*const u8,start:*const u8,range:*const u8,region:*mut c_void,options:u32)->c_int;
 fn onig_free(regex:*mut c_void);
 fn onig_error_code_to_str(message:*mut u8,code:c_int,...)->c_int;
 fn ts_tree_root_node(tree:*const c_void)->TextProbeNode;
 fn ts_node_string(node:TextProbeNode)->*mut c_char;
 fn ts_tree_write_dot_graph(tree:*const c_void,sink:*mut TextProbeSink);
'''
NATIVE_TYPES=r'''
#[repr(C)]struct TextProbeNode{context:[u32;4],id:*const c_void,tree:*const c_void}
#[repr(C)]struct TextProbeSink{write:unsafe extern "C" fn(*mut c_void,*const c_char,usize)->c_int,context:*mut c_void}
unsafe extern "C" fn text_probe_write(_: *mut c_void,bytes:*const c_char,len:usize)->c_int {
 core::hint::black_box((bytes,len));len as c_int
}
'''
NATIVE_ONIG=r'''
  let mut regex=core::ptr::null_mut();let mut error=[0usize;3];
  let pattern=b"(?<native>[a-z]+)";
  let status=onig_new(&mut regex,pattern.as_ptr(),pattern.as_ptr().add(pattern.len()),0,(&raw const OnigEncodingUTF8).cast(),(&raw const OnigSyntaxRuby).cast(),error.as_mut_ptr().cast());
  if status==0 {let text=b"native";let region=onig_region_new();core::hint::black_box(onig_search(regex,text.as_ptr(),text.as_ptr().add(text.len()),text.as_ptr(),text.as_ptr().add(text.len()),region,0));onig_region_free(region,1);onig_free(regex);}
  let invalid=b"(";let code=onig_new(&mut regex,invalid.as_ptr(),invalid.as_ptr().add(invalid.len()),0,(&raw const OnigEncodingUTF8).cast(),(&raw const OnigSyntaxRuby).cast(),error.as_mut_ptr().cast());
  let mut message=[0u8;256];core::hint::black_box(onig_error_code_to_str(message.as_mut_ptr(),code,error.as_ptr()));
'''
NATIVE_TREE=r'''
let node=ts_tree_root_node(t);let text=ts_node_string(node);trueos::native_c_alloc::trueos_blueprint_c_free(text.cast());
let mut sink=TextProbeSink{write:text_probe_write,context:core::ptr::null_mut()};ts_tree_write_dot_graph(t,&mut sink);
'''

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();out=args.output.resolve();out.mkdir(parents=True,exist_ok=True)
    resource.setrlimit(resource.RLIMIT_CORE,(0,0))
    contract=out/'text-contract.c';contract.write_text(CONTRACT.replace('__HEADER__',str(SDK/'src/native_c_text.h')))
    checks=[]
    for mode,flags in [('debug',['-O0']),('release',['-O2'])]:
        binary=out/('text-contract-'+mode)
        command=['cc','-std=c11','-Wall','-Wextra','-Werror','-Wno-unused-function','-Wno-unused-variable',*flags,str(contract),'-o',str(binary)]
        subprocess.run(command,check=True,cwd=out)
        result=subprocess.run([str(binary)],check=True,cwd=out,text=True,capture_output=True)
        comparisons=int(result.stdout.strip().split(':')[1])
        rejected=subprocess.run([str(binary),'unsupported'],cwd=out,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        assert rejected.returncode < 0,rejected.returncode
        checks.append({'mode':mode,'comparisons':comparisons,'unsupported_format_exit':rejected.returncode,'command':command})
    alloc_source=list((Path.home()/'.cargo/registry/src').glob('*/tree-sitter-0.25.10/src/alloc.c'))
    assert len(alloc_source)==1
    fatal=out/'allocation-failure.c'
    fatal.write_text(ALLOCATION_FAILURE.replace('__HEADER__',str(SDK/'src/native_c_text.h')).replace('__ALLOC_SOURCE__',str(alloc_source[0])))
    failures=[]
    for mode,flags in [('debug',['-O0']),('release',['-O2'])]:
        binary=out/('allocation-failure-'+mode)
        subprocess.run(['cc','-std=c11','-Wall','-Wextra','-Werror','-Wno-unused-function','-Wno-unused-variable',*flags,'-I',str(alloc_source[0].parents[1]/'include'),str(fatal),'-o',str(binary)],check=True,cwd=out)
        for operation,diagnostic in [('malloc',b'tree-sitter failed to allocate 123 bytes'),('calloc',b'tree-sitter failed to allocate 133 bytes'),('realloc',b'tree-sitter failed to reallocate 456 bytes')]:
            result=subprocess.run([str(binary),operation],cwd=out,capture_output=True)
            assert result.returncode<0 and result.stderr==diagnostic,(result.returncode,result.stderr)
            failures.append({'mode':mode,'operation':operation,'exit':result.returncode,'diagnostic':result.stderr.decode()})
    # Reuse production staging and real-library checks, extending their actual
    # behavior rather than replacing C with a test formatter or parser.
    base.BEHAVIOR=base.BEHAVIOR.replace('println!("ok actual Bash external scanner and incremental parsing");','println!("ok actual Bash external scanner and incremental parsing");\n'+BEHAVIOR)
    base.BEHAVIOR_GROUPS=6
    base.NATIVE_ENTRY=base.NATIVE_ENTRY.replace('unsafe extern "C" {','unsafe extern "C" {\n'+NATIVE_DECLARATIONS)
    base.NATIVE_ENTRY=base.NATIVE_ENTRY.replace('#[unsafe(no_mangle)]pub extern "C" fn _start()',NATIVE_TYPES+'\n#[unsafe(no_mangle)]pub extern "C" fn _start()')
    base.NATIVE_ENTRY=base.NATIVE_ENTRY.replace('let r=onig_region_new();onig_region_free(r,1);',NATIVE_ONIG)
    base.NATIVE_ENTRY=base.NATIVE_ENTRY.replace('ts_tree_delete(t);',NATIVE_TREE+'ts_tree_delete(t);')
    sys.argv=[sys.argv[0],'--output',str(out/'actual-providers')]
    base.main()
    receipt=json.loads((out/'actual-providers/receipt.json').read_text())
    forbidden={'__fprintf_chk','__snprintf_chk','__vsnprintf_chk','fprintf','snprintf','vsnprintf','fputc','fputs','fwrite','stderr','stdout','fdopen','fclose','dup'}
    assert not forbidden.intersection(receipt['undefined']),receipt['undefined']
    receipt['text_contract']=checks
    receipt['actual_allocation_failure_paths']=failures
    receipt['text_sources']=[base.info(SDK/path)for path in ['src/codex_native_c_text.rs','src/native_c_text.h']]
    receipt['text_behavior_groups_per_profile']=6
    receipt['scope']=__doc__
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print(out/'receipt.json')

if __name__=='__main__':main()
