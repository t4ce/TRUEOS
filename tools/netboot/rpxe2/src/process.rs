use anyhow::{Context, Result, bail};
use nix::{
    errno::Errno,
    sys::{
        prctl::set_pdeathsig,
        signal::{Signal, kill},
    },
    unistd::{Pid, getpid, getppid},
};
use std::{fs, path::Path, time::Duration};
use tokio::{
    process::{Child, Command},
    time::{Instant, sleep, timeout},
};

/// A stale dnsmasq must not outlive the watcher that publishes its boot files.
/// The existing --no-daemon mode neither forks nor changes credentials, so the
/// Linux parent-death setting survives exec and remains armed in dnsmasq.
pub fn kill_with_parent(command: &mut Command) {
    let parent = getpid();
    // SAFETY: this runs between fork and exec, so use only syscall wrappers
    // and allocation-free error construction. SIGKILL also covers a daemon
    // that ignores graceful shutdown after its supervisor disappears.
    unsafe {
        command.pre_exec(move || {
            set_pdeathsig(Signal::SIGKILL).map_err(std::io::Error::from)?;
            // The parent may have died before prctl armed the signal. In that
            // case do not exec an orphan that missed the death notification.
            if getppid() != parent {
                return Err(std::io::Error::from_raw_os_error(nix::libc::ECHILD));
            }
            Ok(())
        });
    }
}

pub async fn stop_existing() -> Result<()> {
    let executable = fs::canonicalize(std::env::current_exe()?)?;
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<i32>().ok())
        else {
            continue;
        };
        if pid == std::process::id() as i32 || !same_server(&entry.path(), &executable) {
            continue;
        }
        println!("Stopping this repository's Rust PXE server (PID {pid})");
        match kill(Pid::from_raw(pid), Signal::SIGTERM) {
            Ok(()) => (),
            Err(Errno::ESRCH) => continue,
            Err(error) => return Err(error.into()),
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while same_server(&entry.path(), &executable) && !is_zombie(&entry.path()) {
            if Instant::now() >= deadline {
                bail!("PXE process {pid} did not stop");
            }
            sleep(Duration::from_millis(100)).await;
        }
    }
    Ok(())
}

fn same_server(proc: &Path, executable: &Path) -> bool {
    let Ok(target) = fs::read_link(proc.join("exe")) else {
        return false;
    };
    // A rebuilt executable can leave its old running inode marked '(deleted)'.
    let mut deleted = executable.as_os_str().to_os_string();
    deleted.push(" (deleted)");
    if target != executable && target != deleted {
        return false;
    }
    let Ok(argv) = fs::read(proc.join("cmdline")) else {
        return false;
    };
    !argv.split(|byte| *byte == 0).any(|arg| arg == b"--stop")
}

fn is_zombie(proc: &Path) -> bool {
    fs::read_to_string(proc.join("stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(") ")
                .map(|(_, fields)| fields.starts_with('Z'))
        })
        .unwrap_or(false)
}

pub fn signal_child(child: &Child, signal: Signal) -> Result<()> {
    if let Some(pid) = child.id() {
        match kill(Pid::from_raw(pid as i32), signal) {
            Ok(()) | Err(Errno::ESRCH) => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub async fn stop_child(child: &mut Child) -> Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    signal_child(child, Signal::SIGTERM)?;
    match timeout(Duration::from_secs(5), child.wait()).await {
        Ok(status) => {
            status.context("Cannot reap dnsmasq")?;
        }
        Err(_) => {
            child
                .start_kill()
                .context("Cannot kill dnsmasq after shutdown timeout")?;
            child.wait().await?;
        }
    }
    Ok(())
}
