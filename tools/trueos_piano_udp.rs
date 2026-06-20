//! Tiny standalone TRUEOS piano UDP sender.
//!
//! This is a normal Rust library file: no crates, no TRUEOS internals, just `std`.
//!
//! Example:
//!
//! ```no_run
//! mod trueos_piano_udp;
//!
//! use std::{thread, time::Duration};
//! use trueos_piano_udp::PianoUdp;
//!
//! fn main() -> std::io::Result<()> {
//!     let mut piano = PianoUdp::open_host("192.168.1.50")?;
//!
//!     piano.note_on(60, Some(100))?; // Middle C on, optional velocity 1..127.
//!     thread::sleep(Duration::from_millis(400));
//!     piano.note_off(60)?;           // Middle C off.
//!
//!     Ok(())
//! }
//! ```

use std::fmt::Write as _;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

pub const TRUEOS_PIANO_UDP_PORT: u16 = 9696;
pub const TRUEOS_PIANO_BASE_NOTE: u8 = 36;
pub const TRUEOS_PIANO_KEY_COUNT: u8 = 96;

pub struct PianoUdp {
    socket: UdpSocket,
    target: SocketAddr,
    seq: u16,
}

impl PianoUdp {
    /// Open a sender to the default TRUEOS piano UDP port, 9696.
    pub fn open_host(host: &str) -> io::Result<Self> {
        Self::open((host, TRUEOS_PIANO_UDP_PORT))
    }

    /// Open a sender to an explicit socket address, for example
    /// `"192.168.1.50:9696"`.
    pub fn open<A: ToSocketAddrs>(target: A) -> io::Result<Self> {
        let target = target.to_socket_addrs()?.next().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "target resolved to no addresses")
        })?;
        let bind_addr = if target.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };

        Ok(Self {
            socket: UdpSocket::bind(bind_addr)?,
            target,
            seq: 0,
        })
    }

    /// Send one MIDI-style note on.
    ///
    /// `note` is a MIDI note number. Middle C is 60.
    /// `velocity` is optional; valid MIDI-style values are 1..127.
    pub fn note_on(&mut self, note: u8, velocity: Option<u8>) -> io::Result<()> {
        let key = note_to_key_index(note)?;
        let mask = 1u128 << key;
        let delta = velocity.map(delta_for_velocity).unwrap_or(64);

        let mut msg = format!("piano seq={} mask=0x{mask:024x} deltas=", self.next_seq());
        for idx in 0..=key {
            if idx != 0 {
                msg.push(',');
            }
            let _ = write!(msg, "{}", if idx == key { delta } else { 0 });
        }

        self.send(msg.as_bytes())
    }

    /// Send note off.
    ///
    /// For this one-note TRUEOS text protocol sender, off means "no keys held",
    /// so the mask is zero.
    pub fn note_off(&mut self, _note: u8) -> io::Result<()> {
        let msg = format!("piano seq={} mask=0x000000000000000000000000", self.next_seq());
        self.send(msg.as_bytes())
    }

    /// Same as `note_off`, useful when you just want silence.
    pub fn all_notes_off(&mut self) -> io::Result<()> {
        self.note_off(0)
    }

    pub fn target(&self) -> SocketAddr {
        self.target
    }

    fn send(&self, bytes: &[u8]) -> io::Result<()> {
        self.socket.send_to(bytes, self.target).map(|_| ())
    }

    fn next_seq(&mut self) -> u16 {
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }
}

fn note_to_key_index(note: u8) -> io::Result<u32> {
    let key = note.checked_sub(TRUEOS_PIANO_BASE_NOTE).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "note is below TRUEOS piano range 36..131",
        )
    })?;

    if key >= TRUEOS_PIANO_KEY_COUNT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "note is above TRUEOS piano range 36..131",
        ));
    }

    Ok(u32::from(key))
}

fn delta_for_velocity(velocity: u8) -> i16 {
    let wanted = velocity.clamp(1, 127);
    let mut best_delta = 12;
    let mut best_error = u8::MAX;

    for delta in 12..=100 {
        let got = trueos_velocity_from_fresh_delta(delta);
        let error = got.abs_diff(wanted);
        if error < best_error {
            best_delta = delta;
            best_error = error;
        }
    }

    best_delta
}

fn trueos_velocity_from_fresh_delta(delta: i16) -> u8 {
    let pressure_velocity = trueos_velocity_from_delta(delta);
    let attack_bonus = ((delta.min(48) as u16) * 10 / 48) as u8;
    pressure_velocity.saturating_add(attack_bonus).min(127)
}

fn trueos_velocity_from_delta(delta: i16) -> u8 {
    let delta = delta.clamp(12, 100) as u16;
    let linear_q8 = (delta - 12) * 255 / (100 - 12);
    let square_q8 = linear_q8 * linear_q8 / 255;
    let curve_q8 = (linear_q8 + square_q8) / 2;
    1 + (curve_q8 * 126 / 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_middle_c_to_key_24() {
        assert_eq!(note_to_key_index(60).unwrap(), 24);
    }

    #[test]
    fn maps_velocity_to_active_delta() {
        assert!(delta_for_velocity(1) >= 12);
        assert!(delta_for_velocity(127) <= 100);
    }
}
