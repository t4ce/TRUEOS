#!/usr/bin/env python3
"""Stage the actual pinned Git path crate and test TRUEOS named-home policy."""
from pathlib import Path
import datetime,hashlib,json,re,shutil,subprocess,tempfile
ROOT=Path(__file__).resolve().parents[2];BLUE=ROOT.parent/'TRUEOS-Blueprints';REG=Path.home()/'.cargo/registry/src/index.crates.io-1949cf8c6b5b557f';AUDIT=ROOT/'bld/codex-native-migration-20261107'
channel=re.search(r'channel\s*=\s*"([^"]+)"',(ROOT/'rust-toolchain.toml').read_text())[1]
def run(command,cwd):
 r=subprocess.run(command,cwd=cwd,text=True,capture_output=True,timeout=120)
 if r.returncode:raise RuntimeError(r.stdout+r.stderr)
 return r
with tempfile.TemporaryDirectory(prefix='trueos-gix-home-') as folder:
 work=Path(folder)
 # Compile the production staging module itself, with filesystem helpers only.
 fixture=Path(__file__).with_name('test_codex_native_identity.py').read_text();start=fixture.index(" host=r'''")+len(" host=r'''");end=fixture.index("'''.replace('@OVERLAY@'",start);stage=fixture[start:end]
 stage=stage.replace('overlay::stage_dirs_sys','overlay::stage_gix_config_value').replace('@OVERLAY@',str(BLUE/'src/native_identity_overlay.rs'))
 (work/'stage.rs').write_text(stage)
 run(['rustc','+'+channel,'--edition=2024',str(work/'stage.rs'),'-o',str(work/'stage')],work)
 run([str(work/'stage'),str(work),str(REG/'gix-config-value-0.21.0')],work)
 staged=work/'source-overlay-crates/gix-config-value-0.21.0-trueos'
 with (staged/'Cargo.toml').open('a') as stream:stream.write('\n[workspace]\n')
 shutil.copyfile(ROOT.parent/'codex/codex-rs/cli/target/trueos-blueprint/work/codex/source-overlay-app/Cargo.lock',staged/'Cargo.lock')
 target=AUDIT/'native-gix-home-target'
 command=['cargo','+'+channel,'rustc','--offline','--manifest-path',str(staged/'Cargo.toml'),'--lib','--target','x86_64-unknown-linux-gnu','--target-dir',str(target),'--message-format=json','--','--cfg','target_os="trueos"','-Aexplicit_builtin_cfgs_in_flags','--check-cfg=cfg(target_os,values("trueos"))']
 result=run(command,work)
 artifacts=[json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
 artifact=next(value for value in artifacts if value.get('reason')=='compiler-artifact' and value['target']['name']=='gix_config_value')
 rlib=next(path for path in artifact['filenames'] if path.endswith('.rlib'))
 tests=r'''use gix_config_value::{Path,path::interpolate::{Context,home_for_user}};
#[path="@HOME@"]mod native_home;
#[test]fn default_named_user_home_is_unavailable_without_a_native_account_home_database(){for user in ["root","t4ce","unknown"]{assert_eq!(home_for_user(user),None);assert!(Path::from(format!("~{user}/repo").as_str()).interpolate(Context::default()).is_err());}}
#[test]fn plain_tilde_uses_actual_captured_home_and_missing_home_remains_error(){unsafe{std::env::set_var("HOME","/apps/codex_7")};let home=native_home::home_dir().unwrap();let context=Context{home_dir:Some(&home),..Default::default()};assert_eq!(Path::from("~").interpolate(context).unwrap(),home);assert_eq!(Path::from("~/.gitconfig").interpolate(context).unwrap(),home.join(".gitconfig"));unsafe{std::env::set_var("HOME","")};assert_eq!(native_home::home_dir(),None);assert!(Path::from("~/.gitconfig").interpolate(Context::default()).is_err());unsafe{std::env::remove_var("HOME")};}
#[test]fn native_policy_preserves_plain_paths_prefix_and_explicit_callbacks(){let home=std::path::Path::new("/apps/codex");assert_eq!(Path::from("repo/config").interpolate(Context::default()).unwrap(),std::path::Path::new("repo/config"));let context=Context{git_install_dir:Some(home),..Default::default()};assert_eq!(Path::from("%(prefix)/gitconfig").interpolate(context).unwrap(),home.join("gitconfig"));fn supplied(user:&str)->Option<std::path::PathBuf>{(user=="known").then(||"/explicit/workspace".into())}let context=Context{home_for_user:Some(supplied),..Default::default()};assert_eq!(Path::from("~known/config").interpolate(context).unwrap(),std::path::Path::new("/explicit/workspace/config"));}
'''.replace('@HOME@',str(BLUE/'src/dirs_sys_trueos.rs'))
 (work/'tests.rs').write_text(tests)
 run(['rustc','+'+channel,'--edition=2024','--test',str(work/'tests.rs'),'--extern','gix_config_value='+rlib,'-L','dependency='+str(target/'x86_64-unknown-linux-gnu/debug/deps'),'-o',str(work/'tests')],work)
 output=run([str(work/'tests'),'--test-threads=1'],work).stdout
 symbols=run(['nm','-u',rlib],work).stdout
 assert not re.search(r'\bgetpwnam\b',symbols), 'native named-home provider retained getpwnam'
 files=[BLUE/'src/native_identity_overlay.rs',BLUE/'src/main.rs',REG/'gix-config-value-0.21.0/src/path.rs',Path(__file__)]
 receipt={'status':'passed','utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'command':command,'stdout':output,'source_sha256':{str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in files},'native_rlib_direct_getpwnam_import':False,'scope':'Actual production pinned gix-config-value staging and full crate compiled with TRUEOS cfg; dependencies are host rlibs. Tests execute exported interpolation API and native HOME provider. No target runtime or filesystem ownership claim.'}
 (AUDIT/'native-gix-home-tests.json').write_text(json.dumps(receipt,indent=2)+'\n');print(output)
