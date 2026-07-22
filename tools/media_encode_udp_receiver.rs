//! Bounded Ubuntu/Linux receiver for the TRUEOS media-encode UDP probe.
//!
//! The first probe is deliberately one-way and discovery-free: TRUEOS broadcasts
//! encoded H.264 Annex-B access-unit fragments to `255.255.255.255:9650`; this
//! tool binds `0.0.0.0:9650`, reassembles complete access units, and writes them
//! in sequence to an `.h264` file. It does not receive decoded video frames.
//!
//! TME1 wire format (all integer fields are big-endian):
//!
//! ```text
//!  0  magic[4] = "TME1"
//!  4  version u8 = 1
//!  5  flags u8: START=1, END=2, KEYFRAME=4, SESSION_END=8
//!  6  header_len u16 = 32
//!  8  session_id u32
//! 12  datagram_seq u32
//! 16  access_unit_seq u32 (starts at zero for each session)
//! 20  fragment_index u16
//! 22  fragment_count u16 (1..=4096)
//! 24  payload_len u16 (1..=1168)
//! 26  reserved u16 = 0
//! 28  payload_crc32 u32 (IEEE CRC-32 of this datagram's payload)
//! 32  payload[payload_len]
//! ```
//!
//! An application datagram is at most 1200 bytes. START must be set exactly on
//! fragment zero and END exactly on the final fragment. SESSION_END is valid
//! only on the final fragment of the final access unit. SPS/PPS may precede the
//! first IDR in access unit zero.
//!
//! Build without adding host dependencies to the TRUEOS kernel crate:
//!
//! ```text
//! rustc --edition=2024 -O tools/media_encode_udp_receiver.rs \
//!   -o media_encode_udp_receiver
//! ./media_encode_udp_receiver --output trueos-probe.h264
//! ```

use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

const MAGIC: [u8; 4] = *b"TME1";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 32;
const MAX_DATAGRAM_LEN: usize = 1200;
const MAX_PAYLOAD_LEN: usize = MAX_DATAGRAM_LEN - HEADER_LEN;
const MAX_FRAGMENT_COUNT: u16 = 4096;
const DEFAULT_PORT: u16 = 9650;
const DEFAULT_MAX_BUFFER_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_MAX_AU_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_INFLIGHT_AUS: usize = 32;
const DEFAULT_REORDER_MS: u64 = 750;
const MAX_AU_SEQUENCE_AHEAD: u32 = 4096;

const FLAG_START: u8 = 1 << 0;
const FLAG_END: u8 = 1 << 1;
const FLAG_KEYFRAME: u8 = 1 << 2;
const FLAG_SESSION_END: u8 = 1 << 3;
const KNOWN_FLAGS: u8 = FLAG_START | FLAG_END | FLAG_KEYFRAME | FLAG_SESSION_END;

#[derive(Debug)]
struct Config {
    bind: String,
    output: PathBuf,
    max_buffer_bytes: usize,
    max_au_bytes: usize,
    max_inflight_aus: usize,
    reorder_timeout: Duration,
    idle_exit: Option<Duration>,
    decoder: Option<DecoderKind>,
}

#[derive(Clone, Copy, Debug)]
enum DecoderKind {
    Ffplay,
    FfmpegCheck,
}

