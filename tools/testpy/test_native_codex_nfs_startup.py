#!/usr/bin/env python3
"""Prove native SQLite startup never selects client-side NFS file migration.

Runs the actual startup prefix through journal-policy selection, actual cached
probe and local-log directory initializer, and actual service migration adapter.
SQL connection establishment is outside this focused branch proof and has its
separate real remote SQLx/native service migration integration matrix.
"""
from pathlib import Path
import argparse
import json
import re
import shutil
import subprocess
import tempfile
import textwrap
from test_native_mio import ROOT, TOOLCHAIN, environment, info, manifest

CODEX = ROOT.parent / 'codex/codex-rs'
SQLITE = CODEX / 'state/src/sqlite.rs'


def function(source, name, indent=0, occurrence=0):
    pattern = r'^' + ' ' * indent + r'(?:pub(?:\([^)]*\))? )?(?:async )?fn ' + re.escape(name) + r'\('
    match = list(re.finditer(pattern, source, re.M))[occurrence]
    end = source.index('\n' + ' ' * indent + '}', match.end()) + 2 + indent
    return textwrap.dedent(source[match.start():end])


HARNESS = r'''
use std::path::{Path,PathBuf};
use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
use tokio::sync::OnceCell;
use local_logs::LocalLogsDir;
use nfs_migration::NfsDbMigration;
mod local_logs;
MOD_SELECTION
mod codex_file_system{use std::{io,path::Path};NATIVE_NFS_PROBE}
static NFS_CALLS:AtomicUsize=AtomicUsize::new(0);
mod nfs{
 pub(super)async fn prepare_database(_: &super::SqliteConfig,_:&std::path::Path)->anyhow::Result<super::NfsDbMigration>{super::NFS_CALLS.fetch_add(1,super::Ordering::SeqCst);anyhow::bail!("native startup selected client migration");}
}
#[derive(Debug,PartialEq)]enum JournalModePolicy{InitializeWal,InitializeDelete,RequireDelete}
struct SqliteConfig{home:PathBuf,local_logs_dir:Option<LocalLogsDir>,enforce_delete_on_nfs:bool,is_nfs:Arc<OnceCell<bool>>,logs_is_nfs:Arc<OnceCell<bool>>}
impl SqliteConfig{
 fn new(home:PathBuf,enforce:bool)->Self{Self{home,local_logs_dir:None,enforce_delete_on_nfs:enforce,is_nfs:Arc::new(OnceCell::new()),logs_is_nfs:Arc::new(OnceCell::new())}}
 fn home(&self)->&Path{&self.home}
 fn logs_db_path(&self)->PathBuf{self.logs_home().join("logs_2.sqlite")}
 METHODS
 async fn selected_startup_policy(&self,path:&Path)->anyhow::Result<(bool,bool,JournalModePolicy)>{STARTUP_PREFIX
 Ok((is_nfs,migration.is_some(),journal_mode_policy))}
}
CACHED_PROBE
#[cfg(test)]mod tests{
 use super::*;
 #[tokio::test]async fn default_home_skips_client_migration_even_when_managed(){let d=tempfile::tempdir().unwrap();for enforce in [false,true]{NFS_CALLS.store(0,Ordering::SeqCst);let home=d.path().join(if enforce{"managed"}else{"ordinary"});let config=SqliteConfig::new(home.clone(),enforce);assert_eq!(config.selected_startup_policy(&home.join("state_5.sqlite")).await.unwrap(),(false,false,JournalModePolicy::InitializeWal));assert!(!home.exists());assert_eq!(NFS_CALLS.load(Ordering::SeqCst),0);}}
 #[tokio::test]async fn configured_log_directory_uses_native_setup_without_nfs_replacement(){let d=tempfile::tempdir().unwrap();NFS_CALLS.store(0,Ordering::SeqCst);let logs=d.path().join("configured-logs");let config=SqliteConfig::new(d.path().join("home"),true).with_local_logs_home(logs.clone());assert_eq!(config.selected_startup_policy(&config.logs_db_path()).await.unwrap(),(false,false,JournalModePolicy::InitializeWal));assert!(logs.is_dir());assert_eq!(std::fs::read_dir(logs).unwrap().count(),0);assert_eq!(NFS_CALLS.load(Ordering::SeqCst),0);}
 #[tokio::test]async fn custom_database_parent_does_not_select_client_nfs_migration(){let d=tempfile::tempdir().unwrap();NFS_CALLS.store(0,Ordering::SeqCst);let config=SqliteConfig::new(d.path().join("home"),true);let custom=d.path().join("alternate/state.sqlite");assert_eq!(config.selected_startup_policy(&custom).await.unwrap(),(false,false,JournalModePolicy::InitializeWal));assert!(!custom.exists());assert!(!custom.parent().unwrap().exists());assert_eq!(NFS_CALLS.load(Ordering::SeqCst),0);}
 #[tokio::test]async fn repeated_native_probes_cache_false_without_creating_local_images(){let d=tempfile::tempdir().unwrap();let home=d.path().join("logical-home");let config=SqliteConfig::new(home.clone(),true);for _ in 0..3{assert!(!config.is_nfs().await.unwrap());}assert_eq!(config.is_nfs.get(),Some(&false));assert!(!home.exists());}
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    out = parser.parse_args().output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    sqlite = SQLITE.read_text()
    startup = function(sqlite, 'open_read_write_pool_with_spec', 4)
    prefix = startup[startup.index('{') + 1:startup.index('let recovery =')]
    methods = '\n\n'.join(function(sqlite, name, 4) for name in [
        'with_local_logs_home', 'logs_home', 'is_nfs', 'logs_are_on_nfs', 'prepare_logs_directory'])
    native_nfs_path = CODEX / 'file-system/src/nfs.rs'
    native_nfs = function(native_nfs_path.read_text(), 'is_nfs', occurrence=1)
    lib = (CODEX / 'state/src/lib.rs').read_text()
    selected = re.search(r'#\[cfg\(not\(target_os = "trueos"\)\)\]\nmod nfs_migration;\n#\[cfg\(target_os = "trueos"\)\]\n#\[path = "nfs_migration_service.rs"\]\nmod nfs_migration;', lib).group()
    with tempfile.TemporaryDirectory(prefix='trueos-nfs-startup-') as temporary:
        work = Path(temporary)
        text = HARNESS.replace('MOD_SELECTION', selected).replace('NATIVE_NFS_PROBE', native_nfs).replace('METHODS', methods).replace('STARTUP_PREFIX', prefix).replace('CACHED_PROBE', function(sqlite, 'probe_nfs'))
        (work / 'lib.rs').write_text(text)
        shutil.copyfile(CODEX / 'state/src/nfs_migration_service.rs', work / 'nfs_migration_service.rs')
        shutil.copyfile(CODEX / 'file-system/src/local_logs.rs', work / 'local_logs.rs')
        # If the original selector ever compiles the local migration file,
        # deliberately fail instead of accidentally testing another substitute.
        (work / 'nfs_migration.rs').write_text('compile_error!("local NFS migration selected for TRUEOS");\n')
        manifest(work, '''[package]
