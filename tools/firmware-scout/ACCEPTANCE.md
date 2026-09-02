# Hardware acceptance receipt

This cycle is successful when the same boot produces all three observations:

```text
TRUEOS FirmwareScout: capture-only HII handoff
FirmwareScout: TRBIOS1 catalog installed
FirmwareScout: chainloading \\EFI\\BOOT\\LIMINE.EFI
```

and, after TRUEOS starts:

```text
bios capture
```

reports:

```text
fallback_preboot_catalog=valid
payload_format=TRPAY1
capture_status_receipt_valid=yes
capture_ready_for_ifr_parser=yes
```

`current_config_captured=no` is not a failure if the firmware declines
`EFI_HII_CONFIG_ROUTING_PROTOCOL.ExportConfig()`. The raw HII form and string
packages are the required input for the next IFR-decoder cycle.

Record the complete preboot text and `bios capture` output. On a chainload
failure, record the hexadecimal UEFI status and restore the original fallback
loader with either staging script's `--restore` mode.
