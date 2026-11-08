#!/usr/bin/env python3
"""Exercise actual ELF weak-symbol resolution and PC-relative thunk selection.

Production functions are extracted unchanged. Only the provider/private-policy
boundary is an explicit fixture; kernel enforcement has separate engine proofs.
"""


def main():
    import argparse
    from pathlib import Path
    import hashlib, json, re, subprocess, tempfile, os
    root=Path(__file__).resolve().parents[2]
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    out=parser.parse_args().output.resolve()
    out.mkdir(parents=True,exist_ok=True)
    source=root/'src/hv/blueprint/blueprint.rs'
    s=source.read_text()
    def item(name):
        start=re.search(r'^fn '+name+r'(?:<[^\n]*?>)?\(',s,re.M).start()
        end=s.index('\n}',start)+2
        return s[start:end]
    names=['parse_elf','section_is_alloc','require_symtab','read_symbol','rel_symbol_value','relocation_type','relocation_symbol','collect_pc_relative_import_symbols','resolve_import']
    items='\n\n'.join(item(n) for n in names)
    (out/'actual-loader.rs').write_text(items)
    (out/'fixture.S').write_text('''
    .text
    .globl main
    main:
     call optional_missing
     call known_weak
     call strong_unknown
     call malloc_usable_size
     ret
    .weak optional_missing
    .weak known_weak
    .weak malloc_usable_size
    .data
    .quad optional_missing
    .quad known_weak
    .quad strong_unknown
    .quad malloc_usable_size
    ''')
    subprocess.run(['cc','-c',str(out/'fixture.S'),'-o',str(out/'fixture.o')],check=True)
    prefix='''
    #![allow(dead_code)]
    extern crate alloc;
    use alloc::collections::BTreeMap;
    use object::read::elf::ElfFile64;
    use object::{Object, ObjectSection, ObjectSymbol, RelocationFlags, RelocationTarget, SectionFlags, SymbolFlags, SymbolIndex, SymbolSection};
    use std::cell::Cell;
    const ELF64_HEADER_LEN:usize=64;
    const STB_WEAK:u8=2;
    const SHF_ALLOC:u64=2;
    const R_X86_64_PC32:u32=2;
    const R_X86_64_PLT32:u32=4;
    type ParsedElf<'data> = ElfFile64<'data, object::Endianness>;
    struct ParsedElfSymbol<'data> {name:&'data str, info:u8, section:SymbolSection, value:u64}
    thread_local! {static FALLBACK:Cell<usize>=const{Cell::new(0)}; static KNOWN:Cell<usize>=const{Cell::new(0)};}
    mod prebind {pub fn retired_import_error(n:&str)->Option<&'static str>{if n=="malloc_usable_size" {Some("retired")}else{None}}}
    mod unix_compat {pub fn unsupported_unix_import_reason(n:&str)->Option<&'static str>{if n=="flock" {Some("unsupported")}else{None}}}
    fn resolve_known_import(n:&str)->Option<usize>{KNOWN.with(|v|v.set(v.get()+1));if n=="known_weak"{Some(0x10000)}else{None}}
    fn resolve_unresolved_import(_: &str)->Option<usize>{FALLBACK.with(|v|v.set(v.get()+1));Some(0x20000)}
    '''
    tests='''
    #[cfg(test)] mod tests {
    use super::*;
    fn fixture()->ParsedElf<'static>{parse_elf(include_bytes!("fixture.o")).unwrap()}
    fn index(f:&ParsedElf<'_>, n:&str)->usize{f.symbols().find(|s|s.name().ok()==Some(n)).unwrap().index().0}
    fn clear(){FALLBACK.with(|v|v.set(0));KNOWN.with(|v|v.set(0));}
    #[test] fn absent_weak_has_zero_address_and_no_fallback(){clear();let f=fixture();assert_eq!(rel_symbol_value(&f,&[],index(&f,"optional_missing")),Ok(0));assert_eq!(FALLBACK.with(Cell::get),0);}
    #[test] fn known_weak_has_real_provider(){clear();let f=fixture();assert_eq!(rel_symbol_value(&f,&[],index(&f,"known_weak")),Ok(0x10000));assert_eq!(FALLBACK.with(Cell::get),0);}
    #[test] fn strong_unknown_keeps_existing_fallback(){clear();let f=fixture();assert_eq!(rel_symbol_value(&f,&[],index(&f,"strong_unknown")),Ok(0x20000));assert_eq!(FALLBACK.with(Cell::get),1);}
    #[test] fn retired_weak_never_reaches_provider_or_fallback(){clear();let f=fixture();assert_eq!(rel_symbol_value(&f,&[],index(&f,"malloc_usable_size")),Ok(0));assert_eq!(KNOWN.with(Cell::get),0);assert_eq!(FALLBACK.with(Cell::get),0);assert!(resolve_import("malloc_usable_size",1).is_none());assert!(resolve_import("flock",2).is_none());}
    #[test] fn pc_relative_thunks_skip_absent_weak(){clear();let f=fixture();let imports=collect_pc_relative_import_symbols(&f).unwrap();assert_eq!(imports.len(),2);assert_eq!(imports.get(&index(&f,"known_weak")),Some(&0x10000));assert_eq!(imports.get(&index(&f,"strong_unknown")),Some(&0x20000));assert!(!imports.contains_key(&index(&f,"optional_missing")));assert!(!imports.contains_key(&index(&f,"malloc_usable_size")));assert_eq!(FALLBACK.with(Cell::get),1);}
    }
    '''
    (out/'lib.rs').write_text(prefix+items+tests)
    (out/'Cargo.toml').write_text('''[package]
    name="trueos-actual-weak-import-tests"
    version="0.1.0"
    edition="2024"
    [lib]
    path="lib.rs"
    [dependencies]
    object={version="=0.39.1",default-features=false,features=["read_core","elf"]}
    [workspace]
    ''')
    logs=[]
    for profile in ['debug','release']:
        cmd=['cargo','+nightly-2026-07-10','test','--offline','--manifest-path',str(out/'Cargo.toml'),'--target','x86_64-unknown-linux-gnu','--target-dir',str(out/'target')]
        if profile=='release':cmd+=['--release']
        p=subprocess.run(cmd,cwd='/var/tmp',stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
        (out/(profile+'.log')).write_text(p.stdout)
        print(profile,p.returncode)
        if p.returncode: print(p.stdout[-1600:])
        if p.returncode: raise SystemExit(p.returncode)
        logs.append({'profile':profile,'passed':5})
    (out/'receipt.json').write_text(json.dumps({'production_source':str(source),'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'extracted_functions':names,'tests':logs,'boundary':'Actual ELF reader, symbol resolution and PC-relative thunk selection execute against a real assembled ELF fixture; provider addresses and retired/unsupported predicates are instrumented fixtures. Kernel provider implementations are not executed.'},indent=2)+'\n')


if __name__ == '__main__':
    main()