name="native-nfs-startup-fixture"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path="lib.rs"
[dependencies]
anyhow="1"
tempfile="3"
tokio={version="=1.54.1",features=["rt","macros","sync"]}
''')
        wrapper = work / 'native-rustc.py'
        wrapper.write_text('''#!/usr/bin/env python3
import os,sys
args=sys.argv[1:]
if "--crate-name" in args and args[args.index("--crate-name")+1]=="native_nfs_startup_fixture":args += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]
os.execvp(args[0],args)
''')
        wrapper.chmod(0o755)
        shutil.copytree(work, out / 'fixture', dirs_exist_ok=True)
        checks = []
        for label, flags in [('debug', []), ('release', ['--release'])]:
            env = environment(out / 'target')
            env['RUSTC_WRAPPER'] = str(wrapper)
            command = ['rustup', 'run', TOOLCHAIN, 'cargo', 'test', '--offline', *flags, '--target', 'x86_64-unknown-linux-gnu', '--lib', '--', '--test-threads=1']
            log = out / (label + '.log')
            with log.open('w') as stream:
                result = subprocess.run(command, cwd=work, env=env, stdout=stream, stderr=subprocess.STDOUT, timeout=300)
            checks.append({'mode': label, 'tests': 5, 'command': command, 'exit': result.returncode, 'log': str(log)})
            if result.returncode or '5 passed' not in log.read_text():
                raise RuntimeError(log.read_text()[-14000:])
    target = CODEX / 'cli/target/trueos-blueprint/x86_64-unknown-trueos.json'
    command = ['rustup', 'run', TOOLCHAIN, 'rustc', '--print', 'cfg', '--target', str(target), '-Zunstable-options']
    cfg = subprocess.run(command, capture_output=True, text=True, check=True).stdout.splitlines()
    assert 'target_os="trueos"' in cfg and 'target_os="linux"' not in cfg
    graph_path = CODEX / 'cli/target/trueos-blueprint/work/codex/native-unit-graph.json'
    graph = json.loads(graph_path.read_text())
    sqlite_units = [u['pkg_id'] for u in graph['units'] if 'libsqlite3-sys' in u['pkg_id']]
    assert not sqlite_units
    shutil.copyfile(graph_path, out / 'native-unit-graph.json')
    files = [SQLITE, CODEX / 'state/src/lib.rs', CODEX / 'state/src/nfs_migration_service.rs',
             native_nfs_path, CODEX / 'file-system/src/local_logs.rs',
             CODEX / 'state/src/sqlite/connection.rs', CODEX / 'cli/src/migrate_nfs_db.rs',
             ROOT.parent / 'TRUEOS-Blueprints/vendor/sqlx-sqlite-0.9.0/src/lib.rs']
    receipt = {'status': 'passed', 'scope': __doc__, 'checks': checks,
               'actual_startup_prefix': prefix, 'actual_native_nfs_probe': native_nfs,
               'actual_module_selector': selected, 'target': info(target), 'target_cfg': cfg,
               'target_cfg_command': command, 'libsqlite3_sys_unit_count': len(sqlite_units),
               'native_unit_graph': info(out / 'native-unit-graph.json'),
               'sources': [info(path) for path in files],
               'conclusion': 'Native startup migration condition is false for home/log/custom DB paths even with managed enforcement; actual local NFS file/permissions modules are not compiled. Explicit client migration uses the service-owned Unsupported adapter without touching images.',
               'limits': 'Focused actual branch/module-selection proof, not an additional live database startup test. Existing real remote SQLx/native service migration suite covers SQL execution.'}
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(out / 'receipt.json')


if __name__ == '__main__':
    main()
