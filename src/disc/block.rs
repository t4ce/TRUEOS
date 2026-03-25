use crate::wait;
use alloc::{boxed::Box, string::String, vec::Vec};
use core::{
    fmt,
    future::Future,
    hash::{Hash, Hasher},
    pin::Pin,
    ptr,
    sync::atomic::{AtomicU32, Ordering},
    task::Waker,
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

pub type Result<T> = core::result::Result<T, Error>;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

struct AsyncMutex<T> {
    locked: core::sync::atomic::AtomicBool,
    waiters: spin::Mutex<Vec<Waker>>,
    value: core::cell::UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for AsyncMutex<T> {}

impl<T> AsyncMutex<T> {
    const fn new(value: T) -> Self {
        Self {
            locked: core::sync::atomic::AtomicBool::new(false),
            waiters: spin::Mutex::new(Vec::new()),
            value: core::cell::UnsafeCell::new(value),
        }
    }

    async fn lock(&self) -> AsyncMutexGuard<'_, T> {
        core::future::poll_fn(|cx| self.poll_lock(cx)).await
    }

    fn poll_lock(
        &self,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<AsyncMutexGuard<'_, T>> {
        if self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            return core::task::Poll::Ready(AsyncMutexGuard { m: self });
        }

        let mut waiters = self.waiters.lock();
        let waker = cx.waker();
        wait::register_waker_list(&mut waiters, waker);
        core::task::Poll::Pending
    }

    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
        let waiters = core::mem::take(&mut *self.waiters.lock());
        for w in waiters {
            w.wake();
        }
    }
}

struct AsyncMutexGuard<'a, T> {
    m: &'a AsyncMutex<T>,
}

impl<T> core::ops::Deref for AsyncMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.m.value.get() }
    }
}

impl<T> core::ops::DerefMut for AsyncMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.m.value.get() }
    }
}

impl<T> Drop for AsyncMutexGuard<'_, T> {
    fn drop(&mut self) {
        self.m.unlock();
    }
}

/// Minimal async block-device interface expected by the kernel and upper layers.
///
/// Contract the implementor must honor:
/// - Every Logical Block Address (LBA) is counted in multiples of `block_size_bytes()`.
/// - Callers use block counts (not byte counts). The returned buffer length must be
///   `blocks * block_size_bytes()`.
/// - Callers must not exceed `max_transfer_bytes()` in a single request; drivers may internally
///   split overly large commands to satisfy additional hardware limits when needed.
/// - `read_blocks`/`write_blocks` must be cooperative: implementations should await/yield while
///   waiting for hardware completion instead of busy-spinning, so the executor can make progress.
/// - Implementations must bounds-check the provided LBA span and return `Error::OutOfBounds`
///   for invalid ranges.
pub trait BlockDevice: Send {
    /// Logical sector size in bytes (typically 512..4096 and power-of-two).
    fn block_size_bytes(&self) -> u32;

    /// Total number of addressable blocks.
    fn block_count(&self) -> u64;

    /// Asynchronous block read.
    fn read_blocks<'a>(&'a mut self, lba: u64, blocks: usize) -> BoxFuture<'a, Result<Vec<u8>>>;

    /// Optional block write. Default is `Error::NotSupported`.
    fn write_blocks<'a>(&'a mut self, _lba: u64, _buf: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Err(Error::NotSupported) })
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
    fn flush<'a>(&'a mut self) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
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
    Partition,
    Ramdisk,
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
    pub user_visible: bool,
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
    pub user_visible: bool,
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
            user_visible: true,
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

    pub fn mark_internal_hidden(mut self) -> Self {
        self.user_visible = false;
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

    pub async fn read_blocks(&self, lba: u64, blocks: usize) -> Result<Vec<u8>> {
        let bs = self.block_size() as u64;
        let blocks_u64 = blocks as u64;
        if bs == 0 {
            return Err(Error::InvalidParam);
        }
        let bytes = blocks_u64.checked_mul(bs).ok_or(Error::InvalidParam)?;

        if self.node.info.max_transfer_bytes > 0 && bytes > self.node.info.max_transfer_bytes {
            return Err(Error::InvalidParam);
        }

        self.validate_lba_range(lba, blocks_u64)?;
        let mut guard = self.node.driver.lock().await;
        (**guard).read_blocks(lba, blocks).await
    }

    pub async fn write_blocks(&self, lba: u64, buf: &[u8]) -> Result<()> {
        if !self.supports_write() {
            return Err(Error::NotSupported);
        }

        let bs = self.block_size() as usize;
        if bs == 0 {
            return Err(Error::InvalidParam);
        }
        if !buf.len().is_multiple_of(bs) {
            return Err(Error::InvalidParam);
        }

        if self.node.info.max_transfer_bytes > 0
            && (buf.len() as u64) > self.node.info.max_transfer_bytes
        {
            return Err(Error::InvalidParam);
        }

        let blocks = blocks_in_buffer(buf.len(), self.block_size())?;
        self.validate_lba_range(lba, blocks)?;

        let mut guard = self.node.driver.lock().await;
        (**guard).write_blocks(lba, buf).await
    }

    pub async fn flush(&self) -> Result<()> {
        let mut guard = self.node.driver.lock().await;
        (**guard).flush().await
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
    driver: AsyncMutex<Box<dyn BlockDevice>>,
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

    if !len.is_multiple_of(block_size as usize) {
        return Err(Error::InvalidParam);
    }

    Ok((len / block_size as usize) as u64)
}

pub fn register_device<D>(descriptor: DeviceDescriptor, device: D) -> DeviceHandle
where
    D: BlockDevice + 'static,
{
    let should_request_trueosfs_mount = descriptor.parent.is_none() && descriptor.user_visible;
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
        user_visible: descriptor.user_visible,
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
        driver: AsyncMutex::new(driver),
    }));

    let handle = node.handle();
    REGISTRY.lock().insert(node);
    if should_request_trueosfs_mount {
        crate::r::fs::trueosfs::request_mount_root(handle);
    }
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
