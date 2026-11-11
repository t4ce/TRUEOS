//! HID-over-I2C framing and commands, independent of the controller and GPIO.
//! The caller reads input only while the device's interrupt line is asserted.

pub(crate) const DESCRIPTOR_BYTES: usize = 30;
pub(crate) const MAX_REPORT_DESCRIPTOR: usize = 4096;
pub(crate) const MAX_INPUT: usize = 512;
const MAX_FEATURE: usize = 256;

pub(crate) trait Transport {
    type Error;
    async fn transfer(&mut self, write: &[u8], read: &mut [u8]) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error<E> {
    Transport(E),
    InvalidDescriptor,
    BufferTooSmall,
    InvalidReport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Descriptor {
    pub report_descriptor_length: u16,
    pub report_descriptor_register: u16,
    pub input_register: u16,
    pub max_input_length: u16,
    pub command_register: u16,
    pub data_register: u16,
    pub vendor_id: u16,
    pub product_id: u16,
    pub version_id: u16,
}

impl Descriptor {
    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < DESCRIPTOR_BYTES || word(bytes, 0) != 30 || word(bytes, 2) != 0x0100 {
            return None;
        }
        let descriptor = Self {
            report_descriptor_length: word(bytes, 4),
            report_descriptor_register: word(bytes, 6),
            input_register: word(bytes, 8),
            max_input_length: word(bytes, 10),
            command_register: word(bytes, 16),
            data_register: word(bytes, 18),
            vendor_id: word(bytes, 20),
            product_id: word(bytes, 22),
            version_id: word(bytes, 24),
        };
        if descriptor.report_descriptor_length == 0
            || usize::from(descriptor.report_descriptor_length) > MAX_REPORT_DESCRIPTOR
            || descriptor.max_input_length < 2
            || usize::from(descriptor.max_input_length) > MAX_INPUT
        {
            return None;
        }
        Some(descriptor)
    }

    pub(crate) async fn read<T: Transport>(
        bus: &mut T,
        register: u16,
    ) -> Result<Self, Error<T::Error>> {
        let mut bytes = [0; DESCRIPTOR_BYTES];
        bus.transfer(&register.to_le_bytes(), &mut bytes)
            .await
            .map_err(Error::Transport)?;
        Self::parse(&bytes).ok_or(Error::InvalidDescriptor)
    }

    pub(crate) async fn report_descriptor<T: Transport>(
        self,
        bus: &mut T,
        output: &mut [u8],
    ) -> Result<usize, Error<T::Error>> {
        let length = usize::from(self.report_descriptor_length);
        let output = output.get_mut(..length).ok_or(Error::BufferTooSmall)?;
        bus.transfer(&self.report_descriptor_register.to_le_bytes(), output)
            .await
            .map_err(Error::Transport)?;
        Ok(length)
    }

    pub(crate) async fn power_on<T: Transport>(self, bus: &mut T) -> Result<(), Error<T::Error>> {
        self.simple_command(bus, 0x08).await
    }

    /// Reset completion is a zero-length input frame on the interrupt line;
    /// the transport owner must wait for that with a bounded deadline.
    pub(crate) async fn reset<T: Transport>(self, bus: &mut T) -> Result<(), Error<T::Error>> {
        self.simple_command(bus, 0x01).await
    }

    async fn simple_command<T: Transport>(
        self,
        bus: &mut T,
        opcode: u8,
    ) -> Result<(), Error<T::Error>> {
        let [low, high] = self.command_register.to_le_bytes();
        bus.transfer(&[low, high, 0, opcode], &mut [])
            .await
            .map_err(Error::Transport)
    }

    /// `report` contains the report ID followed by its feature fields, matching
    /// the descriptor decoder's output. Extended IDs use the standard escape.
    pub(crate) async fn set_feature<T: Transport>(
        self,
        bus: &mut T,
        report: &[u8],
    ) -> Result<(), Error<T::Error>> {
        let (&report_id, _) = report.split_first().ok_or(Error::InvalidReport)?;
        if report_id == 0 || report.len() > MAX_FEATURE {
            return Err(Error::InvalidReport);
        }
        let mut command = [0u8; MAX_FEATURE + 9];
        let mut length = self.report_command(&mut command, 0x03, report_id);
        command[length..length + 2].copy_from_slice(&self.data_register.to_le_bytes());
        length += 2;
        command[length..length + 2].copy_from_slice(&((report.len() + 2) as u16).to_le_bytes());
        length += 2;
        command[length..length + report.len()].copy_from_slice(report);
        length += report.len();
        bus.transfer(&command[..length], &mut [])
            .await
            .map_err(Error::Transport)
    }

    /// Returns feature bytes including the report ID, after validating framing.
    pub(crate) async fn get_feature<T: Transport>(
        self,
        bus: &mut T,
        report_id: u8,
        output: &mut [u8],
    ) -> Result<usize, Error<T::Error>> {
        if report_id == 0 || output.is_empty() || output.len() > MAX_FEATURE {
            return Err(Error::InvalidReport);
        }
        let mut command = [0u8; 7];
        let mut length = self.report_command(&mut command, 0x02, report_id);
        command[length..length + 2].copy_from_slice(&self.data_register.to_le_bytes());
        length += 2;
        let mut response = [0u8; MAX_FEATURE + 2];
        bus.transfer(&command[..length], &mut response[..output.len() + 2])
            .await
            .map_err(Error::Transport)?;
        let frame_length = usize::from(word(&response, 0));
        if frame_length < 3 || frame_length > output.len() + 2 || response[2] != report_id {
            return Err(Error::InvalidReport);
        }
        output[..frame_length - 2].copy_from_slice(&response[2..frame_length]);
        Ok(frame_length - 2)
    }

    fn report_command(self, output: &mut [u8], opcode: u8, report_id: u8) -> usize {
        output[..2].copy_from_slice(&self.command_register.to_le_bytes());
        output[2] = 0x30 | report_id.min(0x0f); // Feature report type.
        output[3] = opcode;
        if report_id >= 0x0f {
            output[4] = report_id;
            5
        } else {
            4
        }
    }

    /// Input is an interrupt-driven plain I2C read, not GET_REPORT or a write
    /// of wInputRegister. Read the advertised maximum and honor its LE16 prefix.
    pub(crate) async fn read_input<T: Transport>(
        self,
        bus: &mut T,
        output: &mut [u8],
    ) -> Result<Input, Error<T::Error>> {
        let length = usize::from(self.max_input_length);
        let output = output.get_mut(..length).ok_or(Error::BufferTooSmall)?;
        bus.transfer(&[], output).await.map_err(Error::Transport)?;
        match usize::from(word(output, 0)) {
            0 => Ok(Input::ResetComplete),
            2 => Ok(Input::Empty),
            frame_length if frame_length >= 3 && frame_length <= length => Ok(Input::Report {
                length: frame_length,
            }),
            _ => Err(Error::InvalidReport),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Input {
    ResetComplete,
    Empty,
    Report { length: usize },
}

fn word(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{
        future::Future,
        task::{Context, Poll, Waker},
    };

    fn run<T>(future: impl Future<Output = T>) -> T {
        let mut future = core::pin::pin!(future);
        let mut context = Context::from_waker(Waker::noop());
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("test bus unexpectedly blocked"),
        }
    }

    struct Fake {
        expected_write: std::vec::Vec<u8>,
        response: std::vec::Vec<u8>,
        fail: bool,
        calls: usize,
    }

    impl Transport for Fake {
        type Error = u8;
        async fn transfer(&mut self, write: &[u8], read: &mut [u8]) -> Result<(), u8> {
            self.calls += 1;
            assert_eq!(write, self.expected_write);
            if self.fail {
                return Err(7);
            }
            assert_eq!(read.len(), self.response.len());
            read.copy_from_slice(&self.response);
            Ok(())
        }
    }

    fn bus(write: &[u8], response: &[u8]) -> Fake {
        Fake {
            expected_write: write.to_vec(),
            response: response.to_vec(),
            fail: false,
            calls: 0,
        }
    }

    fn descriptor_bytes() -> [u8; 30] {
        let mut bytes = [0; 30];
        for (offset, value) in [
            (0, 30u16),
            (2, 0x100),
            (4, 665),
            (6, 0x21),
            (8, 0x22),
            (10, 66),
            (16, 0x24),
            (18, 0x25),
            (20, 0x06cb),
            (22, 0xce1a),
        ] {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn descriptor() -> Descriptor {
        Descriptor::parse(&descriptor_bytes()).unwrap()
    }

    #[test]
    fn descriptor_read_checks_length_version_and_bounds() {
        let bytes = descriptor_bytes();
        let mut fake = bus(&[0x20, 0], &bytes);
        let got = run(Descriptor::read(&mut fake, 0x20)).unwrap();
        assert_eq!(
            (got.vendor_id, got.product_id, got.max_input_length),
            (0x06cb, 0xce1a, 66)
        );
        for offset in [0, 2, 4, 10] {
            let mut invalid = bytes;
            invalid[offset..offset + 2].copy_from_slice(&0xffffu16.to_le_bytes());
            assert_eq!(Descriptor::parse(&invalid), None);
        }
        assert_eq!(Descriptor::parse(&bytes[..29]), None);
    }

    #[test]
    fn descriptor_and_input_reads_do_not_confuse_register_protocols() {
        let mut output = [0u8; 665];
        let mut fake = bus(&[0x21, 0], &[0; 665]);
        assert_eq!(
            run(descriptor().report_descriptor(&mut fake, &mut output)),
            Ok(665)
        );
        let mut frame = [0; 66];
        frame[..6].copy_from_slice(&[6, 0, 2, 1, 3, 4]);
        let mut fake = bus(&[], &frame);
        let mut output = [0; 66];
        assert_eq!(
            run(descriptor().read_input(&mut fake, &mut output)),
            Ok(Input::Report { length: 6 })
        );
        assert_eq!(&output[..6], &frame[..6]);
    }

    #[test]
    fn power_and_reset_commands_have_no_spurious_data_register() {
        assert_eq!(
            run(descriptor().power_on(&mut bus(&[0x24, 0, 0, 8], &[]))),
            Ok(())
        );
        assert_eq!(
            run(descriptor().reset(&mut bus(&[0x24, 0, 0, 1], &[]))),
            Ok(())
        );
    }

    #[test]
    fn feature_set_encodes_type_length_and_extended_id() {
        let mut fake = bus(&[0x24, 0, 0x34, 3, 0x25, 0, 4, 0, 4, 3], &[]);
        assert_eq!(run(descriptor().set_feature(&mut fake, &[4, 3])), Ok(()));
        let mut fake = bus(&[0x24, 0, 0x3f, 3, 0x22, 0x25, 0, 4, 0, 0x22, 3], &[]);
        assert_eq!(run(descriptor().set_feature(&mut fake, &[0x22, 3])), Ok(()));
    }

    #[test]
    fn feature_get_validates_report_id_and_returned_length() {
        let mut fake = bus(&[0x24, 0, 0x34, 2, 0x25, 0], &[4, 0, 4, 3]);
        let mut output = [0; 2];
        assert_eq!(
            run(descriptor().get_feature(&mut fake, 4, &mut output)),
            Ok(2)
        );
        assert_eq!(output, [4, 3]);
        for response in [[4, 0, 9, 3], [5, 0, 4, 3], [0, 0, 4, 3]] {
            let mut fake = bus(&[0x24, 0, 0x34, 2, 0x25, 0], &response);
            assert_eq!(
                run(descriptor().get_feature(&mut fake, 4, &mut output)),
                Err(Error::InvalidReport)
            );
        }
    }

    #[test]
    fn reset_empty_and_malformed_frames_never_become_pointer_reports() {
        for (prefix, expected) in [
            (0u16, Ok(Input::ResetComplete)),
            (2, Ok(Input::Empty)),
            (1, Err(Error::InvalidReport)),
            (67, Err(Error::InvalidReport)),
            (0xffff, Err(Error::InvalidReport)),
        ] {
            let mut frame = [0; 66];
            frame[..2].copy_from_slice(&prefix.to_le_bytes());
            let mut fake = bus(&[], &frame);
            assert_eq!(
                run(descriptor().read_input(&mut fake, &mut [0; 66])),
                expected
            );
        }
    }

    #[test]
    fn short_buffers_and_transport_errors_do_not_expose_partial_input() {
        let mut fake = bus(&[], &[]);
        assert_eq!(
            run(descriptor().read_input(&mut fake, &mut [0; 4])),
            Err(Error::BufferTooSmall)
        );
        assert_eq!(fake.calls, 0);
        fake.fail = true;
        assert_eq!(
            run(descriptor().read_input(&mut fake, &mut [0; 66])),
            Err(Error::Transport(7))
        );
    }
}
