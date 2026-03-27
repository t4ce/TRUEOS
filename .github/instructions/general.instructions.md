---
description: validation runs
applyTo: '**'
---

# builds
cargo fmt
printf 'acpi reboot\n' | nc 192.168.178.94 4245
make iso
# assume: after 1 min new iso loaded (via pxe)

# shell
until nc 192.168.178.94 4245; do sleep 1; done
# raw logs
nc 192.168.178.94 1;