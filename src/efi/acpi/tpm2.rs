//! TPM 2.0 command codec and ACPI CRB/FIFO transport discovery.
mod fifo;
use core::{
    ptr::NonNull,
    sync::atomic::{Ordering, compiler_fence},
};

use tpm2_protocol::{
    TpmWriter,
    data::{TpmCap, TpmPt, TpmSt, TpmsAuthCommand, TpmuCapabilities},
    frame::{
        TpmFrame, TpmGetCapabilityCommand, TpmGetCapabilityResponse, TpmResponse, TpmUnmarshalBody,
        tpm_marshal_command,
    },
};
use trueos_time::{Duration, Instant, Timer};

const FIFO_START_METHOD: u32 = 6;
const CRB_START_METHOD: u32 = 7;
const LOCALITY_BYTES: usize = 4096;
const CONTROL_OFFSET: u64 = 0x40;
const DATA_OFFSET: usize = 0x80;

const LOC_STATE: usize = 0x00;
const LOC_CTRL: usize = 0x08;
const CTRL_REQ: usize = 0x40;
const CTRL_STS: usize = 0x44;
const CTRL_CANCEL: usize = 0x48;
const CTRL_START: usize = 0x4c;
const CMD_SIZE: usize = 0x58;
const CMD_ADDRESS: usize = 0x5c;
const RSP_SIZE: usize = 0x64;
const RSP_ADDRESS: usize = 0x68;

const LOCALITY_ASSIGNED: u32 = 1 << 1;
const ACTIVE_LOCALITY: u32 = 0b111 << 2;
const REGISTERS_VALID: u32 = 1 << 7;
const LOCALITY_ZERO: u32 = REGISTERS_VALID | LOCALITY_ASSIGNED;
const REQUEST_ACCESS: u32 = 1;
const RELINQUISH: u32 = 2;
const COMMAND_READY: u32 = 1;
const GO_IDLE: u32 = 2;
const START: u32 = 1;
const DEVICE_ERROR: u32 = 1;
const POLL_INTERVAL: Duration = Duration::from_millis(1);
const REGISTER_TIMEOUT: Duration = Duration::from_millis(750);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

const HEADER_BYTES: usize = 10;
pub(crate) const MAX_FRAME_BYTES: usize = LOCALITY_BYTES - DATA_OFFSET;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Error {
    NoAcpiTable,
    InvalidTable,
    UnsupportedStartMethod(u32),
    InvalidControlAddress,
    MmioMap,
    DeviceNotPresent,
    FifoBurstTimeout,
    InvalidLocalityState,
    LocalityTimeout,
    CommandBusy,
    ReadyTimeout,
    InvalidBuffer,
    CommandTimeout,
    DeviceFailure,
    CommandEncoding,
    InvalidResponse,
    TpmResponse(u32),
    IdleTimeout,
    ReleaseTimeout,
}

pub(crate) struct Device(Transport);

enum Transport {
    Crb(Crb),
    Fifo(fifo::Fifo),
}

impl Device {
    pub(crate) fn discover() -> Result<Self, Error> {
        let tables = super::ensure_tables().ok_or(Error::NoAcpiTable)?;
        let (phys, _) = tables
            .table_headers()
            .find(|(_, header)| header.signature.as_str() == "TPM2")
            .ok_or(Error::NoAcpiTable)?;
        let table = super::map_table_bytes(phys).ok_or(Error::InvalidTable)?;
        if table.len() < 52 || table.iter().copied().fold(0u8, u8::wrapping_add) != 0 {
            return Err(Error::InvalidTable);
        }
        let address = u64::from_le_bytes(table[40..48].try_into().unwrap());
        let method = u32::from_le_bytes(table[48..52].try_into().unwrap());
        let transport = match method {
            CRB_START_METHOD => Transport::Crb(Crb::map(address)?),
            FIFO_START_METHOD => {
                let base = if address == 0 {
                    fifo::STANDARD_BASE
                } else {
                    address
                };
                Transport::Fifo(fifo::Fifo::map(base)?)
            }
            _ => return Err(Error::UnsupportedStartMethod(method)),
        };
        Ok(Self(transport))
    }

