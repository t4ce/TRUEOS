use alloc::{boxed::Box, string::String, vec::Vec as AllocVec};
use crab_usb::{Device, DeviceInfo, EndpointBulkIn, EndpointBulkOut, USBHost};
use embassy_executor::Spawner;
use embassy_sync::channel::{Channel, TrySendError};
use embassy_sync::signal::Signal;
use embassy_sync::watch::{Receiver as WatchReceiver, Watch};
use heapless::Vec;
use spin::Mutex;

use super::api::claim_interface;
use super::mass;
use crate::disc::block;

const SKHYNIX_GREEN_VID: u16 = 0x152E;
const SKHYNIX_GREEN_PID: u16 = 0x7001;
const MAX_ACTIVE_GREEN_PROBES: usize = 4;
const SKHYNIX_UAS_FLOW_QUEUE_DEPTH: usize = 8;
const SKHYNIX_UAS_FLOW_STATE_RECEIVERS: usize = 4;
const SKHYNIX_UAS_FLOW_MAX_LANES: u8 = 4;
const SKHYNIX_UAS_FLOW_BOOT_BENCH_ENABLED: bool = false;
const SKHYNIX_UAS_FLOW_MAX_TRANSFER_BYTES: u64 = 1024 * 1024;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ActiveGreenProbe {
    controller_id: u32,
    slot_id: u32,
}

