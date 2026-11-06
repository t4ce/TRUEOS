#[cfg(not(target_os = "linux"))]
compile_error!("rpxe2 requires Linux (iproute2, dnsmasq, and /proc)");

mod http;
mod network;
mod process;
mod snapshot;

use anyhow::{Context, Result, bail, ensure};
use nix::sys::signal::Signal;
use std::os::unix::fs::PermissionsExt;
use std::{
    ffi::OsString, fs, future::IntoFuture, io::Write, net::SocketAddrV4, path::PathBuf,
    process::Stdio, time::Duration,
};
use tokio::{
    io::{AsyncReadExt, BufReader},
    net::TcpListener,
    process::{Child, Command},
    signal::unix::{SignalKind, signal},
    time::sleep,
};

struct Options {
    interface: Option<String>,
    source: PathBuf,
    ready_file: Option<PathBuf>,
}

enum Action {
    Help,
    Stop,
    Serve(Options),
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Action> {
    let args: Vec<_> = args.into_iter().collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(Action::Help);
    }
    if args.len() == 1 && args[0] == "--stop" {
        return Ok(Action::Stop);
    }
    let mut args = args.into_iter();
    let mut positional = Vec::new();
    let mut ready_file = None;
    while let Some(arg) = args.next() {
        if arg == "--ready-file" {
            ensure!(ready_file.is_none(), "--ready-file specified twice");
            let value = args.next().context("--ready-file requires a path")?;
            ensure!(
                !value.is_empty() && !value.as_encoded_bytes().starts_with(b"--"),
                "--ready-file requires a path"
            );
            ready_file = Some(PathBuf::from(value));
        } else {
            ensure!(
                !arg.as_encoded_bytes().starts_with(b"-"),
                "Unknown option: {} (use --help)",
                arg.to_string_lossy()
            );
            positional.push(arg);
        }
    }
    ensure!(positional.len() <= 2, "Use --help for usage");
    let source = positional.get(1).map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../tgt/pxe-current")
    });
    // Keep the final source symlink unresolved: completed builds replace it.
    let source = std::path::absolute(source)?;
    let interface = positional
        .first()
        .map(|value| {
            value
                .clone()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Interface name must be UTF-8"))
        })
        .transpose()?;
    Ok(Action::Serve(Options {
        interface,
        source,
        ready_file,
    }))
}

fn help() {
    println!("Usage: rpxe2 [--ready-file PATH] [interface] [artifact-directory]");
    println!("       rpxe2 --stop");
    println!("Defaults: default-route IPv4 interface, this checkout's tgt/pxe-current.");
    println!("Requires Linux, iproute2, dnsmasq, and permission to open PXE/TFTP sockets.");
    println!("IPv4 ProxyDHCP/TFTP for x86-64 UEFI; Axum serves GET /TrueOS.tar.lz4 on :8080.");
    println!("Reloads completed builds every 100 ms; keep running alongside Cargo.");
    println!("--ready-file writes the PID once HTTP and TFTP are ready; removed on shutdown.");
    println!(
        "--stop stops only servers using this executable path, including an older replaced binary."
    );
}

struct ReadyFile(Option<PathBuf>);

impl ReadyFile {
    fn new(path: Option<PathBuf>) -> Result<Self> {
        if let Some(path) = &path {
            match fs::remove_file(path) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error).context("Cannot clear stale ready file"),
            }
        }
        Ok(Self(path))
    }

    fn publish(&self) -> Result<()> {
        if let Some(path) = &self.0 {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."));
            let mut pending = tempfile::NamedTempFile::new_in(parent)?;
            writeln!(pending, "{}", std::process::id())?;
            // A non-root launcher must be able to read a server started by sudo.
            pending
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o644))?;
            pending
                .persist(path)
                .with_context(|| format!("Cannot write readiness: {}", path.display()))?;
        }
        Ok(())
    }
}

