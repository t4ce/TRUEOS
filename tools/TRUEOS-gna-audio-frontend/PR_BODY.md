## Summary

Add a resident `gna-audio-front-end` system service for the first bare-metal
speech-precondition milestone:

```text
HDA microphone -> GNA 3.0 -> noise reduction -> VAD -> wake word -> speech detected
```

The service is registered in the central spawn registry and assigned to an
efficiency/background worker. It starts independently of endpoint readiness so
GNA/HDA discovery failures remain visible during bring-up.

## Behavior

- Polls its observable boundary every 100 ms.
- Reports GNA 3.0 PCI presence for Intel ADL/RPL IDs, including BDF and class.
- Reports HDA input-stream, ADC, microphone-pin, line-input-pin, and capture-DMA state.
- Logs endpoint records only initially or when hardware state changes.
- Exposes lock-free latest-value publication hooks for a later GNA provider.
- Logs VAD only when the observed active/silent state changes.
- Logs wake detections at most once per 250 ms and accounts for suppressed detections.
- Uses `Important` service logs so milestones survive the normal `Up(Warn)` service policy.

## Deliberate non-claims

This PR does not claim the GNA device, map MMIO, configure descriptors/page
tables/interrupts, load a model, configure HDA input DMA, or claim successful
neural inference. The current HDA path is expected to report
`state=discovery-only capture_dma=0`.

## Validation

- Patch structure checked with `git apply --check` against the current registry contexts.
- Pure policy coverage included for mailbox atomicity, the exact 250 ms boundary,
  explicit GNA 3.0 IDs, and VAD payload decoding.
- Bare-metal acceptance is documented in `docs/GNA_AUDIO_FRONTEND_BRINGUP.md`.

## Base

Prepared against `true` at `725095bfcbe5a159feb4731e9ee118eb838a9d6f`.
