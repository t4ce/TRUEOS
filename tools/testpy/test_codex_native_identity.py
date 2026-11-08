#!/usr/bin/env python3
"""Compile actual dirs-sys overlay and Codex native HOME/name helpers on host."""
from pathlib import Path
import datetime,hashlib,json,re,subprocess,tempfile
ROOT=Path(__file__).resolve().parents[2];BLUE=ROOT.parent/'TRUEOS-Blueprints';CODEX=ROOT.parent/'codex/codex-rs'
def item(source,signature,last=False):
 start=source.rindex(signature) if last else source.index(signature);at=source.index('{',start);depth=1;end=at+1
 while depth:
  if source[end]=='{':depth+=1
  if source[end]=='}':depth-=1
  end+=1
 return source[start:end]
def run(command,cwd):
 completed=subprocess.run(command,cwd=cwd,text=True,capture_output=True,timeout=90)
 if completed.returncode:raise RuntimeError(completed.stdout+completed.stderr)
 return completed.stdout+completed.stderr
with tempfile.TemporaryDirectory(prefix='trueos-native-identity-') as folder:
 work=Path(folder);registry=Path.home()/'.cargo/registry/src/index.crates.io-1949cf8c6b5b557f';source=registry/'dirs-sys-0.5.0'
 host=r'''use std::path::Path;
fn package_name(path:&Path)->Result<String,String>{value(path,"name")}
fn package_version(path:&Path)->Result<Option<String>,String>{value(path,"version").map(Some)}
fn value(path:&Path,key:&str)->Result<String,String>{let text=std::fs::read_to_string(path).map_err(|e|e.to_string())?;text.lines().find_map(|line|line.strip_prefix(&format!("{key} = ")).map(|v|v.trim_matches('"').to_owned())).ok_or_else(||"manifest".into())}
fn reset_dir(path:&Path)->Result<(),String>{if path.exists(){std::fs::remove_dir_all(path).map_err(|e|e.to_string())?;}std::fs::create_dir_all(path).map_err(|e|e.to_string())}
fn copy_app_tree(source:&Path,dest:&Path)->Result<(),String>{for entry in std::fs::read_dir(source).map_err(|e|e.to_string())?{let entry=entry.map_err(|e|e.to_string())?;let out=dest.join(entry.file_name());if entry.file_type().map_err(|e|e.to_string())?.is_dir(){std::fs::create_dir_all(&out).map_err(|e|e.to_string())?;copy_app_tree(&entry.path(),&out)?;}else{std::fs::copy(entry.path(),out).map_err(|e|e.to_string())?;}}Ok(())}
#[path="@OVERLAY@"]mod overlay;
fn main(){let args:Vec<_>=std::env::args().collect();overlay::stage_dirs_sys(Path::new(&args[1]),Path::new(&args[2])).unwrap();}
'''.replace('@OVERLAY@',str(BLUE/'src/native_identity_overlay.rs'))
 (work/'stage.rs').write_text(host)
 channel=re.search(r'channel\s*=\s*"([^"]+)"',(ROOT/'rust-toolchain.toml').read_text())[1]
 run(['rustc','+'+channel,'--edition=2024',str(work/'stage.rs'),'-o',str(work/'stage')],work)
 run([str(work/'stage'),str(work),str(source)],work)
 staged=work/'source-overlay-crates/dirs-sys-0.5.0-trueos'
 shell=(CODEX/'shell-command/src/shell_detect.rs').read_text();greeting=(CODEX/'core/src/realtime_prompt.rs').read_text()
 added='''\n#[cfg(test)]mod native_tests {use super::*;
fn set(key:&str,value:Option<&str>){unsafe{if let Some(value)=value{std::env::set_var(key,value)}else{std::env::remove_var(key)}}}
#[test]fn native_home_is_the_captured_value_and_has_no_passwd_fallback(){for value in [None,Some(""),Some("/apps/codex_7"),Some("relative-native-home")]{set("HOME",value);let expected=value.filter(|v|!v.is_empty()).map(std::path::PathBuf::from);assert_eq!(home_dir(),expected);assert_eq!(codex::get_user_home_path(),expected);}set("HOME",None);}
#[test]fn native_home_preserves_nonunicode_bytes_without_identity_lookup(){use std::os::unix::ffi::{OsStringExt,OsStrExt};let value=std::ffi::OsString::from_vec(vec![b'/',b'a',255]);unsafe{std::env::set_var("HOME",&value)};assert_eq!(home_dir().unwrap().as_os_str().as_bytes(),value.as_os_str().as_bytes());set("HOME",None);}
#[test]fn native_shell_uses_the_existing_fallback_and_never_a_passwd_shell(){assert_eq!(codex::get_user_shell_path(),None);}
#[test]fn native_greeting_uses_optional_explicit_name_and_existing_default(){for(user,username,expected)in [(None,None,"there"),(Some(""),Some(""),"there"),(Some("  Ada Lovelace "),Some("ignored"),"Ada"),(Some("  "),Some("Turing"),"Turing"),(None,Some("Grace Hopper"),"Grace")]{set("USER",user);set("USERNAME",username);assert_eq!(codex::current_user_first_name(),expected);}set("USER",None);set("USERNAME",None);}
}
mod codex {use std::path::PathBuf;const DEFAULT_USER_FIRST_NAME:&str="there";
'''
 # Only visibility is widened for sibling fixture tests; production function bodies are unchanged.
 start=shell.index('// TRUEOS executes ShellType::Sh');native=shell[start:shell.index('#[cfg(not(any(unix',start)]
 added+=item(native,'fn get_user_shell_path()').replace('fn get_user_shell_path','pub fn get_user_shell_path',1)
 added+='\n'+item(native,'pub fn get_user_home_path()')+'\n'+item(greeting,'fn current_user_first_name()',last=True).replace('fn current_user_first_name','pub fn current_user_first_name',1)+'\n}\n'
 lib=staged/'src/lib.rs';lib.write_text(lib.read_text()+added)
 option=registry/'option-ext-0.2.0/src/lib.rs'
 if not option.is_file():option=next(registry.glob('option-ext-*/src/lib.rs'))
 run(['rustc','+'+channel,'--edition=2015','--crate-type=rlib','--crate-name','option_ext',str(option),'-o',str(work/'liboption_ext.rlib')],work)
 command=['rustc','+'+channel,'--edition=2024','--test','--crate-name','dirs_sys','--cfg','target_os="trueos"','-Aexplicit_builtin_cfgs_in_flags','--extern','option_ext='+str(work/'liboption_ext.rlib'),str(lib),'-o',str(work/'tests')]
 run(command,work);output=run([str(work/'tests'),'--test-threads=1'],work);print(output)
 files=[BLUE/'src/native_identity_overlay.rs',BLUE/'src/dirs_sys_trueos.rs',CODEX/'shell-command/src/shell_detect.rs',CODEX/'core/src/realtime_prompt.rs',CODEX/'core/Cargo.toml']
 receipt={'status':'passed','utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'command':command,'source_sha256':{str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in files},'stdout':output,'scope':'Actual pinned dirs-sys 0.5 staging module and selected full provider, with native Codex helpers extracted unchanged; host environment controls only. No kernel/user authentication files touched.'}
 (ROOT/'bld/codex-native-migration-20261107/native-identity-tests.json').write_text(json.dumps(receipt,indent=2)+'\n')
