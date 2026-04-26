# i915 ADLS TC3 Power/Route Map

Host facts from `journal-i915.txt`:

- The iGPU is `alderlake_s/raptorlake_s` display version `12.00` on PCI ID `8086:a780`.
- The relevant connector path is `[ENCODER:275:DDI TC3/PHY D]` using `AUX CH USBC3`.
- The observed host ladder is:
  `PW_1 -> always-on -> DC_off -> PW_2..PW_5 -> connector objects -> AUX_USBC3 -> DDI_IO_TC3 -> link training`.

## 1. Journal anchors -> upstream functions -> register clusters

| Host anchor | Upstream entry | What it actually does | Register cluster |
| --- | --- | --- | --- |
| `PW_1`, `PW_2`, `PW_3`, `PW_4`, `PW_5` | `intel_power_well_enable()` -> `hsw_power_well_enable()` | Sets the driver's request bit for the selected main power well and waits for the state bit to assert. | Main power-well control block at `0x45400-0x4540c` (`HSW_PWR_WELL_CTL1-4`) |
| `always-on` | power-well name from the domain map, but always-on wells use no-op enable/disable ops | Software/runtime-PM concept, not a meaningful MMIO toggle on this path | No dedicated per-step poke on ADLS TC3 bring-up |
| `DC_off` | `intel_power_well_enable()` -> `gen9_dc_off_power_well_enable()` | Exits deep display C-states by forcing `DC_STATE_EN` back to `DC_STATE_DISABLE` | `DC_STATE_EN` at `0x45504` |
| `AUX_USBC3` | `intel_power_well_enable()` -> `icl_aux_power_well_enable()` -> `icl_tc_phy_aux_power_well_enable()` | Requests AUX PW for TC3, programs AUX path TBT/non-TBT mode, and on ADL+ waits for the TC PHY uC health bit | AUX PW block `0x45440/44/4c`, AUX channel control for USBC3, plus TC3 DKL uC health |
| `DDI_IO_TC3` | `intel_power_well_enable()` -> `hsw_power_well_enable()` through `icl_ddi_power_well_ops` | Requests the TC3 DDI IO power well | DDI PW block `0x45450/54/5c` |
| `intel_dp_detect_dpcd` | `intel_dp_detect_dpcd()` | Reads DPCD over AUX; this is a consumer of `AUX_USBC3`, not the owner of the AUX/DDI power-well MMIO | AUX transaction registers (`DP_AUX_CH_CTL/DATA`) |
| `intel_dp_prepare_link_train` | `intel_dp_prepare_link_train()` | Programs link parameters over AUX; the actual port-enable write is delegated to `prepare_link_retrain()` | AUX DPCD writes first, then DDI/TP control in `intel_ddi_prepare_link_retrain()` |
| route / training | `intel_ddi_prepare_link_retrain()` and TC helpers in `intel_tc.c` | Enables transport on the port and handles TC ownership/mode sideband state | `DDI_BUF_CTL(port)`, `DP_TP_CTL`, `TCSS_DDI_STATUS`, `PORT_TX_DFLEX*` |

## 2. Exact ADLS/TGL-style TC3 mapping

ADLS display version 12 still uses the Tiger Lake-style Type-C power-domain map for these display wells:

- `DC_off` is the main `gen9_dc_off_power_well_ops` well.
- `PW_2` and `PW_3` are main HSW-style request/state wells.
- `DDI_IO_TC3` is a dedicated DDI power well using `icl_ddi_power_well_ops`.
- `AUX_USBC3` is a dedicated AUX power well using `icl_aux_power_well_ops`.

The TC3-specific domain declarations live in the TGL map:

- `POWER_DOMAIN_PORT_DDI_IO_TC3` is bound to power-well instance `DDI_IO_TC3`.
- `POWER_DOMAIN_AUX_USBC3` is bound to power-well instance `AUX_USBC3`.

## 3. Register clusters you can classify immediately

### `454xx` / `455xx`

This is the real power-domain seam for your host ladder.

- `0x45400-0x4540c` = main power-well control (`PW_1..PW_5`, plus request/state bits).
- `0x45440/0x45444/0x4544c` = AUX power-well control (`AUX_USBC3` lives here, index `TGL_PW_CTL_IDX_AUX_TC3 = 5`).
- `0x45450/0x45454/0x4545c` = DDI power-well control (`DDI_IO_TC3` lives here, index `TGL_PW_CTL_IDX_DDI_TC3 = 5`).
- `0x45504` = `DC_STATE_EN`, which is the `DC_off` stage rather than a normal HSW-style power-well request bit.

Translation:

- `4540x` main block -> `PW_1/PW_2/PW_3/PW_4/PW_5`
- `4544x` -> `AUX_USBC3`
- `4545x` -> `DDI_IO_TC3`
- `45504` -> `DC_off`

### `450xx` / `460xx`

These do not line up with the host's named power-domain ladder.

- `450xx` on ADLS is mostly DBUF/MBUS/watermark/arbitration plumbing, eg. `DBUF_CTL_S0` and `MBUS_*`.
- `460xx` is clocking / DPLL / port-clock territory, eg. `CDCLK_CTL`, `CDCLK_SQUASH_CTL`, `PORTTC*_PLL_ENABLE`, `PORT_CLK_SEL`.