impl Config {
    fn parse() -> Result<Option<Self>, String> {
        let mut config = Self {
            bind: format!("0.0.0.0:{DEFAULT_PORT}"),
            output: PathBuf::from("trueos-media-encode.h264"),
            max_buffer_bytes: DEFAULT_MAX_BUFFER_BYTES,
            max_au_bytes: DEFAULT_MAX_AU_BYTES,
            max_inflight_aus: DEFAULT_MAX_INFLIGHT_AUS,
            reorder_timeout: Duration::from_millis(DEFAULT_REORDER_MS),
            idle_exit: None,
            decoder: None,
        };

        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => return Ok(None),
                "--bind" => config.bind = required_value(&mut args, "--bind")?,
                "--output" => {
                    config.output = PathBuf::from(required_value(&mut args, "--output")?)
                }
                "--max-buffer-bytes" => {
                    config.max_buffer_bytes = parse_positive_usize(
                        &required_value(&mut args, "--max-buffer-bytes")?,
                        "--max-buffer-bytes",
                    )?;
                }
                "--max-au-bytes" => {
                    config.max_au_bytes = parse_positive_usize(
                        &required_value(&mut args, "--max-au-bytes")?,
                        "--max-au-bytes",
                    )?;
                }
                "--max-inflight-aus" => {
                    config.max_inflight_aus = parse_positive_usize(
                        &required_value(&mut args, "--max-inflight-aus")?,
                        "--max-inflight-aus",
                    )?;
                }
                "--reorder-ms" => {
                    let millis = parse_positive_u64(
                        &required_value(&mut args, "--reorder-ms")?,
                        "--reorder-ms",
                    )?;
                    config.reorder_timeout = Duration::from_millis(millis);
                }
                "--idle-exit-secs" => {
                    let seconds = parse_positive_u64(
                        &required_value(&mut args, "--idle-exit-secs")?,
                        "--idle-exit-secs",
                    )?;
                    config.idle_exit = Some(Duration::from_secs(seconds));
                }
                "--ffplay" => set_decoder(&mut config.decoder, DecoderKind::Ffplay)?,
                "--ffmpeg-check" => {
                    set_decoder(&mut config.decoder, DecoderKind::FfmpegCheck)?
                }
                _ => return Err(format!("unknown argument: {arg}")),
            }
        }

        if config.max_au_bytes > config.max_buffer_bytes {
            return Err("--max-au-bytes cannot exceed --max-buffer-bytes".into());
        }

        Ok(Some(config))
    }
}

fn required_value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn parse_positive_usize(value: &str, option: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("invalid value for {option}: {value}"))?;
    if parsed == 0 {
        return Err(format!("{option} must be greater than zero"));
    }
    Ok(parsed)
}

fn parse_positive_u64(value: &str, option: &str) -> Result<u64, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| format!("invalid value for {option}: {value}"))?;
    if parsed == 0 {
        return Err(format!("{option} must be greater than zero"));
    }
    Ok(parsed)
}

fn set_decoder(slot: &mut Option<DecoderKind>, decoder: DecoderKind) -> Result<(), String> {
    if slot.is_some() {
        return Err("choose at most one of --ffplay and --ffmpeg-check".into());
    }
    *slot = Some(decoder);
    Ok(())
}

fn usage(program: &str) {
    println!(
        "\
TRUEOS bounded H.264 UDP receiver

Usage: {program} [OPTIONS]

  --bind ADDR               listen address (default 0.0.0.0:{DEFAULT_PORT})
  --output PATH             Annex-B output (default trueos-media-encode.h264)
  --max-buffer-bytes N      hard in-flight allocation budget (default {DEFAULT_MAX_BUFFER_BYTES})
  --max-au-bytes N          maximum one-AU payload size (default {DEFAULT_MAX_AU_BYTES})
  --max-inflight-aus N      maximum tracked access units (default {DEFAULT_MAX_INFLIGHT_AUS})
  --reorder-ms N            wait before declaring a missing AU dropped (default {DEFAULT_REORDER_MS})
  --idle-exit-secs N        exit after N seconds without a valid pinned-session packet
  --ffplay                  explicitly pipe completed AUs to ffplay as well as the file
  --ffmpeg-check            explicitly decode completed AUs to ffmpeg's null muxer
  -h, --help                show this help

Wire: one-way TRUEOS broadcast to UDP port {DEFAULT_PORT}; TME1 v1, 32-byte
big-endian header, <=1168-byte fragment payload, <=1200-byte datagram. The
payload is an already encoded H.264 Annex-B access unit, not a decoded frame."
    );
}

