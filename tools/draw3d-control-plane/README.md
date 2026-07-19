# TRUEOS Draw3D Control Plane

A native egui desktop controller for the TRUEOS Draw3D protocol. It connects to the fixed
development service at `192.168.178.94:4246` and uses the repository's `trueos-draw3d` crate for
wire-compatible command encoding and response decoding.

## Run

From this directory:

```sh
cargo run --release
```

The local Cargo configuration selects the Linux host target, overriding the repository's
bare-metal TRUEOS default target.

## Features

- Discovers every `tools/draw3d_*.py` scene and launches it with the fixed host.
- Streams unbuffered Python stdout/stderr into the in-app event console and supports cancellation.
- Connect/reconnect, ping, and automatically refreshed scene telemetry.
- Transparent or solid scene start, pause, clear, and permanent stop/discard controls.
- Static look-at camera and elliptical orbit controls using the Draw3D protocol extension.
- Keyboard flycam: WASD, Q/E, arrow keys, and a Shift speed boost.
- Optional script arguments such as `--orbit-speed 0.2` or `--capture`.

The Python scripts open their own short-lived TCP connection while the control plane keeps its
management connection open. The service supports independent framed clients, and telemetry is
refreshed after each script exits.

