# rpxe2

Linux Rust/Axum equivalent of `../pxe3.js` (app 1). App 2, the SSLTerminator
equivalent, is deferred until this PXE app has been checked.

`dnsmasq` still provides IPv4 ProxyDHCP and TFTP. Rust manages its configuration,
process lifetime, and boot-file snapshots; Axum streams the LAN update archive.
The original Node server and the existing build commands remain available.

## Build and run

Requires Rust 1.95.0, a host C linker, `ip` from iproute2, and `dnsmasq`.
Build **from this directory** so the local toolchain and Cargo configuration
select Linux instead of TRUEOS's custom kernel target:

```sh
cd tools/netboot/rpxe2
cargo build --release --locked
../../../tgt/rpxe2/x86_64-unknown-linux-gnu/release/rpxe2 --help
sudo ../../../tgt/rpxe2/x86_64-unknown-linux-gnu/release/rpxe2 enp5s0
```

The interface is optional and defaults to the first IPv4 default-route interface.
The optional second positional argument selects an artifact directory; otherwise
the app watches this checkout's `tgt/pxe-current`, independently of the working
directory used to launch the binary. The default is tied to the checkout where
the binary was compiled; pass an explicit directory when copying it elsewhere.
Prepare a complete build before starting it.

Run one PXE server at a time on a given LAN interface. Stop the existing Node
instance before manually switching to this binary. The build and tests do not
replace or stop that instance.

```sh
# Optional readiness notification, after HTTP binds and dnsmasq announces TFTP:
sudo ../../../tgt/rpxe2/x86_64-unknown-linux-gnu/release/rpxe2 \
  --ready-file /tmp/rpxe2.ready enp5s0 /absolute/path/to/artifacts

# Or stop using Ctrl-C / SIGTERM. This command targets the same binary path:
sudo ../../../tgt/rpxe2/x86_64-unknown-linux-gnu/release/rpxe2 --stop
```

`--stop` matches the executable's absolute path through `/proc`, including an
older executable replaced by rebuilding at that path. Other binary paths and
the Node server are outside its scope. Use the same permissions as the server.
The ready file contains the server PID and is removed during normal shutdown
or startup failure. Startup fails if dnsmasq does not announce readiness within
10 seconds. Shutdown forwards the signal, waits up to 5 seconds for dnsmasq,
and then kills/reaps it if necessary before removing the snapshots.
Linux also arms a parent-death SIGKILL before starting dnsmasq, so an abrupt
supervisor exit cannot leave TFTP serving a snapshot whose watcher has stopped.
This relies on the existing `--no-daemon` mode preserving the child process and
its credentials. SIGKILL cannot remove temporary snapshots or the ready file;
normal shutdown still performs that cleanup.

## Served files

Existing LAN DHCP supplies addresses. The app advertises
`EFI/BOOT/BOOTX64.EFI` for UEFI architecture IDs 7 and 9. Only these three files
enter the temporary TFTP root:

- `EFI/BOOT/BOOTX64.EFI`
- `TRUEOS.elf`
- `EFI/BOOT/tgl_guc_70.bin`

Every 100 ms, Rust checks the publication path and file metadata. A successful
copy and a second metadata check precede the atomic `active` symlink switch.
Failed or interrupted reloads keep the last complete snapshot. Old generations
remain until shutdown so existing transfers can finish. Directories and files
remain readable after dnsmasq drops privileges.

Axum listens on the selected IPv4 address, port 8080. Only the exact request
`GET /TrueOS.tar.lz4` succeeds. Each request resolves the current artifact
directory and streams its archive with `Content-Length` and
`Content-Type: application/octet-stream`. A missing archive returns 503;
other paths, query strings, and methods (including HEAD) return 404.
HTTP does not expose the boot files or other staging files.

The HTTP implementation uses Axum's documented
[streaming response body](https://docs.rs/axum/0.8.9/axum/body/struct.Body.html#method.from_stream)
and [server](https://docs.rs/axum/0.8.9/axum/fn.serve.html) APIs.

## Checks

From this directory:

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked
python3 tests/smoke.py ../../../tgt/rpxe2/x86_64-unknown-linux-gnu/debug/rpxe2
```

The Rust tests exercise snapshot publication and HTTP behavior. The Python smoke
checks use copied binaries, fake `ip`/`dnsmasq` commands, and loopback addresses
127.0.0.23/24 on TCP 8080. They check readiness, reloads, strict HTTP responses,
scoped stopping, signal cleanup, and startup failures. They require no root
privileges and open no DHCP/TFTP sockets.
The abrupt-exit check kills the supervisor during startup and after readiness
and verifies that its fake dnsmasq also exits without Rust cleanup running.

Checked on 2026-11-06: release build, formatting, Clippy with warnings denied,
all 7 Rust tests, all 7 smoke tests against the release binary, and the real
`dnsmasq --test` with the generated configuration passed.

These checks do not replace a subsequent UEFI boot check on the rig. No real
LAN service is switched as part of the automated checks.