#[derive(Debug, PartialEq, Eq)]
enum PacketError {
    TooShort,
    TooLarge,
    BadMagic,
    BadVersion,
    BadHeaderLength,
    UnknownFlags,
    BadFragmentCount,
    BadFragmentIndex,
    BadBoundaryFlags,
    BadSessionEnd,
    BadPayloadLength,
    ReservedNonzero,
    CrcMismatch,
}

#[derive(Debug)]
struct Packet<'a> {
    flags: u8,
    session_id: u32,
    datagram_seq: u32,
    access_unit_seq: u32,
    fragment_index: u16,
    fragment_count: u16,
    payload: &'a [u8],
}

impl<'a> Packet<'a> {
    fn parse(datagram: &'a [u8]) -> Result<Self, PacketError> {
        if datagram.len() < HEADER_LEN {
            return Err(PacketError::TooShort);
        }
        if datagram.len() > MAX_DATAGRAM_LEN {
            return Err(PacketError::TooLarge);
        }
        if datagram[0..4] != MAGIC {
            return Err(PacketError::BadMagic);
        }
        if datagram[4] != VERSION {
            return Err(PacketError::BadVersion);
        }

        let flags = datagram[5];
        if flags & !KNOWN_FLAGS != 0 {
            return Err(PacketError::UnknownFlags);
        }
        if read_u16(datagram, 6) as usize != HEADER_LEN {
            return Err(PacketError::BadHeaderLength);
        }

        let fragment_index = read_u16(datagram, 20);
        let fragment_count = read_u16(datagram, 22);
        if fragment_count == 0 || fragment_count > MAX_FRAGMENT_COUNT {
            return Err(PacketError::BadFragmentCount);
        }
        if fragment_index >= fragment_count {
            return Err(PacketError::BadFragmentIndex);
        }

        let is_first = fragment_index == 0;
        let is_last = fragment_index + 1 == fragment_count;
        if (flags & FLAG_START != 0) != is_first || (flags & FLAG_END != 0) != is_last {
            return Err(PacketError::BadBoundaryFlags);
        }
        if flags & FLAG_SESSION_END != 0 && !is_last {
            return Err(PacketError::BadSessionEnd);
        }

        let payload_len = read_u16(datagram, 24) as usize;
        if payload_len == 0
            || payload_len > MAX_PAYLOAD_LEN
            || datagram.len() != HEADER_LEN + payload_len
        {
            return Err(PacketError::BadPayloadLength);
        }
        if read_u16(datagram, 26) != 0 {
            return Err(PacketError::ReservedNonzero);
        }

        let payload = &datagram[HEADER_LEN..];
        if crc32(payload) != read_u32(datagram, 28) {
            return Err(PacketError::CrcMismatch);
        }

        Ok(Self {
            flags,
            session_id: read_u32(datagram, 8),
            datagram_seq: read_u32(datagram, 12),
            access_unit_seq: read_u32(datagram, 16),
            fragment_index,
            fragment_count,
            payload,
        })
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

#[derive(Default)]
struct Stats {
    datagrams_received: u64,
    datagram_bytes: u64,
    valid_datagrams: u64,
    header_drops: u64,
    crc_failures: u64,
    foreign_drops: u64,
    capacity_drops: u64,
    duplicate_fragments: u64,
    late_or_reordered_datagrams: u64,
    datagram_gap_events: u64,
    datagrams_missing_observed: u64,
    access_units_written: u64,
    access_units_dropped: u64,
    annex_b_failures: u64,
    output_bytes: u64,
}

impl Stats {
    fn report(&self, final_report: bool) {
        let label = if final_report { "final" } else { "status" };
        eprintln!(
            "{label} rx_packets={} valid={} rx_bytes={} output_aus={} output_bytes={} \
             gaps={} missing_observed={} reordered_or_late={} duplicates={} crc_failures={} \
             header_drops={} foreign_drops={} capacity_drops={} au_drops={} annex_b_failures={}",
            self.datagrams_received,
            self.valid_datagrams,
            self.datagram_bytes,
            self.access_units_written,
            self.output_bytes,
            self.datagram_gap_events,
            self.datagrams_missing_observed,
            self.late_or_reordered_datagrams,
            self.duplicate_fragments,
            self.crc_failures,
            self.header_drops,
            self.foreign_drops,
            self.capacity_drops,
            self.access_units_dropped,
            self.annex_b_failures,
        );
    }
}

struct DatagramSequence {
    highest: Option<u32>,
}

impl DatagramSequence {
    fn new() -> Self {
        Self { highest: None }
    }

    fn observe(&mut self, sequence: u32, stats: &mut Stats) {
        let Some(highest) = self.highest else {
            self.highest = Some(sequence);
            return;
        };

        if sequence > highest {
            if sequence > highest + 1 {
                let missing = u64::from(sequence - highest - 1);
                stats.datagram_gap_events += 1;
                stats.datagrams_missing_observed += missing;
                eprintln!(
                    "datagram gap expected={} got={} missing={missing}",
                    highest + 1,
                    sequence
                );
            }
            self.highest = Some(sequence);
        } else {
            stats.late_or_reordered_datagrams += 1;
        }
    }
}

struct AccessUnit {
    fragments: Vec<Option<Box<[u8]>>>,
    received_fragments: usize,
    payload_bytes: usize,
    accounted_bytes: usize,
    keyframe: bool,
    session_end: bool,
    first_seen: Instant,
    last_seen: Instant,
}

impl AccessUnit {
    fn new(fragment_count: u16, now: Instant) -> Self {
        let mut fragments = Vec::with_capacity(fragment_count as usize);
        fragments.resize_with(fragment_count as usize, || None);
        let accounted_bytes = fragments.capacity() * std::mem::size_of::<Option<Box<[u8]>>>();
        Self {
            fragments,
            received_fragments: 0,
            payload_bytes: 0,
            accounted_bytes,
            keyframe: false,
            session_end: false,
            first_seen: now,
            last_seen: now,
        }
    }

    fn is_complete(&self) -> bool {
        self.received_fragments == self.fragments.len()
    }

    fn has_annex_b_start_code(&self) -> bool {
        let mut prefix = [0u8; 4];
        let mut length = 0;
        for fragment in self.fragments.iter().flatten() {
            for &byte in fragment.iter() {
                if length == prefix.len() {
                    break;
                }
                prefix[length] = byte;
                length += 1;
            }
            if length == prefix.len() {
                break;
            }
        }
        length >= 3
            && (prefix[..3] == [0, 0, 1]
                || (length >= 4 && prefix[..4] == [0, 0, 0, 1]))
    }
}

struct Reassembler {
    access_units: BTreeMap<u32, AccessUnit>,
    next_access_unit: u32,
    used_bytes: usize,
    max_buffer_bytes: usize,
    max_au_bytes: usize,
    max_inflight_aus: usize,
    reorder_timeout: Duration,
}

enum InsertResult {
    Accepted,
    Duplicate,
    Rejected(&'static str),
}

impl Reassembler {
    fn new(config: &Config) -> Self {
        Self {
            access_units: BTreeMap::new(),
            next_access_unit: 0,
            used_bytes: 0,
            max_buffer_bytes: config.max_buffer_bytes,
            max_au_bytes: config.max_au_bytes,
            max_inflight_aus: config.max_inflight_aus,
            reorder_timeout: config.reorder_timeout,
        }
    }

    fn insert(&mut self, packet: &Packet<'_>, now: Instant) -> InsertResult {
        if packet.access_unit_seq < self.next_access_unit {
            return InsertResult::Rejected("late access unit");
        }
        if packet.access_unit_seq - self.next_access_unit > MAX_AU_SEQUENCE_AHEAD {
            return InsertResult::Rejected("access-unit sequence too far ahead");
        }

        if let Some(existing) = self.access_units.get(&packet.access_unit_seq) {
            if existing.fragments.len() != packet.fragment_count as usize {
                return InsertResult::Rejected("fragment-count changed within access unit");
            }
        } else {
            if self.access_units.len() >= self.max_inflight_aus {
                return InsertResult::Rejected("in-flight access-unit limit reached");
            }
            let access_unit = AccessUnit::new(packet.fragment_count, now);
            if self.used_bytes.saturating_add(access_unit.accounted_bytes) > self.max_buffer_bytes {
                return InsertResult::Rejected("reassembly byte budget reached");
            }
            self.used_bytes += access_unit.accounted_bytes;
            self.access_units
                .insert(packet.access_unit_seq, access_unit);
        }

        let access_unit = self.access_units.get_mut(&packet.access_unit_seq).unwrap();
        let fragment_slot = &mut access_unit.fragments[packet.fragment_index as usize];
        if fragment_slot.is_some() {
            return InsertResult::Duplicate;
        }
        if access_unit
            .payload_bytes
            .saturating_add(packet.payload.len())
            > self.max_au_bytes
        {
            return InsertResult::Rejected("access unit exceeds byte limit");
        }
        if self.used_bytes.saturating_add(packet.payload.len()) > self.max_buffer_bytes {
            return InsertResult::Rejected("reassembly byte budget reached");
        }

        *fragment_slot = Some(packet.payload.to_vec().into_boxed_slice());
        access_unit.received_fragments += 1;
        access_unit.payload_bytes += packet.payload.len();
        access_unit.accounted_bytes += packet.payload.len();
        access_unit.keyframe |= packet.flags & FLAG_KEYFRAME != 0;
        access_unit.session_end |= packet.flags & FLAG_SESSION_END != 0;
        access_unit.last_seen = now;
        self.used_bytes += packet.payload.len();
        InsertResult::Accepted
    }

    fn take_next_complete(&mut self) -> Option<(u32, AccessUnit)> {
        let is_complete = self
            .access_units
            .get(&self.next_access_unit)
            .is_some_and(AccessUnit::is_complete);
        if !is_complete {
            return None;
        }
        let sequence = self.next_access_unit;
        self.next_access_unit += 1;
        let access_unit = self.access_units.remove(&sequence).unwrap();
        self.used_bytes -= access_unit.accounted_bytes;
        Some((sequence, access_unit))
    }

    fn skip_stalled(&mut self, now: Instant, stats: &mut Stats) -> bool {
        if let Some(access_unit) = self.access_units.get(&self.next_access_unit) {
            if !access_unit.is_complete()
                && now.duration_since(access_unit.last_seen) >= self.reorder_timeout
                && self.access_units.range((self.next_access_unit + 1)..).next().is_some()
            {
                let sequence = self.next_access_unit;
                let access_unit = self.access_units.remove(&sequence).unwrap();
                self.used_bytes -= access_unit.accounted_bytes;
                self.next_access_unit += 1;
                stats.access_units_dropped += 1;
                eprintln!(
                    "dropping incomplete AU sequence={sequence} fragments={}/{} bytes={}",
                    access_unit.received_fragments,
                    access_unit.fragments.len(),
                    access_unit.payload_bytes
                );
                return true;
            }
            return false;
        }

        let Some((&first_sequence, first_access_unit)) = self.access_units.first_key_value() else {
            return false;
        };
        if first_sequence <= self.next_access_unit
            || now.duration_since(first_access_unit.first_seen) < self.reorder_timeout
        {
            return false;
        }

        let missing = first_sequence - self.next_access_unit;
        eprintln!(
            "dropping missing AU range={}..{} count={missing}",
            self.next_access_unit,
            first_sequence - 1
        );
        stats.access_units_dropped += u64::from(missing);
        self.next_access_unit = first_sequence;
        true
    }
}

struct DecoderProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    description: &'static str,
}

impl DecoderProcess {
    fn spawn(kind: DecoderKind) -> io::Result<Self> {
        let (program, args, description): (&str, &[&str], &str) = match kind {
            DecoderKind::Ffplay => (
                "ffplay",
                &[
                    "-hide_banner",
                    "-loglevel",
                    "warning",
                    "-fflags",
                    "nobuffer",
                    "-flags",
                    "low_delay",
                    "-f",
                    "h264",
                    "-i",
                    "pipe:0",
                ],
                "ffplay",
            ),
            DecoderKind::FfmpegCheck => (
                "ffmpeg",
                &[
                    "-hide_banner",
                    "-loglevel",
                    "warning",
                    "-f",
                    "h264",
                    "-i",
                    "pipe:0",
                    "-f",
                    "null",
                    "-",
                ],
                "ffmpeg decode check",
            ),
        };

        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("decoder child has no stdin"))?;
        Ok(Self {
            child,
            stdin: Some(stdin),
            description,
        })
    }

    fn write_fragment(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stdin
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "decoder stdin is closed"))?
            .write_all(bytes)
    }

    fn finish(mut self) {
        drop(self.stdin.take());
        match self.child.wait() {
            Ok(status) => eprintln!("{} exited with {status}", self.description),
            Err(error) => eprintln!("could not wait for {}: {error}", self.description),
        }
    }
}

