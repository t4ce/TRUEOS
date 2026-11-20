//! Native TPM 2.0 CRB transport for ACPI start method 7.
use core::{
    ptr::NonNull,
    sync::atomic::{Ordering, compiler_fence},
};

use tpm2_protocol::{
    TpmWriter,
    data::{TpmCap, TpmPt, TpmSt, TpmuCapabilities},
    frame::{TpmGetCapabilityCommand, TpmGetCapabilityResponse, TpmResponse, tpm_marshal_command},
};
use trueos_time::{Duration, Instant, Timer};

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
const REGISTER_TIMEOUT: Duration = Duration::from_millis(750);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

const MANUFACTURER_COMMAND_BYTES: usize = 22;
const HEADER_BYTES: usize = 10;
const MANUFACTURER_RESPONSE_BYTES: usize = 27;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Error {
    NoAcpiTable,
    InvalidTable,
    UnsupportedStartMethod(u32),
    InvalidControlAddress,
    MmioMap,
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

pub(crate) struct Crb {
    base: u64,
    registers: NonNull<u8>,
}

impl Crb {
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
        let method = u32::from_le_bytes(table[48..52].try_into().unwrap());
        if method != CRB_START_METHOD {
            return Err(Error::UnsupportedStartMethod(method));
        }
        let control = u64::from_le_bytes(table[40..48].try_into().unwrap());
        let base = control & !(LOCALITY_BYTES as u64 - 1);
        if base == 0 || control - base != CONTROL_OFFSET || base >= 1 << 52 {
            return Err(Error::InvalidControlAddress);
        }
        let registers = crate::pci::mmio::map_mmio_region_exact(base, LOCALITY_BYTES)
            .map_err(|_| Error::MmioMap)?;
        Ok(Self { base, registers })
    }

    pub(crate) fn control_address(&self) -> u64 {
        self.base + CONTROL_OFFSET
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
            Timer::after(Duration::from_millis(1)).await;
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

    async fn command(&self) -> Result<[u8; 4], Error> {
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
        let request = TpmGetCapabilityCommand {
            handles: [],
            cap: TpmCap::TpmProperties,
            property: TpmPt::Manufacturer.value().into(),
            property_count: 1u32.into(),
        };
        let mut command = [0; MANUFACTURER_COMMAND_BYTES];
        let mut writer = TpmWriter::new(&mut command);
        tpm_marshal_command(&request, TpmSt::NoSessions, &[], &mut writer)
            .map_err(|_| Error::CommandEncoding)?;
        let command = writer.as_bytes();
        let cmd = self.buffer(CMD_ADDRESS, self.read(CMD_SIZE) as usize, command.len())?;
        let rsp_size = self.read(RSP_SIZE) as usize;
        let rsp = self.buffer(RSP_ADDRESS, rsp_size, MANUFACTURER_RESPONSE_BYTES)?;
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
        let mut response = [0; MANUFACTURER_RESPONSE_BYTES];
        for (offset, byte) in response[..HEADER_BYTES].iter_mut().enumerate() {
            *byte = unsafe { rsp.as_ptr().add(offset).read_volatile() };
        }
        let length = u32::from_be_bytes(response[2..6].try_into().unwrap()) as usize;
        if length > rsp_size {
            return Err(Error::InvalidResponse);
        }
        let Some(frame) = response
            .get_mut(..length)
            .filter(|frame| frame.len() >= HEADER_BYTES)
        else {
            return Err(Error::InvalidResponse);
        };
        for (offset, byte) in frame.iter_mut().enumerate().skip(HEADER_BYTES) {
            *byte = unsafe { rsp.as_ptr().add(offset).read_volatile() };
        }
        let frame = TpmResponse::cast(frame).map_err(|_| Error::InvalidResponse)?;
        if frame.tag().map_err(|_| Error::InvalidResponse)? != TpmSt::NoSessions {
            return Err(Error::InvalidResponse);
        }
        let code = frame.rc().map_err(|_| Error::InvalidResponse)?.value();
        if code != 0 {
            return Err(Error::TpmResponse(code));
        }
        let response: TpmGetCapabilityResponse =
            frame.unmarshal().map_err(|_| Error::InvalidResponse)?;
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

    pub(crate) async fn get_manufacturer(&self) -> Result<[u8; 4], Error> {
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

        let result = self.command().await;
        if self.read(CTRL_START) & START != 0 {
            return result;
        }
        self.write(CTRL_REQ, GO_IDLE);
        let idle = self.wait_for(CTRL_REQ, GO_IDLE, 0, REGISTER_TIMEOUT).await;
        self.write(LOC_CTRL, RELINQUISH);
        let released = self
            .wait_for(LOC_STATE, LOCALITY_ASSIGNED, 0, REGISTER_TIMEOUT)
            .await;
        let manufacturer = result?;
        if !idle {
            return Err(Error::IdleTimeout);
        }
        if !released {
            return Err(Error::ReleaseTimeout);
        }
        Ok(manufacturer)
    }
}
