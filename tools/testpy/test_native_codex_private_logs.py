#!/usr/bin/env python3
"""Exercise native private diagnostic-file opening before any storage effects.

Production functions are extracted unchanged. Only the provider/private-policy
boundary is an explicit fixture; kernel enforcement has separate engine proofs.
"""


def main():
    import argparse
    from pathlib import Path
    import hashlib,json,subprocess
    root=Path(__file__).resolve().parents[2]
    base=root.parent/'codex/codex-rs'
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    out=parser.parse_args().output.resolve()
    out.mkdir(parents=True,exist_ok=True)
    source=base/'core/src/native_fs.rs'
    s=source.read_text().replace('//!','//')
    fixture='''
    mod trueos {pub mod async_fs {
    use std::{cell::Cell,future::Future,pin::pin,task::{Context,Poll,Waker}};
    thread_local! {pub static RESULT:Cell<i32>=const{Cell::new(0)};}
    pub async fn assert_private_path(_: &[u8])->Result<(),i32>{match RESULT.with(Cell::get){0=>Ok(()),c=>Err(c)}}
    pub fn block_on<F:Future>(f:F)->F::Output {let mut f=pin!(f);match f.as_mut().poll(&mut Context::from_waker(Waker::noop())){Poll::Ready(v)=>v,Poll::Pending=>panic!("fixture must be ready")}}
    }}
    #[cfg(test)] mod tests {
    use super::*;
    fn dir(label:&str)->std::path::PathBuf {let p=std::env::temp_dir().join(format!("trueos-private-log-{}-{label}",std::process::id()));let _=std::fs::remove_dir_all(&p);std::fs::create_dir(&p).unwrap();p}
    fn policy(c:i32){trueos::async_fs::RESULT.with(|v|v.set(c));}
    #[test] fn denied_creation_leaves_no_directories(){let d=dir("denycreate");let p=d.join("new/logs/session.jsonl");policy(-19);assert_eq!(open_private_log(&p,false).unwrap_err().kind(),std::io::ErrorKind::PermissionDenied);assert!(!d.join("new").exists());std::fs::remove_dir_all(d).unwrap();}
    #[test] fn denied_truncation_preserves_existing_bytes(){let d=dir("denytruncate");let p=d.join("session.jsonl");std::fs::write(&p,b"retained").unwrap();policy(-19);assert!(open_private_log(&p,false).is_err());assert_eq!(std::fs::read(&p).unwrap(),b"retained");std::fs::remove_dir_all(d).unwrap();}
    #[test] fn authorized_append_keeps_prior_entries(){use std::io::Write;let d=dir("append");let p=d.join("logs/login.log");policy(0);open_private_log(&p,true).unwrap().write_all(b"one").unwrap();open_private_log(&p,true).unwrap().write_all(b"two").unwrap();assert_eq!(std::fs::read(&p).unwrap(),b"onetwo");std::fs::remove_dir_all(d).unwrap();}
    #[test] fn authorized_session_log_truncates(){use std::io::Write;let d=dir("truncate");let p=d.join("session.jsonl");std::fs::write(&p,b"prior").unwrap();policy(0);open_private_log(&p,false).unwrap().write_all(b"new").unwrap();assert_eq!(std::fs::read(&p).unwrap(),b"new");std::fs::remove_dir_all(d).unwrap();}
    #[test] fn unavailable_policy_has_no_write_fallback(){let d=dir("unsupported");let p=d.join("new/log");policy(-20);assert_eq!(open_private_log(&p,true).unwrap_err().kind(),std::io::ErrorKind::Unsupported);assert!(!d.join("new").exists());std::fs::remove_dir_all(d).unwrap();}
    }
    '''
    (out/'actual.rs').write_text(s+fixture)
    logs=[]
    for profile,opt in [('debug','0'),('release','3')]:
        exe=out/profile
        subprocess.run(['rustc','+nightly-2026-07-10','--edition=2024','--test','-C','opt-level='+opt,str(out/'actual.rs'),'-o',str(exe)],check=True,cwd='/var/tmp')
        p=subprocess.run([str(exe)],capture_output=True,text=True)
        (out/(profile+'.log')).write_text(p.stdout+p.stderr)
        print(profile,p.returncode)
        if p.returncode: print(p.stdout+p.stderr)
        if p.returncode:raise SystemExit(p.returncode)
        logs.append({'profile':profile,'passed':5})
    paths=[source,base/'core/src/lib.rs',base/'cli/src/login.rs',base/'tui/src/startup_orchestration.rs',base/'tui/src/session_log.rs']
    (out/'receipt.json').write_text(json.dumps({'sources':[{'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()}for p in paths],'tests':logs,'boundary':'Actual production native log opener executes against real host files; only async private-path policy result is an explicit fixture. Native authorization engine is separately tested. Actual TUI/CLI linkage awaits normal Codex build.','policy':'Private-path assertion precedes directory creation, append, and truncate; configured public paths fail through existing per-caller error behavior.'},indent=2)+'\n')


if __name__ == '__main__':
    main()