struct Output {
    file: File,
    decoder: Option<DecoderProcess>,
}

impl Output {
    fn open(config: &Config) -> io::Result<Self> {
        let file = File::create(&config.output)?;
        let decoder = config.decoder.map(DecoderProcess::spawn).transpose()?;
        Ok(Self { file, decoder })
    }

    fn write_access_unit(&mut self, access_unit: &AccessUnit) -> io::Result<()> {
        for fragment in access_unit.fragments.iter().flatten() {
            self.file.write_all(fragment)?;
            if let Some(decoder) = &mut self.decoder {
                if let Err(error) = decoder.write_fragment(fragment) {
                    eprintln!("decoder pipe failed ({error}); capture will continue to the file");
                    self.decoder = None;
                }
            }
        }
        self.file.flush()
    }

    fn finish(mut self) -> io::Result<()> {
        self.file.flush()?;
        if let Some(decoder) = self.decoder.take() {
            decoder.finish();
        }
        Ok(())
    }
}

fn drain_ready(
    reassembler: &mut Reassembler,
    output: &mut Output,
    stats: &mut Stats,
    now: Instant,
) -> io::Result<Option<u32>> {
    let mut finished_session = None;
    loop {
        if let Some((sequence, access_unit)) = reassembler.take_next_complete() {
            if !access_unit.has_annex_b_start_code() {
                stats.annex_b_failures += 1;
                stats.access_units_dropped += 1;
                eprintln!("dropping AU sequence={sequence}: missing Annex-B start code");
            } else {
                output.write_access_unit(&access_unit)?;
                stats.access_units_written += 1;
                stats.output_bytes += access_unit.payload_bytes as u64;
                eprintln!(
                    "AU sequence={sequence} bytes={} fragments={} keyframe={} session_end={}",
                    access_unit.payload_bytes,
                    access_unit.fragments.len(),
                    u8::from(access_unit.keyframe),
                    u8::from(access_unit.session_end)
                );
            }
            if access_unit.session_end {
                finished_session = Some(sequence);
                break;
            }
            continue;
        }
        if reassembler.skip_stalled(now, stats) {
            continue;
        }
        break;
    }
    Ok(finished_session)
}

