# Intel wireless firmware

`iwlwifi-QuZ-a0-hr-b0-77.ucode` is the unmodified Intel firmware image used by
the laptop's Linux AX201 driver (`8086:a0f0` rev20, subsystem `8086:0070`).
Linux reported version `77.f39cc7f9.0`, family `QuZ-a0-hr-b0`.

The binary was extracted from the host's linux-firmware package by decompressing
`/lib/firmware/iwlwifi-QuZ-a0-hr-b0-77.ucode.zst`. Upstream distribution:
<https://gitlab.com/kernel-firmware/linux-firmware>.

- Size: 1,406,716 bytes
- SHA-256: `69ca0913ccca609dedff5e30b1d478482487fa14ad6b7be079f8cc856ac26cac`
- License: [LICENSE.iwlwifi_firmware](LICENSE.iwlwifi_firmware)

TRUEOS embeds this image in the AX201 driver. It does not require a separately
supplied UEFI firmware module. The parser validates the TLV envelope and retains
LMAC, UMAC, and paging payloads without changing their contents. Parsing is not
proof that hardware accepted the image; runtime bring-up requires an ALIVE
notification from the device.

The existing `iwlwifi-so-a0-gf-a0-89.ucode` and matching PNVM belong to the AX211
path and must not be loaded on the AX201 QuZ profile.

Firmware container definitions are documented in the upstream Linux driver:

- <https://github.com/torvalds/linux/blob/v6.12/drivers/net/wireless/intel/iwlwifi/fw/file.h>
- <https://github.com/torvalds/linux/blob/v6.12/drivers/net/wireless/intel/iwlwifi/iwl-drv.c>