    pub(crate) fn interface(&self) -> (&'static str, u64) {
        match &self.0 {
            Transport::Crb(device) => ("CRB", device.base + CONTROL_OFFSET),
            Transport::Fifo(device) => ("FIFO", device.address()),
        }
    }

    async fn transact(&self, command: &[u8], response: &mut [u8]) -> Result<usize, Error> {
        match &self.0 {
            Transport::Crb(device) => device.transact(command, response).await,
            Transport::Fifo(device) => device.transact(command, response).await,
        }
    }
    pub(crate) async fn execute<R: TpmUnmarshalBody>(
        &self,
        request: &impl TpmFrame,
        sessions: &[TpmsAuthCommand],
    ) -> Result<R, Error> {
        let tag = if sessions.is_empty() {
            TpmSt::NoSessions
        } else {
            TpmSt::Sessions
        };
        let mut command = zeroize::Zeroizing::new([0; MAX_FRAME_BYTES]);
        let mut writer = TpmWriter::new(&mut *command);
        tpm_marshal_command(request, tag, sessions, &mut writer)
            .map_err(|_| Error::CommandEncoding)?;
        let mut response = zeroize::Zeroizing::new([0; MAX_FRAME_BYTES]);
        let length = self.transact(writer.as_bytes(), &mut *response).await?;
        let frame = TpmResponse::cast(&response[..length]).map_err(|_| Error::InvalidResponse)?;
        let code = frame.rc().map_err(|_| Error::InvalidResponse)?.value();
        if code != 0 {
            return Err(Error::TpmResponse(code));
        }
        if frame.tag().map_err(|_| Error::InvalidResponse)? != tag {
            return Err(Error::InvalidResponse);
        }
        frame.unmarshal().map_err(|_| Error::InvalidResponse)
    }

    pub(crate) async fn get_manufacturer(&self) -> Result<[u8; 4], Error> {
        let request = TpmGetCapabilityCommand {
            handles: [],
            cap: TpmCap::TpmProperties,
            property: TpmPt::Manufacturer.value().into(),
            property_count: 1u32.into(),
        };
        let response: TpmGetCapabilityResponse = self.execute(&request, &[]).await?;
        let TpmuCapabilities::TpmProperties(properties) = response.capability_data.data else {
            return Err(Error::InvalidResponse);
        };
        let [property] = &*properties else {
            return Err(Error::InvalidResponse);
        };
        if property.property != TpmPt::Manufacturer {
            return Err(Error::InvalidResponse);
        }
        Ok(property.value.value().to_be_bytes())
    }
}

fn response_length(response: &[u8]) -> Result<usize, Error> {
    let length = u32::from_be_bytes(response[2..6].try_into().unwrap()) as usize;
    if !(HEADER_BYTES..=response.len()).contains(&length) {
        return Err(Error::InvalidResponse);
    }
    Ok(length)
}

struct Crb {
    base: u64,
    registers: NonNull<u8>,
}

impl Crb {
    fn map(control: u64) -> Result<Self, Error> {
        let base = control & !(LOCALITY_BYTES as u64 - 1);
        if base == 0 || control - base != CONTROL_OFFSET || base >= 1 << 52 {
            return Err(Error::InvalidControlAddress);
        }
        let registers = crate::pci::mmio::map_mmio_region_exact(base, LOCALITY_BYTES)
            .map_err(|_| Error::MmioMap)?;
        Ok(Self { base, registers })
    }

    fn read(&self, offset: usize) -> u32 {
        // Registers are aligned DWORDs in the uncached locality-zero mapping.
        unsafe {
            self.registers
                .as_ptr()
                .add(offset)
                .cast::<u32>()
                .read_volatile()
        }
    }

    fn write(&self, offset: usize, value: u32) {
        unsafe {
            self.registers
                .as_ptr()
                .add(offset)
                .cast::<u32>()
                .write_volatile(value)
        }
    }

    async fn wait_for(&self, offset: usize, mask: u32, value: u32, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.read(offset) & mask == value {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            Timer::after(POLL_INTERVAL).await;
        }
    }