static ACTIVE_GREEN_PROBES: Mutex<Vec<ActiveGreenProbe, MAX_ACTIVE_GREEN_PROBES>> =
    Mutex::new(Vec::new());

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SkhynixUasFlowDirection {
    NoData,
    DataIn,
    DataOut,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SkhynixUasFlowError {
    QueueFull,
    NotReady,
    Transport,
    ShortData,
    Status,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SkhynixUasFlowState {
    pub online: bool,
    pub max_lanes: u8,
    pub active_lanes: u8,
    pub queued: u8,
    pub completed: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub last_error: Option<SkhynixUasFlowError>,
}

#[allow(dead_code)]
impl SkhynixUasFlowState {
    pub const fn offline() -> Self {
        Self {
            online: false,
            max_lanes: SKHYNIX_UAS_FLOW_MAX_LANES,
            active_lanes: 0,
            queued: 0,
            completed: 0,
            bytes_in: 0,
            bytes_out: 0,
            last_error: None,
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct SkhynixUasFlowCompletion {
    pub tag: u16,
    pub direction: SkhynixUasFlowDirection,
    pub transferred: usize,
    pub result: Result<(), SkhynixUasFlowError>,
}

#[allow(dead_code)]
pub(crate) enum SkhynixUasFlowRequest {
    Read {
        lba: u32,
        blocks: u16,
    },
    Write {
        lba: u32,
        blocks: u16,
        data: AllocVec<u8>,
    },
    Flush,
}

#[allow(dead_code)]
pub(crate) enum SkhynixUasFlowReply {
    Read {
        tag: u16,
        lba: u32,
        blocks: u16,
        result: Result<AllocVec<u8>, SkhynixUasFlowError>,
    },
    Write {
        tag: u16,
        lba: u32,
        blocks: u16,
        result: Result<(), SkhynixUasFlowError>,
    },
    Flush {
        tag: u16,
        result: Result<(), SkhynixUasFlowError>,
    },
}

type SkhynixUasFlowChannel =
    Channel<crate::wait::EmbassySpinRawMutex, SkhynixUasFlowRequest, SKHYNIX_UAS_FLOW_QUEUE_DEPTH>;

static SKHYNIX_UAS_FLOW_STATE: Watch<
    crate::wait::EmbassySpinRawMutex,
    SkhynixUasFlowState,
    SKHYNIX_UAS_FLOW_STATE_RECEIVERS,
> = Watch::new_with(SkhynixUasFlowState::offline());

static SKHYNIX_UAS_FLOW_REPLY: Signal<crate::wait::EmbassySpinRawMutex, SkhynixUasFlowReply> =
    Signal::new();

static SKHYNIX_UAS_FLOW_SERVICE: SkhynixUasFlowService = SkhynixUasFlowService::new();

#[allow(dead_code)]
pub(crate) type SkhynixUasFlowStateReceiver<'a> = WatchReceiver<
    'a,
    crate::wait::EmbassySpinRawMutex,
    SkhynixUasFlowState,
    SKHYNIX_UAS_FLOW_STATE_RECEIVERS,
>;

#[allow(dead_code)]
pub(crate) struct SkhynixUasFlowService {
    requests: SkhynixUasFlowChannel,
}

#[allow(dead_code)]
impl SkhynixUasFlowService {
    pub const fn new() -> Self {
        Self {
            requests: Channel::new(),
        }
    }

    pub fn subscribe_state() -> Option<SkhynixUasFlowStateReceiver<'static>> {
        SKHYNIX_UAS_FLOW_STATE.receiver()
    }

    pub fn latest_state() -> SkhynixUasFlowState {
        SKHYNIX_UAS_FLOW_STATE
            .try_get()
            .unwrap_or(SkhynixUasFlowState::offline())
    }

    pub fn try_submit(&self, request: SkhynixUasFlowRequest) -> Result<(), SkhynixUasFlowError> {
        self.requests.try_send(request).map_err(|err| match err {
            TrySendError::Full(_) => SkhynixUasFlowError::QueueFull,
        })
    }

    pub async fn submit(&self, request: SkhynixUasFlowRequest) {
        self.requests.send(request).await;
    }

    pub async fn read_blocks(
        &self,
        lba: u32,
        blocks: u16,
    ) -> Result<AllocVec<u8>, SkhynixUasFlowError> {
        self.submit(SkhynixUasFlowRequest::Read { lba, blocks })
            .await;
        match SKHYNIX_UAS_FLOW_REPLY.wait().await {
            SkhynixUasFlowReply::Read { result, .. } => result,
            _ => Err(SkhynixUasFlowError::Transport),
        }
    }

    pub async fn write_blocks(
        &self,
        lba: u32,
        blocks: u16,
        data: AllocVec<u8>,
    ) -> Result<(), SkhynixUasFlowError> {
        self.submit(SkhynixUasFlowRequest::Write { lba, blocks, data })
            .await;
        match SKHYNIX_UAS_FLOW_REPLY.wait().await {
            SkhynixUasFlowReply::Write { result, .. } => result,
            _ => Err(SkhynixUasFlowError::Transport),
        }
    }

    pub async fn flush(&self) -> Result<(), SkhynixUasFlowError> {
        self.submit(SkhynixUasFlowRequest::Flush).await;
        match SKHYNIX_UAS_FLOW_REPLY.wait().await {
            SkhynixUasFlowReply::Flush { result, .. } => result,
            _ => Err(SkhynixUasFlowError::Transport),
        }
    }
}

struct SkhynixUasFlowBlockDevice {
    block_size: u32,
    block_count: u64,
}

impl SkhynixUasFlowBlockDevice {
    fn read_bounds(&self, lba: u64, blocks: usize) -> block::Result<(u32, u16, usize)> {
        let block_size = self.block_size.max(1) as usize;
        let bytes = blocks
            .checked_mul(block_size)
            .ok_or(block::Error::InvalidParam)?;
        if bytes as u64 > SKHYNIX_UAS_FLOW_MAX_TRANSFER_BYTES {
            return Err(block::Error::InvalidParam);
        }
        let end = lba
            .checked_add(blocks as u64)
            .ok_or(block::Error::OutOfBounds)?;
        if end > self.block_count || lba > u64::from(u32::MAX) || blocks > u16::MAX as usize {
            return Err(block::Error::OutOfBounds);
        }
        Ok((lba as u32, blocks as u16, bytes))
    }

    fn write_bounds(&self, lba: u64, buf_len: usize) -> block::Result<(u32, u16)> {
        let block_size = self.block_size.max(1) as usize;
        if buf_len == 0 || !buf_len.is_multiple_of(block_size) {
            return Err(block::Error::InvalidParam);
        }
        if buf_len as u64 > SKHYNIX_UAS_FLOW_MAX_TRANSFER_BYTES {
            return Err(block::Error::InvalidParam);
        }
        let blocks = buf_len / block_size;
        let end = lba
            .checked_add(blocks as u64)
            .ok_or(block::Error::OutOfBounds)?;
        if end > self.block_count || lba > u64::from(u32::MAX) || blocks > u16::MAX as usize {
            return Err(block::Error::OutOfBounds);
        }
        Ok((lba as u32, blocks as u16))
    }
}

impl block::BlockDevice for SkhynixUasFlowBlockDevice {
    fn block_size_bytes(&self) -> u32 {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn max_transfer_bytes(&self) -> u64 {
        SKHYNIX_UAS_FLOW_MAX_TRANSFER_BYTES
    }

    fn supports_write(&self) -> bool {
        true
    }

    fn read_blocks<'a>(
        &'a mut self,
        lba: u64,
        blocks: usize,
    ) -> block::BoxFuture<'a, block::Result<AllocVec<u8>>> {
        Box::pin(async move {
            let (lba, blocks, _bytes) = self.read_bounds(lba, blocks)?;
            SKHYNIX_UAS_FLOW_SERVICE
                .read_blocks(lba, blocks)
                .await
                .map_err(flow_error_to_block_error)
        })
    }

    fn read_blocks_into<'a>(
        &'a mut self,
        lba: u64,
        blocks: usize,
        dst: &'a mut [u8],
    ) -> block::BoxFuture<'a, block::Result<()>> {
        Box::pin(async move {
            let (lba, blocks, bytes) = self.read_bounds(lba, blocks)?;
            if dst.len() != bytes {
                return Err(block::Error::InvalidParam);
            }
            let data = SKHYNIX_UAS_FLOW_SERVICE
                .read_blocks(lba, blocks)
                .await
                .map_err(flow_error_to_block_error)?;
            if data.len() != dst.len() {
                return Err(block::Error::Corrupted);
            }
            dst.copy_from_slice(data.as_slice());
            Ok(())
        })
    }

    fn write_blocks<'a>(
        &'a mut self,
        lba: u64,
        buf: &'a [u8],
    ) -> block::BoxFuture<'a, block::Result<()>> {
        Box::pin(async move {
            let (lba, blocks) = self.write_bounds(lba, buf.len())?;
            SKHYNIX_UAS_FLOW_SERVICE
                .write_blocks(lba, blocks, buf.to_vec())
                .await
                .map_err(flow_error_to_block_error)
        })
    }

    fn flush<'a>(&'a mut self) -> block::BoxFuture<'a, block::Result<()>> {
        Box::pin(async move {
            SKHYNIX_UAS_FLOW_SERVICE
                .flush()
                .await
                .map_err(flow_error_to_block_error)
        })
    }
}

