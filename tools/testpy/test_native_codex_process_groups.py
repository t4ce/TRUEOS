#!/usr/bin/env python3
"""Exercise actual native process-group boundary; no libc symbol is supplied."""
from pathlib import Path
import argparse,json,subprocess,tempfile
from test_native_mio import ROOT,TOOLCHAIN,environment,info
SOURCE=ROOT.parent/'codex/codex-rs/utils/pty/src/process_group.rs'
HARNESS=r'''extern crate self as tokio;
pub mod process { pub enum Child {} }
#[path="@SOURCE@"]mod process_group;
#[test]fn native_group_operations_report_unsupported_instead_of_success(){
 use process_group::*;use std::io::ErrorKind;
 for result in[set_parent_death_signal(0),detach_from_tty(),set_process_group(),kill_process_group_by_pid(0),interrupt_process_group(0),kill_process_group(0)]{assert_eq!(result.unwrap_err().kind(),ErrorKind::Unsupported);}
 assert_eq!(terminate_process_group(0).unwrap_err().kind(),ErrorKind::Unsupported);
}
'''
def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True);out=parser.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 with tempfile.TemporaryDirectory(prefix='trueos-native-process-groups-')as temporary:
  work=Path(temporary);source=work/'lib.rs';source.write_text(HARNESS.replace('@SOURCE@',str(SOURCE)))
  for mode,opt in[('debug','0'),('release','3')]:
   binary=work/mode;command=['rustup','run',TOOLCHAIN,'rustc','--edition=2024','--test','--cfg','target_os="trueos"','-Aexplicit_builtin_cfgs_in_flags','-C','opt-level='+opt,str(source),'-o',str(binary)]
   with(out/(mode+'.log')).open('w')as log:
    subprocess.run(command,env=environment(out/'target'),stdout=log,stderr=subprocess.STDOUT,check=True);subprocess.run([str(binary),'--test-threads=1'],stdout=log,stderr=subprocess.STDOUT,check=True)
   checks.append({'mode':mode,'command':command,'log':str(out/(mode+'.log'))})
 (out/'receipt.json').write_text(json.dumps({'status':'passed','scope':__doc__,'checks':checks,'sources':[info(SOURCE)]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
