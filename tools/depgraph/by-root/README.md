# TRUEOS dependency graph split by root dependency

Source: `tools/docs/depgraph/trueos-depth-tree.txt`

Each SVG expands one direct dependency of the TRUEOS root. Blue note nodes are incoming cross-image edges; yellow note nodes are outgoing cross-image edges.

| Root dependency | SVG | Owned nodes | Input images | Output images |
| --- | --- | ---: | ---: | ---: |
| acpi<br>6.1.1<br>/vendor/acpi-6.1.1 | [`acpi-v6.1.1.svg`](acpi-v6.1.1.svg) | 9 | 10 | 1 |
| alsa<br>0.11.0<br>/vendor/alsa-0.11.0 | [`alsa-v0.11.0.svg`](alsa-v0.11.0.svg) | 2 | 12 | 1 |
| aml<br>0.16.4<br>/vendor/aml-0.16.4 | [`aml-v0.16.4.svg`](aml-v0.16.4.svg) | 7 | 0 | 1 |
| atomic-waker<br>1.1.2 | [`atomic-waker-v1.1.2.svg`](atomic-waker-v1.1.2.svg) | 1 | 1 | 0 |
| bytes<br>1.12.1 | [`bytes-v1.12.1.svg`](bytes-v1.12.1.svg) | 1 | 1 | 0 |
| chacha20poly1305<br>0.10.1 | [`chacha20poly1305-v0.10.1.svg`](chacha20poly1305-v0.10.1.svg) | 18 | 11 | 2 |
| core3<br>0.1.2 | [`core3-v0.1.2.svg`](core3-v0.1.2.svg) | 1 | 3 | 1 |
| crab-usb<br>0.9.1<br>/vendor/CrabUSB/usb-host | [`crab-usb-v0.9.1.svg`](crab-usb-v0.9.1.svg) | 39 | 10 | 6 |
| crc32fast<br>1.5.0<br>/vendor/crc32fast-1.5.0 | [`crc32fast-v1.5.0.svg`](crc32fast-v1.5.0.svg) | 1 | 2 | 1 |
| dma-api<br>0.7.3<br>/vendor/dma-api-0.7.3 | [`dma-api-v0.7.3.svg`](dma-api-v0.7.3.svg) | 1 | 1 | 3 |
| ed25519-dalek<br>2.2.0 | [`ed25519-dalek-v2.2.0.svg`](ed25519-dalek-v2.2.0.svg) | 14 | 3 | 5 |
| embassy-sync<br>0.8.0 | [`embassy-sync-v0.8.0.svg`](embassy-sync-v0.8.0.svg) | 5 | 3 | 5 |
| embassy-time-driver<br>0.2.2 | [`embassy-time-driver-v0.2.2.svg`](embassy-time-driver-v0.2.2.svg) | 3 | 2 | 0 |
| embedded-io-async<br>0.7.0 | [`embedded-io-async-v0.7.0.svg`](embedded-io-async-v0.7.0.svg) | 1 | 1 | 1 |
| embedded-websocket<br>0.9.4<br>/vendor/embedded-websocket-0.9.4 | [`embedded-websocket-v0.9.4.svg`](embedded-websocket-v0.9.4.svg) | 4 | 2 | 7 |
| getrandom<br>0.2.17 | [`getrandom-v0.2.17.svg`](getrandom-v0.2.17.svg) | 1 | 3 | 1 |
| half<br>2.7.1 | [`half-v2.7.1.svg`](half-v2.7.1.svg) | 3 | 2 | 2 |
| hashbrown<br>0.17.1 | [`hashbrown-v0.17.1.svg`](hashbrown-v0.17.1.svg) | 3 | 4 | 0 |
| heapless<br>0.9.3 | [`heapless-v0.9.3.svg`](heapless-v0.9.3.svg) | 1 | 3 | 1 |
| hyper<br>1.9.0<br>/vendor/hyper-1.9.0 | [`hyper-v1.9.0.svg`](hyper-v1.9.0.svg) | 9 | 3 | 6 |
| infer<br>0.22.0-trueos.1<br>/vendor/infer | [`infer-v0.22.0-trueos.1.svg`](infer-v0.22.0-trueos.1.svg) | 1 | 2 | 0 |
| libm<br>0.2.16 | [`libm-v0.2.16.svg`](libm-v0.2.16.svg) | 1 | 14 | 0 |
| limine<br>0.6.5 | [`limine-v0.6.5.svg`](limine-v0.6.5.svg) | 1 | 0 | 0 |
| log-os<br>0.0.2<br>/home/t4ce/Repos/TRUEOS-Blueprints/crates/log-os | [`log-os-v0.0.2.svg`](log-os-v0.0.2.svg) | 1 | 0 | 1 |
| lzma-rust2<br>0.16.5 | [`lzma-rust2-v0.16.5.svg`](lzma-rust2-v0.16.5.svg) | 1 | 0 | 0 |
| memchr<br>2.8.3 | [`memchr-v2.8.3.svg`](memchr-v2.8.3.svg) | 1 | 3 | 0 |
| microfont<br>3.7.8 | [`microfont-v3.7.8.svg`](microfont-v3.7.8.svg) | 1 | 0 | 0 |
| miniz_oxide<br>0.9.1 | [`miniz_oxide-v0.9.1.svg`](miniz_oxide-v0.9.1.svg) | 2 | 1 | 0 |
| mio<br>1.2.0<br>/vendor/mio-1.2.0 | [`mio-v1.2.0.svg`](mio-v1.2.0.svg) | 1 | 0 | 3 |
| object<br>0.39.1 | [`object-v0.39.1.svg`](object-v0.39.1.svg) | 1 | 0 | 1 |
| png<br>0.18.1<br>/vendor/png-0.18.1 | [`png-v0.18.1.svg`](png-v0.18.1.svg) | 3 | 0 | 4 |
| qrcodegen-no-heap<br>1.8.1 | [`qrcodegen-no-heap-v1.8.1.svg`](qrcodegen-no-heap-v1.8.1.svg) | 1 | 0 | 0 |
| rand_chacha<br>0.3.1 | [`rand_chacha-v0.3.1.svg`](rand_chacha-v0.3.1.svg) | 2 | 1 | 2 |
| rand_core<br>0.6.4 | [`rand_core-v0.6.4.svg`](rand_core-v0.6.4.svg) | 1 | 5 | 1 |
| raw-cpuid<br>11.6.0 | [`raw-cpuid-v11.6.0.svg`](raw-cpuid-v11.6.0.svg) | 1 | 0 | 1 |
| rdrand<br>0.8.3 | [`rdrand-v0.8.3.svg`](rdrand-v0.8.3.svg) | 1 | 0 | 1 |
| redb<br>4.2.0 | [`redb-v4.2.0.svg`](redb-v4.2.0.svg) | 1 | 0 | 0 |
| ring<br>0.17.14<br>/vendor/ring-0.17.14 | [`ring-v0.17.14.svg`](ring-v0.17.14.svg) | 2 | 2 | 2 |
| rustls<br>0.23.43 | [`rustls-v0.23.43.svg`](rustls-v0.23.43.svg) | 3 | 2 | 4 |
| rustls-rustcrypto<br>0.0.2-alpha<br>/vendor/rustls-rustcrypto-0.0.2-alpha | [`rustls-rustcrypto-v0.0.2-alpha.svg`](rustls-rustcrypto-v0.0.2-alpha.svg) | 27 | 3 | 12 |
| serde<br>1.0.229 | [`serde-v1.0.229.svg`](serde-v1.0.229.svg) | 2 | 1 | 3 |
| serde_json<br>1.0.151 | [`serde_json-v1.0.151.svg`](serde_json-v1.0.151.svg) | 2 | 1 | 3 |
| sha2<br>0.10.9 | [`sha2-v0.10.9.svg`](sha2-v0.10.9.svg) | 1 | 7 | 3 |
| skrifa<br>0.44.0 | [`skrifa-v0.44.0.svg`](skrifa-v0.44.0.svg) | 6 | 1 | 3 |
| smoltcp<br>0.13.1<br>/vendor/smoltcp-0.13.1 | [`smoltcp-v0.13.1.svg`](smoltcp-v0.13.1.svg) | 2 | 0 | 3 |
| spin<br>0.10.1 | [`spin-v0.10.1.svg`](spin-v0.10.1.svg) | 1 | 7 | 1 |
| symphonia-codec-aac<br>0.5.5<br>/vendor/symphonia-codec-aac-0.5.5-trueos | [`symphonia-codec-aac-v0.5.5.svg`](symphonia-codec-aac-v0.5.5.svg) | 1 | 0 | 3 |
| symphonia-core<br>0.5.5<br>/vendor/symphonia-core-0.5.5-trueos | [`symphonia-core-v0.5.5.svg`](symphonia-core-v0.5.5.svg) | 1 | 1 | 3 |
| tinyaudio<br>2.0.0<br>/vendor/tinyaudio | [`tinyaudio-v2.0.0.svg`](tinyaudio-v2.0.0.svg) | 1 | 0 | 0 |
| trueos-credential-store<br>0.1.0<br>/crates/trueos-credential-store | [`trueos-credential-store-v0.1.0.svg`](trueos-credential-store-v0.1.0.svg) | 1 | 0 | 3 |
| trueos-crypto<br>0.1.0<br>/crates/trueos-crypto | [`trueos-crypto-v0.1.0.svg`](trueos-crypto-v0.1.0.svg) | 1 | 2 | 2 |
| trueos-executor<br>0.10.0<br>/crates/trueos-executor | [`trueos-executor-v0.10.0.svg`](trueos-executor-v0.10.0.svg) | 10 | 0 | 3 |
| trueos-fs<br>0.0.1<br>/crates/trueos-fs | [`trueos-fs-v0.0.1.svg`](trueos-fs-v0.0.1.svg) | 1 | 1 | 2 |
| trueos-helio-artifact<br>0.1.0<br>/crates/trueos-helio-artifact | [`trueos-helio-artifact-v0.1.0.svg`](trueos-helio-artifact-v0.1.0.svg) | 1 | 1 | 2 |
| trueos-helio-runtime<br>0.1.0<br>/crates/trueos-helio-runtime | [`trueos-helio-runtime-v0.1.0.svg`](trueos-helio-runtime-v0.1.0.svg) | 1 | 0 | 2 |
| trueos-kokoro-aot<br>0.1.0<br>/crates/trueos-kokoro/aot | [`trueos-kokoro-aot-v0.1.0.svg`](trueos-kokoro-aot-v0.1.0.svg) | 1 | 3 | 0 |
| trueos-kokoro-audio<br>0.1.0<br>/crates/trueos-kokoro/audio | [`trueos-kokoro-audio-v0.1.0.svg`](trueos-kokoro-audio-v0.1.0.svg) | 1 | 0 | 0 |
| trueos-kokoro-conv<br>0.1.0<br>/crates/trueos-kokoro/conv | [`trueos-kokoro-conv-v0.1.0.svg`](trueos-kokoro-conv-v0.1.0.svg) | 1 | 1 | 1 |
| trueos-kokoro-dispatch<br>0.1.0<br>/crates/trueos-kokoro/dispatch | [`trueos-kokoro-dispatch-v0.1.0.svg`](trueos-kokoro-dispatch-v0.1.0.svg) | 1 | 0 | 13 |
| trueos-kokoro-duration<br>0.1.0<br>/crates/trueos-kokoro/duration | [`trueos-kokoro-duration-v0.1.0.svg`](trueos-kokoro-duration-v0.1.0.svg) | 1 | 1 | 1 |
| trueos-kokoro-exec<br>0.1.0<br>/crates/trueos-kokoro/exec | [`trueos-kokoro-exec-v0.1.0.svg`](trueos-kokoro-exec-v0.1.0.svg) | 1 | 2 | 1 |
| trueos-kokoro-f32<br>0.1.0<br>/crates/trueos-kokoro/f32 | [`trueos-kokoro-f32-v0.1.0.svg`](trueos-kokoro-f32-v0.1.0.svg) | 1 | 1 | 1 |
| trueos-kokoro-g2p<br>0.1.0<br>/crates/trueos-kokoro/g2p | [`trueos-kokoro-g2p-v0.1.0.svg`](trueos-kokoro-g2p-v0.1.0.svg) | 1 | 1 | 1 |
| trueos-kokoro-gemm<br>0.1.0<br>/crates/trueos-kokoro/gemm | [`trueos-kokoro-gemm-v0.1.0.svg`](trueos-kokoro-gemm-v0.1.0.svg) | 1 | 1 | 1 |
| trueos-kokoro-layout<br>0.1.0<br>/crates/trueos-kokoro/layout | [`trueos-kokoro-layout-v0.1.0.svg`](trueos-kokoro-layout-v0.1.0.svg) | 1 | 2 | 0 |
| trueos-kokoro-lexicon<br>0.1.0<br>/crates/trueos-kokoro/lexicon | [`trueos-kokoro-lexicon-v0.1.0.svg`](trueos-kokoro-lexicon-v0.1.0.svg) | 1 | 0 | 2 |
| trueos-kokoro-lstm<br>0.1.0<br>/crates/trueos-kokoro/lstm | [`trueos-kokoro-lstm-v0.1.0.svg`](trueos-kokoro-lstm-v0.1.0.svg) | 1 | 1 | 1 |
| trueos-kokoro-memory<br>0.1.0<br>/crates/trueos-kokoro/memory | [`trueos-kokoro-memory-v0.1.0.svg`](trueos-kokoro-memory-v0.1.0.svg) | 1 | 1 | 2 |
| trueos-kokoro-resize<br>0.1.0<br>/crates/trueos-kokoro/resize | [`trueos-kokoro-resize-v0.1.0.svg`](trueos-kokoro-resize-v0.1.0.svg) | 1 | 1 | 1 |
| trueos-kokoro-scalar<br>0.1.0<br>/crates/trueos-kokoro/scalar | [`trueos-kokoro-scalar-v0.1.0.svg`](trueos-kokoro-scalar-v0.1.0.svg) | 1 | 1 | 1 |
| trueos-kokoro-stft<br>0.1.0<br>/crates/trueos-kokoro/stft | [`trueos-kokoro-stft-v0.1.0.svg`](trueos-kokoro-stft-v0.1.0.svg) | 1 | 1 | 0 |
| trueos-kokoro-voice<br>0.1.0<br>/crates/trueos-kokoro/voice | [`trueos-kokoro-voice-v0.1.0.svg`](trueos-kokoro-voice-v0.1.0.svg) | 1 | 0 | 1 |
| trueos-lfm25-cpu<br>0.1.0<br>/crates/trueos-lfm25-cpu | [`trueos-lfm25-cpu-v0.1.0.svg`](trueos-lfm25-cpu-v0.1.0.svg) | 1 | 0 | 4 |
| trueos-lfm25-model<br>0.1.0<br>/crates/trueos-lfm25-model | [`trueos-lfm25-model-v0.1.0.svg`](trueos-lfm25-model-v0.1.0.svg) | 1 | 1 | 0 |
| trueos-locale<br>0.1.0<br>/crates/trueos-locale | [`trueos-locale-v0.1.0.svg`](trueos-locale-v0.1.0.svg) | 1 | 0 | 0 |
| trueos-math<br>0.1.0<br>/crates/trueos-math | [`trueos-math-v0.1.0.svg`](trueos-math-v0.1.0.svg) | 1 | 1 | 1 |
| trueos-time<br>0.5.1<br>/crates/trueos-executor/embassy-time | [`trueos-time-v0.5.1.svg`](trueos-time-v0.5.1.svg) | 1 | 0 | 2 |
| trueos-ttstt-cpu<br>0.1.0<br>/crates/trueos-ttstt-cpu | [`trueos-ttstt-cpu-v0.1.0.svg`](trueos-ttstt-cpu-v0.1.0.svg) | 1 | 1 | 0 |
| trueos-vm<br>0.1.0<br>/crates/trueos-vm | [`trueos-vm-v0.1.0.svg`](trueos-vm-v0.1.0.svg) | 1 | 0 | 1 |
| unicode-segmentation<br>1.13.3 | [`unicode-segmentation-v1.13.3.svg`](unicode-segmentation-v1.13.3.svg) | 1 | 1 | 0 |
| unicode-width<br>0.2.2 | [`unicode-width-v0.2.2.svg`](unicode-width-v0.2.2.svg) | 1 | 0 | 0 |
| v<br>0.1.0<br>/crates/trueos-v | [`v-v0.1.0.svg`](v-v0.1.0.svg) | 1 | 1 | 8 |
| webpki-roots<br>1.0.9 | [`webpki-roots-v1.0.9.svg`](webpki-roots-v1.0.9.svg) | 1 | 0 | 1 |
| x86_64<br>0.15.5 | [`x86_64-v0.15.5.svg`](x86_64-v0.15.5.svg) | 3 | 0 | 2 |
| zeroize<br>1.9.0 | [`zeroize-v1.9.0.svg`](zeroize-v1.9.0.svg) | 1 | 5 | 1 |
| zune-core<br>0.5.1<br>/vendor/zune-core-0.5.1 | [`zune-core-v0.5.1.svg`](zune-core-v0.5.1.svg) | 1 | 1 | 0 |
| zune-jpeg<br>0.5.15<br>/vendor/zune-jpeg-0.5.15 | [`zune-jpeg-v0.5.15.svg`](zune-jpeg-v0.5.15.svg) | 1 | 0 | 1 |

Total direct TRUEOS roots: 87
Total owned nodes excluding TRUEOS root: 243