fn run(config: Config) -> io::Result<()> {
    let socket = UdpSocket::bind(&config.bind)?;
    socket.set_read_timeout(Some(Duration::from_millis(100)))?;
    let local = socket.local_addr()?;
    let mut output = Output::open(&config)?;
    let mut reassembler = Reassembler::new(&config);
    let mut stats = Stats::default();
    let mut pinned: Option<(SocketAddr, u32)> = None;
    let mut datagram_sequence = DatagramSequence::new();
    let mut datagram = [0u8; MAX_DATAGRAM_LEN + 1];
    let started = Instant::now();
    let mut last_valid_packet = started;
    let mut last_report = started;

    eprintln!(
        "listening on {local}; output={} max_buffer_bytes={} max_au_bytes={} max_inflight_aus={}",
        config.output.display(),
        config.max_buffer_bytes,
        config.max_au_bytes,
        config.max_inflight_aus
    );

    loop {
        let now = Instant::now();
        match socket.recv_from(&mut datagram) {
            Ok((length, peer)) => {
                stats.datagrams_received += 1;
                stats.datagram_bytes += length as u64;
                let packet = match Packet::parse(&datagram[..length]) {
                    Ok(packet) => packet,
                    Err(error) => {
                        if error == PacketError::CrcMismatch {
                            stats.crc_failures += 1;
                        } else {
                            stats.header_drops += 1;
                        }
                        eprintln!("dropping datagram from {peer}: {error:?}");
                        continue;
                    }
                };

                match pinned {
                    None => {
                        pinned = Some((peer, packet.session_id));
                        eprintln!(
                            "pinned source={peer} session_id=0x{:08x}",
                            packet.session_id
                        );
                    }
                    Some((wanted_peer, wanted_session))
                        if peer != wanted_peer || packet.session_id != wanted_session =>
                    {
                        stats.foreign_drops += 1;
                        continue;
                    }
                    Some(_) => {}
                }

                stats.valid_datagrams += 1;
                last_valid_packet = now;
                datagram_sequence.observe(packet.datagram_seq, &mut stats);
                match reassembler.insert(&packet, now) {
                    InsertResult::Accepted => {}
                    InsertResult::Duplicate => stats.duplicate_fragments += 1,
                    InsertResult::Rejected(reason) => {
                        stats.capacity_drops += 1;
                        eprintln!(
                            "dropping AU fragment au={} fragment={}/{}: {reason}",
                            packet.access_unit_seq,
                            packet.fragment_index,
                            packet.fragment_count
                        );
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(error) => return Err(error),
        }

        let now = Instant::now();
        if drain_ready(&mut reassembler, &mut output, &mut stats, now)?.is_some() {
            eprintln!("session-end access unit drained");
            break;
        }
        if now.duration_since(last_report) >= Duration::from_secs(1) {
            stats.report(false);
            last_report = now;
        }
        if let Some(idle_exit) = config.idle_exit {
            if now.duration_since(last_valid_packet) >= idle_exit {
                eprintln!("idle timeout reached");
                break;
            }
        }
    }

    output.finish()?;
    stats.report(true);
    Ok(())
}

fn main() {
    let program = env::args()
        .next()
        .unwrap_or_else(|| "media_encode_udp_receiver".into());
    let config = match Config::parse() {
        Ok(Some(config)) => config,
        Ok(None) => {
            usage(&program);
            return;
        }
        Err(error) => {
            eprintln!("error: {error}\n");
            usage(&program);
            std::process::exit(2);
        }
    };

    if let Err(error) = run(config) {
        eprintln!("receiver failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn datagram(
        session: u32,
        datagram_seq: u32,
        access_unit_seq: u32,
        fragment_index: u16,
        fragment_count: u16,
        extra_flags: u8,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut flags = extra_flags;
        if fragment_index == 0 {
            flags |= FLAG_START;
        }
        if fragment_index + 1 == fragment_count {
            flags |= FLAG_END;
        }
        let mut bytes = vec![0u8; HEADER_LEN + payload.len()];
        bytes[0..4].copy_from_slice(&MAGIC);
        bytes[4] = VERSION;
        bytes[5] = flags;
        bytes[6..8].copy_from_slice(&(HEADER_LEN as u16).to_be_bytes());
        bytes[8..12].copy_from_slice(&session.to_be_bytes());
        bytes[12..16].copy_from_slice(&datagram_seq.to_be_bytes());
        bytes[16..20].copy_from_slice(&access_unit_seq.to_be_bytes());
        bytes[20..22].copy_from_slice(&fragment_index.to_be_bytes());
        bytes[22..24].copy_from_slice(&fragment_count.to_be_bytes());
        bytes[24..26].copy_from_slice(&(payload.len() as u16).to_be_bytes());
        bytes[28..32].copy_from_slice(&crc32(payload).to_be_bytes());
        bytes[HEADER_LEN..].copy_from_slice(payload);
        bytes
    }

    fn test_config() -> Config {
        Config {
            bind: String::new(),
            output: PathBuf::new(),
            max_buffer_bytes: 1024 * 1024,
            max_au_bytes: 1024 * 1024,
            max_inflight_aus: 4,
            reorder_timeout: Duration::from_millis(10),
            idle_exit: None,
            decoder: None,
        }
    }

    #[test]
    fn parses_tme1_header_and_payload() {
        let bytes = datagram(0x1122_3344, 7, 2, 0, 1, FLAG_KEYFRAME, b"\0\0\0\x01e");
        let packet = Packet::parse(&bytes).unwrap();
        assert_eq!(packet.session_id, 0x1122_3344);
        assert_eq!(packet.datagram_seq, 7);
        assert_eq!(packet.access_unit_seq, 2);
        assert_eq!(packet.fragment_index, 0);
        assert_eq!(packet.fragment_count, 1);
        assert_eq!(packet.payload, b"\0\0\0\x01e");
        assert_ne!(packet.flags & FLAG_KEYFRAME, 0);
    }

    #[test]
    fn rejects_payload_crc_corruption() {
        let mut bytes = datagram(1, 0, 0, 0, 1, 0, b"\0\0\x01e");
        *bytes.last_mut().unwrap() ^= 0x80;
        assert_eq!(Packet::parse(&bytes), Err(PacketError::CrcMismatch));
    }

    #[test]
    fn rejects_inconsistent_fragment_boundary_flags() {
        let mut bytes = datagram(1, 0, 0, 0, 2, 0, b"\0\0");
        bytes[5] &= !FLAG_START;
        assert_eq!(Packet::parse(&bytes), Err(PacketError::BadBoundaryFlags));
    }

    #[test]
    fn reassembles_out_of_order_fragments_with_a_hard_budget() {
        let now = Instant::now();
        let first = datagram(1, 10, 0, 0, 2, FLAG_KEYFRAME, b"\0\0\0");
        let second = datagram(1, 11, 0, 1, 2, 0, b"\x01ehello");
        let first = Packet::parse(&first).unwrap();
        let second = Packet::parse(&second).unwrap();
        let mut reassembler = Reassembler::new(&test_config());

        assert!(matches!(
            reassembler.insert(&second, now),
            InsertResult::Accepted
        ));
        assert!(reassembler.take_next_complete().is_none());
        assert!(matches!(
            reassembler.insert(&first, now),
            InsertResult::Accepted
        ));

        let (sequence, access_unit) = reassembler.take_next_complete().unwrap();
        assert_eq!(sequence, 0);
        assert!(access_unit.is_complete());
        assert!(access_unit.has_annex_b_start_code());
        assert_eq!(access_unit.payload_bytes, 10);
        assert_eq!(reassembler.used_bytes, 0);
    }

    #[test]
    fn rejects_payload_that_exceeds_reassembly_budget() {
        let now = Instant::now();
        let bytes = datagram(1, 0, 0, 0, 1, 0, b"\0\0\x01e");
        let packet = Packet::parse(&bytes).unwrap();
        let mut config = test_config();
        config.max_buffer_bytes = 4;
        config.max_au_bytes = 4;
        let mut reassembler = Reassembler::new(&config);
        assert!(matches!(
            reassembler.insert(&packet, now),
            InsertResult::Rejected("reassembly byte budget reached")
        ));
        assert_eq!(reassembler.used_bytes, 0);
    }
}
