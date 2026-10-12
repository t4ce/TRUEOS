#!/usr/bin/env python3
"""Check production prompt submission against VMX selection and lease identity."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    shell = (ROOT / 'src/shell3/shell3.rs').read_text()
    submit = re.search(r'^    fn submit_app_prompt\(.*?^    }', shell, re.M | re.S).group()
    source = r'''
const OPERATOR: char = '§';
#[derive(Debug)] struct MatrixSlotLease {name:String, lifetime:u64}
impl MatrixSlotLease {fn from_identity(name:String,lifetime:u64)->Self {Self{name,lifetime}}}
mod hv {
    pub mod matrix {pub fn vm_for_slot(name:&str,lifetime:u64)->Option<u8> {(name=="app" && lifetime==7).then_some(2)}}
    pub fn vm_run_generation(vm:u8)->Option<u64> {(vm==2).then_some(9)}
    pub fn blueprint_console_submit_stdin_for_lease(vm:u8,lease:&super::MatrixSlotLease,run:u64,input:&[u8])->usize {
        assert_eq!((vm,lease.name.as_str(),lease.lifetime,run),(2,"app",7,9));
        super::INPUT.lock().unwrap().extend_from_slice(input);input.len()
    }
}
static INPUT:std::sync::Mutex<Vec<u8>>=std::sync::Mutex::new(Vec::new());
struct MatrixSlots;
impl MatrixSlots {fn echo(name:Option<&str>,lifetime:Option<u64>,_:String) {assert_eq!((name,lifetime),(Some("app"),Some(7)));}}
#[derive(Default)] struct History {entries:Vec<String>}
impl History {fn remember(&mut self,text:&str){self.entries.push(text.into());} fn end_recall(&mut self){}}
struct Prompt {text:String}
struct Shell3 {vmx:bool,active_matrix_slot:Option<String>,active_matrix_lifetime:Option<u64>,prompt:Prompt,command_history:History}
impl Shell3 {
    fn new(vmx:bool,text:&str)->Self {Self{vmx,active_matrix_slot:Some("app".into()),active_matrix_lifetime:Some(7),prompt:Prompt{text:text.into()},command_history:History::default()}}
    fn active_vmx_app(&self)->Option<()> {self.vmx.then_some(())}
    fn set_prompt(&mut self,text:&str){self.prompt.text=text.into();}
'''
    source += submit + '\n}\n'
    source += r'''
#[test] fn vmx_lines_submit_but_other_views_and_operators_do_not() {
    for text in ["https://example.com/", "hello world", "", "日本語"] {
        INPUT.lock().unwrap().clear();
        let mut shell=Shell3::new(true,text);
        assert!(shell.submit_app_prompt());
        assert_eq!(*INPUT.lock().unwrap(),format!("{text}\n").as_bytes());
        assert!(shell.prompt.text.is_empty());assert_eq!(shell.command_history.entries,vec![text]);
    }
    for (vmx,text) in [(false,"hello"),(true,"§app")] {
        INPUT.lock().unwrap().clear();let mut shell=Shell3::new(vmx,text);
        assert!(!shell.submit_app_prompt());assert_eq!(shell.prompt.text,text);
        assert!(shell.command_history.entries.is_empty());assert!(INPUT.lock().unwrap().is_empty());
    }
    let mut shell=Shell3::new(true,"stale");shell.active_matrix_lifetime=Some(8);
    assert!(!shell.submit_app_prompt());assert_eq!(shell.prompt.text,"stale");
}
'''
    with tempfile.TemporaryDirectory(prefix='sh3-vmx-input-') as directory:
        path = Path(directory)
        (path / 'test.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'test.rs'), '-o', str(path / 'test')], check=True)
        subprocess.run([str(path / 'test')], check=True)


if __name__ == '__main__':
    main()
