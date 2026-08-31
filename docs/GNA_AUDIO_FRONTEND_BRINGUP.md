# GNA audio front-end service bring-up

## Milestone scope

`gna-audio-front-end` establishes the long-lived service boundary for:

```text
HDA microphone
  -> Intel GNA 3.0
       -> noise-reduction state
       -> voice-activity state
       -> wake-word event
  -> speech-detected handoff
```

This milestone does **not** claim the GNA PCI function, program a model, start an
HDA input stream, or generate synthetic detection results. It remains in
`awaiting-gna` until a later hardware owner explicitly publishes state and
observations.

The central service registry admits the task only after both
`INTEL_HDA_READY` and `BACKGROUND_AP_WORKER_READY`. The task is assigned through
the efficiency-core-preferred background-worker selector.

## Observation and logging contract

The hardware/model owner may publish:

- pipeline lifecycle (`awaiting-gna`, `awaiting-model`, `ready`, `streaming`, or
  `faulted`);
- noise-reduction active/inactive level plus Q15 confidence;
- voice-activity active/inactive level plus Q15 confidence;
- a numeric wake-word identifier plus Q15 confidence.

Publication is allocation-free and lock-free. A concurrent publisher loses
admission and receives `false`; the stable observation is never exposed with
partially updated fields.

The service samples observations at a 100 ms soft cadence. Noise reduction and
VAD are logged only when the observed boolean level changes. Wake-word logs are
limited to one Important service record per 250 ms. Bursts retain the latest
wake event and report the number of coalesced events.

No 100 ms heartbeat is emitted. Normal idle operation produces only the
startup marker and one cadence measurement, avoiding log-volume coupling to
the inference frame rate.

## Bare-metal acceptance evidence

A fresh boot for this milestone should establish all of the following:

1. The system-service snapshot contains `gna-audio-front-end` with requirements
   `INTEL_HDA_READY|BACKGROUND_AP_WORKER_READY`.
2. The system-service snapshot reports `started=1` after HDA and a background
   worker become ready.
3. One Important record reports the path
   `hda-microphone->gna3(noise-reduction,vad,wake-word)->speech-detected`,
   `poll_softcap_ms=100`, `wake_log_softcap_ms=250`, and `fail_closed=1`.
4. After ten service intervals, one Important `baremetal=poll-cadence` record
   reports observed minimum, maximum, and average interval lengths.
5. With no hardware publisher connected, there are no noise, VAD, wake-word,
   ready, or streaming claims.

After the HDA/GNA owner is connected, acceptance extends with:

1. A VAD transition produces exactly one `event=voice-activity state=on`
   record; continued active observations do not repeat it.
2. The return to silence produces exactly one
   `event=voice-activity state=off` record.
3. Wake-word records are separated by at least 250 ms of service uptime; bursts
   report `coalesced=N` rather than flooding the global log sinks.
4. Noise-reduction level changes follow the same edge-only rule.
5. Pipeline readiness appears only after authenticated model admission and a
   successful hardware bring-up.

Actual GNA/HDA inference validation belongs to the hardware-owner milestone;
this checklist prevents that later work from bypassing the service, cadence,
and logging boundary established here.