#[allow(dead_code)]
pub(crate) struct SkhynixUasFlowPipes {
    device: Device,
    command_out: EndpointBulkOut,
    status_in: EndpointBulkIn,
    data_in: EndpointBulkIn,
    data_out: EndpointBulkOut,
    next_tag: u32,
    block_size: u32,
    block_count: u64,
}

impl SkhynixUasFlowPipes {
    fn next_command_tag(&mut self) -> u16 {
        let tag = mass::uas_stream_id_from_tag(self.next_tag);
        self.next_tag = self.next_tag.wrapping_add(1).max(1);
        tag
    }
}

#[allow(dead_code)]
#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn skhynix_uas_flow_service_task(
    service: &'static SkhynixUasFlowService,
    mut pipes: SkhynixUasFlowPipes,
) {
    let state_sender = SKHYNIX_UAS_FLOW_STATE.sender();
    let mut state = SkhynixUasFlowState {
        online: true,
        ..SkhynixUasFlowState::offline()
    };
    state_sender.send(state);

    if SKHYNIX_UAS_FLOW_BOOT_BENCH_ENABLED {
        let bench_config = crate::shell2::cmds::bench::SkhynixUasFlowBenchConfig::boot_default(
            pipes.block_size,
            pipes.block_count,
        );
        match crate::shell2::cmds::bench::skhynix_uas_flow_read_bench(
            &mut pipes.command_out,
            &mut pipes.status_in,
            &mut pipes.data_in,
            &mut pipes.next_tag,
            bench_config,
        )
        .await
        {
            Ok(report) => {
                state.completed = state.completed.saturating_add(u64::from(report.reads));
                state.bytes_in = state.bytes_in.saturating_add(report.bytes);
                state.last_error = None;
                state_sender.send(state);
                crate::log!(
                    "crabusb: skhynix-green proof=uas-flow ready=true bench=read-lanes temporary_default=true lanes={} reads={} bytes={} elapsed_ms={} speed={} min_read_us={} max_read_us={} no_block_register=true\n",
                    report.lanes,
                    report.reads,
                    report.bytes,
                    report.elapsed_ms,
                    crate::shell2::cmds::bench::format_speed(
                        crate::shell2::cmds::bench::bps_from_progress(
                            report.bytes,
                            report.elapsed_ms
                        )
                    ),
                    report.min_read_us,
                    report.max_read_us
                );
            }
            Err(err) => {
                state.last_error = Some(map_mass_probe_error(err));
                state_sender.send(state);
                crate::log!(
                    "crabusb: skhynix-green proof=uas-flow ready=true bench=read-lanes temporary_default=true status=failed err={:?} no_block_register=true\n",
                    err
                );
            }
        }
    } else {
        crate::log!(
            "crabusb: skhynix-green proof=uas-flow ready=true bench=disabled no_block_register=true\n"
        );
    }

    loop {
        let request = service.requests.receive().await;
        state.queued = 0;
        state.active_lanes = 1;
        state_sender.send(state);

        let tag = pipes.next_command_tag();
        match request {
            SkhynixUasFlowRequest::Read { lba, blocks } => {
                let bytes = (blocks as usize).saturating_mul(pipes.block_size.max(1) as usize);
                let mut data = alloc::vec![0u8; bytes];
                let result = mass::read_blocks_uas_skhynix(
                    &mut pipes.command_out,
                    &mut pipes.status_in,
                    &mut pipes.data_in,
                    lba,
                    blocks,
                    data.as_mut_slice(),
                    u32::from(tag),
                )
                .await
                .map(|()| data)
                .map_err(map_mass_probe_error);

                state.active_lanes = 0;
                state.completed = state.completed.saturating_add(1);
                match &result {
                    Ok(data) => {
                        state.bytes_in = state.bytes_in.saturating_add(data.len() as u64);
                        state.last_error = None;
                    }
                    Err(err) => state.last_error = Some(*err),
                }
                state_sender.send(state);
                SKHYNIX_UAS_FLOW_REPLY.signal(SkhynixUasFlowReply::Read {
                    tag,
                    lba,
                    blocks,
                    result,
                });
            }
            SkhynixUasFlowRequest::Write { lba, blocks, data } => {
                let data_len = data.len();
                let result = mass::write_blocks_uas_skhynix(
                    &mut pipes.command_out,
                    &mut pipes.status_in,
                    &mut pipes.data_out,
                    lba,
                    blocks,
                    data.as_slice(),
                    u32::from(tag),
                )
                .await
                .map_err(map_mass_probe_error);

                state.active_lanes = 0;
                state.completed = state.completed.saturating_add(1);
                match result {
                    Ok(()) => {
                        state.bytes_out = state.bytes_out.saturating_add(data_len as u64);
                        state.last_error = None;
                        state_sender.send(state);
                        SKHYNIX_UAS_FLOW_REPLY.signal(SkhynixUasFlowReply::Write {
                            tag,
                            lba,
                            blocks,
                            result: Ok(()),
                        });
                    }
                    Err(err) => {
                        state.last_error = Some(err);
                        state_sender.send(state);
                        SKHYNIX_UAS_FLOW_REPLY.signal(SkhynixUasFlowReply::Write {
                            tag,
                            lba,
                            blocks,
                            result: Err(err),
                        });
                    }
                }
            }
            SkhynixUasFlowRequest::Flush => {
                let result = mass::synchronize_cache_uas_skhynix(
                    &mut pipes.command_out,
                    &mut pipes.status_in,
                    u32::from(tag),
                )
                .await
                .map_err(map_mass_probe_error);

                state.active_lanes = 0;
                state.completed = state.completed.saturating_add(1);
                match result {
                    Ok(()) => state.last_error = None,
                    Err(err) => state.last_error = Some(err),
                }
                state_sender.send(state);
                SKHYNIX_UAS_FLOW_REPLY.signal(SkhynixUasFlowReply::Flush { tag, result });
            }
        }
    }
}