Translation:

- Treat `450xx` as display-core/pipe support state, not as TC3 AUX/DDI gating.
- Treat `460xx` as later route/clock/PLL setup, not as the initial power-well ladder.

### `1380xx` / `6C0xx`

These look like side-support or legacy/alternate PHY space, not the direct TC3 domain ladder on this platform.

- `1380xx` matches only a small number of display-related legacy/GT-side registers in current i915 headers, eg. `BXT_P_CR_GT_DISP_PWRON` and `D_COMP_BDW`.
- `6C0xx` matches combo-PHY / older DPLL space, eg. `DISPIO_CR_TX_BMU_CR0`, `DPLL_CTRL1/2`, `DPLL_STATUS`, and combo-PHY bases.

For your ADLS TC3 path the active TC PHY space is Dekel/TCSS/FIA, not `6C0xx`:

- TC3 Dekel PHY base: `0x16A000`
- TCSS status: `0x161500/0x161504`
- FIA1 sideband base: `0x163000`

Translation:

- `1380xx` -> side-support / GT-display-compensation class, not the named TC3 power-domain ladder
- `6C0xx` -> legacy combo-PHY / DPLL support space, not the ADLS TC3 AUX/DDI gate itself

## 4. Small-function trace for the exact journal strings

### `intel_power_well_enable`

Generic wrapper:

- logs `enabling <name>`
- dispatches to the power well's `ops->enable()`

For the relevant host names on ADLS TC3:

- `PW_1..PW_5` -> `hsw_power_well_enable()`
- `DC_off` -> `gen9_dc_off_power_well_enable()`
- `AUX_USBC3` -> `icl_tc_phy_aux_power_well_enable()`
- `DDI_IO_TC3` -> `hsw_power_well_enable()` via `icl_ddi_power_well_ops`

### `intel_dp_detect_dpcd`

This function does not enable a power domain directly. It:

- resumes LSPCON if needed
- calls `intel_dp_get_dpcd()`
- checks MST / branch / sink-count state

So the useful register cluster is not inside `intel_dp_detect_dpcd()` itself. The useful dependency is:

- `intel_dp_detect_dpcd()` needs the AUX domain already alive
- on this host that means `AUX_USBC3` must already be enabled

### `intel_dp_prepare_link_train`

This function is also mostly a link-parameter setup function. It:

- optionally calls `prepare_link_retrain()`
- computes `LINK_BW_SET` / `LINK_RATE_SET`
- programs training parameters via AUX DPCD writes

The actual display-side port enable for this step is in:

- `intel_ddi_prepare_link_retrain()`

That function:

- enables `DP_TP_CTL`
- writes `DDI_BUF_CTL(port)` with `DDI_BUF_CTL_ENABLE`

So if your host ladder says `DDI_IO_TC3` then `intel_dp_prepare_link_train`, the register split is:

- power gate first: `0x4545x`
- transport enable after that: `DDI_BUF_CTL(port)` for the TC3 port

### `intel_hdmi_init_connector`

This is connector-object setup only:

- it logs `Adding HDMI connector`
- sets up DRM connector state, DDC pin, properties, helpers

It is a good host anchor for the "connector objects are created after wells, before AUX/DDI IO training" part of the ladder, but it is not itself the register write you want to clone.

## 5. Most useful concrete registers for TC3

For the ADLS `DDI TC3 / PHY D / AUX_USBC3` path, the most actionable set is:

- main PW control: `HSW_PWR_WELL_CTL2` at `0x45404`
- AUX PW control: `ICL_PWR_WELL_CTL_AUX2` at `0x45444`
- DDI PW control: `ICL_PWR_WELL_CTL_DDI2` at `0x45454`
- DC state: `DC_STATE_EN` at `0x45504`
- TC3 AUX control: `DP_AUX_CH_CTL(AUX_CH_USBC3)` -> `0x64510`
- TC3 DDI buffer control: `DDI_BUF_CTL(port)` for TC3 port -> `0x64500`
- TCSS ready state: `TCSS_DDI_STATUS(TC3)` -> `0x161504`
- FIA sideband / lane-mode state: `PORT_TX_DFLEXPA1(FIA1)`, `PORT_TX_DFLEXDPSP(FIA1)`, `PORT_TX_DFLEXDPMLE1(FIA1)`, `PORT_TX_DFLEXDPCSSS(FIA1)`
- TC3 DKL uC health wait used during AUX PW enable: `DKL_CMN_UC_DW_27(TC3)` -> MMIO window at `0x16A36C`

## 6. Practical next step

If we now want a host-to-guest translation table instead of ad hoc pokes, the clean cut is:

1. `PW_1/PW_2/PW_3/PW_4/PW_5` -> `4540x`
2. `DC_off` -> `45504`
3. connector enumeration only -> no clone target
4. `AUX_USBC3` -> `45444` plus `0x64510`
5. `DDI_IO_TC3` -> `45454`
6. TC mode / route / ownership -> `1615xx`, `1638xx`, `64500`
7. link training -> AUX DPCD traffic plus `DDI_BUF_CTL` / `DP_TP_CTL`
