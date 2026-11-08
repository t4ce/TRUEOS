#!/usr/bin/env python3
"""Drive the actual native-selected color detector with host PTYs and pipes.

The provider delegates terminal identity to std; real host std supplies this
fixture boundary. Native std uses the attached TRUEOS console capability.
"""
from pathlib import Path
import argparse,errno,json,os,pty,subprocess,tempfile
from test_native_mio import ROOT,TOOLCHAIN,environment,info,manifest
from test_native_rama_tcp import STAGE_DRIVER
SDK=ROOT.parent/'TRUEOS-Blueprints'
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);out=p.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);checks=[]
 with tempfile.TemporaryDirectory(prefix='trueos-supports-color-')as tmp:
  work=Path(tmp)
  def run(label,cmd,env=None):
   log=out/(label+'.log')
   with log.open('w')as stream:r=subprocess.run(cmd,cwd=work,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=180)
   checks.append({'label':label,'exit':r.returncode,'log':str(log),'command':cmd})
   if r.returncode:raise RuntimeError(log.read_text()[-10000:])
   return log.read_text()
  driver=STAGE_DRIVER.replace('@MODULE@',json.dumps(str(SDK/'src/codex_native_data_overlay.rs'))).replace('overlay::stage(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]))','overlay::stage_supports_color(&PathBuf::from(&args[1]),&PathBuf::from(&args[2]))');(work/'stage.rs').write_text(driver)
  run('compile',['rustup','run',TOOLCHAIN,'rustc','--edition=2024',str(work/'stage.rs'),'-o',str(work/'stage')]);source=list((Path.home()/'.cargo/registry/src').glob('*/supports-color-2.1.0'));assert len(source)==1
  staged=Path(run('stage',[str(work/'stage'),str(work/'staged'),str(source[0])]).strip());manifest(work,f'''[package]
name="native-color-proof"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
supports-color={{path={json.dumps(str(staged))}}}
[[bin]]
name="native-color-proof"
path="main.rs"
''');(work/'main.rs').write_text('fn level(c:Option<supports_color::ColorLevel>)->u8{c.map_or(0,|c|if c.has_16m{3}else if c.has_256{2}else{1})}\nfn main(){println!("{} {}",level(supports_color::on(supports_color::Stream::Stdout)),level(supports_color::on(supports_color::Stream::Stderr)));}\n')
  wrapper=work/'native-rustc.py';wrapper.write_text('''#!/usr/bin/env python3
import os,sys
args=sys.argv[1:]
if "--crate-name"in args and args[args.index("--crate-name")+1]=="supports_color":args += ["--cfg",'target_os="trueos"',"-Aexplicit_builtin_cfgs_in_flags"]
os.execvp(args[0],args)
''');wrapper.chmod(0o755)
  for mode,flags in [('debug',[]),('release',['--release'])]:
   env=environment(out/'target');env['RUSTC_WRAPPER']=str(wrapper);run('build-'+mode,['rustup','run',TOOLCHAIN,'cargo','build','--offline',*flags,'--target','x86_64-unknown-linux-gnu'],env);binary=out/'target/x86_64-unknown-linux-gnu'/mode/'native-color-proof'
   base={k:v for k,v in os.environ.items()if k not in ['FORCE_COLOR','NO_COLOR','CLICOLOR','CLICOLOR_FORCE','COLORTERM','TERM_PROGRAM','IGNORE_IS_TERMINAL','CI']};base['TERM']='xterm-256color'
   for label,terminal,extra,wanted in [('pipe',False,{},'0 0'),('terminal',True,{},'2 2'),('no-color',True,{'NO_COLOR':'1'},'0 0'),('force-color',False,{'FORCE_COLOR':'3'},'3 3')]:
    case_env=base|extra
    if terminal:
     master,slave=pty.openpty();proc=subprocess.Popen([str(binary)],stdin=subprocess.DEVNULL,stdout=slave,stderr=slave,env=case_env);os.close(slave);data=b''
     try:
      while True:
       try:part=os.read(master,4096)
       except OSError as e:
        if e.errno==errno.EIO:break
        raise
       if not part:break
       data+=part
     finally:os.close(master)
     assert proc.wait(timeout=10)==0
    else:data=subprocess.check_output([str(binary)],stderr=subprocess.STDOUT,env=case_env,timeout=10)
    assert data.decode().strip()==wanted,(mode,label,data,wanted);checks.append({'label':mode+'-'+label,'observed':data.decode().strip(),'expected':wanted})
 (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'checks':checks,'sources':[info(SDK/'src/codex_native_data_overlay.rs')]},indent=2)+'\n');print(out/'receipt.json')
if __name__=='__main__':main()