fn flow_error_to_block_error(err: SkhynixUasFlowError) -> block::Error {
    match err {
        SkhynixUasFlowError::QueueFull | SkhynixUasFlowError::NotReady => block::Error::NotReady,
        SkhynixUasFlowError::Transport | SkhynixUasFlowError::Status => block::Error::Io,
        SkhynixUasFlowError::ShortData => block::Error::Corrupted,
    }
}

fn map_mass_probe_error(err: mass::MassProbeError) -> SkhynixUasFlowError {
    match err {
        mass::MassProbeError::Transport(_) => SkhynixUasFlowError::Transport,
        mass::MassProbeError::ShortData => SkhynixUasFlowError::ShortData,
        mass::MassProbeError::Csw => SkhynixUasFlowError::Status,
    }
}

fn is_skhynix_green_candidate(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == SKHYNIX_GREEN_VID && product_id == SKHYNIX_GREEN_PID
}

fn register_active_green_probe(probe: ActiveGreenProbe) -> bool {
    let mut probes = ACTIVE_GREEN_PROBES.lock();
    if probes.iter().any(|active| *active == probe) {
        return false;
    }
    probes.push(probe).is_ok()
}

fn unregister_active_green_probe(probe: ActiveGreenProbe) {
    let mut probes = ACTIVE_GREEN_PROBES.lock();
    if let Some(idx) = probes.iter().position(|active| *active == probe) {
        probes.remove(idx);
    }
}

