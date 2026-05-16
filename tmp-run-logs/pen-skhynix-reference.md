# pen.rs SK hynix / UAS Removal Reference

This note preserves the shape of the SK hynix-specific USB mass-storage code removed from `src/usb2/device/pen.rs`.

## Removed runtime surface

- `MassIoProfile::UasSkhynix`
- `UsbMassEndpoints::UasSkhynix { command_out, status_in, data_in, data_out }`
- UAS stream bookkeeping in `UsbMassRuntime`: next stream tag, dead stream mask, stream fault count
- SK hynix disk lookup helpers: `is_uas_skhynix_disk`, `find_uas_skhynix_route_disk`

## Removed benchmark and route APIs

- Constants: UAS pipe IDs, UAS bench default total/chunk/inflight, status bytes, tick, flight timeout
- Bench types: `UasBenchConfig`, `UasBenchProgress`, `UasBenchStats`, `UasBenchFlight`, `UasBenchStep`
- Route types: `UasRoutePhase`, `UasRouteTiming`, `UasRouteCounters`, `UasRouteProbeKind`, `UasRouteProbeConfig`, `UasRouteProbeResult`
- Route and bench entry points:
  - `set_uas_skhynix_route_window`
  - `set_uas_skhynix_write_window_for_bench`
  - `reset_uas_skhynix_route_transport`
  - `reset_uas_skhynix_transport_for_bench`
  - `run_uas_skhynix_route_probe`
  - `run_uas_skhynix_stream_bench`

## Removed transport execution

- UAS descriptor/pipe-role refinement helpers used by the SK hynix task
- UAS stream allocation, retirement, and exhaustion logging helpers
- SK hynix UAS read/write/flush branches inside the block device implementation
- UAS keepalive branch in the BOT mass-storage lifecycle
- `mass_storage_uas_skhynix_task`
- The legacy `is_skhynix_pssd_x31(0x152E, 0x7001)` special-case that ignored the device for old handoff behavior

## Step 2 impact candidates

These external users will need pruning or replacement after this `pen.rs` cut:

- `src/shell2/cmds/bench.rs`: UAS SK hynix bench selection, write-window, reset, and stream bench calls
- `src/usb2/device/uas_skhynix_route_probe.rs`: route probe config/result imports and route/reset/window calls
- any module that spawns or references `mass_storage_uas_skhynix_task`
