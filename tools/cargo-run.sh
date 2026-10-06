#!/bin/sh
# Cargo appends the kernel ELF and any program arguments. Deliberately do not
# execute them: booting TRUEOS requires the boot image and an explicit launcher.
cat >&2 <<'EOF'
TRUEOS is a bare-metal kernel. cargo run cannot execute it on the host.

Build the kernel:
  make kernel

Build a bootable ISO without publishing or starting the physical-rig log drain:
  make iso START_BAREMETAL_LOG=0 PUBLISH_RELEASE_SMB=0 RELEASE_BUMP_CNT=0

Boot the built ISO in QEMU:
  tools/qemu/run.sh iso -snapshot

See tools/qemu/README.md for firmware setup and runtime verification.
EOF
exit 1
