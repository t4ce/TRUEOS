# trueos-ttstt

`trueos-ttstt` is one local Rust CLI for both directions:

- `trueos-ttstt tts` / `trueos-ttstt speak`: text to Kokoro speech, played and discarded by
  default or optionally written to a WAV file, through a local patch of
  [`tts-rs`](https://docs.rs/tts-rs/latest/tts_rs/)
- `trueos-ttstt stt` / `trueos-ttstt transcribe`: a WAV file, or continuous raw PCM from
  stdin when no file is given, to text through
  [`whisper-rs`](https://docs.rs/whisper-rs/latest/whisper_rs/), using the same
  Whisper backend and transcription result types as
  [`transcribe-rs`](https://docs.rs/transcribe-rs/latest/transcribe_rs/)
- `trueos-ttstt listen` / `trueos-ttstt stream`: compatibility commands for the same
  continuous raw-PCM mode
- `trueos-ttstt serve`: a persistent localhost service that keeps both models warm
  for applications such as `txt`

By default no speech leaves the host or is sent to a cloud service. Model files
must be downloaded once and are then loaded locally.

This compatibility CLI is ordinary TRUEOS source, not a submodule and not a
member of the kernel's default build. Normal `make`, `make kernel`, and release
builds do not compile it for Linux. Use the explicit host targets below when
the legacy combined STT/TTS utility is needed.

## Requirements

- Rust 1.94 or newer
- CMake, a C/C++ compiler, and LLVM/libclang (for `whisper-rs`)
- `espeak-ng` on `PATH` (for Kokoro phonemization)
- `aplay` from `alsa-utils`, or `ffplay` from FFmpeg, for default TTS playback
  (`--output FILE` works without an audio player)
- A raw PCM producer such as `arecord` from `alsa-utils` for continuous STT
  (optional when another application supplies the PCM)
- Network access during the first build so `ort` can obtain ONNX Runtime, unless
  it is already supplied by your build environment

Typical Debian/Ubuntu setup:

```sh
sudo apt-get install alsa-utils build-essential clang cmake espeak-ng libclang-dev libssl-dev pkg-config
```

Typical macOS setup:

```sh
brew install cmake espeak-ng ffmpeg llvm
```

From the TRUEOS repository root, preserve the Ubuntu build explicitly:

```sh
make trueos-ttstt-ubuntu
tools/trueos-ttstt/target/x86_64-unknown-linux-gnu/release/trueos-ttstt --version
```

This passes `--target x86_64-unknown-linux-gnu` deliberately, overriding the
TRUEOS root Cargo configuration whose default is the bare-metal kernel target.
For the current host triple on another supported platform, run
`make trueos-ttstt-host`. Both targets keep output below the ignored
`tools/trueos-ttstt/target/` directory. A global-style install remains
possible when its target is equally explicit:

```sh
cargo install --path tools/trueos-ttstt --locked \
  --target x86_64-unknown-linux-gnu
```

The examples below use `trueos-ttstt` for readability. Without an install, use
the executable path printed by the selected Make target.

## Models

The model weights are separate from host packages and from the Rust binary.
Installing `espeak-ng`, Clang, or `trueos-ttstt` does not install them.

TRUEOS keeps the two pinned Kokoro AOT inputs together under the ignored
`tools/ttstt/models/kokoro/` tooling directory. The host utility's quantized
source model, Whisper model, and generated runtime artifacts remain in its
ignored `.ttstt/` cache. From the TRUEOS repository root, populate the source
downloads with:

```sh
HOST_MODELS="$PWD/tools/trueos-ttstt/.ttstt/models"
AOT_MODELS="$PWD/tools/ttstt/models/kokoro"
mkdir -p "$HOST_MODELS/kokoro" "$HOST_MODELS/whisper" "$AOT_MODELS"

curl -fL \
  https://github.com/taylorchu/kokoro-onnx/releases/download/v0.2.0/kokoro-quant-convinteger.onnx \
  -o "$HOST_MODELS/kokoro/kokoro-quant-convinteger.onnx"

curl -fL \
  https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin \
  -o "$AOT_MODELS/voices-v1.0.bin"

curl -fL \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin \
  -o "$HOST_MODELS/whisper/ggml-base.bin"
```

Prepare the pinned RTen graph beside its voice archive:

```sh
python3 -m venv tools/trueos-ttstt/.venv-rten-bridge
tools/trueos-ttstt/.venv-rten-bridge/bin/pip install 'onnx>=1.16,<2' numpy
tools/trueos-ttstt/.venv-rten-bridge/bin/python \
  tools/trueos-ttstt/tools/prepare_kokoro_rten.py \
  tools/trueos-ttstt/.ttstt/models/kokoro/kokoro-quant-convinteger.onnx \
  tools/ttstt/models/kokoro/kokoro-rten.onnx
```

The pinned shared inputs are:

| Input | Bytes | SHA-256 |
| --- | ---: | --- |
| `kokoro-rten.onnx` | 124,604,222 | `239d9f4df112a375bea52146570b97eb5c5af727c007761ee121ed123fd1ab29` |
| `voices-v1.0.bin` | 28,214,398 | `bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d` |

Verify the pair after obtaining or regenerating it:

```sh
sha256sum tools/ttstt/models/kokoro/kokoro-rten.onnx \
  tools/ttstt/models/kokoro/voices-v1.0.bin
```

Then synthesize locally:

```sh
trueos-ttstt tts "Hello from Rust"

# Or preserve the generated speech as a file instead of playing it:
trueos-ttstt tts -o speech.wav "Hello from Rust"
```

The resulting ignored layout is:

```text
tools/
├── ttstt/models/kokoro/
│   ├── kokoro-rten.onnx
│   └── voices-v1.0.bin
└── trueos-ttstt/.ttstt/models/
    ├── kokoro/
    │   ├── kokoro-quant-convinteger.onnx
    │   ├── kokoro.kkaot                  # generated TRUEOS runtime program
    │   ├── en.g2p                        # generated frontend model
    │   └── misaki-us.klex                # generated pronunciation overlay
    └── whisper/ggml-base.bin
```

### Native Rust RTen reference

ONNX Runtime remains the default and numerical oracle. A second CPU path runs
the graph through RTen, without Burn or ONNX Runtime inference. Prepare its
graph once from the same quantized model using the commands above. To
regenerate it manually:

```sh
tools/trueos-ttstt/.venv-rten-bridge/bin/python \
  tools/trueos-ttstt/tools/prepare_kokoro_rten.py \
  tools/trueos-ttstt/.ttstt/models/kokoro/kokoro-quant-convinteger.onnx \
  tools/ttstt/models/kokoro/kokoro-rten.onnx

trueos-ttstt tts --backend rten -o speech.wav "Hello from Rust"
```

The bridge validates the expected graph before changing it. It converts the
six unsupported `DynamicQuantizeLSTM` nodes to standard float LSTMs and expands
one `FusedMatMul`; the 148 integer matrix multiplies and 87 integer convolutions
remain quantized. Consequently RTen is a reproducible structural and
performance reference, but its waveform is not expected to be sample-identical
to ONNX Runtime's dynamically quantized LSTM result.

For deterministic backend comparisons, bypass espeak-ng and supply Kokoro IPA
directly:

```sh
RTEN_TIMING=1 trueos-ttstt tts --backend rten --phonemes \
  -o rten.wav 'həlˈoʊ fɹʌm ɹʌst'
trueos-ttstt tts --backend ort --phonemes \
  -o ort.wav 'həlˈoʊ fɹʌm ɹʌst'
```

The source path is embedded when this project is built, so the installed binary
finds the shared Kokoro inputs and host Whisper cache from any working
directory. Run `trueos-ttstt paths` to confirm both locations. Set `TTSTT_HOME`
only if you deliberately want one conventional model root containing both
`kokoro/` and `whisper/`; `TTSTT_TTS_MODEL_DIR` and `TTSTT_STT_MODEL` can
override the two sides independently.

Any compatible Whisper GGML model can be selected with `--model` or the
`TTSTT_STT_MODEL` environment variable. Multilingual models support automatic
language detection and `--translate`; files whose name ends in `.en.bin` are
English-only. Use `--model-dir` or `TTSTT_TTS_MODEL_DIR` for a different Kokoro
directory. Use `--model` or `TTSTT_STT_MODEL` for a different Whisper file.
Keep only one `.onnx` file in the Kokoro directory unless the desired file is
named `kokoro-quant-convinteger.onnx`.

Model files have their own licenses and terms; review those at their download
sources before redistributing them.

## Persistent localhost service

Applications that make repeated TTS or STT requests should run one persistent
service instead of starting a new inference process for each operation:

```sh
make trueos-ttstt-host
trueos-ttstt serve
```

The service loads Whisper and Kokoro concurrently, once, then listens on
`127.0.0.1:1100`. One worker owns Whisper for the lifetime of the service and
one worker owns Kokoro, so sequential STT sessions and queued TTS requests all
reuse the already-loaded models. The workers are independent: STT and TTS may
be active at the same time, although they still share CPU capacity. TTS audio
plays on the machine running the service; it is not sent back over TCP.

The loopback-only default is deliberate because protocol v1 has no
authentication or encryption. A different local address can be selected with
`--bind` or `TTSTT_BIND`:

```sh
trueos-ttstt serve --bind 127.0.0.1:1100
```

Binding to an address such as `0.0.0.0:1100` is supported and emits a warning,
but it exposes microphone input and speech playback control to the network.
Only do that behind an appropriate trusted boundary.

`serve` stays in the foreground, which lets a terminal, systemd, or another
service manager own its lifetime. For example, a transient user service can be
started and stopped with:

```sh
systemd-run --user --unit=trueos-ttstt --collect --property=Restart=on-failure \
  "$PWD/target/release/trueos-ttstt" serve
systemctl --user stop trueos-ttstt
```

The versioned Rust wire implementation is the `trueos-ttstt-protocol` workspace crate
under `protocol/`. Each TCP connection starts with `HELLO` / `HELLO_OK`, then
performs one operation:

- STT: `STT_OPEN`, `STT_READY`, zero or more raw-PCM frames, streamed immutable
  `STT_TEXT` records, and `STT_FINISH` or `STT_CANCEL` followed by `STT_DONE`.
- TTS: `TTS_SPEAK`, `TTS_ACCEPTED`, local synthesis/playback, then `TTS_DONE`.
- Any rejected request receives a typed `ERROR` record.

STT accepts the same headerless 16 kHz mono s16le stream as the CLI. There is
one active STT session at a time; a second receives `stt_busy`. `STT_FINISH`
transcribes pending speech, while `STT_CANCEL` discards it. Because Whisper
inference is synchronous, cancellation is observed after an in-flight window
returns, normally within a few seconds. An STT client can set
`detect_language: true` in `STT_OPEN`; otherwise its explicit `language` is
used. TTS cancellation is reserved but not implemented in protocol v1. The
standalone `tts`, file `stt`, and stdin-based continuous `stt` commands remain
available without the service.

## Text to speech

Text can come from arguments, a UTF-8 file, or standard input:

```sh
trueos-ttstt tts "Hello from Rust"
trueos-ttstt speak --voice bf_emma --speed 0.9 "Good morning"
trueos-ttstt tts --input script.txt
printf 'Piped text\n' | trueos-ttstt tts

# File output is the explicit, save-only path:
trueos-ttstt tts --output narration.wav --input script.txt
trueos-ttstt tts -o hello.wav "Keep this recording"
```

Without `--output`, audio remains in memory, is sent as raw mono float samples
to `aplay` (preferred) or `ffplay`, and is discarded after synchronous playback.
No WAV or temporary audio file is created. `--output FILE` bypasses the audio
device and writes the old mono, 24 kHz, 32-bit float WAV format instead, which
also makes it the right mode for a headless machine.

Kokoro produces short UI text in one blocking inference, so playback begins
when that waveform is ready. For text longer than 510 phonemes, the vendored
`tts-rs` patch preserves the existing 10 ms crossfade while a background player
consumes one completed model chunk during inference of the next. Long chunks
only prefer punctuation splits in the final fifth of their 510-phoneme window,
which avoids starving playback on an unusually early sentence break. List the
voices contained in the downloaded archive with:

```sh
trueos-ttstt tts --list-voices
```

If `espeak-ng` is bundled rather than installed globally, pass `--espeak-bin`
and `--espeak-data`. `--optimized-model-cache FILE` stores an optimized ONNX
graph and can make later model loads faster.

## Speech to text

Plain text is written to stdout, while progress goes to stderr:

```sh
trueos-ttstt stt recording.wav
trueos-ttstt transcribe --language de recording.wav
trueos-ttstt stt --translate interview.wav
trueos-ttstt -q stt recording.wav > transcript.txt
```

For files up to 10 seconds, the normalized 16 kHz sample count selects the
smallest fitting Whisper context: 256 tokens (5.12 seconds), 384 tokens (7.68
seconds), or 512 tokens (10.24 seconds). Longer files use Whisper's full model
context. Pass `--language CODE` when the language is known, because automatic
language detection can require an additional full-context encoder pass.

Structured and subtitle output is also available:

```sh
trueos-ttstt stt recording.wav --format json -o transcript.json
trueos-ttstt stt recording.wav --format jsonl
trueos-ttstt stt recording.wav --format srt -o captions.srt
trueos-ttstt stt recording.wav --format vtt -o captions.vtt
```

The CLI accepts integer PCM WAV (8/16/24/32-bit) and 32-bit float WAV, downmixes
multiple channels, and resamples to Whisper's 16 kHz mono input. Convert other
containers first, for example:

```sh
ffmpeg -i recording.mp3 recording.wav
```

A WAV produced by `trueos-ttstt tts` can therefore be transcribed directly:

```sh
trueos-ttstt tts -o speech.wav "A round trip test"
trueos-ttstt stt speech.wav
```

## Continuous speech to text

Bare `stt` is a long-lived process that reads headerless, signed 16-bit
little-endian PCM from stdin and writes committed text chunks to stdout. The
stream must be mono at 16 kHz. It does not open a microphone itself: the calling
application owns capture and feeds PCM, or a tool such as `arecord` can do so:

```sh
arecord -q -t raw -f S16_LE -r 16000 -c 1 | trueos-ttstt -q stt
```

`trueos-ttstt stt -`, `trueos-ttstt listen`, and `trueos-ttstt stream` select the same mode. A path
instead selects the existing one-shot mode: `trueos-ttstt stt recording.wav`.

The English language is selected by default so every window avoids automatic
language detection. Select another language explicitly, or pass
`--detect-language` to test automatic detection on every live window:

```sh
arecord -q -t raw -f S16_LE -r 16000 -c 1 | trueos-ttstt -q stt --language de
arecord -q -t raw -f S16_LE -r 16000 -c 1 | trueos-ttstt -q stt --detect-language
arecord -q -t raw -f S16_LE -r 16000 -c 1 | trueos-ttstt -q stt -o notes.txt
```

Detection costs additional work and is less reliable on very short utterances,
so an explicit language remains the faster production default.

The model is loaded once and the command runs until input ends or Ctrl+C is
pressed. A lightweight energy VAD commits ordinary utterances after a pause.
Uninterrupted speech is bounded to 10-second windows with three seconds of
acoustic overlap. A low-energy frame in that overlap becomes the ownership
boundary, and token timestamps assign every output token to exactly one
window; text is never stitched with suffix/prefix guessing. Use
`--vad-threshold` if the default `0.01` is too sensitive or not sensitive enough
for the microphone gain and room noise.

Plain mode emits one immutable, append-only fragment per line and flushes every
line immediately. A fragment is committed at a VAD pause or the forced
10-second boundary; after a forced window, the three-second overlap means the
next commit represents roughly seven seconds of new uninterrupted speech.

For a stable application protocol, use JSON Lines:

```sh
arecord -q -t raw -f S16_LE -r 16000 -c 1 | \
  trueos-ttstt -q stt --format jsonl
```

Each stdout line is a complete JSON object:

```json
{"sequence":0,"text":"Hello, my name is Jonas.","utterance_end":true}
```

`sequence` is zero-based, and `utterance_end` distinguishes a natural VAD pause
from a forced long-speech fragment. All records are final and are never revised.
Within one utterance, concatenate `text` fields verbatim; a later delta can
intentionally start with a space so words and punctuation reconstruct exactly.
An empty final record can close an utterance after an earlier non-final record.

### Calling continuous STT from another application

For a long-lived application, use `trueos-ttstt serve` and the shared
`trueos-ttstt-protocol` crate described above. Keep one full-duplex STT connection
open, send PCM frames as they arrive, and consume each `STT_TEXT` frame until
the application sends `STT_FINISH` or `STT_CANCEL`. This is the path used by
the sibling `txt` editor: `txt` keeps microphone capture local and the service
keeps Whisper warm across Ctrl-L sessions.

The original subprocess interface remains a useful CLI/pipe fallback. Spawn
`trueos-ttstt -q stt --format jsonl` with stdin and stdout piped, feed 16 kHz mono
s16le bytes to the child's stdin, and read its stdout line by line. Progress is
suppressed by `-q`; actual failures still go to stderr.

The process contract is:

1. PCM bytes enter child stdin.
2. One flushed transcript record at a time leaves child stdout.
3. Close child stdin for graceful cancellation. Pending speech is transcribed,
   the final record is flushed, stdout closes, and the child exits.
4. Kill the child only for immediate cancellation; a pending final fragment is
   not guaranteed after a hard kill.

No transcript file is created unless `--output FILE` is explicitly passed.
A lightweight input worker continues draining stdin while Whisper is running;
its bounded queue holds about four seconds of PCM. If inference falls that far
behind real time, stdin applies normal pipe backpressure rather than dropping
bytes inside `trueos-ttstt`.

## Execution

Both engines intentionally run on CPU. Whisper uses three decoding threads and
dynamic contexts up to 10.24 seconds for short files and live windows.
Continuous CLI mode adds one I/O-only stdin worker. Service mode instead uses
connection handlers plus a single model-owning worker for each engine; Kokoro
uses three ONNX Runtime CPU threads there by default. TTS playback is only an
audio-device write and does not introduce GPU inference.

Run `trueos-ttstt --help`, `trueos-ttstt tts --help`, `trueos-ttstt stt --help`, or
`trueos-ttstt listen --help` for every option.

## Development

```sh
cargo fmt --manifest-path tools/trueos-ttstt/Cargo.toml --all -- --check
cargo test --manifest-path tools/trueos-ttstt/Cargo.toml \
  --workspace --all-targets --target x86_64-unknown-linux-gnu \
  --target-dir tools/trueos-ttstt/target
cargo clippy --manifest-path tools/trueos-ttstt/Cargo.toml \
  --workspace --all-targets --target x86_64-unknown-linux-gnu \
  --target-dir tools/trueos-ttstt/target -- -D warnings
```

Unit and CLI tests do not require model downloads.

## Troubleshooting

### `trueos-ttstt: command not found`

Confirm that Cargo installed the binary, then add its directory to `PATH`:

```sh
ls -l "$HOME/.cargo/bin/trueos-ttstt"
export PATH="$HOME/.cargo/bin:$PATH"
hash -r
```

Unix shells also do not search the current directory for executables. Inside
`target/release`, use `./trueos-ttstt`, not just `trueos-ttstt`.

### `Kokoro model directory ... does not exist`

Installing the compiler packages and `espeak-ng` does not install model
weights. Download the two Kokoro files under [Models](#models), and use
`trueos-ttstt paths` to confirm where the CLI expects them. A custom location can be
selected with `TTSTT_HOME`, an absolute `--model-dir`, or
`TTSTT_TTS_MODEL_DIR`.

### No audio player or output device is available

Default TTS playback needs `aplay` or `ffplay` on `PATH` and a working default
audio device. On Debian/Ubuntu, install `alsa-utils`; on macOS, install FFmpeg.
Use `trueos-ttstt tts --output speech.wav "text"` to bypass playback completely. The
CLI reports playback failures rather than silently creating a file.

`aplay` is preferred when both programs exist. If it is installed but its ALSA
default device is unusable, select the other backend explicitly:

```sh
TTSTT_PLAYER=ffplay trueos-ttstt tts "Hello from Rust"
```

Automatic fallback only covers `aplay` not being found on `PATH`; the CLI does
not retry after a runtime failure because that could repeat partial speech.
