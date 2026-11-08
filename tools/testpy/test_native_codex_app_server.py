#!/usr/bin/env python3
"""Check actual app-server managed-daemon boundary and backup-location consumers.

The public entry's native guard is extracted before its first cloud/transport
operation. Recovery structs, location descriptions and notice builders are
actual production sources. Filesystem effects use real host files; no SQLite
service or VM lifecycle is simulated as successful.
"""
from pathlib import Path
import argparse,json,subprocess,tempfile
from test_native_mio import ROOT,TOOLCHAIN,environment,info,manifest
from test_native_codex_signals import function
CODEX=ROOT.parent/'codex/codex-rs'
SOURCE=CODEX/'app-server/src/lib.rs'
RECOVERY=CODEX/'state/src/runtime/recovery.rs'
HARNESS=r'''
#![allow(dead_code)]
extern crate self as codex_state;
use std::{path::{Path,PathBuf},collections::HashSet,io::{self,ErrorKind,Result as IoResult}};
__LOCATIONS__
__FUNCTIONS__
struct AppServerRuntimeOptions{managed_daemon:bool}
fn public_native_entry_prefix(runtime_options:AppServerRuntimeOptions,effect:&Path)->IoResult<()>{
 __GUARD__
 std::fs::write(effect,b"foreground transport setup")
}
#[cfg(test)]mod tests{
 use super::*;
 fn service(instance:&str,database:&str,locator:&str)->RuntimeDbBackup{RuntimeDbBackup{original_path:PathBuf::from("/logical/codex/state.sqlite"),backup:RuntimeDbBackupLocation::SqliteService{instance:instance.into(),database:database.into(),locator:locator.into()}}}
 #[test]fn service_backup_locators_remain_opaque_and_are_not_treated_as_paths(){let locator="sqlite-backup:v1:opaque/keep-this-final-component";let entries=recovered_sqlite_databases(&[service("one","state",locator)]);assert_eq!(entries.len(),1);assert_eq!(entries[0].backup_location,locator);assert!(entries[0].service_owned);}
 #[test]fn client_sidecars_are_grouped_by_their_actual_backup_folder(){let backups=["state.sqlite","state.sqlite-wal"].map(|name|RuntimeDbBackup{original_path:PathBuf::from(name),backup:RuntimeDbBackupLocation::ClientFile(PathBuf::from("/backups/group").join(name))});let entries=recovered_sqlite_databases(&backups);assert_eq!(entries.len(),1);assert_eq!(entries[0].backup_location,"/backups/group");assert!(!entries[0].service_owned);}
 #[test]fn same_locator_in_distinct_service_owners_does_not_collapse(){let entries=recovered_sqlite_databases(&[service("one","state","opaque"),service("two","state","opaque"),service("one","logs","opaque"),service("one","state","opaque")]);assert_eq!(entries.len(),3);}
 #[test]fn recovery_notice_labels_client_folders_and_sqlite_owned_images(){let mut backups=vec![service("one","state","opaque-final")];backups.push(RuntimeDbBackup{original_path:PathBuf::from("/local/logs.sqlite"),backup:RuntimeDbBackupLocation::ClientFile(PathBuf::from("/backups/group/logs.sqlite"))});let notice=sqlite_recovery_notice(&recovered_sqlite_databases(&backups)).unwrap();assert!(notice.details.contains("SQLite-owned backup: opaque-final"));assert!(notice.details.contains("Backup folder: /backups/group"));assert!(sqlite_recovery_notice(&[]).is_none());}
 #[test]fn native_recovery_does_not_treat_a_client_file_as_the_sqlite_service_home(){let d=tempfile::tempdir().unwrap();let blocked=d.path().join("logical-home");std::fs::write(&blocked,b"unrelated client file").unwrap();assert!(!sqlite_home_is_blocking_file(&blocked.join("state.sqlite")));}
 #[test]fn public_managed_daemon_rejection_precedes_any_setup_effect(){let d=tempfile::tempdir().unwrap();let effect=d.path().join("setup");let error=public_native_entry_prefix(AppServerRuntimeOptions{managed_daemon:true},&effect).unwrap_err();assert_eq!(error.kind(),ErrorKind::Unsupported);assert!(!effect.exists());public_native_entry_prefix(AppServerRuntimeOptions{managed_daemon:false},&effect).unwrap();assert_eq!(std::fs::read(effect).unwrap(),b"foreground transport setup");}
}
'''
def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True);out=parser.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 source=SOURCE.read_text();recovery=RECOVERY.read_text();locations='\n'.join(function(recovery,item)for item in['pub struct RuntimeDbBackup {','pub enum RuntimeDbBackupLocation {'])
 functions='\n'.join(function(source,item)for item in['struct SqliteRecoveryNotice {','struct RecoveredSqliteDatabase {','fn recovered_sqlite_databases(','fn sqlite_home_is_blocking_file(','fn sqlite_recovery_notice('])
 entry=source.index('pub async fn run_main_with_transport_options(');start=source.index(') -> IoResult<AppServerExit> {',entry)+len(') -> IoResult<AppServerExit> {');end=source.index('    let cloud_skill_provider =',start);guard=source[start:end];assert 'runtime_options.managed_daemon'in guard and'ErrorKind::Unsupported'in guard
 for fragment in['mod managed_daemon;','let nofile_limit_result =','if let Some(Err(err)) = nofile_limit_result']:
  index=source.index(fragment);assert '#[cfg(all(unix, not(target_os = "trueos")))]'in source[max(0,index-70):index]
 with tempfile.TemporaryDirectory(prefix='trueos-app-server-native-')as temporary:
  work=Path(temporary);manifest(work,'[package]\nname="native-codex-app-server-boundaries"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\ntempfile="3"\n');(work/'lib.rs').write_text(HARNESS.replace('__LOCATIONS__',locations).replace('__FUNCTIONS__',functions).replace('__GUARD__',guard))
  wrapper=work/'native-rustc.py';wrapper.write_text('''#!/usr/bin/env python3
import os,sys
args=sys.argv[1:]
if "--crate-name"in args and args[args.index("--crate-name")+1]=="native_codex_app_server_boundaries":args += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]
os.execvp(args[0],args)
''');wrapper.chmod(0o755)
  env=environment(out/'target');env['RUSTC_WRAPPER']=str(wrapper)
  for mode,flags in [('debug',[]),('release',['--release'])]:
   command=['rustup','run',TOOLCHAIN,'cargo','test','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--lib','--','--test-threads=1'];log=out/(mode+'.log')
   with log.open('w')as stream:result=subprocess.run(command,cwd=work,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=180)
   checks.append({'mode':mode,'exit':result.returncode,'command':command,'log':str(log)})
   if result.returncode:raise RuntimeError(log.read_text()[-12000:])
   assert'6 passed'in log.read_text()
 (out/'receipt.json').write_text(json.dumps({'status':'passed','scope':__doc__,'checks':checks,'sources':[info(SOURCE),info(RECOVERY)]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
