//! Bounded wire format for firmware-owned PCI BAR reservations.
//!
//! Shared by the UEFI producer and the kernel consumer. Parsing validates the
//! complete module before returning a matching reservation; no allocation or
//! hardware access is performed here.

pub(crate) const MODULE_CMDLINE: &[u8] = b"trueos.pci.bar-reservations.v1";
pub(crate) const HEADER_BYTES: usize = 16;
pub(crate) const RECORD_BYTES: usize = 32;
pub(crate) const MAX_RECORDS: usize = 16;
const MAGIC: &[u8; 8] = b"TPBARS01";
const PAGE_BYTES: u64 = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    BufferTooSmall,
    InvalidMagic,
    TooManyRecords,
    InvalidRecordSize,
    ReservedBytes,
    LengthMismatch,
    InvalidPciAddress,
    InvalidMemoryRange,
    DuplicateKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Reservation {
    pub segment: u16,
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub bar_index: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub base: u64,
    pub length: u64,
}

impl Reservation {
    fn validate(self) -> Result<(), Error> {
        if self.slot >= 32 || self.function >= 8 || self.bar_index >= 6 {
            return Err(Error::InvalidPciAddress);
        }
        if self.base == 0
            || self.base % PAGE_BYTES != 0
            || self.length < PAGE_BYTES
            || self.length % PAGE_BYTES != 0
            || !self.length.is_power_of_two()
            || self.base % self.length != 0
            || self.base.checked_add(self.length).is_none()
        {
            return Err(Error::InvalidMemoryRange);
        }
        Ok(())
    }

    fn same_key(self, other: Self) -> bool {
        self.segment == other.segment
            && self.bus == other.bus
            && self.slot == other.slot
            && self.function == other.function
            && self.bar_index == other.bar_index
            && self.vendor_id == other.vendor_id
            && self.device_id == other.device_id
    }

    /// Encode one record into the first 32 bytes, leaving any suffix intact.
    /// Invalid input leaves the destination untouched.
    pub(crate) fn encode(self, output: &mut [u8]) -> Result<(), Error> {
        self.validate()?;
        let output = output
            .get_mut(..RECORD_BYTES)
            .ok_or(Error::BufferTooSmall)?;
        output.fill(0);
        output[..2].copy_from_slice(&self.segment.to_le_bytes());
        output[2] = self.bus;
        output[3] = self.slot;
        output[4] = self.function;
        output[5] = self.bar_index;
        output[8..10].copy_from_slice(&self.vendor_id.to_le_bytes());
        output[10..12].copy_from_slice(&self.device_id.to_le_bytes());
        output[16..24].copy_from_slice(&self.base.to_le_bytes());
        output[24..32].copy_from_slice(&self.length.to_le_bytes());
        Ok(())
    }

    fn decode(input: &[u8]) -> Result<Self, Error> {
        let input = input.get(..RECORD_BYTES).ok_or(Error::BufferTooSmall)?;
        if input[6..8]
            .iter()
            .chain(&input[12..16])
            .any(|byte| *byte != 0)
        {
            return Err(Error::ReservedBytes);
        }
        let reservation = Self {
            segment: u16::from_le_bytes(input[..2].try_into().unwrap()),
            bus: input[2],
            slot: input[3],
            function: input[4],
            bar_index: input[5],
            vendor_id: u16::from_le_bytes(input[8..10].try_into().unwrap()),
            device_id: u16::from_le_bytes(input[10..12].try_into().unwrap()),
            base: u64::from_le_bytes(input[16..24].try_into().unwrap()),
            length: u64::from_le_bytes(input[24..32].try_into().unwrap()),
        };
        reservation.validate()?;
        Ok(reservation)
    }
}

/// Encode only the header prefix. The caller encodes exactly `count` records
/// after it and publishes precisely `HEADER_BYTES + count * RECORD_BYTES` bytes.
pub(crate) fn encode_header(output: &mut [u8], count: usize) -> Result<(), Error> {
    if count > MAX_RECORDS {
        return Err(Error::TooManyRecords);
    }
    let output = output
        .get_mut(..HEADER_BYTES)
        .ok_or(Error::BufferTooSmall)?;
    output.fill(0);
    output[..8].copy_from_slice(MAGIC);
    output[8..10].copy_from_slice(&(count as u16).to_le_bytes());
    output[10..12].copy_from_slice(&(RECORD_BYTES as u16).to_le_bytes());
    Ok(())
}

