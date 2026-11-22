//! Locality-zero TPM Interface Specification FIFO over PC-client MMIO.
use core::ptr::NonNull;
use trueos_time::{Instant, Timer};

use super::{
    COMMAND_TIMEOUT, Error, HEADER_BYTES, LOCALITY_BYTES, POLL_INTERVAL, REGISTER_TIMEOUT,
    response_length,
};

const ACCESS: usize = 0x00;
const STATUS: usize = 0x18;
const DATA_FIFO: usize = 0x24;
const DID_VID: usize = 0xf00;

const ACCESS_VALID: u8 = 0x80;
const ACTIVE_LOCALITY: u8 = 0x20;
const REQUEST_USE: u8 = 0x02;

const STATUS_VALID: u8 = 0x80;
const COMMAND_READY: u8 = 0x40;
const GO: u8 = 0x20;
const DATA_AVAILABLE: u8 = 0x10;
const EXPECT: u8 = 0x08;

pub(super) const STANDARD_BASE: u64 = 0xfed4_0000;

pub(super) struct Fifo {
    base: u64,
    registers: NonNull<u8>,
}

impl Fifo {
    pub(super) fn map(base: u64) -> Result<Self, Error> {
        if base == 0 || base & (LOCALITY_BYTES as u64 - 1) != 0 || base >= 1 << 52 {
            return Err(Error::InvalidControlAddress);
        }
        let registers = crate::pci::mmio::map_mmio_region_exact(base, LOCALITY_BYTES)
            .map_err(|_| Error::MmioMap)?;
        let device = Self { base, registers };
        // DID_VID is a 32-bit register. Its low word is the vendor ID.
        if matches!(device.read_word(DID_VID) as u16, 0 | u16::MAX) {
            return Err(Error::DeviceNotPresent);
        }
        Ok(device)
    }

    pub(super) fn address(&self) -> u64 {
        self.base
    }

    fn read_byte(&self, offset: usize) -> u8 {
        unsafe { self.registers.as_ptr().add(offset).read_volatile() }
    }

    fn write_byte(&self, offset: usize, value: u8) {
        unsafe { self.registers.as_ptr().add(offset).write_volatile(value) }
    }

    fn read_word(&self, offset: usize) -> u32 {
        unsafe {
            self.registers
                .as_ptr()
                .add(offset)
                .cast::<u32>()
                .read_volatile()
        }
    }

    async fn wait(&self, offset: usize, mask: u8, value: u8, deadline: Instant) -> Result<u8, ()> {
        loop {
            let status = self.read_byte(offset);
            if status & mask == value {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(());
            }
            Timer::after(POLL_INTERVAL).await;
        }
    }

    async fn burst(&self, deadline: Instant) -> Result<usize, Error> {
        let deadline = deadline.min(Instant::now() + REGISTER_TIMEOUT);
        loop {
            let count = (self.read_word(STATUS) >> 8) as u16;
            if count != 0 {
                return Ok(usize::from(count));
            }
            if Instant::now() >= deadline {
                return Err(Error::FifoBurstTimeout);
            }
            Timer::after(POLL_INTERVAL).await;
        }
    }

    async fn send(&self, command: &[u8]) -> Result<(), Error> {
        let (&last, mut remaining) = command.split_last().ok_or(Error::CommandEncoding)?;
        self.write_byte(STATUS, COMMAND_READY);
        self.wait(
            STATUS,
            COMMAND_READY,
            COMMAND_READY,
            Instant::now() + REGISTER_TIMEOUT,
        )
        .await
        .map_err(|()| Error::ReadyTimeout)?;

        let deadline = Instant::now() + COMMAND_TIMEOUT;
        while !remaining.is_empty() {
            let count = self.burst(deadline).await?.min(remaining.len());
            let (burst, rest) = remaining.split_at(count);
            for &byte in burst {
                self.write_byte(DATA_FIFO, byte);
            }
            remaining = rest;
            let status = self
                .wait(STATUS, STATUS_VALID, STATUS_VALID, deadline)
                .await
                .map_err(|()| Error::CommandTimeout)?;
            if status & EXPECT == 0 {
                return Err(Error::DeviceFailure);
            }
        }
        self.burst(deadline).await?;
        self.write_byte(DATA_FIFO, last);
        let status = self
            .wait(STATUS, STATUS_VALID, STATUS_VALID, deadline)
            .await
            .map_err(|()| Error::CommandTimeout)?;
        if status & EXPECT != 0 {
            return Err(Error::DeviceFailure);
        }
        self.write_byte(STATUS, GO);
        Ok(())
    }

    async fn read_fifo(&self, mut remaining: &mut [u8], deadline: Instant) -> Result<(), Error> {
        while !remaining.is_empty() {
            self.wait(
                STATUS,
                STATUS_VALID | DATA_AVAILABLE,
                STATUS_VALID | DATA_AVAILABLE,
                deadline,
            )
            .await
            .map_err(|()| Error::CommandTimeout)?;
            let count = self.burst(deadline).await?.min(remaining.len());
            let (burst, rest) = remaining.split_at_mut(count);
            for byte in burst {
                *byte = self.read_byte(DATA_FIFO);
            }
            remaining = rest;
        }
        Ok(())
    }

    async fn receive(&self, response: &mut [u8]) -> Result<usize, Error> {
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        self.read_fifo(&mut response[..HEADER_BYTES], deadline)
            .await?;
        let length = response_length(response)?;
        self.read_fifo(&mut response[HEADER_BYTES..length], deadline)
            .await?;
        let status = self
            .wait(STATUS, STATUS_VALID, STATUS_VALID, deadline)
            .await
            .map_err(|()| Error::CommandTimeout)?;
        if status & DATA_AVAILABLE != 0 {
            return Err(Error::InvalidResponse);
        }
        Ok(length)
    }

    pub(super) async fn transact(
        &self,
        command: &[u8],
        response: &mut [u8],
    ) -> Result<usize, Error> {
        let access = self
            .wait(
                ACCESS,
                ACCESS_VALID,
                ACCESS_VALID,
                Instant::now() + REGISTER_TIMEOUT,
            )
            .await
            .map_err(|()| Error::InvalidLocalityState)?;
        if access & ACTIVE_LOCALITY == 0 {
            self.write_byte(ACCESS, REQUEST_USE);
            self.wait(
                ACCESS,
                ACCESS_VALID | ACTIVE_LOCALITY | REQUEST_USE,
                ACCESS_VALID | ACTIVE_LOCALITY,
                Instant::now() + REGISTER_TIMEOUT,
            )
            .await
            .map_err(|()| Error::LocalityTimeout)?;
        }

        let result = match self.send(command).await {
            Ok(()) => self.receive(response).await,
            Err(error) => Err(error),
        };
        // CommandReady discards unread data or aborts an operation before
        // locality is relinquished. Run this on both success and failure.
        self.write_byte(STATUS, COMMAND_READY);
        self.write_byte(ACCESS, ACTIVE_LOCALITY);
        let released = self
            .wait(
                ACCESS,
                ACCESS_VALID | ACTIVE_LOCALITY,
                ACCESS_VALID,
                Instant::now() + REGISTER_TIMEOUT,
            )
            .await;
        let length = result?;
        released.map_err(|()| Error::ReleaseTimeout)?;
        Ok(length)
    }
}
