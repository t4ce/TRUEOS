#!/usr/bin/env python3
"""Diskless development SSH boot policy, using production code and OpenSSH.

The harness provides no filesystem, readiness, or configured account. Two fresh
server processes must admit Shell3 with the same embedded identity while the
client keeps strict host verification enabled after its first connection.
"""
from pathlib import Path
import fcntl
import os
import pty
import struct
import subprocess
import tempfile
import termios


ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_time;
use std::io::{Read, Write};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant(std::time::Instant);
impl Instant {
    pub fn now() -> Self { Self(std::time::Instant::now()) }
}
pub struct Duration(std::time::Duration);
impl Duration {
    pub fn from_millis(ms: u64) -> Self { Self(std::time::Duration::from_millis(ms)) }
}
impl std::ops::Add<Duration> for Instant {
    type Output = Self;
    fn add(self, duration: Duration) -> Self { Self(self.0 + duration.0) }
}

mod crypt {
    #[derive(Debug)]
    pub enum CryError { NotConfigured }
    pub fn ssh_key_allowed(_: &str, _: &[u8; 32]) -> bool { false }
}
mod net { pub mod adapter {
    pub struct NetQueue<T>(spin::Mutex<std::collections::VecDeque<T>>);
    impl<T> NetQueue<T> {
        pub fn new_leaked(_: &str, _: usize) -> &'static Self {
            Box::leak(Box::new(Self(spin::Mutex::new(Default::default()))))
        }
        pub fn push(&self, item: T) -> Result<(), ()> {
            self.0.lock().push_back(item); Ok(())
        }
        pub fn drain(&self, count: usize) -> Vec<T> {
            let mut queue = self.0.lock();
            (0..count).filter_map(|_| queue.pop_front()).collect()
        }
    }
}}
mod shell3 {
    pub mod tty {
        pub struct Terminal { pub output: Vec<u8>, pub closing: bool }
        impl Terminal {
            pub fn new(cols: usize, rows: usize) -> Self {
                Self {
                    output: format!("AUTHENTICATED-SHELL3\r\nSIZE:{cols}x{rows}\r\n").into_bytes(),
                    closing: false,
                }
            }
            pub fn input(&mut self, bytes: &[u8]) {
                if bytes.iter().any(|byte| matches!(*byte, b'\n' | b'\r' | 4)) {
                    self.closing = true;
                }
            }
            pub fn resize(&mut self, cols: usize, rows: usize) {
                self.output.extend_from_slice(format!("RESIZE:{cols}x{rows}\r\n").as_bytes());
            }
        }
    }
    #[path = "__POLICY_PATH__"] pub mod ssh_boot;
    #[path = "__SSH_PATH__"] pub mod ssh;
    pub fn check_boot_policy() {
        assert!(ssh_boot::allow_none());
        let seed = ssh_boot::host_seed().unwrap();
        assert_eq!(*seed, *ssh_boot::host_seed().unwrap());
    }
}

fn ready<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    match future.as_mut().poll(&mut context) {
        std::task::Poll::Ready(result) => result,
        _ => panic!("unexpected disk or readiness wait"),
    }
}