/// Validate every record and return only the complete requested PCI identity.
/// Malformed unrelated records and duplicate complete keys reject the module.
pub(crate) fn find(
    bytes: &[u8],
    segment: u16,
    bus: u8,
    slot: u8,
    function: u8,
    vendor_id: u16,
    device_id: u16,
    bar_index: u8,
) -> Result<Option<Reservation>, Error> {
    let header = bytes.get(..HEADER_BYTES).ok_or(Error::BufferTooSmall)?;
    if &header[..8] != MAGIC {
        return Err(Error::InvalidMagic);
    }
    let count = usize::from(u16::from_le_bytes(header[8..10].try_into().unwrap()));
    if count > MAX_RECORDS {
        return Err(Error::TooManyRecords);
    }
    if usize::from(u16::from_le_bytes(header[10..12].try_into().unwrap())) != RECORD_BYTES {
        return Err(Error::InvalidRecordSize);
    }
    if header[12..16].iter().any(|byte| *byte != 0) {
        return Err(Error::ReservedBytes);
    }
    if bytes.len() != HEADER_BYTES + count * RECORD_BYTES {
        return Err(Error::LengthMismatch);
    }
    if slot >= 32 || function >= 8 || bar_index >= 6 {
        return Err(Error::InvalidPciAddress);
    }
    let mut records = [None; MAX_RECORDS];
    let mut matched = None;
    for index in 0..count {
        let start = HEADER_BYTES + index * RECORD_BYTES;
        let reservation = Reservation::decode(&bytes[start..start + RECORD_BYTES])?;
        if records[..index]
            .iter()
            .flatten()
            .any(|previous| reservation.same_key(*previous))
        {
            return Err(Error::DuplicateKey);
        }
        records[index] = Some(reservation);
        if reservation.segment == segment
            && reservation.bus == bus
            && reservation.slot == slot
            && reservation.function == function
            && reservation.vendor_id == vendor_id
            && reservation.device_id == device_id
            && reservation.bar_index == bar_index
        {
            matched = Some(reservation);
        }
    }
    Ok(matched)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACKPAD: Reservation = Reservation {
        segment: 0,
        bus: 0,
        slot: 0x15,
        function: 0,
        bar_index: 0,
        vendor_id: 0x8086,
        device_id: 0xa0e8,
        base: 0x4017_000000,
        length: 4096,
    };

    fn encoded(records: &[Reservation]) -> std::vec::Vec<u8> {
        let mut output = std::vec![0xa5; HEADER_BYTES + records.len() * RECORD_BYTES];
        encode_header(&mut output, records.len()).unwrap();
        for (index, record) in records.iter().enumerate() {
            record
                .encode(&mut output[HEADER_BYTES + index * RECORD_BYTES..])
                .unwrap();
        }
        output
    }

    fn lookup(bytes: &[u8], identity: Reservation) -> Result<Option<Reservation>, Error> {
        find(
            bytes,
            identity.segment,
            identity.bus,
            identity.slot,
            identity.function,
            identity.vendor_id,
            identity.device_id,
            identity.bar_index,
        )
    }

    #[test]
    fn two_devices_round_trip_and_every_identity_field_participates_in_lookup() {
        let screen = Reservation {
            function: 1,
            device_id: 0xa0e9,
            base: TRACKPAD.base + 4096,
            ..TRACKPAD
        };
        let bytes = encoded(&[TRACKPAD, screen]);
        assert_eq!(lookup(&bytes, TRACKPAD), Ok(Some(TRACKPAD)));
        assert_eq!(lookup(&bytes, screen), Ok(Some(screen)));
        for other in [
            Reservation {
                segment: 1,
                ..TRACKPAD
            },
            Reservation { bus: 1, ..TRACKPAD },
            Reservation {
                slot: 0x16,
                ..TRACKPAD
            },
            Reservation {
                function: 2,
                ..TRACKPAD
            },
            Reservation {
                bar_index: 2,
                ..TRACKPAD
            },
            Reservation {
                vendor_id: 0x8087,
                ..TRACKPAD
            },
            Reservation {
                device_id: 0xa0e9,
                ..TRACKPAD
            },
        ] {
            assert_eq!(lookup(&bytes, other), Ok(None));
        }
    }

    #[test]
    fn wire_offsets_and_endian_order_are_explicit() {
        let bytes = encoded(&[TRACKPAD]);
        assert_eq!(&bytes[..16], b"TPBARS01\x01\x00\x20\x00\0\0\0\0");
        assert_eq!(
            &bytes[16..32],
            &[
                0, 0, 0, 0x15, 0, 0, 0, 0, 0x86, 0x80, 0xe8, 0xa0, 0, 0, 0, 0
            ]
        );
        assert_eq!(&bytes[32..40], &TRACKPAD.base.to_le_bytes());
        assert_eq!(&bytes[40..48], &4096u64.to_le_bytes());
    }

    #[test]
    fn empty_and_maximum_record_counts_are_bounded() {
        assert_eq!(lookup(&encoded(&[]), TRACKPAD), Ok(None));
        let records: std::vec::Vec<_> = (0..MAX_RECORDS)
            .map(|index| Reservation {
                bus: index as u8,
                base: TRACKPAD.base + index as u64 * 4096,
                ..TRACKPAD
            })
            .collect();
        let bytes = encoded(&records);
        assert_eq!(
            lookup(&bytes, records[MAX_RECORDS - 1]),
            Ok(Some(records[MAX_RECORDS - 1]))
        );
        assert_eq!(
            encode_header(&mut [0; HEADER_BYTES], MAX_RECORDS + 1),
            Err(Error::TooManyRecords)
        );
        let mut too_many = bytes;
        too_many[8..10].copy_from_slice(&17u16.to_le_bytes());
        assert_eq!(lookup(&too_many, TRACKPAD), Err(Error::TooManyRecords));
    }

    #[test]
    fn every_incomplete_prefix_and_trailing_bytes_are_rejected() {
        let bytes = encoded(&[TRACKPAD]);
        for length in 0..bytes.len() {
            assert!(
                lookup(&bytes[..length], TRACKPAD).is_err(),
                "prefix {length}"
            );
        }
        let mut extra = bytes;
        extra.push(0);
        assert_eq!(lookup(&extra, TRACKPAD), Err(Error::LengthMismatch));
    }

    #[test]
    fn header_magic_version_record_size_and_reserved_bytes_are_checked() {
        for index in 0..HEADER_BYTES {
            let mut bytes = encoded(&[TRACKPAD]);
            bytes[index] ^= 0x80;
            assert!(lookup(&bytes, TRACKPAD).is_err(), "header byte {index}");
        }
        for offset in [6, 7, 12, 13, 14, 15] {
            let mut bytes = encoded(&[TRACKPAD]);
            bytes[HEADER_BYTES + offset] = 1;
            assert_eq!(lookup(&bytes, TRACKPAD), Err(Error::ReservedBytes));
        }
    }

    #[test]
    fn duplicate_complete_keys_reject_the_whole_module() {
        let other_range = Reservation {
            base: TRACKPAD.base + 4096,
            ..TRACKPAD
        };
        let bytes = encoded(&[TRACKPAD, other_range]);
        assert_eq!(lookup(&bytes, TRACKPAD), Err(Error::DuplicateKey));
        assert_eq!(
            lookup(&bytes, Reservation { bus: 5, ..TRACKPAD }),
            Err(Error::DuplicateKey)
        );
        let other_device = Reservation {
            device_id: 0xa0e9,
            base: TRACKPAD.base + 4096,
            ..TRACKPAD
        };
        assert_eq!(
            lookup(&encoded(&[TRACKPAD, other_device]), other_device),
            Ok(Some(other_device))
        );
    }

    #[test]
    fn a_matching_first_record_does_not_hide_a_malformed_later_record() {
        let other = Reservation {
            function: 1,
            base: TRACKPAD.base + 4096,
            ..TRACKPAD
        };
        let mut bytes = encoded(&[TRACKPAD, other]);
        bytes[HEADER_BYTES + RECORD_BYTES + 3] = 32;
        assert_eq!(lookup(&bytes, TRACKPAD), Err(Error::InvalidPciAddress));
    }

    #[test]
    fn invalid_pci_and_memory_ranges_are_rejected_before_encoding() {
        let invalid = [
            Reservation {
                slot: 32,
                ..TRACKPAD
            },
            Reservation {
                function: 8,
                ..TRACKPAD
            },
            Reservation {
                bar_index: 6,
                ..TRACKPAD
            },
            Reservation {
                base: 0,
                ..TRACKPAD
            },
            Reservation {
                base: TRACKPAD.base + 1,
                ..TRACKPAD
            },
            Reservation {
                length: 0,
                ..TRACKPAD
            },
            Reservation {
                length: 2048,
                ..TRACKPAD
            },
            Reservation {
                length: 8193,
                ..TRACKPAD
            },
            Reservation {
                length: 12288,
                ..TRACKPAD
            },
            Reservation {
                base: TRACKPAD.base + 4096,
                length: 8192,
                ..TRACKPAD
            },
            Reservation {
                base: u64::MAX - 4095,
                ..TRACKPAD
            },
        ];
        for record in invalid {
            let mut output = [0xa5; RECORD_BYTES];
            assert!(record.encode(&mut output).is_err(), "{record:?}");
            assert_eq!(output, [0xa5; RECORD_BYTES]);
        }
    }

    #[test]
    fn corrupt_wire_addresses_and_memory_ranges_are_rejected_on_decode() {
        for (offset, invalid) in [(3, 32), (4, 8), (5, 6)] {
            let mut bytes = encoded(&[TRACKPAD]);
            bytes[HEADER_BYTES + offset] = invalid;
            assert_eq!(lookup(&bytes, TRACKPAD), Err(Error::InvalidPciAddress));
        }
        for base in [0, TRACKPAD.base + 1, u64::MAX - 4095] {
            let mut bytes = encoded(&[TRACKPAD]);
            bytes[HEADER_BYTES + 16..HEADER_BYTES + 24].copy_from_slice(&base.to_le_bytes());
            assert_eq!(lookup(&bytes, TRACKPAD), Err(Error::InvalidMemoryRange));
        }
        for length in [0u64, 2048, 8193, 12288, 1 << 63] {
            let mut bytes = encoded(&[TRACKPAD]);
            bytes[HEADER_BYTES + 24..HEADER_BYTES + 32].copy_from_slice(&length.to_le_bytes());
            assert_eq!(lookup(&bytes, TRACKPAD), Err(Error::InvalidMemoryRange));
        }
        let mut bytes = encoded(&[TRACKPAD]);
        bytes[HEADER_BYTES + 16..HEADER_BYTES + 24]
            .copy_from_slice(&(TRACKPAD.base + 4096).to_le_bytes());
        bytes[HEADER_BYTES + 24..HEADER_BYTES + 32].copy_from_slice(&8192u64.to_le_bytes());
        assert_eq!(lookup(&bytes, TRACKPAD), Err(Error::InvalidMemoryRange));
    }

    #[test]
    fn largest_identity_values_and_large_valid_memory_ranges_round_trip() {
        let record = Reservation {
            segment: u16::MAX,
            bus: u8::MAX,
            slot: 31,
            function: 7,
            bar_index: 5,
            base: 1 << 62,
            length: 1 << 62,
            ..TRACKPAD
        };
        assert_eq!(lookup(&encoded(&[record]), record), Ok(Some(record)));
    }

    #[test]
    fn encoding_bounds_and_suffix_preservation_are_checked() {
        let mut output = [0xa5; HEADER_BYTES + RECORD_BYTES + 1];
        assert_eq!(
            encode_header(&mut output[..15], 1),
            Err(Error::BufferTooSmall)
        );
        assert_eq!(output, [0xa5; HEADER_BYTES + RECORD_BYTES + 1]);
        encode_header(&mut output, 1).unwrap();
        assert_eq!(
            TRACKPAD.encode(&mut output[16..47]),
            Err(Error::BufferTooSmall)
        );
        assert_eq!(&output[16..], &[0xa5; RECORD_BYTES + 1]);
        TRACKPAD.encode(&mut output[16..]).unwrap();
        assert_eq!(output[48], 0xa5);
        assert_eq!(lookup(&output[..48], TRACKPAD), Ok(Some(TRACKPAD)));
    }
}
