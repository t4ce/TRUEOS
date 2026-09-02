# TRUEOS FirmwareScout

`FirmwareScout.efi` is a capture-only UEFI application that runs before
`ExitBootServices()`, exports the firmware's Human Interface Infrastructure
(HII) package lists and current HII configuration when those protocols are
available, publishes the result as a bounded `TRBIOS1` configuration-table
handoff, and then chainloads Limine from `\\EFI\\BOOT\\LIMINE.EFI`.

The first cycle is deliberately observational:

- calls `EFI_HII_DATABASE_PROTOCOL.ExportPackageLists()`;
- calls `EFI_HII_CONFIG_ROUTING_PROTOCOL.ExportConfig()`;
- never calls `RouteConfig()`;
- never calls `SetVariable()`;
- never invokes the Form Browser;
- never writes a setup question, storage controller, USB policy, capsule, or
  firmware flash region.

The exported configuration can contain sensitive platform state. TRUEOS keeps
it in reserved memory, validates CRCs, and reports only metadata in the first
kernel-side decoder. It is not dumped to the ordinary shell or log.

## Build

From the repository root:

```sh
tools/firmware-scout/build.sh
```

The artifact is written to:

```text
bld/firmware-scout-target/x86_64-unknown-uefi/release/trueos-firmware-scout.efi
```

The crate is a standalone workspace and is pinned to `r-efi 5.3.0`. It targets
`x86_64-unknown-uefi` with the UEFI `efiapi` calling convention.

## Stage a directory-backed EFI boot tree

This is useful for an installed ESP, a mounted USB ESP, or TRUEOS's TFTP boot
tree:

```sh
tools/firmware-scout/stage-tree.sh bld/EFI/BOOT
```

The script performs this reversible layout change:

```text
EFI/BOOT/BOOTX64.EFI  <- FirmwareScout.efi
EFI/BOOT/LIMINE.EFI   <- original BOOTX64.EFI
```

Restore the original loader with:

```sh
tools/firmware-scout/stage-tree.sh --restore bld/EFI/BOOT
```

## Stage the FAT EFI image embedded in the ISO

After the normal TRUEOS ISO build has created `bld/efi.img`:

```sh
tools/firmware-scout/stage-efi-image.sh bld/efi.img
```

Restore it with:

```sh
tools/firmware-scout/stage-efi-image.sh --restore bld/efi.img
```

A rebuilt ISO must include the modified `efi.img`; staging an already embedded
copy does not retroactively rewrite an existing ISO9660 image.

## First hardware acceptance run

1. Keep a normal, unmodified TRUEOS boot entry or USB available.
2. Stage FirmwareScout on the experimental boot path.
3. Boot it. A successful preboot sequence prints:

   ```text
   TRUEOS FirmwareScout: capture-only HII handoff
   FirmwareScout: TRBIOS1 catalog installed
   FirmwareScout: chainloading \\EFI\\BOOT\\LIMINE.EFI
   ```

4. In TRUEOS run:

   ```text
   bios capture
   bios handoff
   bios setup
   ```

The decisive success state is:

```text
fallback_preboot_catalog=valid
payload_format=TRPAY1
capture_ready_for_ifr_parser=yes
```

If `LoadImage` fails, record the hexadecimal UEFI status shown by
FirmwareScout. Returning from the application should leave the firmware boot
manager usable, so the preserved `LIMINE.EFI` can also be selected manually on
firmware that exposes a file browser.

## Handoff format

The existing `TRBIOS1` header points at a payload whose first bytes are a
`TRPAY1` directory. Every section has a bounded offset and length, an individual
CRC32, and the payload has a second aggregate CRC32 in `TRBIOS1`.

Version 1 section kinds are:

| Kind | Payload |
|---:|---|
| 1 | Fixed capture-status receipt (`TRSTAT1`) |
| 2 | Raw concatenated HII package lists |
| 3 | NUL-terminated UTF-16 HII configuration response |

The complete payload is capped at 16 MiB. Package export is capped at 12 MiB
and configuration export at 4 MiB. The kernel parser treats every offset,
length, count, status, and CRC as untrusted firmware input.

## Next cycle

After hardware proves the raw package-list section survives the chainload, the
next layer can parse a conservative IFR subset and resolve prompts such as
RAID/RST/VMD/USB into form sets, question IDs, varstores, valid options,
defaults, suppression expressions, and reset requirements. Firmware writes
remain a separate, recoverable transaction layer.
