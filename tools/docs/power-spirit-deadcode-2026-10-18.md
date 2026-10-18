# Power and Spirit dead-code detail — 2026-10-18

Based on the successful default-feature kernel compiler census, with `--force-warn dead_code`, followed by source/caller inspection. Counts are diagnostics, not removable functions or lines. No code was removed. Feature-dependent use was checked in source, not with a second feature-enabled build.

## Power: 105 diagnostics

| File | Diagnostics | Unused surface | Assessment |
| --- | ---: | --- | --- |
| `hfi.rs` | 33 | HFI table allocation/enable/disable, stable table capture, population tracking, text diagnostics | Table-management path has no live entry caller. CPUID capability detection and per-CPU metadata remain used. |
| `hwp.rs` | 34 | Scoped HWP performance guard and its MSR encoding/validation support | **Feature-dependent.** `src/ai/ttstt_service.rs:1597` calls `ScopedHwpPerformance::try_begin`; that service is gated by `trueos_ttstt`, off by default. Do not treat this subsystem as abandoned. |
| `mod.rs` | 9 | MSR-detail probing, current ratio, P-state ratio setter, idle-policy setter and four constants | Unwired control/probe APIs. Boot `power::init` and idle-policy infrastructure remain. `MSR_ARMED` starts at zero and no store enabling it was found. |
| `rapl.rs` | 5 | `has_data`, `RaplReceiver`, `subscribe`, `anon_snapshot`, `log_local_probe` | Unused alternate observation/diagnostic APIs. Energy sampling, snapshots, history and persistence have current callers. |
| `thermal.rs` | 15 | Ten decoded status/log fields, one completion field, `has_data`, receiver/subscription helpers and `anon_snapshot` | Mostly unused telemetry fields and alternate observation APIs. Thermal service and passive per-core polling remain called. |
| `turbo.rs` | 9 | `set_armed`, `local_state`, local/all-CPU setters, apply callback/report, two return constants, `Disarmed` variant | Turbo mutation path is uncalled. Read/status and all-CPU verification paths remain called by ACPI diagnostics. |

Power is therefore a mixture of disconnected write/control paths, unused telemetry convenience APIs, and an optional speech optimization. Removing entire files would discard live code or the optional speech path.

## Spirit: 108 diagnostics / 130 primary source locations

| File | Diagnostics | Unused surface | Assessment |
| --- | ---: | --- | --- |
| `Spirit_VFX.rs` | 53 | UI control metadata; names/wire-name parsers; UI configuration structs/JSON conversion; publish/select/reset API; rotation and edge-fade setters | Large unwired control/UI layer. VFX rendering, snapshots and automatic movement/idle/window/power-mode effects remain connected. |
| `mod.rs` | 13 | `SpiritColor`, command-stream helpers; selected fence/wait methods; `CPU_AND_GPU`; `External` variant; `acquire_frame`, `submit`, manual movement setters | Spare presentation/producer APIs. Spirit worker and internal frame/render paths remain connected. |
| `gpu_logger.rs` | 12 | Lease acquisition/release, producer publish/stop, status types and associated state methods | **Producer side disconnected:** no current callers for `request` or `publish`. Worker-side snapshot/rendering and shell sample reads still have callers. Those readers alone cannot make the producer publish data. |
| `lilly_protocol.rs` | 9 | General package/JSON ingestion, single animation/text helpers, timeline replacement, stop-at-boundary, some error variants | General orchestration APIs uncalled. Emotion enqueue and animation consumption paths remain connected. |
| `response_window.rs` | 8 | `ReasoningResponseStream`, begin/update/finish/abort streaming API and turn accessor | Incremental producer API uncalled. Completed response enqueue and presenter task remain connected to Lumen. |
| `lilly.rs` | 7 | Part lookup, frame/part convenience accessors and tuple fields | Unused accessors/data reads. Resident preparation, asset/animation lookup and frame counting remain called. |
| `intel_cursor.rs` | 6 | Cursor radius/hotspot constants, local coordinate and circle helpers, normalized-screen conversion | Unused drawing/coordinate helpers. Does not establish that Intel cursor support is unused. |

The strongest follow-up candidates are the disconnected Spirit control-panel and producer APIs, the unused incremental response interface, Power's HFI table-management entry points, and the uncalled P-state/turbo mutation APIs. Decide which should be retired versus wired before removing their supporting trees. Hardware layouts, serialized types, and optional features require separate review.

## Complete compiler findings

The table below preserves every primary compiler location. Repeated messages with the same ID are one diagnostic covering several items. Source locations refer to the audited working tree.


### `src/power`