impl Drop for ReadyFile {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

async fn serve(options: Options) -> Result<()> {
    // Register before filesystem work so startup signals also reach cleanup.
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    let ready = ReadyFile::new(options.ready_file)?;
    let (interface, ip) = network::interface_and_address(options.interface)?;
    let snapshot = snapshot::Snapshot::new(options.source.clone())?;
    let args = network::dnsmasq_args(&interface, ip, snapshot.root());
    network::validate_dnsmasq(&args)?;
    // Bind first so a port conflict cannot publish readiness or orphan dnsmasq.
    let listener = TcpListener::bind(SocketAddrV4::new(ip, 8080))
        .await
        .with_context(|| format!("Cannot bind LAN live update on {ip}:8080"))?;
    let marker = format!("TFTP root is {}", snapshot.root().display());
    let mut command = Command::new("dnsmasq");
    process::kill_with_parent(&mut command);
    let mut child = command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("Cannot start dnsmasq")?;
    let watcher = snapshot.watch();
    println!(
        "TRUEOS IPv4 PXE on {interface} ({ip}); artifacts: {}",
        options.source.display()
    );
    println!("The Rust EFI loader fetches the kernel and firmware over PXE/TFTP.");
    println!("LAN live update: http://{ip}:8080/TrueOS.tar.lz4");
    let outcome = supervise(
        &mut child,
        listener,
        options.source,
        marker,
        &ready,
        &mut interrupt,
        &mut terminate,
    )
    .await;
    let stopped = process::stop_child(&mut child).await;
    drop(watcher);
    outcome.and(stopped)
}

async fn supervise(
    child: &mut Child,
    listener: TcpListener,
    source: PathBuf,
    marker: String,
    ready: &ReadyFile,
    interrupt: &mut tokio::signal::unix::Signal,
    terminate: &mut tokio::signal::unix::Signal,
) -> Result<()> {
    let mut stderr = BufReader::new(child.stderr.take().context("dnsmasq stderr unavailable")?);
    let mut buffer = [0; 4096];
    let mut recent = Vec::new();
    let mut stderr_open = true;
    let mut announced = false;
    let server = axum::serve(listener, http::router(source)).into_future();
    let startup_deadline = sleep(Duration::from_secs(10));
    tokio::pin!(server, startup_deadline);
    loop {
        tokio::select! {
            biased;
            _ = interrupt.recv() => {
                process::signal_child(child, Signal::SIGINT)?;
                return Ok(());
            }
            _ = terminate.recv() => return Ok(()),
            status = child.wait() => {
                let status = status.context("Cannot wait for dnsmasq")?;
                bail!("dnsmasq exited unexpectedly: {status}");
            }
            result = &mut server => {
                result.context("LAN live update server failed")?;
                bail!("LAN live update server stopped unexpectedly");
            }
            _ = &mut startup_deadline, if !announced => bail!("dnsmasq did not announce TFTP readiness within 10 seconds"),
            read = stderr.read(&mut buffer), if stderr_open => {
                let count = read.context("Cannot read dnsmasq stderr")?;
                if count == 0 {
                    stderr_open = false;
                    continue;
                }
                std::io::stderr().write_all(&buffer[..count])?;
                recent.extend_from_slice(&buffer[..count]);
                if recent.len() > 16384 {
                    recent.drain(..recent.len() - 16384);
                }
                if !announced && recent.windows(marker.len()).any(|bytes| bytes == marker.as_bytes()) {
                    // HTTP is already bound, and dnsmasq has bound its sockets.
                    ready.publish()?;
                    announced = true;
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let result = match parse(std::env::args_os().skip(1)) {
        Ok(Action::Help) => {
            help();
            Ok(())
        }
        Ok(Action::Stop) => process::stop_existing().await,
        Ok(Action::Serve(options)) => serve(options).await,
        Err(error) => Err(error),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rpxe2: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_incomplete_or_ambiguous_cli() {
        for args in [
            vec!["--ready-file"],
            vec!["--ready-file", "--stop"],
            vec!["lo", "source", "extra"],
            vec!["--stop", "lo"],
            vec!["--unknown"],
        ] {
            assert!(parse(args.into_iter().map(OsString::from)).is_err());
        }
    }
}
