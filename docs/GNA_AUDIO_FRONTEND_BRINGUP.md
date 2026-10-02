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

The original service-boundary milestone did not start an HDA input stream.
The current implementation starts and observes the separate HDA-owned capture
lane. It still does **not** claim the GNA PCI function, program a model, or
generate synthetic detection results. Inference remains in `awaiting-gna`
until a later hardware/model owner explicitly publishes state and observations.

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
   `hda-capture->gna3(noise-reduction,vad,wake-word)->speech-detected`,
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

## Shell2 microphone recording acceptance

`rec` appears beside `img shot vid film cam` in Media. In Default mode:

- `rec` records until stopped.
- `rec 1` records for one minute; integer durations from 1 through 10 are accepted,
  matching `film`'s units.
- `rec stop` or `§rec§` stops and saves completed audio, including the tail.

The exact Matrix slot `rec` reports progress and the final file path. Leaving
that slot or closing the invoking frontend does not stop recording. Deleting
or interrupting the slot stops recording; its lifetime prevents late output
from reaching a replacement slot. Only one microphone recording runs at a time.
`rec stop` addresses that single recorder, including when invoked from another
shell. Screen recording and microphone recording use separate capture hardware.

Output is a standard RIFF WAV containing the capture lane's native 48 kHz,
signed 16-bit PCM, with one or two interleaved channels. Files are written to
the preferred writable TRUEOSFS root at
`recordings/microphone-<unix-seconds>-<monotonic-nanoseconds>.wav`.
The command waits up to ten seconds for microphone readiness; recording duration
starts when capture becomes ready. It does not publish GNA/VAD/wake-word results.

To verify on hardware:

1. Run `rec`, remain silent briefly, then speak and tap near the microphone.
2. Check that the `rec` slot reports advancing frames and changing peak/nonzero
   statistics. These statistics are measured PCM levels, not speech detection.
3. Run `rec stop` or `§rec§` and check the reported saved WAV path.
4. Retrieve that exact WAV from TRUEOSFS and play it with a WAV-capable player.
   Confirm intelligible speech, correct speed, both channels where present,
   and the last words before stop. A saved all-zero file is not microphone proof.
5. Run `rec 1` to verify automatic completion; recording should play for roughly
   one minute. Repeat while `film` runs to check the combined workload.

The recorder uses an independent sequential cursor on completed HDA DMA frames;
it never consumes another reader's samples or reconfigures capture/playback.
A capture restart, a long read/poll gap, or an unread backlog approaching the DMA ring capacity stops
recording and saves the valid prefix with the reason reported. It does not
silently duplicate, skip, or join samples across that discontinuity.

TRUEOSFS needs a known final length, so bounded raw PCM `.wav.part000000` chunks
are committed during recording, then pinned reads assemble the WAV with its
final header. Chunks are removed only after commit and length verification.
Finalization needs space for a second copy, and the append-only filesystem does
not reclaim deleted chunk records immediately. If saving fails, retain the
reported chunks from that exact recording in numeric order; they are raw
`s16le`, 48000 Hz, with the channel count reported in the error. Do not substitute
chunks from another recording. An I/O error can prevent saving the buffered tail.
Indefinite recording also stops before the classic RIFF 32-bit size limit.

Host regression coverage: `python3 tools/testpy/test_audio_recording.py`.
Bare-metal capture and listening acceptance remain to be performed.
