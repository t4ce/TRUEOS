#!/usr/bin/env python3
"""OpenSSH interoperability against the production kernel SSH/cry code.

The host harness substitutes the clock, disk IO, and Shell3 model. SSH transport,
encrypted credential envelopes, TOTP, and the authentication bridge are real.
"""
from pathlib import Path
import os
import subprocess
import tempfile
import pty
import fcntl
import termios
import struct
import signal
import select

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_time;
extern crate self as embassy_time_driver;
use std::{io::{Read,Write}, sync::atomic::{AtomicU64,AtomicBool,Ordering}};
pub const TICK_HZ:u64=1000;
static UNIX:AtomicU64=AtomicU64::new(1800000000);
static FAIL_DISK:AtomicBool=AtomicBool::new(false);
pub fn now()->u64 { 1000 }
#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord)] pub struct Instant(std::time::Instant);
impl Instant {pub fn now()->Self {Self(std::time::Instant::now())}}
pub struct Duration(std::time::Duration);
impl Duration {pub fn from_millis(ms:u64)->Self {Self(std::time::Duration::from_millis(ms))}}
impl std::ops::Add<Duration> for Instant {type Output=Self;fn add(self,d:Duration)->Self {Self(self.0+d.0)}}
mod tyche {pub fn fill_bytes(out:&mut[u8])->bool {getrandom_02::getrandom(out).is_ok()}}
mod time {pub fn unix_time_seconds()->Option<u64> {Some(crate::UNIX.load(crate::Ordering::Relaxed))}}
mod r {pub mod net {pub mod ntp {pub fn current_unix_seconds()->Option<u64> {crate::time::unix_time_seconds()}}}}
mod net {pub mod adapter {
    pub struct NetQueue<T>(spin::Mutex<std::collections::VecDeque<T>>);
    impl<T> NetQueue<T> {
        pub fn new_leaked(_name:&str,_capacity:usize)->&'static Self {Box::leak(Box::new(Self(spin::Mutex::new(Default::default()))))}
        pub fn push(&self,item:T)->Result<(),()> {self.0.lock().push_back(item);Ok(())}
        pub fn drain(&self,count:usize)->Vec<T> {let mut q=self.0.lock();(0..count).filter_map(|_|q.pop_front()).collect()}
    }
}}
mod crypt {
__CRYPT__
    pub fn test_secret()->[u8;20] {CRY_STATE.lock().totp.as_ref().unwrap().secret}
    pub fn test_reset() {*CRY_STATE.lock()=CryState::new();}
    pub fn test_plan_key(plan:&CryPersistencePlan)->[u8;32] {plan.recovery_key.as_slice().try_into().unwrap()}
}
mod shell2 {pub mod cmds {pub mod cry {
    pub async fn write_persistence(plan:&crate::crypt::CryPersistencePlan)->Result<(),String> {
        if crate::FAIL_DISK.load(crate::Ordering::Relaxed) {return Err("disk write failed".into());}
        let key=crate::crypt::test_plan_key(plan);
        let opened=trueos_credential_store::open(&plan.username,&key,&plan.envelope).unwrap();
        assert_eq!(opened.credential.last_accepted_step,Some(crate::UNIX.load(crate::Ordering::Relaxed)/30));
        Ok(())
    }
}}}
mod shell3 {
    pub mod tty {
        pub struct Terminal {pub output:Vec<u8>,pub closing:bool,pub overflow:bool}
        impl Terminal {
            pub fn new()->Self {Self {output:b"AUTHENTICATED-SHELL3\r\n".to_vec(),closing:false,overflow:false}}
            pub fn input(&mut self,bytes:&[u8]) {if bytes.iter().any(|b|*b==b'\n'||*b==b'\r'||*b==4) {self.closing=true;}}
            pub fn resize(&mut self,cols:usize,rows:usize) {self.output.extend_from_slice(format!("RESIZE:{cols}x{rows}\r\n").as_bytes());}
        }
    }
    #[path="__SSH_PATH__"] pub mod ssh;
}
fn ready<F:std::future::Future>(f:F)->F::Output {
    let mut f=std::pin::pin!(f);let mut cx=std::task::Context::from_waker(std::task::Waker::noop());
    match f.as_mut().poll(&mut cx) {std::task::Poll::Ready(r)=>r,_=>panic!("unexpected host IO wait")}
}
fn code(secret:&[u8;20])->String {
    format!("{:06}",trueos_crypto::generate_totp_sha1(secret,UNIX.load(Ordering::Relaxed)/30).unwrap())
}
fn bad_code(secret:&[u8;20])->String {
    let unix=UNIX.load(Ordering::Relaxed);
    let value=(0..1000000).find(|code|trueos_crypto::verify_totp_sha1(secret,unix,*code,1).unwrap().is_none()).unwrap();
    format!("{value:06}")
}
fn fixture()->[u8;20] {
    crypt::test_reset();UNIX.store(1800000000,Ordering::Relaxed);
    assert!(crypt::ssh_host_seed().is_err());
    crypt::setup_root_key("alice").unwrap();crypt::begin_totp_enrollment().unwrap();
    let secret=crypt::test_secret();
    assert!(crypt::prepare_remote_login("alice",&code(&secret)).is_err());
    let report=crypt::prepare_login(&code(&secret),7).unwrap();
    let plan=crypt::prepare_persistence(report.challenge_sequence).unwrap();
    crypt::complete_persisted_login(plan).unwrap();
    UNIX.fetch_add(60,Ordering::Relaxed);
    if let Ok(public)=std::env::var("TRUEOS_TEST_PUBLIC_KEY") {
        let key=crypt::parse_ssh_public_key(&public).unwrap();
        let report=crypt::prepare_ssh_key_change(&code(&secret),key,false).unwrap();
        assert!(!crypt::ssh_key_allowed("alice",&key));
        let plan=crypt::prepare_persistence(report.challenge_sequence).unwrap();
        crypt::complete_persisted_remote_login(plan).unwrap();
        assert!(crypt::ssh_key_allowed("alice",&key));
        assert!(!crypt::ssh_key_allowed("mallory",&key));
        UNIX.fetch_add(60,Ordering::Relaxed);
    }
    secret
}
fn crypto_checks() {
    let secret=fixture();let local=crypt::status().session;let seed=*crypt::ssh_host_seed().unwrap();
    let authorized=crypt::ssh_authorized_keys();
    assert!(crypt::prepare_remote_login("wrong-user",&code(&secret)).is_err());
    let report=crypt::prepare_remote_login("alice",&code(&secret)).unwrap();
    assert_eq!(crypt::status().session,local);
    let plan=crypt::prepare_persistence(report.challenge_sequence).unwrap();
    let envelope=plan.envelope.to_vec();let key=crypt::test_plan_key(&plan);
    crypt::complete_persisted_remote_login(plan).unwrap();
    assert_eq!(crypt::status().session,local);
    assert_eq!(crypt::prepare_remote_login("alice",&code(&secret)),Err(crypt::CryError::TotpReplay));
    crypt::test_reset();crypt::unlock_persisted("alice",&key,&envelope).unwrap();
    assert_eq!(crypt::ssh_authorized_keys(),authorized);
    assert_eq!(*crypt::ssh_host_seed().unwrap(),seed);
    assert!(crypt::status().session.is_none());
    assert_eq!(crypt::prepare_remote_login("alice",&code(&secret)),Err(crypt::CryError::TotpReplay));
    UNIX.fetch_add(60,Ordering::Relaxed);
    let good=code(&secret);let bad=bad_code(&secret);
    for _ in 0..4 {assert_eq!(crypt::prepare_remote_login("alice",&bad),Err(crypt::CryError::InvalidTotpCode));}
    assert!(matches!(crypt::prepare_remote_login("alice",&bad),Err(crypt::CryError::TotpRateLimited{..})));
    assert!(matches!(crypt::prepare_remote_login("alice",&good),Err(crypt::CryError::TotpRateLimited{..})));
    // A failed durable key change must never authorize its replacement set.
    let secret=fixture();let key=crypt::ssh_authorized_keys()[0];
    let report=crypt::prepare_ssh_key_change(&code(&secret),key,true).unwrap();
    assert!(crypt::ssh_key_allowed("alice",&key));
    crypt::abort_pending_login(report.challenge_sequence);
    assert!(crypt::ssh_key_allowed("alice",&key));
    UNIX.fetch_add(60,Ordering::Relaxed);
    let report=crypt::prepare_ssh_key_change(&code(&secret),key,true).unwrap();
    let plan=crypt::prepare_persistence(report.challenge_sequence).unwrap();
    let envelope=plan.envelope.to_vec();let recovery=crypt::test_plan_key(&plan);
    crypt::complete_persisted_remote_login(plan).unwrap();
    assert!(!crypt::ssh_key_allowed("alice",&key));
    crypt::test_reset();crypt::unlock_persisted("alice",&recovery,&envelope).unwrap();
    assert!(!crypt::ssh_key_allowed("alice",&key));
    UNIX.fetch_add(60,Ordering::Relaxed);
    let report=crypt::prepare_ssh_key_change(&code(&secret),key,false).unwrap();
    let plan=crypt::prepare_persistence(report.challenge_sequence).unwrap();
    crypt::complete_persisted_remote_login(plan).unwrap();
    assert!(crypt::ssh_key_allowed("alice",&key));
}
// Advertise the trusted public key but sign with another private key. This
// checks that a public-key offer cannot authenticate without a valid signature.
fn bad_agent(path:&str) {
    use ed25519_dalek::Signer;
    fn string(out:&mut Vec<u8>,bytes:&[u8]) {out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());out.extend_from_slice(bytes);}
    fn take<'a>(input:&mut &'a [u8])->&'a [u8] {
        let count=u32::from_be_bytes(input[..4].try_into().unwrap()) as usize;
        let value=&input[4..4+count];*input=&input[4+count..];value
    }
    let public=std::env::var("TRUEOS_TEST_PUBLIC_KEY").unwrap();
    let key=crypt::parse_ssh_public_key(&public).unwrap();
    let mut wire=Vec::new();string(&mut wire,b"ssh-ed25519");string(&mut wire,&key);
    let signer=ed25519_dalek::SigningKey::from_bytes(&[7;32]);
    let listener=std::os::unix::net::UnixListener::bind(path).unwrap();
    println!("agent-ready");std::io::stdout().flush().unwrap();
    for socket in listener.incoming() {
        let mut socket=socket.unwrap();
        loop {
            let mut length=[0;4];if socket.read_exact(&mut length).is_err() {break;}
            let count=u32::from_be_bytes(length) as usize;assert!(count<=8192);
            let mut request=vec![0;count];socket.read_exact(&mut request).unwrap();
            let mut response=Vec::new();
            match request[0] {
                11=>{response.push(12);response.extend_from_slice(&1u32.to_be_bytes());string(&mut response,&wire);string(&mut response,b"forged-signature-test");}
                13=>{
                    let mut input=&request[1..];assert_eq!(take(&mut input),wire);
                    let data=take(&mut input);let signature=signer.sign(data).to_bytes();
                    let mut encoded=Vec::new();string(&mut encoded,b"ssh-ed25519");string(&mut encoded,&signature);
                    response.push(14);string(&mut response,&encoded);
                }
                _=>response.push(5),
            }
            socket.write_all(&(response.len() as u32).to_be_bytes()).unwrap();socket.write_all(&response).unwrap();
        }
    }
}
fn main() {
    if std::env::args().nth(1).as_deref()==Some("bad-agent") {
        bad_agent(&std::env::args().nth(2).unwrap());return;
    }
    crypto_checks();let secret=fixture();
    let mode=std::env::args().nth(1).unwrap();
    FAIL_DISK.store(mode=="disk-failure",Ordering::Relaxed);
    let answer=if mode=="bad-code" {bad_code(&secret)}else {code(&secret)};
    let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    println!("{} {}",listener.local_addr().unwrap().port(),answer);std::io::stdout().flush().unwrap();
    let(mut socket,_)=listener.accept().unwrap();socket.set_nonblocking(true).unwrap();
    shell3::ssh::init();let mut ssh=shell3::ssh::Session::new().unwrap();let mut terminal=None;
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(12);
    while std::time::Instant::now()<deadline {
        let mut bytes=[0;32768];match socket.read(&mut bytes) {
            Ok(0)=>break,Ok(n)=>ssh.input(&bytes[..n]),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>{},Err(e)=>panic!("{e}"),
        }
        ssh.pump(terminal.as_mut());ready(shell3::ssh::service_auth());
        if terminal.is_none()&&ssh.wants_shell() {
            let (cols,rows)=ssh.size().expect("confirmed PTY dimensions before shell");
            let mut tty=shell3::tty::Terminal::new();
            tty.output.extend_from_slice(format!("SIZE:{cols}x{rows}\r\n").as_bytes());
            terminal=Some(tty);
        }
        let bytes=ssh.output();let count=match socket.write(bytes) {
            Ok(n)=>n,Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>0,Err(_)=>break,
        };ssh.consume_output(count);
        if ssh.closed||(ssh.finished&&ssh.output().is_empty()) {break;}
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    println!("shell-created={}",terminal.is_some());
}
'''


def main():
    with tempfile.TemporaryDirectory(prefix='trueos-ssh-test-') as temp:
        work = Path(temp)
        (work/'src').mkdir()
        deps = f'''[package]
name="trueos-ssh-host-test"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
sunset={{path="{ROOT}/vendor/sunset",default-features=false,features=["alloc"]}}
trueos-crypto={{path="{ROOT}/crates/trueos-crypto"}}
trueos-credential-store={{path="{ROOT}/crates/trueos-credential-store"}}
ed25519-dalek={{version="2.2",default-features=false,features=["zeroize"]}}
sha2={{version="0.10",default-features=false}}
spin="0.9"
zeroize={{version="1",features=["alloc"]}}
base64={{version="0.22",default-features=false,features=["alloc"]}}
getrandom_02={{package="getrandom",version="0.2"}}
'''
        (work/'Cargo.toml').write_text(deps)
        source = HARNESS.replace('__CRYPT__', (ROOT/'src/crypt.rs').read_text().replace('//!','//'))
        # Widen only the test harness's visibility so its driver can pump sessions.
        ssh_source = (ROOT/'src/shell3/ssh.rs').read_text().replace('pub(super)', 'pub(crate)')
        # Executor wiring stays kernel-only; the host drives service_auth directly.
        start = ssh_source.index('#[trueos_executor::task]')
        end = ssh_source.index('pub(crate) struct Session', start)
        ssh_source = ssh_source[:start] + ssh_source[end:]
        (work/'src/ssh.rs').write_text(ssh_source)
        source = source.replace('__SSH_PATH__', str(work/'src/ssh.rs'))
        (work/'src/main.rs').write_text(source)
        toolchain = subprocess.check_output(['rustup','show','active-toolchain'], cwd=ROOT, text=True).split()[0]
        env = dict(os.environ, RUSTUP_TOOLCHAIN=toolchain, CARGO_TARGET_DIR=str(ROOT/'tgt/ssh-host-tests'))
        subprocess.run(['cargo','build','--quiet','--manifest-path',str(work/'Cargo.toml')], cwd=work, env=env, check=True)
        binary = ROOT/'tgt/ssh-host-tests/debug/trueos-ssh-host-test'
        askpass = work/'askpass'
        askpass.write_text('#!/bin/sh\nprintf "%s\\n" "$TRUEOS_TEST_CODE"\n')
        askpass.chmod(0o700)
        trusted = work/'trusted'
        other = work/'other'
        for identity in (trusted, other):
            subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(identity)],check=True)
        public = trusted.with_suffix('.pub').read_text()
        cases = [('success','alice','keyboard-interactive',True),
                 ('success-pty','alice','keyboard-interactive',True),
                 ('key-success','alice','publickey',True),
                 ('key-untrusted','alice','publickey',False),
                 ('key-forged-signature','alice','publickey',False),
                 ('key-wrong-user','mallory','publickey',False),
                 ('bad-code','alice','keyboard-interactive',False),
                 ('disk-failure','alice','keyboard-interactive',False),
                 ('wrong-user','mallory','keyboard-interactive',False),
                 ('password-disabled','alice','password',False)]
        cases += [('missing-pty','alice','publickey',False), ('zero-pty','alice','publickey',False)]
        cases += [('resize','alice','publickey',True)]
        for mode, user, method, expected in cases:
            agent = None
            master, slave = pty.openpty()
            cols, rows = (0, 0) if mode == 'zero-pty' else (132, 43)
            fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',rows,cols,0,0))
            if mode != 'resize': os.write(master,b'\n')
            server = subprocess.Popen([str(binary),mode], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                env=dict(os.environ,TRUEOS_TEST_PUBLIC_KEY=public))
            try:
                port, code = server.stdout.readline().split()
                ssh_env = dict(os.environ, SSH_ASKPASS=str(askpass), SSH_ASKPASS_REQUIRE='force', DISPLAY=':test', TRUEOS_TEST_CODE=code)
                identity = other if mode == 'key-untrusted' else trusted
                if mode == 'key-forged-signature':
                    socket = work/'bad-agent.sock'
                    agent = subprocess.Popen([str(binary),'bad-agent',str(socket)], stdout=subprocess.PIPE,
                        env=dict(os.environ,TRUEOS_TEST_PUBLIC_KEY=public),text=True)
                    assert agent.stdout.readline().strip() == 'agent-ready'
                    ssh_env['SSH_AUTH_SOCK'] = str(socket)
                    identity = trusted.with_suffix('.pub')
                command = ['ssh','-F','/dev/null','-T' if mode == 'missing-pty' else '-tt','-p',port,'-o','StrictHostKeyChecking=no',
                    '-o','UserKnownHostsFile=/dev/null','-o','ConnectTimeout=5',
                    '-o','IdentitiesOnly=yes','-i',str(identity),
                    '-o','NumberOfPasswordPrompts=1','-o',f'PreferredAuthentications={method}',
                    f'{user}@127.0.0.1']
                if mode == 'resize':
                    process = subprocess.Popen(command,stdin=slave,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=ssh_env,bufsize=0)
                    prefix = b''
                    def read_until(marker):
                        nonlocal prefix
                        while marker not in prefix:
                            assert select.select([process.stdout],[],[],5)[0], 'SSH resize test timed out'
                            byte = process.stdout.read(1)
                            assert byte, 'SSH ended before expected size report'
                            prefix += byte
                    try:
                        read_until(b'SIZE:132x43')
                        fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',51,142,0,0))
                        process.send_signal(signal.SIGWINCH)
                        read_until(b'RESIZE:142x51')
                        os.write(master,b'\n')
                        stdout, stderr = process.communicate(timeout=15)
                        client = subprocess.CompletedProcess(command,process.returncode,prefix+stdout,stderr)
                    finally:
                        if process.poll() is None:
                            process.kill()
                            process.communicate()
                else:
                    client = subprocess.run(command,stdin=slave,capture_output=True,env=ssh_env,timeout=15)
                output, errors = server.communicate(timeout=15)
                assert server.returncode == 0, errors
                assert (b'AUTHENTICATED-SHELL3' in client.stdout) == expected, (mode,client.stdout,client.stderr,errors)
                assert f'shell-created={str(expected).lower()}' in output, (mode,output,client.stderr)
                if expected:
                    assert b'SIZE:132x43' in client.stdout, client.stdout
                    assert client.returncode == 0, client.stderr
                else:
                    assert client.returncode != 0, client.stderr
                print(f'{mode}: passed')
            finally:
                os.close(master)
                os.close(slave)
                if agent is not None:
                    agent.terminate()
                    agent.communicate(timeout=5)
                if server.poll() is None:
                    server.kill()
                    server.communicate()


if __name__ == '__main__':
    main()
