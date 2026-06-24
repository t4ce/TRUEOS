# TRUEOS dependency graph split by root dependency

Source: `tools/depgraph/trueos-depth-tree.txt`

Each SVG expands one direct dependency of the TRUEOS root. Blue note nodes are incoming cross-image edges; yellow note nodes are outgoing cross-image edges.

| Root dependency | SVG | Owned nodes | Input images | Output images |
| --- | --- | ---: | ---: | ---: |
| acpi<br>6.1.1 | [`acpi-v6.1.1.svg`](acpi-v6.1.1.svg) | 8 | 11 | 0 |
| alsa<br>0.11.0<br>/vendor/alsa-0.11.0 | [`alsa-v0.11.0.svg`](alsa-v0.11.0.svg) | 2 | 8 | 1 |
| aml<br>0.16.4<br>/vendor/aml-0.16.4 | [`aml-v0.16.4.svg`](aml-v0.16.4.svg) | 6 | 0 | 1 |
| bytes<br>1.12.0 | [`bytes-v1.12.0.svg`](bytes-v1.12.0.svg) | 1 | 1 | 0 |
| core3<br>0.1.2 | [`core3-v0.1.2.svg`](core3-v0.1.2.svg) | 1 | 5 | 1 |
| crab-usb<br>0.9.1<br>/vendor/CrabUSB/usb-host | [`crab-usb-v0.9.1.svg`](crab-usb-v0.9.1.svg) | 48 | 15 | 5 |
| crc32fast<br>1.5.0<br>/vendor/crc32fast-1.5.0 | [`crc32fast-v1.5.0.svg`](crc32fast-v1.5.0.svg) | 1 | 1 | 1 |
| dma-api<br>0.7.3<br>/vendor/dma-api-0.7.3 | [`dma-api-v0.7.3.svg`](dma-api-v0.7.3.svg) | 1 | 1 | 3 |
| embassy-executor<br>0.10.0<br>/crates/trueos-executor | [`embassy-executor-v0.10.0.svg`](embassy-executor-v0.10.0.svg) | 13 | 3 | 1 |
| embassy-sync<br>0.8.0 | [`embassy-sync-v0.8.0.svg`](embassy-sync-v0.8.0.svg) | 4 | 3 | 6 |
| embassy-time<br>0.5.1<br>/vendor/embassy-time-0.5.1-trueos | [`embassy-time-v0.5.1.svg`](embassy-time-v0.5.1.svg) | 1 | 2 | 2 |
| embassy-time-driver<br>0.2.2 | [`embassy-time-driver-v0.2.2.svg`](embassy-time-driver-v0.2.2.svg) | 1 | 2 | 1 |
| embedded-io-async<br>0.7.0 | [`embedded-io-async-v0.7.0.svg`](embedded-io-async-v0.7.0.svg) | 1 | 1 | 1 |
| embedded-websocket<br>0.9.4<br>/vendor/embedded-websocket-0.9.4 | [`embedded-websocket-v0.9.4.svg`](embedded-websocket-v0.9.4.svg) | 14 | 6 | 6 |
| euclid<br>0.22.13 | [`euclid-v0.22.13.svg`](euclid-v0.22.13.svg) | 1 | 1 | 1 |
| getrandom<br>0.2.17<br>/vendor/getrandom-0.2.17 | [`getrandom-v0.2.17.svg`](getrandom-v0.2.17.svg) | 1 | 1 | 1 |
| hashbrown<br>0.17.1 | [`hashbrown-v0.17.1.svg`](hashbrown-v0.17.1.svg) | 2 | 2 | 0 |
| heapless<br>0.9.3 | [`heapless-v0.9.3.svg`](heapless-v0.9.3.svg) | 1 | 4 | 1 |
| hyper<br>1.9.0<br>/vendor/hyper-1.9.0 | [`hyper-v1.9.0.svg`](hyper-v1.9.0.svg) | 9 | 4 | 5 |
| kurbo<br>0.11.3<br>/vendor/kurbo-0.11.3 | [`kurbo-v0.11.3.svg`](kurbo-v0.11.3.svg) | 2 | 4 | 2 |
| libm<br>0.2.16 | [`libm-v0.2.16.svg`](libm-v0.2.16.svg) | 1 | 9 | 0 |
| limine<br>0.6.5 | [`limine-v0.6.5.svg`](limine-v0.6.5.svg) | 1 | 0 | 0 |
| lyon_geom<br>1.0.19 | [`lyon_geom-v1.0.19.svg`](lyon_geom-v1.0.19.svg) | 1 | 1 | 3 |
| lyon_tessellation<br>1.0.20 | [`lyon_tessellation-v1.0.20.svg`](lyon_tessellation-v1.0.20.svg) | 3 | 0 | 2 |
| lzma-rust2<br>0.16.4 | [`lzma-rust2-v0.16.4.svg`](lzma-rust2-v0.16.4.svg) | 1 | 0 | 0 |
| memchr<br>2.8.2 | [`memchr-v2.8.2.svg`](memchr-v2.8.2.svg) | 1 | 2 | 0 |
| miniz_oxide<br>0.9.1 | [`miniz_oxide-v0.9.1.svg`](miniz_oxide-v0.9.1.svg) | 2 | 1 | 0 |
| mio<br>1.2.0<br>/vendor/mio-1.2.0 | [`mio-v1.2.0.svg`](mio-v1.2.0.svg) | 1 | 0 | 3 |
| parry2d<br>0.26.1 | [`parry2d-v0.26.1.svg`](parry2d-v0.26.1.svg) | 9 | 1 | 6 |
| png<br>0.18.1<br>/vendor/png-0.18.1 | [`png-v0.18.1.svg`](png-v0.18.1.svg) | 3 | 0 | 4 |
| rand_chacha<br>0.3.1 | [`rand_chacha-v0.3.1.svg`](rand_chacha-v0.3.1.svg) | 3 | 1 | 1 |
| rand_core<br>0.6.4 | [`rand_core-v0.6.4.svg`](rand_core-v0.6.4.svg) | 1 | 4 | 1 |
| raw-cpuid<br>11.6.0 | [`raw-cpuid-v11.6.0.svg`](raw-cpuid-v11.6.0.svg) | 1 | 0 | 1 |
| rdrand<br>0.8.3 | [`rdrand-v0.8.3.svg`](rdrand-v0.8.3.svg) | 1 | 0 | 1 |
| regex-automata<br>0.4.14 | [`regex-automata-v0.4.14.svg`](regex-automata-v0.4.14.svg) | 2 | 0 | 0 |
| rustls<br>0.23.41 | [`rustls-v0.23.41.svg`](rustls-v0.23.41.svg) | 4 | 2 | 3 |
| rustls-rustcrypto<br>0.0.2-alpha<br>/vendor/rustls-rustcrypto-0.0.2-alpha | [`rustls-rustcrypto-v0.0.2-alpha.svg`](rustls-rustcrypto-v0.0.2-alpha.svg) | 46 | 2 | 11 |
| serde<br>1.0.228 | [`serde-v1.0.228.svg`](serde-v1.0.228.svg) | 3 | 2 | 1 |
| serde_json<br>1.0.150 | [`serde_json-v1.0.150.svg`](serde_json-v1.0.150.svg) | 2 | 1 | 3 |
| sha2<br>0.10.9 | [`sha2-v0.10.9.svg`](sha2-v0.10.9.svg) | 1 | 1 | 2 |
| smoltcp<br>0.13.1<br>/vendor/smoltcp-0.13.1 | [`smoltcp-v0.13.1.svg`](smoltcp-v0.13.1.svg) | 2 | 0 | 3 |
| socket2<br>0.6.3<br>/vendor/socket2-0.6.3 | [`socket2-v0.6.3.svg`](socket2-v0.6.3.svg) | 1 | 0 | 3 |
| spin<br>0.10.0 | [`spin-v0.10.0.svg`](spin-v0.10.0.svg) | 1 | 7 | 1 |
| symphonia-codec-aac<br>0.5.5<br>/vendor/symphonia-codec-aac-0.5.5-trueos | [`symphonia-codec-aac-v0.5.5.svg`](symphonia-codec-aac-v0.5.5.svg) | 2 | 2 | 3 |
| symphonia-core<br>0.5.5<br>/vendor/symphonia-core-0.5.5-trueos | [`symphonia-core-v0.5.5.svg`](symphonia-core-v0.5.5.svg) | 1 | 1 | 3 |
| tiny-skia-path<br>0.11.4 | [`tiny-skia-path-v0.11.4.svg`](tiny-skia-path-v0.11.4.svg) | 3 | 1 | 2 |
| tinyaudio<br>2.0.0<br>/vendor/tinyaudio | [`tinyaudio-v2.0.0.svg`](tinyaudio-v2.0.0.svg) | 1 | 0 | 0 |
| tower<br>0.5.3<br>/vendor/tower-0.5.3 | [`tower-v0.5.3.svg`](tower-v0.5.3.svg) | 4 | 0 | 1 |
| trueos-c4<br>0.1.0<br>/crates/trueos-c4 | [`trueos-c4-v0.1.0.svg`](trueos-c4-v0.1.0.svg) | 1 | 0 | 0 |
| trueos-esp<br>0.1.0<br>/crates/trueos-esp | [`trueos-esp-v0.1.0.svg`](trueos-esp-v0.1.0.svg) | 1 | 0 | 3 |
| trueos-fs<br>0.0.1<br>/crates/trueos-fs | [`trueos-fs-v0.0.1.svg`](trueos-fs-v0.0.1.svg) | 1 | 0 | 0 |
| trueos-io<br>0.1.0<br>/crates/trueos-io | [`trueos-io-v0.1.0.svg`](trueos-io-v0.1.0.svg) | 1 | 4 | 1 |
| trueos-locale<br>0.1.0<br>/crates/trueos-locale | [`trueos-locale-v0.1.0.svg`](trueos-locale-v0.1.0.svg) | 1 | 1 | 0 |
| trueos-lsd<br>1.1.5<br>/crates/trueos-lsd | [`trueos-lsd-v1.1.5.svg`](trueos-lsd-v1.1.5.svg) | 1 | 0 | 2 |
| trueos-math<br>0.1.0<br>/crates/trueos-math | [`trueos-math-v0.1.0.svg`](trueos-math-v0.1.0.svg) | 1 | 1 | 1 |
| trueos-qjs<br>0.1.0<br>/crates/trueos-qjs | [`trueos-qjs-v0.1.0.svg`](trueos-qjs-v0.1.0.svg) | 11 | 1 | 13 |
| trueos-silk<br>0.1.0<br>/crates/trueos-silk | [`trueos-silk-v0.1.0.svg`](trueos-silk-v0.1.0.svg) | 1 | 0 | 0 |
| trueos-vm<br>0.1.0<br>/crates/trueos-vm | [`trueos-vm-v0.1.0.svg`](trueos-vm-v0.1.0.svg) | 1 | 0 | 1 |
| unicode-segmentation<br>1.13.3 | [`unicode-segmentation-v1.13.3.svg`](unicode-segmentation-v1.13.3.svg) | 1 | 1 | 0 |
| usvg<br>0.45.1<br>/vendor/usvg-0.45.1 | [`usvg-v0.45.1.svg`](usvg-v0.45.1.svg) | 1 | 1 | 4 |
| v<br>0.1.0<br>/crates/trueos-v | [`v-v0.1.0.svg`](v-v0.1.0.svg) | 1 | 4 | 3 |
| webpki-roots<br>1.0.8 | [`webpki-roots-v1.0.8.svg`](webpki-roots-v1.0.8.svg) | 1 | 0 | 1 |
| x86_64<br>0.15.4 | [`x86_64-v0.15.4.svg`](x86_64-v0.15.4.svg) | 3 | 0 | 2 |
| zeroize<br>1.9.0 | [`zeroize-v1.9.0.svg`](zeroize-v1.9.0.svg) | 1 | 3 | 1 |
| zune-core<br>0.5.1<br>/vendor/zune-core-0.5.1 | [`zune-core-v0.5.1.svg`](zune-core-v0.5.1.svg) | 1 | 1 | 0 |
| zune-jpeg<br>0.5.15<br>/vendor/zune-jpeg-0.5.15 | [`zune-jpeg-v0.5.15.svg`](zune-jpeg-v0.5.15.svg) | 1 | 0 | 1 |

Total direct TRUEOS roots: 66
Total owned nodes excluding TRUEOS root: 250
