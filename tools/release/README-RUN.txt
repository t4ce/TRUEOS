TRUEOS QEMU bundle
==================

This archive includes:

- trueos.iso
- ovmf-code-x86_64.fd
- run-linux.sh
- run-macos.sh
- OVMF-LICENSE.txt, when available from the packaging host

OVMF is QEMU's UEFI firmware. It runs before the ISO and loads
EFI/BOOT/BOOTX64.EFI from the TRUEOS ISO, just like motherboard firmware would
on real hardware.

Linux:

    ./run-linux.sh

macOS:

    ./run-macos.sh

If you want to use a different ISO or firmware file:

    TRUEOS_ISO=/path/to/trueos.iso TRUEOS_OVMF=/path/to/ovmf.fd ./run-linux.sh