    fn buffer(
        &self,
        address_register: usize,
        size: usize,
        minimum: usize,
    ) -> Result<NonNull<u8>, Error> {
        let address = u64::from(self.read(address_register))
            | (u64::from(self.read(address_register + 4)) << 32);
        // This transport supports buffers in the CRB locality-zero aperture.
        // Never dereference a device-provided pointer outside that mapping.
        let offset = address.checked_sub(self.base).ok_or(Error::InvalidBuffer)?;
        if offset < DATA_OFFSET as u64
            || offset > LOCALITY_BYTES as u64
            || size < minimum
            || size > LOCALITY_BYTES - offset as usize
        {
            return Err(Error::InvalidBuffer);
        }
        Ok(unsafe { NonNull::new_unchecked(self.registers.as_ptr().add(offset as usize)) })
    }

    async fn transfer(&self, command: &[u8], response: &mut [u8]) -> Result<usize, Error> {
        self.write(CTRL_REQ, COMMAND_READY);
        if !self
            .wait_for(CTRL_REQ, COMMAND_READY, 0, REGISTER_TIMEOUT)
            .await
        {
            return Err(Error::ReadyTimeout);
        }
        if self.read(CTRL_STS) & DEVICE_ERROR != 0 {
            return Err(Error::DeviceFailure);
        }
        let cmd = self.buffer(CMD_ADDRESS, self.read(CMD_SIZE) as usize, command.len())?;
        let rsp_size = self.read(RSP_SIZE) as usize;
        let rsp = self.buffer(RSP_ADDRESS, rsp_size, HEADER_BYTES)?;
        for (offset, byte) in command.iter().copied().enumerate() {
            unsafe { cmd.as_ptr().add(offset).write_volatile(byte) };
        }
        compiler_fence(Ordering::SeqCst);
        self.write(CTRL_START, START);
        if !self.wait_for(CTRL_START, START, 0, COMMAND_TIMEOUT).await {
            self.write(CTRL_CANCEL, 1);
            self.wait_for(CTRL_START, START, 0, REGISTER_TIMEOUT).await;
            self.write(CTRL_CANCEL, 0);
            return Err(Error::CommandTimeout);
        }
        if self.read(CTRL_STS) & DEVICE_ERROR != 0 {
            return Err(Error::DeviceFailure);
        }
        for (offset, byte) in response[..HEADER_BYTES].iter_mut().enumerate() {
            *byte = unsafe { rsp.as_ptr().add(offset).read_volatile() };
        }
        let length = response_length(response)?;
        if length > rsp_size {
            return Err(Error::InvalidResponse);
        }
        let frame = &mut response[..length];
        for (offset, byte) in frame.iter_mut().enumerate().skip(HEADER_BYTES) {
            *byte = unsafe { rsp.as_ptr().add(offset).read_volatile() };
        }
        Ok(length)
    }

    async fn transact(&self, command: &[u8], response: &mut [u8]) -> Result<usize, Error> {
        let state = self.read(LOC_STATE);
        if state & REGISTERS_VALID == 0 || state == u32::MAX {
            return Err(Error::InvalidLocalityState);
        }
        if self.read(CTRL_START) & START != 0 {
            return Err(Error::CommandBusy);
        }
        self.write(LOC_CTRL, REQUEST_ACCESS);
        if !self
            .wait_for(
                LOC_STATE,
                LOCALITY_ZERO | ACTIVE_LOCALITY,
                LOCALITY_ZERO,
                REGISTER_TIMEOUT,
            )
            .await
        {
            self.write(LOC_CTRL, RELINQUISH);
            return Err(Error::LocalityTimeout);
        }

        let result = self.transfer(command, response).await;
        if self.read(CTRL_START) & START != 0 {
            return result;
        }
        self.write(CTRL_REQ, GO_IDLE);
        let idle = self.wait_for(CTRL_REQ, GO_IDLE, 0, REGISTER_TIMEOUT).await;
        self.write(LOC_CTRL, RELINQUISH);
        let released = self
            .wait_for(LOC_STATE, LOCALITY_ASSIGNED, 0, REGISTER_TIMEOUT)
            .await;
        let length = result?;
        if !idle {
            return Err(Error::IdleTimeout);
        }
        if !released {
            return Err(Error::ReleaseTimeout);
        }
        Ok(length)
    }
}
