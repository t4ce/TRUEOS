use alloc::{boxed::Box, string::String, vec::Vec};
use core::{
    fmt,
    hash::{Hash, Hasher},
    ptr,
    sync::atomic::{AtomicU32, Ordering},
};

const DEFAULT_DMA_ALIGNMENT: u32 = 64;
const DEFAULT_MAX_TRANSFER_BYTES: u64 = 256 * 1024;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    NotSupported,
    NotReady,
    InvalidParam,
    OutOfBounds,
    DmaUnavailable,
    MmioMapFailed,
    Timeout,
    Io,
    Corrupted,
}

impl fatfs::IoError for Error {
    fn is_interrupted(&self) -> bool {
        false
    }

    fn new_unexpected_eof_error() -> Self {
        Error::OutOfBounds
    }

    fn new_write_zero_error() -> Self {
        Error::Io
    }
}

pub type Result<T> = core::result::Result<T, Error>;

/// Minimal synchronous block-device interface expected by the kernel and upper layers.
///
/// Contract the implementor must honor:
/// - Every Logical Block Address (LBA) is counted in multiples of `block_size_bytes()`.
/// - Callers always pass buffers whose length is a multiple of `block_size_bytes()` and that
///   are aligned to `dma_alignment_bytes()`; the implementation may assume this and return
///   `Error::InvalidParam`/`Error::DmaUnavailable` otherwise.
/// - Callers must not exceed `max_transfer_bytes()` in a single request; drivers may further split
///   overly large commands to satisfy additional hardware limits when needed.
/// - `read_blocks`/`write_blocks` are synchronous and must not return until the transfer either
///   completes, fails with a concrete `Error`, or needs the caller to retry later via
///   `Error::NotReady`.
/// - Implementations must bounds-check the provided LBA span and return `Error::OutOfBounds`
///   for invalid ranges.
pub trait BlockDevice: Send {
    /// Logical sector size in bytes (typically 512..4096 and power-of-two).
    fn block_size_bytes(&self) -> u32;

    /// Total number of addressable blocks.
    fn block_count(&self) -> u64;

    /// Synchronous block read. `buf.len()` is always a multiple of `block_size_bytes()`.
    fn read_blocks(&mut self, lba: u64, buf: &mut [u8]) -> Result<()>;

    /// Optional block write. Default is `Error::NotSupported`.
    fn write_blocks(&mut self, _lba: u64, _buf: &[u8]) -> Result<()> {
        Err(Error::NotSupported)
    }

    /// Required DMA alignment in bytes (defaults to 64 bytes which matches NVMe/AHCI).
    fn dma_alignment_bytes(&self) -> u32 {
        DEFAULT_DMA_ALIGNMENT
    }

    /// Maximum transfer size the device can service in a single command.
    fn max_transfer_bytes(&self) -> u64 {
        DEFAULT_MAX_TRANSFER_BYTES
    }

    /// Whether `write_blocks` is expected to succeed.
    fn supports_write(&self) -> bool {
        false
    }

    /// Optional flush hook for devices that need explicit cache drains.
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct DiscId(u32);

impl DiscId {
    pub fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Display for DiscId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "disc{:03}", self.0)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DeviceKind {
    Nvme,
    Ahci,
    Partition,
    Ramdisk,
    Virtual,
    Unknown,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PciAddress {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
}

impl PciAddress {
    pub const fn new(bus: u8, slot: u8, function: u8) -> Self {
        Self {
            bus,
            slot,
            function,
        }
    }
}

impl fmt::Display for PciAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02X}:{:02X}.{}", self.bus, self.slot, self.function)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceSerial(String);

impl DeviceSerial {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for DeviceSerial {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

impl From<String> for DeviceSerial {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub id: DiscId,
    pub kind: DeviceKind,
    pub parent: Option<DiscId>,
    pub label: Option<String>,
    pub pci: Option<PciAddress>,
    pub block_size: u32,
    pub block_count: u64,
    pub capacity_bytes: u128,
    pub max_transfer_bytes: u64,
    pub dma_alignment: u32,
    pub serial: Option<DeviceSerial>,
    pub writable: bool,
}

impl DeviceInfo {
    pub fn is_read_only(&self) -> bool {
        !self.writable
    }
}

#[derive(Clone, Debug)]
pub struct DeviceDescriptor {
    pub kind: DeviceKind,
    pub label: Option<String>,
    pub parent: Option<DiscId>,
    pub pci: Option<PciAddress>,
    pub serial: Option<DeviceSerial>,
    pub read_only: bool,
}

impl DeviceDescriptor {
    pub fn new(kind: DeviceKind) -> Self {
        Self {
            kind,
            label: None,
            parent: None,
            pci: None,
            serial: None,
            read_only: false,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_parent(mut self, parent: DiscId) -> Self {
        self.parent = Some(parent);
        self
    }

    pub fn with_pci(mut self, pci: PciAddress) -> Self {
        self.pci = Some(pci);
        self
    }

    pub fn with_serial(mut self, serial: impl Into<DeviceSerial>) -> Self {
        self.serial = Some(serial.into());
        self
    }

    pub fn mark_read_only(mut self) -> Self {
        self.read_only = true;
        self
    }
}

#[derive(Clone, Copy)]
pub struct DeviceHandle {
    node: &'static DeviceNode,
}

impl fmt::Debug for DeviceHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceHandle")
            .field("id", &self.id())
            .field("kind", &self.node.info.kind)
            .finish()
    }
}

impl PartialEq for DeviceHandle {
    fn eq(&self, other: &Self) -> bool {
        ptr::eq(self.node, other.node)
    }
}

impl Eq for DeviceHandle {}

impl Hash for DeviceHandle {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.node as *const DeviceNode as usize).hash(state);
    }
}

impl DeviceHandle {
    pub fn id(self) -> DiscId {
        self.node.info.id
    }