fn main() {
    // No account or storage initialization precedes policy/session creation.
    shell3::check_boot_policy();
    shell3::ssh::init();
    let mut ssh = shell3::ssh::Session::new().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    println!("{}", listener.local_addr().unwrap().port());
    std::io::stdout().flush().unwrap();
    let (mut socket, _) = listener.accept().unwrap();
    socket.set_nonblocking(true).unwrap();
    let mut terminal = None;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
    while std::time::Instant::now() < deadline {
        let mut bytes = [0; 32768];
        match socket.read(&mut bytes) {
            Ok(0) => break,
            Ok(count) => ssh.input(&bytes[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {},
            Err(error) => panic!("{error}"),
        }
        ssh.pump(terminal.as_mut());
        ready(shell3::ssh::service_auth());
        if terminal.is_none() && ssh.wants_shell() {
            let (cols, rows) = ssh.size().expect("PTY required before shell");
            terminal = Some(shell3::tty::Terminal::new(cols, rows));
        }
        let count = match socket.write(ssh.output()) {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => 0,
            Err(_) => break,
        };
        ssh.consume_output(count);
        if ssh.closed || (ssh.finished && ssh.output().is_empty()) { break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    println!("shell-created={}", terminal.is_some());
}
'''


def main():
    with tempfile.TemporaryDirectory(prefix='trueos-ssh-boot-test-') as directory:
        work = Path(directory)
        (work / 'src').mkdir()
        build_source = (ROOT / 'build.rs').read_text()
        start = build_source.index('fn generate_development_ssh_identity(')
        end = build_source.index('\nfn generate_ring_runtime_imports(', start)
        # Use the production build-time provider in this temporary workspace.
        (work / 'build.rs').write_text('''
use std::{env, fs, path::{Path, PathBuf}};
fn main() {
    use std::os::unix::fs::PermissionsExt;
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let source = manifest.join(".local/ssh-host-seed.bin");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("ssh-dev-host-seed.bin");
    generate_development_ssh_identity(&manifest).unwrap();
    let original = fs::read(&source).unwrap();
    assert_eq!(original.len(), 32);
    assert_eq!(fs::metadata(&source).unwrap().permissions().mode() & 0o777, 0o600);
    // Rebuilding must reuse the existing identity, including the embedded copy.
    generate_development_ssh_identity(&manifest).unwrap();
    assert_eq!(fs::read(&source).unwrap(), original);
    assert_eq!(fs::read(&output).unwrap(), original);
    assert_eq!(fs::metadata(&output).unwrap().permissions().mode() & 0o777, 0o600);
    // A damaged existing key must cause an error, never silently rotate identity.
    fs::write(&source, b"damaged").unwrap();
    assert!(generate_development_ssh_identity(&manifest).is_err());
    assert_eq!(fs::read(&source).unwrap(), b"damaged");
    fs::write(source, original).unwrap();
}
''' + build_source[start:end])
        (work / 'Cargo.toml').write_text(f'''[package]
name="trueos-ssh-boot-host-test"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
sunset={{path="{ROOT}/vendor/sunset",default-features=false,features=["alloc"]}}
ed25519-dalek={{version="2.2",default-features=false,features=["zeroize"]}}
spin="0.9"
zeroize={{version="1",features=["alloc"]}}
''')
        ssh_source = (ROOT / 'src/shell3/ssh.rs').read_text().replace('pub(super)', 'pub(crate)')
        # The host pumps authentication directly instead of spawning a kernel task.
        start = ssh_source.index('#[trueos_executor::task]')
        end = ssh_source.index('pub(crate) struct Session', start)
        (work / 'src/ssh.rs').write_text(ssh_source[:start] + ssh_source[end:])
        source = HARNESS.replace('__POLICY_PATH__', str(ROOT / 'src/shell3/ssh_boot.rs'))
        source = source.replace('__SSH_PATH__', str(work / 'src/ssh.rs'))
        (work / 'src/main.rs').write_text(source)
        toolchain = subprocess.check_output(
            ['rustup', 'show', 'active-toolchain'], cwd=ROOT, text=True).split()[0]
        env = dict(os.environ, RUSTUP_TOOLCHAIN=toolchain,
                   CARGO_TARGET_DIR=str(ROOT / 'tgt/ssh-host-tests'))
        subprocess.run(['cargo', 'build', '--offline', '--quiet', '--manifest-path',
                        str(work / 'Cargo.toml')], cwd=work, env=env, check=True)
        print('build-identity-reuse-and-permissions: passed')
        binary = ROOT / 'tgt/ssh-host-tests/debug/trueos-ssh-boot-host-test'
        known_hosts = work / 'known_hosts'

        def connect(mode, strict, expected):
            master, slave = pty.openpty()
            rows, cols = (0, 0) if mode == 'zero-pty' else (43, 132)
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', rows, cols, 0, 0))
            os.write(master, b'\n')
            server = subprocess.Popen([str(binary)], stdout=subprocess.PIPE,
                                      stderr=subprocess.PIPE, text=True)
            try:
                port = server.stdout.readline().strip()
                assert port.isdigit(), server.communicate(timeout=5)
                command = [
                    'ssh', '-F', '/dev/null', '-T' if mode == 'missing-pty' else '-tt',
                    '-p', port, '-o', f'StrictHostKeyChecking={strict}',
                    '-o', f'UserKnownHostsFile={known_hosts}',
                    '-o', 'GlobalKnownHostsFile=/dev/null',
                    '-o', 'HostKeyAlias=trueos-development-fixture',
                    '-o', 'UpdateHostKeys=no', '-o', 'ConnectTimeout=5',
                    '-o', 'BatchMode=yes', '-o', 'IdentitiesOnly=yes',
                    '-o', 'IdentityAgent=none', '-i', '/dev/null',
                    't4ce@127.0.0.1',
                ]
                client = subprocess.run(command, stdin=slave, capture_output=True, timeout=15)
                output, errors = server.communicate(timeout=15)
                assert server.returncode == 0, errors
                assert (b'AUTHENTICATED-SHELL3' in client.stdout) == expected, (
                    mode, client.stdout, client.stderr, errors)
                assert f'shell-created={str(expected).lower()}' in output, (mode, output)
                assert b'REMOTE HOST IDENTIFICATION HAS CHANGED' not in client.stderr, client.stderr
                if expected:
                    assert b'SIZE:132x43' in client.stdout, client.stdout
                    assert client.returncode == 0, client.stderr
                else:
                    assert client.returncode != 0, client.stderr
                print(f'{mode}: passed')
            finally:
                os.close(master)
                os.close(slave)
                if server.poll() is None:
                    server.kill()
                    server.communicate()

        connect('diskless-first-boot', 'accept-new', True)
        saved_identity = known_hosts.read_bytes()
        assert saved_identity, 'first connection did not store the host identity'
        connect('diskless-reboot-strict-host-key', 'yes', True)
        assert known_hosts.read_bytes() == saved_identity, 'reboot changed the saved identity'
        connect('missing-pty', 'yes', False)
        connect('zero-pty', 'yes', False)


if __name__ == '__main__':
    main()
