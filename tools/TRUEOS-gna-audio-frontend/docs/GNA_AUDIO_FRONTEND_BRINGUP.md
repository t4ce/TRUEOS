# GNA audio front-end: milestone 1 bring-up

## Purpose

This milestone establishes one resident system service for the low-power speech
precondition path:

```text
HDA microphone input
        |
        v
Intel GNA 3.0
  - noise reduction
  - voice-activity detection
  - wake-word detection
        |
        v
speech detected
```

The service owns scheduling, signal coalescing, and important-log policy. The
HDA driver remains responsible for PCM capture. A later GNA driver remains
responsible for MMIO, model residency, inference, and publishing VAD/wake
results into the service.

## What this change proves

The central registry starts `gna-audio-front-end` once a background worker is
available. GNA and HDA are deliberately not hidden registry gates: the service
reports each endpoint as present, unavailable, or changed, which makes failed
hardware discovery visible during bring-up.

On supported bare metal, the service independently validates and reports:

- the exact GNA PCI BDF, device ID, class, and subclass;
- HDA input-stream, ADC-widget, microphone-pin, and line-input-pin discovery;
- the current HDA capture-DMA truth value;
- VAD state changes at a 100 ms observation cadence;
- wake-word detections with a 250 ms important-log soft cap.

The VAD and wake producer interface is lock-free and latest-value based. A GNA
producer may score at its model-native cadence; unchanged or superseded samples
do not create an unbounded service queue.

## What this change does not claim

A successful service start is not yet successful neural inference. This
milestone does not:

- claim or map the GNA PCI function;
- configure GNA page tables, descriptors, interrupts, or scoring;
- upload a noise-reduction, VAD, or wake-word model;
- configure HDA input DMA;
- emit cleaned PCM;
- fabricate VAD or wake-word results.

The checked-in HDA capability currently reports `capture_dma=0`. Therefore the
expected first hardware state is `discovery-only`, not `capture-ready`.

## Expected important logs

A successful first boot on the Raptor Lake target should contain one instance
of each unchanged endpoint record:

```text
gna-audio-front-end: online contract=hda-microphone>gna3>noise-reduction>vad>wake-word>speech-detected poll_softcap_ms=100 wake_log_softcap_ms=250 event_policy=transition-or-softcap inference_provider=awaiting-driver
gna-audio-front-end: endpoint=gna3-pci state=present generation=3.0 bdf=00:08.0 vendor=0x8086 device=0xA74F class=0x08 subclass=0x80 ownership=probe-only service_mmio=not-configured
gna-audio-front-end: endpoint=hda-capture state=discovery-only input_streams=<n> adc_widgets=<n> microphone_pins=<n> line_input_pins=<n> capture_dma=0
```

The BDF and HDA topology counts are machine-derived and may differ. The service
can briefly report `missing` or `unavailable` if it starts before device setup;
the stable endpoint state is the acceptance evidence. An HDA record that
remains `topology-incomplete` fails the microphone-discovery part of this
milestone.

Once the hardware producer is connected, observed signal logs are:

```text
gna-audio-front-end: signal=voice-activity state=active speech_detected=1 confidence_milli=<0..1000> coalesced_updates=<n> source=gna-provider
gna-audio-front-end: signal=voice-activity state=silent speech_detected=0 confidence_milli=<0..1000> coalesced_updates=<n> source=gna-provider
gna-audio-front-end: signal=wake-word word_id=<model-id> confidence_milli=<0..1000> suppressed_since_previous=<n> source=gna-provider
```

## Bare-metal acceptance

1. Build with the repository's normal kernel build path and confirm the new
   service introduces no warnings or task-count assertion failure.
2. Boot the i5-14500T target and inspect the central system-service snapshot.
   `gna-audio-front-end` must show `enabled=1`, `gate_open=1`, and `started=1`;
   its only registry prerequisite is a background worker.
3. Confirm the stable GNA endpoint reports `state=present`, `generation=3.0`, and the
   enumerated PCI identity.
4. Confirm the HDA endpoint reports at least one input stream, ADC widget, and
   microphone or line-input pin. For this milestone, `capture_dma=0` is honest
   and expected.
5. Observe at least ten seconds of idle runtime. The unchanged GNA and HDA
   endpoint records must not repeat every 100 ms.
6. After the GNA producer is implemented, publish actual alternating VAD states.
   Only observed state transitions may log, with no more than one service
   observation per 100 ms.
7. Publish a burst of actual wake detections. Important wake logs must be at
   least 250 ms apart; the next admitted record must account for suppressed
   detections.

## Next hardware boundary

The GNA driver should publish completed model results only after descriptor and
output validation:

```rust
publish_voice_activity(VoiceActivity::Active, confidence_milli)?;
publish_wake_word(model_word_id, confidence_milli)?;
```

Neither function performs hardware access. This keeps interrupt completion and
model validation in the device layer while the service remains the sole owner
of cadence and global-log policy.