    pub fn info(&self) -> DeviceInfo {
        self.node.info.clone()
    }

    pub fn block_size(&self) -> u32 {
        self.node.info.block_size
    }

    pub fn block_count(&self) -> u64 {
        self.node.info.block_count
    }

    pub fn max_transfer_bytes(&self) -> u64 {
        self.node.info.max_transfer_bytes
    }

    pub fn parent(&self) -> Option<DiscId> {
        self.node.info.parent
    }

    pub fn supports_write(&self) -> bool {
        self.node.info.writable
    }

    pub fn read_blocks(&self, lba: u64, buf: &mut [u8]) -> Result<()> {
        self.validate_buffer(buf)?;
        let blocks = blocks_in_buffer(buf.len(), self.block_size())?;
        self.validate_lba_range(lba, blocks)?;
        self.with_driver_mut(|dev| dev.read_blocks(lba, buf))
    }

    pub fn write_blocks(&self, lba: u64, buf: &[u8]) -> Result<()> {
        if !self.supports_write() {
            return Err(Error::NotSupported);
        }
        self.validate_buffer(buf)?;
        let blocks = blocks_in_buffer(buf.len(), self.block_size())?;
        self.validate_lba_range(lba, blocks)?;
        self.with_driver_mut(|dev| dev.write_blocks(lba, buf))
    }

    pub fn flush(&self) -> Result<()> {
        self.with_driver_mut(|dev| dev.flush())
    }

    pub fn with_driver_mut<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut dyn BlockDevice) -> Result<R>,
    {
        let mut guard = self.node.driver.lock();
        f(&mut **guard)
    }

    fn validate_buffer(&self, buf: &[u8]) -> Result<()> {
        if buf.is_empty() {
            return Ok(());
        }

        let block_size = self.block_size() as usize;
        if buf.len() % block_size != 0 {
            return Err(Error::InvalidParam);
        }

        let align = self.node.info.dma_alignment.max(1) as usize;
        if align > 1 && (buf.as_ptr() as usize) % align != 0 {
            return Err(Error::DmaUnavailable);
        }

        if self.node.info.max_transfer_bytes > 0
            && (buf.len() as u64) > self.node.info.max_transfer_bytes
        {
            return Err(Error::InvalidParam);
        }

        Ok(())
    }

    fn validate_lba_range(&self, lba: u64, blocks: u64) -> Result<()> {
        if blocks == 0 {
            return Ok(());
        }

        let end = lba.checked_add(blocks).ok_or(Error::OutOfBounds)?;
        if end > self.block_count() {
            return Err(Error::OutOfBounds);
        }

        Ok(())
    }
}

struct DeviceNode {
    info: DeviceInfo,
    driver: spin::Mutex<Box<dyn BlockDevice>>,
}

impl DeviceNode {
    fn handle(&'static self) -> DeviceHandle {
        DeviceHandle { node: self }
    }
}

struct Registry {
    devices: Vec<&'static DeviceNode>,
}

impl Registry {
    const fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    fn insert(&mut self, node: &'static DeviceNode) {
        self.devices.push(node);
    }

    fn find(&self, id: DiscId) -> Option<&'static DeviceNode> {
        self.devices.iter().copied().find(|node| node.info.id == id)
    }
}

static REGISTRY: spin::Mutex<Registry> = spin::Mutex::new(Registry::new());
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

fn allocate_id() -> DiscId {
    DiscId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

fn blocks_in_buffer(len: usize, block_size: u32) -> Result<u64> {
    if len == 0 {
        return Ok(0);
    }

    if len % block_size as usize != 0 {
        return Err(Error::InvalidParam);
    }

    Ok((len / block_size as usize) as u64)
}

pub fn register_device<D>(descriptor: DeviceDescriptor, device: D) -> DeviceHandle
where
    D: BlockDevice + 'static,
{
    let driver: Box<dyn BlockDevice> = Box::new(device);
    let block_size = driver.block_size_bytes();
    let block_count = driver.block_count();
    let dma_alignment = driver.dma_alignment_bytes().max(1);
    let max_transfer = driver.max_transfer_bytes().max(block_size as u64).max(1);
    let writable = driver.supports_write() && !descriptor.read_only;

    let id = allocate_id();
    let info = DeviceInfo {
        id,
        kind: descriptor.kind,
        parent: descriptor.parent,
        label: descriptor.label.clone(),
        pci: descriptor.pci,
        block_size,
        block_count,
        capacity_bytes: (block_size as u128) * (block_count as u128),
        max_transfer_bytes: max_transfer,
        dma_alignment,
        serial: descriptor.serial.clone(),
        writable,
    };

    let node = Box::leak(Box::new(DeviceNode {
        info,
        driver: spin::Mutex::new(driver),
    }));

    let handle = node.handle();
    REGISTRY.lock().insert(node);
    handle
}

pub fn device_handles() -> Vec<DeviceHandle> {
    let registry = REGISTRY.lock();
    registry.devices.iter().map(|node| node.handle()).collect()
}

pub fn devices() -> Vec<DeviceInfo> {
    let registry = REGISTRY.lock();
    registry
        .devices
        .iter()
        .map(|node| node.info.clone())
        .collect()
}

pub fn device_handle(id: DiscId) -> Option<DeviceHandle> {
    let registry = REGISTRY.lock();
    registry.find(id).map(|node| node.handle())
}