| ID | Location | Compiler finding | Item source |
| ---: | --- | --- | --- |
| 2294 | `src/power/hfi.rs:21` | constant `HFI_MAX_TABLE_PAGES` is never used | const HFI_MAX_TABLE_PAGES: usize = 16; |
| 2295 | `src/power/hfi.rs:22` | constant `HFI_TIMESTAMP_BYTES` is never used | const HFI_TIMESTAMP_BYTES: usize = core::mem::size_of::<u64>(); |
| 2296 | `src/power/hfi.rs:23` | constant `HFI_STABLE_CAPTURE_ATTEMPTS` is never used | const HFI_STABLE_CAPTURE_ATTEMPTS: usize = 32; |
| 2297 | `src/power/hfi.rs:24` | constant `HFI_INITIAL_READY_CAPTURE_ROUNDS` is never used | const HFI_INITIAL_READY_CAPTURE_ROUNDS: usize = 64; |
| 2298 | `src/power/hfi.rs:26` | constant `MSR_IA32_HW_FEEDBACK_PTR` is never used | const MSR_IA32_HW_FEEDBACK_PTR: u32 = 0x17D0; |
| 2299 | `src/power/hfi.rs:27` | constant `MSR_IA32_HW_FEEDBACK_CONFIG` is never used | const MSR_IA32_HW_FEEDBACK_CONFIG: u32 = 0x17D1; |
| 2300 | `src/power/hfi.rs:28` | constant `HW_FEEDBACK_PTR_VALID` is never used | const HW_FEEDBACK_PTR_VALID: u64 = 1 << 0; |
| 2301 | `src/power/hfi.rs:29` | constant `HW_FEEDBACK_CONFIG_HFI_ENABLE` is never used | const HW_FEEDBACK_CONFIG_HFI_ENABLE: u64 = 1 << 0; |
| 2302 | `src/power/hfi.rs:39` | struct `HfiTableState` is never constructed | struct HfiTableState { |
| 2303 | `src/power/hfi.rs:52` | struct `HfiTableCapture` is never constructed | struct HfiTableCapture { |
| 2304 | `src/power/hfi.rs:59` | static `HFI_TABLE` is never used | static HFI_TABLE: Mutex<Option<HfiTableState>> = Mutex::new(None); |
| 2305 | `src/power/hfi.rs:183` | function `round_up_8` is never used | fn round_up_8(value: usize) -> usize { |
| 2306 | `src/power/hfi.rs:187` | function `capability_layout` is never used | fn capability_layout(mask: u8) -> Option<(usize, usize, Option<usize>, Option<usize>)> { |
| 2307 | `src/power/hfi.rs:208` | function `registered_row_count` is never used | fn registered_row_count() -> Option<usize> { |
| 2308 | `src/power/hfi.rs:230` | function `single_package_registered` is never used | fn single_package_registered() -> bool { |
| 2309 | `src/power/hfi.rs:248` | function `read_u8` is never used | fn read_u8(base: usize, offset: usize) -> u8 { |
| 2310 | `src/power/hfi.rs:252` | function `read_u64` is never used | fn read_u64(base: usize, offset: usize) -> u64 { |
| 2311 | `src/power/hfi.rs:256` | function `used_table_bytes` is never used | fn used_table_bytes(state: HfiTableState) -> Option<usize> { |
| 2312 | `src/power/hfi.rs:262` | function `capture_once` is never used | fn capture_once(state: HfiTableState, attempt: usize) -> Option<HfiTableCapture> { |
| 2313 | `src/power/hfi.rs:292` | function `capture_stable` is never used | fn capture_stable(state: HfiTableState) -> Option<HfiTableCapture> { |
| 2314 | `src/power/hfi.rs:315` | function `capture_epoch_baseline` is never used | fn capture_epoch_baseline(state: HfiTableState) -> Option<u64> { |
| 2315 | `src/power/hfi.rs:328` | function `updated_columns_mask` is never used | fn updated_columns_mask(capability_mask: u8, image: &[u8]) -> u8 { |
| 2316 | `src/power/hfi.rs:349` | function `initial_columns_complete` is never used | fn initial_columns_complete(state: HfiTableState) -> bool { |
| 2317 | `src/power/hfi.rs:354` | function `initial_population_ready` is never used | fn initial_population_ready(state: HfiTableState) -> bool { |
| 2318 | `src/power/hfi.rs:358` | function `capture_belongs_to_current_enable_epoch` is never used | fn capture_belongs_to_current_enable_epoch( |
| 2319 | `src/power/hfi.rs:365` | function `begin_enable_epoch` is never used | fn begin_enable_epoch(state: &mut HfiTableState, baseline_timestamp: u64) { |
| 2320 | `src/power/hfi.rs:370` | function `observe_stable_capture` is never used | fn observe_stable_capture(state: &mut HfiTableState, capture: &HfiTableCapture) { |
| 2321 | `src/power/hfi.rs:380` | function `wait_for_initial_population` is never used | fn wait_for_initial_population(state: &mut HfiTableState) { |
| 2322 | `src/power/hfi.rs:402` | function `enable_table_explicit` is never used | pub(crate) fn enable_table_explicit() -> Result<(), &'static str> { |
| 2323 | `src/power/hfi.rs:504` | function `disable_table_explicit` is never used | pub(crate) fn disable_table_explicit() -> Result<(), &'static str> { |
| 2324 | `src/power/hfi.rs:521` | function `table_snapshot_text` is never used | pub(crate) fn table_snapshot_text() -> String { |
| 2325 | `src/power/hfi.rs:666` | function `explicit_bringup_text` is never used | pub(crate) fn explicit_bringup_text() -> String { |
| 2326 | `src/power/hfi.rs:680` | function `explicit_disable_text` is never used | pub(crate) fn explicit_disable_text() -> String { |
| 2327 | `src/power/hwp.rs:14` | constant `IA32_PM_ENABLE` is never used | const IA32_PM_ENABLE: u32 = 0x770; |
| 2328 | `src/power/hwp.rs:15` | constant `IA32_HWP_CAPABILITIES` is never used | const IA32_HWP_CAPABILITIES: u32 = 0x771; |
| 2329 | `src/power/hwp.rs:16` | constant `IA32_HWP_REQUEST` is never used | const IA32_HWP_REQUEST: u32 = 0x774; |
| 2330 | `src/power/hwp.rs:18` | constant `PM_ENABLE_HWP` is never used | const PM_ENABLE_HWP: u64 = 1; |
| 2331 | `src/power/hwp.rs:19` | constant `BYTE_MASK` is never used | const BYTE_MASK: u64 = 0xff; |
| 2332 | `src/power/hwp.rs:20` | constant `REQUEST_MIN_SHIFT` is never used | const REQUEST_MIN_SHIFT: u32 = 0; |
| 2333 | `src/power/hwp.rs:21` | constant `REQUEST_MAX_SHIFT` is never used | const REQUEST_MAX_SHIFT: u32 = 8; |
| 2334 | `src/power/hwp.rs:22` | constant `REQUEST_DESIRED_SHIFT` is never used | const REQUEST_DESIRED_SHIFT: u32 = 16; |
| 2335 | `src/power/hwp.rs:23` | constant `REQUEST_EPP_SHIFT` is never used | const REQUEST_EPP_SHIFT: u32 = 24; |
| 2336 | `src/power/hwp.rs:24` | constant `CAP_HIGHEST_SHIFT` is never used | const CAP_HIGHEST_SHIFT: u32 = 0; |
| 2337 | `src/power/hwp.rs:25` | constant `CAP_GUARANTEED_SHIFT` is never used | const CAP_GUARANTEED_SHIFT: u32 = 8; |
| 2338 | `src/power/hwp.rs:26` | constant `CAP_MOST_EFFICIENT_SHIFT` is never used | const CAP_MOST_EFFICIENT_SHIFT: u32 = 16; |
| 2339 | `src/power/hwp.rs:27` | constant `CAP_LOWEST_SHIFT` is never used | const CAP_LOWEST_SHIFT: u32 = 24; |
| 2340 | `src/power/hwp.rs:31` | constant `AUDIT_UNCLAIMED` is never used | const AUDIT_UNCLAIMED: u8 = 0; |
| 2341 | `src/power/hwp.rs:32` | constant `AUDIT_ACTIVE` is never used | const AUDIT_ACTIVE: u8 = 1; |
| 2342 | `src/power/hwp.rs:33` | constant `AUDIT_FINISHED` is never used | const AUDIT_FINISHED: u8 = 2; |
| 2343 | `src/power/hwp.rs:34` | static `ACTIVATION_AUDIT` is never used | static ACTIVATION_AUDIT: AtomicU8 = AtomicU8::new(AUDIT_UNCLAIMED); |
| 2344 | `src/power/hwp.rs:35` | static `UNAVAILABLE_LOGGED` is never used | static UNAVAILABLE_LOGGED: AtomicBool = AtomicBool::new(false); |
| 2345 | `src/power/hwp.rs:38` | enum `HwpRequestError` is never used | pub enum HwpRequestError { |
| 2346 | `src/power/hwp.rs:45` | method `as_str` is never used | pub const fn as_str(self) -> &'static str { |
| 2347 | `src/power/hwp.rs:55` | struct `HwpCapabilities` is never constructed | pub struct HwpCapabilities { |
| 2348 | `src/power/hwp.rs:64` | struct `HwpRequestFields` is never constructed | pub struct HwpRequestFields { |
| 2349 | `src/power/hwp.rs:76` | struct `ScopedHwpPerformance` is never constructed | pub struct ScopedHwpPerformance { |
| 2350 | `src/power/hwp.rs:92` | associated function `try_begin` is never used | pub fn try_begin() -> Result<Self, HwpRequestError> { |
| 2351 | `src/power/hwp.rs:152` | method `previous_request` is never used | pub fn previous_request(&self) -> u64 { |
| 2352 | `src/power/hwp.rs:157` | method `requested` is never used | pub fn requested(&self) -> u64 { |
| 2353 | `src/power/hwp.rs:187` | struct `LocalHwpSupport` is never constructed | struct LocalHwpSupport { |
| 2354 | `src/power/hwp.rs:191` | function `local_hwp_support` is never used | fn local_hwp_support() -> Option<LocalHwpSupport> { |
| 2355 | `src/power/hwp.rs:210` | function `log_unavailable_once` is never used | fn log_unavailable_once(error: HwpRequestError, pm_enable: u64) { |
| 2356 | `src/power/hwp.rs:221` | function `byte` is never used | const fn byte(raw: u64, shift: u32) -> u8 { |
| 2357 | `src/power/hwp.rs:225` | function `replace_byte` is never used | const fn replace_byte(raw: u64, shift: u32, value: u8) -> u64 { |
| 2358 | `src/power/hwp.rs:230` | function `decode_capabilities` is never used | pub const fn decode_capabilities(raw: u64) -> HwpCapabilities { |
| 2359 | `src/power/hwp.rs:240` | function `decode_request` is never used | pub const fn decode_request(raw: u64) -> HwpRequestFields { |
| 2360 | `src/power/hwp.rs:249` | function `encode_performance_request` is never used | pub const fn encode_performance_request( |
| 2285 | `src/power/mod.rs:14` | constant `IA32_MSR_PLATFORM_INFO` is never used | const IA32_MSR_PLATFORM_INFO: u32 = 0xCE; |
| 2286 | `src/power/mod.rs:16` | constant `IA32_MSR_PERF_STATUS` is never used | const IA32_MSR_PERF_STATUS: u32 = 0x198; |
| 2287 | `src/power/mod.rs:18` | constant `IA32_MSR_PERF_CTL` is never used | const IA32_MSR_PERF_CTL: u32 = 0x199; |
| 2288 | `src/power/mod.rs:20` | constant `IA32_MSR_HWP_CAPABILITIES` is never used | const IA32_MSR_HWP_CAPABILITIES: u32 = 0x771; |
| 2289 | `src/power/mod.rs:126` | function `probe_msr_details` is never used | pub unsafe fn probe_msr_details() -> Option<&'static PowerMsrDetails> { |
| 2290 | `src/power/mod.rs:140` | function `set_idle_policy` is never used | pub fn set_idle_policy(policy: IdlePolicy) -> IdlePolicy { |
| 2291 | `src/power/mod.rs:146` | function `current_ratio` is never used | pub fn current_ratio() -> Option<u8> { |
| 2292 | `src/power/mod.rs:156` | function `set_pstate_ratio` is never used | pub fn set_pstate_ratio(requested: u8) -> Result<u8, &'static str> { |
| 2293 | `src/power/mod.rs:197` | function `detect_msr_details` is never used | fn detect_msr_details() -> Option<PowerMsrDetails> { |
| 2361 | `src/power/rapl.rs:150` | method `has_data` is never used | pub const fn has_data(self) -> bool { |
| 2362 | `src/power/rapl.rs:160` | type alias `RaplReceiver` is never used | pub type RaplReceiver<'a> = |
| 2363 | `src/power/rapl.rs:206` | function `subscribe` is never used | pub fn subscribe() -> Option<RaplReceiver<'static>> { |
| 2364 | `src/power/rapl.rs:211` | function `anon_snapshot` is never used | pub fn anon_snapshot() -> RaplSnapshot { |
| 2365 | `src/power/rapl.rs:287` | function `log_local_probe` is never used | pub unsafe fn log_local_probe() -> bool { |
| 2366 | `src/power/thermal.rs:73` | field `thermal_log` is never read | pub thermal_log: bool, |
| 2367 | `src/power/thermal.rs:76` | field `prochot_log` is never read | pub prochot_log: bool, |
| 2368 | `src/power/thermal.rs:79` | field `critical_log` is never read | pub critical_log: bool, |
| 2369 | `src/power/thermal.rs:81` | field `threshold1_status` is never read | pub threshold1_status: bool, |
| 2370 | `src/power/thermal.rs:83` | field `threshold1_log` is never read | pub threshold1_log: bool, |
| 2371 | `src/power/thermal.rs:85` | field `threshold2_status` is never read | pub threshold2_status: bool, |
| 2372 | `src/power/thermal.rs:87` | field `threshold2_log` is never read | pub threshold2_log: bool, |
| 2373 | `src/power/thermal.rs:90` | field `power_limit_log` is never read | pub power_limit_log: bool, |
| 2374 | `src/power/thermal.rs:93` | field `current_limit_log` is never read | pub current_limit_log: bool, |
| 2375 | `src/power/thermal.rs:96` | field `cross_domain_limit_log` is never read | pub cross_domain_limit_log: bool, |
| 2376 | `src/power/thermal.rs:104` | field `completed` is never read | pub completed: bool, |
| 2377 | `src/power/thermal.rs:158` | method `has_data` is never used | pub const fn has_data(&self) -> bool { |
| 2378 | `src/power/thermal.rs:190` | type alias `ThermalReceiver` is never used | pub type ThermalReceiver<'a> = |
| 2379 | `src/power/thermal.rs:219` | function `subscribe` is never used | pub fn subscribe() -> Option<ThermalReceiver<'static>> { |
| 2380 | `src/power/thermal.rs:224` | function `anon_snapshot` is never used | pub fn anon_snapshot() -> ThermalSnapshot { |
| 2381 | `src/power/turbo.rs:28` | variant `Disarmed` is never constructed | Disarmed, |
| 2382 | `src/power/turbo.rs:33` | struct `TurboApplyReport` is never constructed | pub struct TurboApplyReport { |
| 2383 | `src/power/turbo.rs:62` | function `set_armed` is never used | pub fn set_armed(v: bool) { |
| 2384 | `src/power/turbo.rs:88` | function `local_state` is never used | pub fn local_state() -> Result<TurboState, TurboSetError> { |
| 2385 | `src/power/turbo.rs:118` | function `set_enabled_local` is never used | pub fn set_enabled_local(enable: bool) -> Result<TurboState, TurboSetError> { |
| 2386 | `src/power/turbo.rs:139` | constant `RET_OK` is never used | const RET_OK: u64 = 0; |
| 2387 | `src/power/turbo.rs:142` | constant `RET_DISARMED` is never used | const RET_DISARMED: u64 = 2; |
| 2388 | `src/power/turbo.rs:145` | function `smp_apply_turbo` is never used | fn smp_apply_turbo(arg: u64) -> u64 { |
| 2389 | `src/power/turbo.rs:159` | function `set_enabled_all` is never used | pub fn set_enabled_all(enable: bool) -> Result<TurboApplyReport, TurboSetError> { |

### `src/spirit`

| ID | Location | Compiler finding | Item source |
| ---: | --- | --- | --- |
| 2900 | `src/spirit/Spirit_VFX.rs:46` | constant `TRANSFORM_CONTROLS` is never used | pub(crate) const TRANSFORM_CONTROLS: [SpiritVfxSliderSpec; 3] = [ |
| 2901 | `src/spirit/Spirit_VFX.rs:52` | constant `ALPHA_CUTOFF_CONTROL` is never used | pub(crate) const ALPHA_CUTOFF_CONTROL: SpiritVfxSliderSpec = |
| 2902 | `src/spirit/Spirit_VFX.rs:55` | constant `EDGE_FADE_CONTROL` is never used | pub(crate) const EDGE_FADE_CONTROL: SpiritVfxSliderSpec = |
| 2903 | `src/spirit/Spirit_VFX.rs:64` | constant `PARTICLE_CONTROLS` is never used | pub(crate) const PARTICLE_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2904 | `src/spirit/Spirit_VFX.rs:71` | constant `NO_EFFECT_CONTROLS` is never used | const NO_EFFECT_CONTROLS: [SpiritVfxSliderSpec; 0] = []; |
| 2905 | `src/spirit/Spirit_VFX.rs:72` | constant `AURA_BLOOM_CONTROLS` is never used | const AURA_BLOOM_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2906 | `src/spirit/Spirit_VFX.rs:78` | constant `NEON_EDGE_CONTROLS` is never used | const NEON_EDGE_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2907 | `src/spirit/Spirit_VFX.rs:84` | constant `FIRE_RIM_CONTROLS` is never used | const FIRE_RIM_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2908 | `src/spirit/Spirit_VFX.rs:90` | constant `ICE_SHIMMER_CONTROLS` is never used | const ICE_SHIMMER_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2909 | `src/spirit/Spirit_VFX.rs:96` | constant `HOLOGRAM_CONTROLS` is never used | const HOLOGRAM_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2910 | `src/spirit/Spirit_VFX.rs:102` | constant `RGB_GLITCH_CONTROLS` is never used | const RGB_GLITCH_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2911 | `src/spirit/Spirit_VFX.rs:108` | constant `DISSOLVE_CONTROLS` is never used | const DISSOLVE_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2912 | `src/spirit/Spirit_VFX.rs:114` | constant `GHOST_TRAIL_CONTROLS` is never used | const GHOST_TRAIL_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2913 | `src/spirit/Spirit_VFX.rs:120` | constant `ELECTRIC_ARC_CONTROLS` is never used | const ELECTRIC_ARC_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2914 | `src/spirit/Spirit_VFX.rs:126` | constant `RAINBOW_PRISM_CONTROLS` is never used | const RAINBOW_PRISM_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2915 | `src/spirit/Spirit_VFX.rs:132` | constant `HIT_FLASH_CONTROLS` is never used | const HIT_FLASH_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2916 | `src/spirit/Spirit_VFX.rs:138` | constant `PIXEL_WAVE_CONTROLS` is never used | const PIXEL_WAVE_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2917 | `src/spirit/Spirit_VFX.rs:144` | constant `TOON_INK_CONTROLS` is never used | const TOON_INK_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2918 | `src/spirit/Spirit_VFX.rs:150` | constant `LIQUID_WARP_CONTROLS` is never used | const LIQUID_WARP_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2919 | `src/spirit/Spirit_VFX.rs:156` | constant `DREAM_BLOOM_CONTROLS` is never used | const DREAM_BLOOM_CONTROLS: [SpiritVfxSliderSpec; 4] = [ |
| 2920 | `src/spirit/Spirit_VFX.rs:173` | associated items `ui_name`, `wire_name`, and `from_wire_name` are never used | pub(crate) const fn ui_name(self) -> &'static str { |
| 2920 | `src/spirit/Spirit_VFX.rs:180` | associated items `ui_name`, `wire_name`, and `from_wire_name` are never used | const fn wire_name(self) -> &'static str { |
| 2920 | `src/spirit/Spirit_VFX.rs:187` | associated items `ui_name`, `wire_name`, and `from_wire_name` are never used | fn from_wire_name(name: &str) -> Option<Self> { |
| 2921 | `src/spirit/Spirit_VFX.rs:220` | associated items `from_id`, `ui_name`, `hint`, and `controls` are never used | pub(crate) const fn from_id(id: u8) -> Option<Self> { |
| 2921 | `src/spirit/Spirit_VFX.rs:242` | associated items `from_id`, `ui_name`, `hint`, and `controls` are never used | pub(crate) const fn ui_name(self) -> &'static str { |
| 2921 | `src/spirit/Spirit_VFX.rs:263` | associated items `from_id`, `ui_name`, `hint`, and `controls` are never used | pub(crate) const fn hint(self) -> &'static str { |
| 2921 | `src/spirit/Spirit_VFX.rs:284` | associated items `from_id`, `ui_name`, `hint`, and `controls` are never used | pub(crate) const fn controls(self) -> &'static [SpiritVfxSliderSpec] { |
| 2922 | `src/spirit/Spirit_VFX.rs:378` | associated function `from_id` is never used | pub(crate) const fn from_id(id: u8) -> Option<Self> { |
| 2923 | `src/spirit/Spirit_VFX.rs:485` | associated items `ui_name`, `wire_name`, and `from_wire_name` are never used | pub(crate) const fn ui_name(self) -> &'static str { |
| 2923 | `src/spirit/Spirit_VFX.rs:499` | associated items `ui_name`, `wire_name`, and `from_wire_name` are never used | const fn wire_name(self) -> &'static str { |
| 2923 | `src/spirit/Spirit_VFX.rs:513` | associated items `ui_name`, `wire_name`, and `from_wire_name` are never used | fn from_wire_name(name: &str) -> Option<Self> { |
| 2924 | `src/spirit/Spirit_VFX.rs:538` | associated items `ui_name`, `wire_name`, and `from_wire_name` are never used | pub(crate) const fn ui_name(self) -> &'static str { |
| 2924 | `src/spirit/Spirit_VFX.rs:545` | associated items `ui_name`, `wire_name`, and `from_wire_name` are never used | const fn wire_name(self) -> &'static str { |
| 2924 | `src/spirit/Spirit_VFX.rs:552` | associated items `ui_name`, `wire_name`, and `from_wire_name` are never used | fn from_wire_name(name: &str) -> Option<Self> { |
| 2925 | `src/spirit/Spirit_VFX.rs:712` | associated items `preview_default` and `sanitize` are never used | pub(crate) fn preview_default() -> Self { |
| 2925 | `src/spirit/Spirit_VFX.rs:722` | associated items `preview_default` and `sanitize` are never used | fn sanitize(&mut self) { |
| 2926 | `src/spirit/Spirit_VFX.rs:778` | struct `SpiritVfxUiConfig` is never constructed | pub(crate) struct SpiritVfxUiConfig { |
| 2927 | `src/spirit/Spirit_VFX.rs:791` | struct `SpiritVfxUiTransform` is never constructed | pub(crate) struct SpiritVfxUiTransform { |
| 2928 | `src/spirit/Spirit_VFX.rs:801` | struct `SpiritVfxUiLayer` is never constructed | pub(crate) struct SpiritVfxUiLayer { |
| 2929 | `src/spirit/Spirit_VFX.rs:810` | struct `SpiritVfxUiParticles` is never constructed | pub(crate) struct SpiritVfxUiParticles { |
| 2930 | `src/spirit/Spirit_VFX.rs:820` | struct `SpiritVfxUiOutput` is never constructed | pub(crate) struct SpiritVfxUiOutput { |
| 2931 | `src/spirit/Spirit_VFX.rs:827` | enum `SpiritVfxUiConfigError` is never used | pub(crate) enum SpiritVfxUiConfigError { |
| 2932 | `src/spirit/Spirit_VFX.rs:843` | enum `SpiritVfxControlError` is never used | pub(crate) enum SpiritVfxControlError { |
| 2933 | `src/spirit/Spirit_VFX.rs:852` | associated items `from_control_panel` and `into_control_panel` are never used | pub(crate) fn from_control_panel(panel: SpiritVfxControlPanel) -> Self { |
| 2933 | `src/spirit/Spirit_VFX.rs:906` | associated items `from_control_panel` and `into_control_panel` are never used | pub(crate) fn into_control_panel( |
| 2934 | `src/spirit/Spirit_VFX.rs:981` | function `control_panel_ui_json` is never used | pub(crate) fn control_panel_ui_json() -> Result<String, SpiritVfxUiConfigError> { |
| 2935 | `src/spirit/Spirit_VFX.rs:987` | function `publish_control_panel_ui_json` is never used | pub(crate) fn publish_control_panel_ui_json(json: &str) -> Result<u64, SpiritVfxUiConfigError> { |
| 2936 | `src/spirit/Spirit_VFX.rs:1051` | constant `SPIRIT_BACKGROUND_PRESENT_SCALE` is never used | const SPIRIT_BACKGROUND_PRESENT_SCALE: f32 = |
| 2937 | `src/spirit/Spirit_VFX.rs:1196` | function `publish_control_panel` is never used | pub(crate) fn publish_control_panel(mut panel: SpiritVfxControlPanel) -> u64 { |
| 2938 | `src/spirit/Spirit_VFX.rs:1205` | function `select_cpp_repass` is never used | pub(crate) fn select_cpp_repass( |
| 2939 | `src/spirit/Spirit_VFX.rs:1242` | function `reset_cpp_repass` is never used | pub(crate) fn reset_cpp_repass() -> u64 { |
| 2940 | `src/spirit/Spirit_VFX.rs:1287` | function `set_rotation_degrees` is never used | pub(crate) fn set_rotation_degrees(degrees: f32) -> Result<u64, SpiritVfxControlError> { |
| 2941 | `src/spirit/Spirit_VFX.rs:1295` | function `rotate_left_degrees` is never used | pub(crate) fn rotate_left_degrees(degrees: f32) -> Result<u64, SpiritVfxControlError> { |
| 2942 | `src/spirit/Spirit_VFX.rs:1300` | function `rotate_right_degrees` is never used | pub(crate) fn rotate_right_degrees(degrees: f32) -> Result<u64, SpiritVfxControlError> { |
| 2943 | `src/spirit/Spirit_VFX.rs:1304` | function `rotation_degrees` is never used | pub(crate) fn rotation_degrees() -> f32 { |
| 2944 | `src/spirit/Spirit_VFX.rs:1310` | function `set_edge_fade_pixels` is never used | pub(crate) fn set_edge_fade_pixels(pixels: f32) -> Result<u64, SpiritVfxControlError> { |
| 2945 | `src/spirit/Spirit_VFX.rs:1502` | function `bounded` is never used | fn bounded(value: f32, min: f32, max: f32, fallback: f32) -> f32 { |
| 2946 | `src/spirit/Spirit_VFX.rs:1510` | function `validate_rotation_delta` is never used | fn validate_rotation_delta(degrees: f32) -> Result<f32, SpiritVfxControlError> { |
| 2947 | `src/spirit/Spirit_VFX.rs:1520` | function `rotate_relative_degrees` is never used | fn rotate_relative_degrees(delta: f32) -> Result<u64, SpiritVfxControlError> { |
| 2948 | `src/spirit/Spirit_VFX.rs:1530` | function `update_rotation_degrees` is never used | fn update_rotation_degrees(degrees: f32) -> u64 { |
| 2949 | `src/spirit/Spirit_VFX.rs:1539` | function `canonical_rotation_degrees` is never used | fn canonical_rotation_degrees(degrees: f32) -> f32 { |
| 2950 | `src/spirit/Spirit_VFX.rs:1544` | function `css_color` is never used | fn css_color(color: SpiritVfxRgb8) -> String { |
| 2951 | `src/spirit/Spirit_VFX.rs:1548` | function `parse_css_color` is never used | fn parse_css_color(value: &str) -> Result<SpiritVfxRgb8, SpiritVfxUiConfigError> { |
| 2952 | `src/spirit/Spirit_VFX.rs:1565` | function `hex_nibble` is never used | const fn hex_nibble(value: u8) -> Option<u8> { |
| 2858 | `src/spirit/gpu_logger.rs:17` | constant `MIN_TTL_MS` is never used | const MIN_TTL_MS: u64 = 1_000; |
| 2859 | `src/spirit/gpu_logger.rs:18` | constant `MAX_TTL_MS` is never used | const MAX_TTL_MS: u64 = 300_000; |
| 2860 | `src/spirit/gpu_logger.rs:32` | struct `GpuLoggerLease` is never constructed | pub(crate) struct GpuLoggerLease { |
| 2861 | `src/spirit/gpu_logger.rs:72` | struct `BusyStatus` is never constructed | pub(crate) struct BusyStatus { |
| 2862 | `src/spirit/gpu_logger.rs:79` | struct `GpuLoggerStatus` is never constructed | pub(crate) struct GpuLoggerStatus { |
| 2863 | `src/spirit/gpu_logger.rs:106` | field `next_generation` is never read | next_generation: u64, |
| 2864 | `src/spirit/gpu_logger.rs:132` | multiple methods are never used | fn store_latest_sample(&mut self, source: GpuLoggerSource, sample: GpuLoggerSample) { |
| 2864 | `src/spirit/gpu_logger.rs:149` | multiple methods are never used | fn next_generation(&mut self) -> u64 { |
| 2864 | `src/spirit/gpu_logger.rs:154` | multiple methods are never used | fn request_at( |
| 2864 | `src/spirit/gpu_logger.rs:187` | multiple methods are never used | fn release_at(&mut self, lease: GpuLoggerLease, now_ms: u64) -> bool { |
| 2864 | `src/spirit/gpu_logger.rs:199` | multiple methods are never used | fn stop_source_at(&mut self, source: GpuLoggerSource, now_ms: u64) -> bool { |
| 2864 | `src/spirit/gpu_logger.rs:208` | multiple methods are never used | fn publish_at( |
| 2864 | `src/spirit/gpu_logger.rs:238` | multiple methods are never used | fn status_at(&mut self, now_ms: u64) -> GpuLoggerStatus { |
| 2865 | `src/spirit/gpu_logger.rs:262` | function `request` is never used | pub(crate) fn request(source: GpuLoggerSource, ttl_ms: u64) -> Result<GpuLoggerLease, BusyStatus> { |
| 2866 | `src/spirit/gpu_logger.rs:267` | function `release` is never used | pub(crate) fn release(lease: GpuLoggerLease) -> bool { |
| 2867 | `src/spirit/gpu_logger.rs:274` | function `stop_source` is never used | pub(crate) fn stop_source(source: GpuLoggerSource) -> bool { |
| 2868 | `src/spirit/gpu_logger.rs:281` | function `publish` is never used | pub(crate) fn publish(source: GpuLoggerSource, sample: GpuLoggerSample) -> bool { |
| 2869 | `src/spirit/gpu_logger.rs:285` | function `status` is never used | pub(crate) fn status() -> GpuLoggerStatus { |
| 2870 | `src/spirit/intel_cursor.rs:14` | constant `SPIRIT_CURSOR_RADIUS` is never used | const SPIRIT_CURSOR_RADIUS: i32 = 10; |
| 2871 | `src/spirit/intel_cursor.rs:15` | constant `SPIRIT_CURSOR_HOTSPOT` is never used | const SPIRIT_CURSOR_HOTSPOT: i32 = (SPIRIT_CURSOR_DIM / 2) as i32; |
| 2872 | `src/spirit/intel_cursor.rs:115` | function `spirit_cursor_local_point` is never used | pub(super) fn spirit_cursor_local_point( |
| 2873 | `src/spirit/intel_cursor.rs:318` | function `spirit_cursor_draw_solid_circle` is never used | pub(super) fn spirit_cursor_draw_solid_circle( |
| 2874 | `src/spirit/intel_cursor.rs:777` | function `normalized_screen_point_to_px` is never used | fn normalized_screen_point_to_px(normalized: f64, extent: u32) -> i32 { |
| 2875 | `src/spirit/intel_cursor.rs:795` | function `draw_solid_circle` is never used | fn draw_solid_circle(surface: SpiritCursorSurface, color: u32) { |
| 2876 | `src/spirit/lilly.rs:67` | method `part` is never used | pub(crate) fn part(self, semantic: &str) -> Option<LillyResidentFrame> { |
| 2877 | `src/spirit/lilly.rs:135` | field `0` is never read | Archive(crate::r::tar::Error), |
| 2878 | `src/spirit/lilly.rs:139` | field `0` is never read | PngDecode(i32), |
| 2879 | `src/spirit/lilly.rs:141` | field `0` is never read | Catalog(&'static str), |
| 2880 | `src/spirit/lilly.rs:144` | field `0` is never read | Resident(&'static str), |
| 2881 | `src/spirit/lilly.rs:198` | function `resident_frame` is never used | pub(crate) fn resident_frame(name: &str) -> Option<LillyResidentFrame> { |
| 2882 | `src/spirit/lilly.rs:223` | function `resident_part` is never used | pub(crate) fn resident_part(key: &str, semantic: &str) -> Option<LillyResidentFrame> { |
| 2883 | `src/spirit/lilly_protocol.rs:23` | constant `MAX_JSON_BYTES` is never used | const MAX_JSON_BYTES: usize = 4 * 1024; |
| 2884 | `src/spirit/lilly_protocol.rs:241` | variants `InvalidJson` and `PackageTooLarge` are never constructed | InvalidJson, |
| 2884 | `src/spirit/lilly_protocol.rs:244` | variants `InvalidJson` and `PackageTooLarge` are never constructed | PackageTooLarge, |
| 2885 | `src/spirit/lilly_protocol.rs:461` | methods `clear` and `replace` are never used | fn clear(&mut self) -> usize { |
| 2885 | `src/spirit/lilly_protocol.rs:469` | methods `clear` and `replace` are never used | fn replace(&mut self, packages: &[SpiritPackage]) -> Result<usize, LillyProtocolError> { |
| 2886 | `src/spirit/lilly_protocol.rs:514` | function `enqueue_package` is never used | pub(crate) fn enqueue_package(package: SpiritPackage) -> Result<usize, LillyProtocolError> { |
| 2887 | `src/spirit/lilly_protocol.rs:531` | function `enqueue_json` is never used | pub(crate) fn enqueue_json(json: &str) -> Result<usize, LillyProtocolError> { |
| 2888 | `src/spirit/lilly_protocol.rs:571` | function `enqueue_animation` is never used | pub(crate) fn enqueue_animation(tag: &str, rgba: SpiritRgba8) -> Result<usize, LillyProtocolError> { |
| 2889 | `src/spirit/lilly_protocol.rs:584` | function `enqueue_text` is never used | pub(crate) fn enqueue_text(text: &str) -> Result<usize, LillyProtocolError> { |
| 2890 | `src/spirit/lilly_protocol.rs:680` | function `replace_timeline` is never used | pub(crate) fn replace_timeline(packages: &[SpiritPackage]) -> Result<usize, LillyProtocolError> { |
| 2891 | `src/spirit/lilly_protocol.rs:690` | function `stop_after_boundary` is never used | pub(crate) fn stop_after_boundary() -> usize { |
| 2845 | `src/spirit/mod.rs:96` | struct `SpiritColor` is never constructed | pub(crate) struct SpiritColor(u32); |
| 2846 | `src/spirit/mod.rs:100` | associated items `STARTUP` and `rgba` are never used | pub(crate) const STARTUP: Self = Self::rgba(0x52, 0xD6, 0xFF, 0xFF); |
| 2846 | `src/spirit/mod.rs:102` | associated items `STARTUP` and `rgba` are never used | pub(crate) const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self { |
| 2847 | `src/spirit/mod.rs:117` | struct `SpiritCommandStream` is never constructed | pub(crate) struct SpiritCommandStream { |
| 2848 | `src/spirit/mod.rs:125` | associated function `centered` is never used | pub(crate) const fn centered(color: SpiritColor) -> Self { |
| 2849 | `src/spirit/mod.rs:149` | associated constant `CPU_AND_GPU` is never used | pub(crate) const CPU_AND_GPU: Self = Self(Self::VALID_BITS); |
| 2850 | `src/spirit/mod.rs:176` | methods `is_complete` and `wait` are never used | pub(crate) fn is_complete(self) -> bool { |
| 2850 | `src/spirit/mod.rs:180` | methods `is_complete` and `wait` are never used | pub(crate) async fn wait(self) { |
| 2851 | `src/spirit/mod.rs:203` | method `fence` is never used | pub(crate) const fn fence(self) -> SpiritFence { |
| 2852 | `src/spirit/mod.rs:240` | variant `External` is never constructed | External, |
| 2853 | `src/spirit/mod.rs:287` | methods `is_complete` and `wait` are never used | pub(crate) fn is_complete(self) -> bool { |
| 2853 | `src/spirit/mod.rs:291` | methods `is_complete` and `wait` are never used | pub(crate) async fn wait(self) { |
| 2854 | `src/spirit/mod.rs:366` | function `acquire_frame` is never used | pub(crate) fn acquire_frame( |
| 2855 | `src/spirit/mod.rs:793` | function `move_spirit_by` is never used | pub(crate) fn move_spirit_by( |
| 2856 | `src/spirit/mod.rs:805` | function `set_position` is never used | pub(crate) fn set_position( |
| 2857 | `src/spirit/mod.rs:846` | function `submit` is never used | pub(crate) fn submit( |
| 2892 | `src/spirit/response_window.rs:121` | struct `ReasoningResponseStream` is never constructed | pub(crate) struct ReasoningResponseStream { |
| 2893 | `src/spirit/response_window.rs:133` | method `update` is never used | pub(crate) fn update(&self, text_prefix: &str) -> bool { |
| 2894 | `src/spirit/response_window.rs:141` | method `finish` is never used | pub(crate) fn finish(mut self, text: &str) -> bool { |
| 2895 | `src/spirit/response_window.rs:148` | method `turn` is never used | pub(crate) const fn turn(&self) -> u64 { |
| 2896 | `src/spirit/response_window.rs:216` | function `begin_reasoning_response` is never used | pub(crate) fn begin_reasoning_response(turn: u64) -> Option<ReasoningResponseStream> { |
| 2897 | `src/spirit/response_window.rs:259` | function `update_response_prefix` is never used | fn update_response_prefix(id: u64, text_prefix: &str) -> bool { |
| 2898 | `src/spirit/response_window.rs:283` | function `finish_response` is never used | fn finish_response(id: u64, text: &str) -> bool { |
| 2899 | `src/spirit/response_window.rs:305` | function `abort_response` is never used | fn abort_response(id: u64) { |
