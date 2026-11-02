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

## AX201 recovery and diagnosis

Private boot profiles retry a failed or previously disconnected station after
five seconds. The AX201 driver keeps the same PCI claim, shared driver and
registered network interface. Recovery stops the DMA master and requires
`MASTER_DISABLED`, verifies PCI bus mastering is off, performs a software reset,
and then restarts firmware initialization, NVM, scanning and association.
RX/command/context/firmware allocations are reused. Station queues are freed
only after the checked stop/reset; an unconfirmed stop retains their DMA and
fails recovery. Initial firmware-start failures retain the runtime too.

Session-protection lifetime is independent of carrier. A valid protection
failure/end after association must not tear down the established station;
before association, failure still closes it. This follows Linux v6.14
[`iwl_mvm_te_check_disconnect`](https://github.com/torvalds/linux/blob/v6.14/drivers/net/wireless/intel/iwlwifi/mvm/time-event.c).
The stop/reset ordering follows
[`iwl_pcie_gen2_apm_stop`](https://github.com/torvalds/linux/blob/v6.14/drivers/net/wireless/intel/iwlwifi/pcie/trans-gen2.c)
and
[`iwl_pcie_apm_stop_master`](https://github.com/torvalds/linux/blob/v6.14/drivers/net/wireless/intel/iwlwifi/pcie/trans.c).

Failure logs record runtime stage, pending command, station lifetime, TX/RX and
drop counts, AP disconnection reason codes, and throttled EAPOL/CCMP rejection
causes. Capture the first failure and `recovery-start previous_error` before
attributing a real AP disconnect to the session-protection bug. Never log a
profile password. Live pairwise/group WPA2 rekey remains unsupported; a rejected
rekey is diagnostic evidence, not proof of a failed transport.

The host harnesses in `tools/testpy/test_ax201_{transport,mvm,station}.py` and
`test_wifi_autoconnect.py` cover reset failures, repeated cycles, queue wraps,
carrier gating, and firmware/station state transitions. They use scripted MMIO
or notifications. They do not establish sustained radio/AP stability: verify
that separately on the laptop, including traffic through disconnect/recovery.
