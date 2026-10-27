#!/bin/sh
# Cargo appends the kernel ELF and any program arguments. Deliberately do not
# execute them: booting TRUEOS requires the boot image and an explicit launcher.
cat >&2 <<'EOF'
TRUEOS is a bare-metal kernel. cargo run cannot execute it on the host.

Build the kernel:
  cargo build --release

Kernel ELF:
  tgt/x86_64-unknown-trueos/release/TRUEOS

Boot the built ISO in QEMU:
  tools/qemu/run.sh iso -snapshot

See tools/qemu/README.md for firmware setup and runtime verification.
EOF
exit 1
