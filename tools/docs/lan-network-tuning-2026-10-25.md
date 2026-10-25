# LAN network tuning, 2026-10-25

Three small changes are implemented and tested against the physical rig at
`192.168.178.94`. The updated kernel is deployed; the RDP release viewer and
headless probe are rebuilt in `../TRUEOS-RDP/target/release/`.

## Changes

- `src/net/adapter.rs` and `src/net/poll_cadence.rs`: keep each NIC's receive
  polling active for 100 ms after received or transmitted traffic. Empty polls
  request 250 us during that window, which rounds to 1 ms with the current
  1 kHz timer. Busy receive drains still yield immediately; quiet NICs keep
  their existing 5 ms interval. Outgoing video therefore keeps incoming input
  and control traffic responsive between frames.
- `../TRUEOS-RDP/src/media.rs`: replace bitwise IEEE CRC32 validation with
  `crc32fast`, preserving the wire checksum and corruption checks. An optimized
  host benchmark over 1,168-byte payloads measured about 3.67 us versus 29 ns
  per checksum. This is a checksum-only result; the cost was already small at
  the observed packet rates.
- The receiver requests a 1 MiB socket receive buffer to absorb keyframe
  bursts and brief decoder-pipe stalls. This host reports an effective
  2 MiB allocation. Buffer sizing remains nonfatal when an OS denies or clamps
  the request. A larger buffer adds capacity without waiting to fill it.

The client also has a bounded headless probe using the actual receiver and
FFmpeg decoder, with cumulative counters for repeatable live verification.

## Observed before and after

The host's Ethernet link negotiated 2.5 Gbit/s, full duplex, MTU 1500.

| Measurement | Before | After |
| --- | ---: | ---: |
| TCP connect, median, 150 requests | 5.44 ms | 3.91 ms |
| TCP connect, 95th percentile | 8.00 ms | 5.18 ms |
| Small HTTP response including connection, median | 27.19 ms | 23.97 ms |
| Same HTTP measurement, 95th percentile | 31.96 ms | 27.95 ms |
| 16 MiB synthetic HTTP upload, elapsed time | 2.168 s | 1.549 s |
| 16 MiB synthetic HTTP download, median of three | 166.3 Mbit/s | 167.3 Mbit/s |
| Active decoded 2560x1440 stream | About 33 fps | About 33 fps |

The HTTP latency probe used randomized 5–25 ms gaps and a small missing-file
response. Uploads used the same curl arguments and generated text. Their
elapsed times include HTTP `Expect: 100-continue` handling and storage work;
they are endpoint transfer measurements, not a raw NIC throughput benchmark.
Every downloaded body passed SHA-256 verification. The synthetic rig file
was deleted after testing.

The baseline receiver had about five seconds of active streaming in its
20-second probe. The after probe ran for 40 seconds across four sessions,
receiving 1,265 access units and decoding 1,223 frames, with zero reported
receiver anomalies. The startup AU/frame count offset does not establish
ongoing decode latency. Receiver anomalies count sequence gaps, malformed
packets and other reassembly issues; this is not an independent packet-loss
capture. The after desktop scene used less bitrate, so the two stream rates
are not comparable throughput workloads. The observed streams used only a
few Mbit/s and showed no evidence of exhausting available transfer capacity.

This is a before/after observation across a normal working-tree redeploy.
Concurrent workspace changes prevent strict attribution of every difference
to the poll policy. Input-to-display latency was not measured.

## Verification and reproduction

- Normal `make iso` build and packaged-artifact verification passed. The PXE
  receipt verifies runtime ELF SHA-256
  `211f9d8674bb79023500b30277f9e49b9734871c4d65e727ae7f1c38fe66f94c`.
  The initial log collector exited before finding its marker; the subsequent
  collector verified the fresh boot successfully.
- Four cadence tests and 16 existing focused TCP/Mio/network-owner checks
  passed. The cadence tests cover outgoing streams, finite decay, RX bursts,
  independent NIC state, short idle caps and counter wrap.
- All 40 client tests passed, including independent IEEE CRC vectors and
  corrupt-packet rejection. Client formatting and release builds passed.
- Broader unchanged checks still have stale expectations:
  `test_shell3_tcp.py` calls the removed `Terminal::new`, and
  `check_mio_poll_alignment.py` expects an obsolete `swap_remove` expression.

Run the live receiver without taking the graphical viewer's local UDP port:

```sh
cd /home/t4ce/Repos/TRUEOS-RDP
cargo run --release --example media-probe -- 192.168.178.94 40
```

The probe binds an ephemeral port and sends the normal subscription. Its
rate includes accepted media datagrams and IP/UDP headers, excluding Ethernet
overhead. FFmpeg can print broken-pipe messages when the bounded probe exits;
the test confirmed that its child exits afterward.

Raw HTTP samples, transfer hashes, receiver logs, client test/build logs,
checksum benchmark source/results, deploy receipts and kernel build output
are under `bld/network-lan-20261025/`. The HTTP reproduction script is
`bld/network-lan-20261025/http_latency.py`.

## Follow-up found by inspection

The buffered TCP transmit path can discard a frame when hardware admission
fails after smoltcp has consumed its transmit token. Its drop counter is also
not incremented. This warrants a separate bounded-queue/backpressure change
and a saturated TCP test. The RDP checked UDP path already handles hardware
backpressure, so this issue was left outside the present change. RTL8125 DMA
cache-flush costs and ring sizing also need separate measurements before
changing hardware policy.
