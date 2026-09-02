# Hardware acceptance receipt

The preboot handoff remains a prerequisite. The same boot must still show:

```text
TRUEOS FirmwareScout: capture-only HII handoff
FirmwareScout: TRBIOS1 catalog installed
FirmwareScout: chainloading \\EFI\\BOOT\\LIMINE.EFI
```

and `bios capture` must continue to report a valid `TRPAY1` handoff with form
and string packages ready for the parser.

## Read-only BIOS schema acceptance

After TRUEOS starts, run:

```text
bios packages
bios languages
bios strings status
bios schema
```

The first useful board result is:

```text
bios schema
  state=ready
  formsets=<nonzero>
  forms=<nonzero>
  questions=<nonzero>
  strings_resolved=<nonzero>
  malformed_packages=0
  active_write_path=none
```

Then run the primary storage and USB searches:

```text
bios find raid
bios find rst
bios find vmd
bios find sata
bios find usb
bios find xhci
```

Each search must return one or more complete `Question` records or exactly:

```text
question_match=none
```

A loose HII string, SMBIOS hint, or PCI label is not a question match. Use the
returned form-set GUID, form ID, question ID, validated storage binding, and
options to correlate a firmware question with the Intel xHCI controller at
`00:14.0`, MEI at `00:16.0`, or SATA controller at `00:17.0`.

## Safety and privacy boundary

This cycle never calls variable, routing, browser, reset, capsule, or firmware
mutation services. Current configuration data remains redacted. The browser
only reports that configuration was captured; it does not decode or display a
current value in this cycle. Every command must end with:

```text
active_write_path=none
```

On a chainload failure, record the hexadecimal UEFI status and restore the
original fallback loader with either staging script's `--restore` mode.