pub(crate) async fn maybe_start_skhynix_green(
    host: &mut USBHost,
    dev_info: &DeviceInfo,
    spawner: &Spawner,
    controller_id: u32,
) -> bool {
    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    if !is_skhynix_green_candidate(vendor_id, product_id) {
        return false;
    }

    let topology = dev_info.topology();
    let location = dev_info.location();
    crate::log!(
        "crabusb: skhynix-green {:04X}:{:04X} proof=detect ctrl={} root_port={} route=0x{:X} speed={:?} cfgs={}\n",
        vendor_id,
        product_id,
        controller_id,
        topology.root_port_id,
        location.route_string,
        topology.port_speed,
        dev_info.configurations().len()
    );

    let transport_plan = mass::inspect_mass_transports(dev_info.configurations());
    crate::log!(
        "crabusb: skhynix-green {:04X}:{:04X} proof=transport-plan uas_candidates={} bot_present={}\n",
        vendor_id,
        product_id,
        transport_plan.uas.len(),
        transport_plan.bot.is_some()
    );

    let Some(target) =
        mass::pick_skhynix_uas_target(vendor_id, product_id, transport_plan.uas.as_slice())
    else {
        crate::log!(
            "crabusb: skhynix-green {:04X}:{:04X} proof=uas-target status=missing fallback=bot no_block_register=true\n",
            vendor_id,
            product_id
        );
        return false;
    };

    crate::log!(
        "crabusb: skhynix-green {:04X}:{:04X} proof=uas-target if#{} alt={} cfg={} cmd_out=0x{:02X}/{} status_in=0x{:02X}/{} data_in=0x{:02X}/{} data_out=0x{:02X}/{}\n",
        vendor_id,
        product_id,
        target.interface_number,
        target.alternate_setting,
        target.configuration_value,
        target.command_out,
        target.command_out_max_packet_size,
        target.status_in,
        target.status_in_max_packet_size,
        target.data_in,
        target.data_in_max_packet_size,
        target.data_out,
        target.data_out_max_packet_size
    );

    let active_probe = ActiveGreenProbe {
        controller_id,
        slot_id: dev_info.id() as u32,
    };
    if !register_active_green_probe(active_probe) {
        crate::log!(
            "crabusb: skhynix-green {:04X}:{:04X} proof=single-active status=busy-before-open ctrl={} slot={} no_block_register=true\n",
            vendor_id,
            product_id,
            controller_id,
            active_probe.slot_id
        );
        return true;
    }

    let mut device = match host.open_device(dev_info).await {
        Ok(device) => device,
        Err(err) => {
            crate::log!(
                "crabusb: skhynix-green {:04X}:{:04X} proof=open status=failed err={:?} no_block_register=true\n",
                vendor_id,
                product_id,
                err
            );
            unregister_active_green_probe(active_probe);
            return true;
        }
    };

    crate::log!(
        "crabusb: skhynix-green {:04X}:{:04X} proof=open status=ok ctrl={} slot={}\n",
        vendor_id,
        product_id,
        controller_id,
        u32::from(device.slot_id())
    );

    if let Err(err) = device
        .ep_ctrl()
        .set_configuration(target.configuration_value)
        .await
    {
        crate::log!(
            "crabusb: skhynix-green {:04X}:{:04X} proof=set-config cfg={} status=failed err={:?} no_block_register=true\n",
            vendor_id,
            product_id,
            target.configuration_value,
            err
        );
        unregister_active_green_probe(active_probe);
        return true;
    }

    crate::log!(
        "crabusb: skhynix-green {:04X}:{:04X} proof=set-config cfg={} status=ok\n",
        vendor_id,
        product_id,
        target.configuration_value
    );

    let mut interface = match claim_interface(
        &mut device,
        target.interface_number,
        target.alternate_setting,
    )
    .await
    {
        Ok(interface) => interface,
        Err(err) => {
            crate::log!(
                "crabusb: skhynix-green {:04X}:{:04X} proof=claim if#{} alt={} status=failed err={:?} no_block_register=true\n",
                vendor_id,
                product_id,
                target.interface_number,
                target.alternate_setting,
                err
            );
            unregister_active_green_probe(active_probe);
            return true;
        }
    };

    crate::log!(
        "crabusb: skhynix-green {:04X}:{:04X} proof=claim if#{} alt={} status=ok\n",
        vendor_id,
        product_id,
        target.interface_number,
        target.alternate_setting
    );

    let mut command_out = match interface.endpoint_bulk_out(target.command_out).await {
        Ok(endpoint) => endpoint,
        Err(err) => {
            crate::log!(
                "crabusb: skhynix-green {:04X}:{:04X} proof=endpoints cmd_out=false err={:?} no_block_register=true\n",
                vendor_id,
                product_id,
                err
            );
            unregister_active_green_probe(active_probe);
            return true;
        }
    };
    let mut status_in = match interface.endpoint_bulk_in(target.status_in).await {
        Ok(endpoint) => endpoint,
        Err(err) => {
            crate::log!(
                "crabusb: skhynix-green {:04X}:{:04X} proof=endpoints status_in=false err={:?} no_block_register=true\n",
                vendor_id,
                product_id,
                err
            );
            unregister_active_green_probe(active_probe);
            return true;
        }
    };
    let mut data_in = match interface.endpoint_bulk_in(target.data_in).await {
        Ok(endpoint) => endpoint,
        Err(err) => {
            crate::log!(
                "crabusb: skhynix-green {:04X}:{:04X} proof=endpoints data_in=false err={:?} no_block_register=true\n",
                vendor_id,
                product_id,
                err
            );
            unregister_active_green_probe(active_probe);
            return true;
        }
    };
    let mut data_out = match interface.endpoint_bulk_out(target.data_out).await {
        Ok(endpoint) => endpoint,
        Err(err) => {
            crate::log!(
                "crabusb: skhynix-green {:04X}:{:04X} proof=endpoints data_out=false err={:?} no_block_register=true\n",
                vendor_id,
                product_id,
                err
            );
            unregister_active_green_probe(active_probe);
            return true;
        }
    };

    crate::log!(
        "crabusb: skhynix-green {:04X}:{:04X} proof=endpoints cmd_out=true status_in=true data_in=true data_out=true no_scsi_yet=false no_block_register_yet=true no_trueosfs_yet=true\n",
        vendor_id,
        product_id
    );

    let probe = match mass::exercise_mass_uas_skhynix(
        &mut command_out,
        &mut status_in,
        &mut data_in,
        &mut data_out,
    )
    .await
    {
        Ok(probe) => probe,
        Err(err) => {
            crate::log!(
                "crabusb: skhynix-green {:04X}:{:04X} proof=scsi-probe status=failed err={:?} no_block_register=true\n",
                vendor_id,
                product_id,
                err
            );
            unregister_active_green_probe(active_probe);
            return true;
        }
    };

    crate::log!(
        "crabusb: skhynix-green {:04X}:{:04X} proof=scsi-probe status=ok bs={} blocks={} vendor='{}' product='{}'\n",
        vendor_id,
        product_id,
        probe.block_size,
        probe.block_count,
        probe.vendor,
        probe.product
    );

    drop(interface);

    let pipes = SkhynixUasFlowPipes {
        device,
        command_out,
        status_in,
        data_in,
        data_out,
        next_tag: 0x4752_0001,
        block_size: probe.block_size.max(1),
        block_count: probe.block_count.max(1),
    };

    let task = match skhynix_uas_flow_service_task(&SKHYNIX_UAS_FLOW_SERVICE, pipes) {
        Ok(task) => task,
        Err(err) => {
            crate::log!(
                "crabusb: skhynix-green {:04X}:{:04X} proof=uas-flow status=task-token-failed err={:?} no_block_register=true trueosfs_auto_mount=false\n",
                vendor_id,
                product_id,
                err
            );
            unregister_active_green_probe(active_probe);
            return true;
        }
    };

    spawner.spawn(task);
    crate::log!(
        "crabusb: skhynix-green {:04X}:{:04X} proof=uas-flow status=started bs={} blocks={} clean_stack_only=true trueosfs_auto_mount=false\n",
        vendor_id,
        product_id,
        probe.block_size.max(1),
        probe.block_count.max(1)
    );

    let desc = block::DeviceDescriptor::new(block::DeviceKind::Unknown)
        .with_label(String::from("skhynix-uas-flow"));
    let handle = block::register_device_deferred_mount(
        desc,
        SkhynixUasFlowBlockDevice {
            block_size: probe.block_size.max(1),
            block_count: probe.block_count.max(1),
        },
    );
    crate::log!(
        "crabusb: skhynix-green {:04X}:{:04X} proof=mass-handoff status=registered disk={} label=skhynix-uas-flow writable=1 deferred_mount=true\n",
        vendor_id,
        product_id,
        handle.id()
    );

    true
}
