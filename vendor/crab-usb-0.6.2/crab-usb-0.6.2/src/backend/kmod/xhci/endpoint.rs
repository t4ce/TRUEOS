use core::{
    ptr::NonNull,
    sync::atomic::{AtomicU64, Ordering},
};

use alloc::{collections::BTreeMap, sync::Arc};

use dma_api::{DArray, DmaDirection};
use mbarrier::mb;
use spin::Mutex;
use usb_if::{
    descriptor::{self, EndpointDescriptor},
    err::TransferError,
    transfer::{BmRequestType, Direction},
};
use xhci::{
    registers::doorbell,
    ring::trb::{
        event::TransferEvent,
        transfer::{self, Isoch, Normal},
    },
};

use super::{reg::SlotBell, ring::SendRing, transfer::TransferId, DirectionExt};
use crate::{
    backend::{
        ty::{
            ep::{EndpointOp, TransferHandle},
            transfer::{Transfer, TransferKind},
        },
        Dci,
    },
    debug_record_submit_stream,
    err::{ConvertXhciError, HostError},
    osal::Kernel,
    BusAddr,
};

const STREAM_CONTEXT_ALIGNMENT: usize = 64;
const STREAM_CONTEXT_SCT_PRIMARY_TR: u64 = 1 << 1;
const STREAM_CONTEXT_DCS: u64 = 1;
static XHCI_COMPLETION_LAST_LOG_TICK: AtomicU64 = AtomicU64::new(0);

fn xhci_completion_log_allowed() -> bool {
    let interval = embassy_time_driver::TICK_HZ.max(1);
    let now = embassy_time_driver::now();
    let now_marker = now.saturating_add(1);
    let mut previous_marker = XHCI_COMPLETION_LAST_LOG_TICK.load(Ordering::Relaxed);

    loop {
        if previous_marker != 0 {
            let previous = previous_marker.saturating_sub(1);
            if now >= previous && now.saturating_sub(previous) < interval {
                return false;
            }
        }

        match XHCI_COMPLETION_LAST_LOG_TICK.compare_exchange_weak(
            previous_marker,
            now_marker,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return true,
            Err(actual) => previous_marker = actual,
        }
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct StreamContext([u32; 4]);

impl StreamContext {
    fn primary_transfer_ring(ring: BusAddr) -> Self {
        let raw = ring.raw() | STREAM_CONTEXT_SCT_PRIMARY_TR | STREAM_CONTEXT_DCS;
        Self([raw as u32, (raw >> 32) as u32, 0, 0])
    }
}

pub struct Endpoint {
    slot_id: u8,
    dci: Dci,
    pub ring: SendRing<TransferEvent>,
    stream_contexts: Option<DArray<StreamContext>>,
    stream_rings: BTreeMap<u16, SendRing<TransferEvent>>,
    max_primary_streams: u8,
    bell: Arc<Mutex<SlotBell>>,
    transfers: BTreeMap<TransferId, Transfer>,
    kernel: Kernel,
}

unsafe impl Send for Endpoint {}
unsafe impl Sync for Endpoint {}

impl Endpoint {
    pub fn new(
        slot_id: u8,
        dci: Dci,
        kernel: &Kernel,
        bell: Arc<Mutex<SlotBell>>,
    ) -> crate::err::Result<Self> {
        let ring = SendRing::new(DmaDirection::Bidirectional, kernel)?;

        Ok(Self {
            slot_id,
            dci,
            ring,
            stream_contexts: None,
            stream_rings: BTreeMap::new(),
            max_primary_streams: 0,
            bell,
            transfers: BTreeMap::new(),
            kernel: kernel.clone(),
        })
    }

    pub fn bus_addr(&self) -> BusAddr {
        self.ring.bus_addr()
    }

    pub fn config_dequeue_pointer(&self) -> BusAddr {
        self.stream_contexts
            .as_ref()
            .map(|contexts| contexts.dma_addr().as_u64().into())
            .unwrap_or_else(|| self.ring.bus_addr())
    }

    pub fn has_primary_streams(&self) -> bool {
        self.stream_contexts.is_some()
    }

    pub fn max_primary_streams(&self) -> u8 {
        self.max_primary_streams
    }

    pub fn stream_rings(&self) -> impl Iterator<Item = &SendRing<TransferEvent>> {
        self.stream_rings.values()
    }

    pub fn enable_primary_streams(
        &mut self,
        stream_context_count: usize,
    ) -> crate::err::Result<()> {
        assert!(stream_context_count.is_power_of_two());
        assert!(stream_context_count >= 2);

        let mut contexts = self
            .kernel
            .array_zero_with_align::<StreamContext>(
                stream_context_count,
                STREAM_CONTEXT_ALIGNMENT,
                DmaDirection::ToDevice,
            )
            .map_err(HostError::from)?;
        let mut stream_rings = BTreeMap::new();

        for stream_id in 1..stream_context_count {
            let ring = SendRing::new(DmaDirection::Bidirectional, &self.kernel)?;
            contexts.set(stream_id, StreamContext::primary_transfer_ring(ring.bus_addr()));
            stream_rings.insert(stream_id as u16, ring);
        }

        self.max_primary_streams = (stream_context_count.trailing_zeros() as u8).saturating_sub(1);
        self.stream_contexts = Some(contexts);
        self.stream_rings = stream_rings;

        info!(
            "crabusb/xhci/ep: primary-streams dci={} contexts={} max_pstreams={} ctx=0x{:x}",
            self.dci.as_u8(),
            stream_context_count,
            self.max_primary_streams,
            self.config_dequeue_pointer().raw()
        );
        Ok(())
    }

    fn doorbell(&mut self, stream_id: u16) {
        let mut bell = doorbell::Register::default();
        bell.set_doorbell_target(self.dci.into());
        if stream_id != 0 {
            bell.set_doorbell_stream_id(stream_id);
        }
        self.bell.lock().ring(bell);
    }

    pub fn ring(&self) -> &SendRing<TransferEvent> {
        &self.ring
    }

    fn handle_transfer_completion(
        &mut self,
        c: &TransferEvent,
        handle: BusAddr,
    ) -> Result<Transfer, TransferError> {
        let mut t = self.transfers.remove(&TransferId(handle)).unwrap();
        let completion_code = c.completion_code().ok();
        match c.completion_code() {
            Ok(code) => match code.to_result() {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
            Err(_e) => Err(TransferError::Other(anyhow!("Transfer failed"))),
        }?;

        let transfer_len;

        // xHCI 规范：trb_transfer_length 字段根据端点方向有不同的含义
        // - IN 端点（设备到主机）：表示未传输的剩余字节数
        // - OUT 端点（主机到设备）：表示实际传输的字节数
        if matches!(t.direction, Direction::In) {
            // 对于 IN 端点，实际传输长度 = 请求长度 - 剩余长度
            transfer_len = t
                .buffer_len()
                .saturating_sub(c.trb_transfer_length() as usize);

            if transfer_len > 0 {
                // 刷新/失效缓存，确保从 DMA 缓冲读取到有效数据
                // t.dma_slice().prepare_read_all();
                t.prepare_read_all();
            }
        } else {
            // xHCI transfer events report remaining bytes. For a successful
            // OUT transfer this is typically 0, so actual bytes sent are
            // requested - residual.
            transfer_len = t
                .buffer_len()
                .saturating_sub(c.trb_transfer_length() as usize);
        }
        t.transfer_len = transfer_len;
        if xhci_completion_log_allowed() {
            info!(
                "crabusb/xhci/ep: completion dci={} dir={:?} requested={} residual={} actual={} code={:?} ptr=0x{:x}",
                self.dci.as_u8(),
                t.direction,
                t.buffer_len(),
                c.trb_transfer_length(),
                t.transfer_len,
                completion_code,
                handle.raw()
            );
        }
        Ok(t)
    }

    fn enque_trb(&mut self, trb: transfer::Allowed) -> TransferId {
        TransferId(self.ring.enque_transfer(trb))
    }

    fn enque_trb_on(ring: &mut SendRing<TransferEvent>, trb: transfer::Allowed) -> TransferId {
        TransferId(ring.enque_transfer(trb))
    }

    fn enque_bulk_or_interrupt_on(
        ring: &mut SendRing<TransferEvent>,
        bus_addr: u64,
        len: usize,
    ) -> TransferId {
        const MAX_NORMAL_TRB_BYTES: usize = 64 * 1024;

        if len <= MAX_NORMAL_TRB_BYTES {
            let trb = transfer::Allowed::Normal(
                *Normal::new()
                    .set_data_buffer_pointer(bus_addr as _)
                    .set_trb_transfer_length(len as _)
                    .set_interrupter_target(0)
                    .set_interrupt_on_short_packet()
                    .set_interrupt_on_completion(),
            );
            return Self::enque_trb_on(ring, trb);
        }

        let mut handle = TransferId(BusAddr(0));
        let mut offset = 0usize;
        while offset < len {
            let chunk = core::cmp::min(MAX_NORMAL_TRB_BYTES, len - offset);
            let last = offset + chunk >= len;
            let mut trb = *Normal::new()
                .set_data_buffer_pointer(bus_addr + offset as u64)
                .set_trb_transfer_length(chunk as _)
                .set_interrupter_target(0);

            if last {
                trb.set_interrupt_on_short_packet()
                    .set_interrupt_on_completion();
            } else {
                trb.set_chain_bit();
            }

            handle = Self::enque_trb_on(ring, transfer::Allowed::Normal(trb));
            offset += chunk;
        }

        handle
    }

    fn transfer_ring(&self, stream_id: u16) -> Option<&SendRing<TransferEvent>> {
        if stream_id == 0 {
            if self.has_primary_streams() {
                None
            } else {
                Some(&self.ring)
            }
        } else {
            self.stream_rings.get(&stream_id)
        }
    }

    fn transfer_ring_mut(
        &mut self,
        stream_id: u16,
    ) -> Result<&mut SendRing<TransferEvent>, TransferError> {
        if stream_id == 0 {
            if self.has_primary_streams() {
                return Err(TransferError::Other(anyhow!(
                    "stream-capable endpoint requires a non-zero stream id"
                )));
            }
            Ok(&mut self.ring)
        } else {
            self.stream_rings.get_mut(&stream_id).ok_or_else(|| {
                TransferError::Other(anyhow!("endpoint stream id {} is not configured", stream_id))
            })
        }
    }

    fn enque_iso(&mut self, bus_addr: u64, buff_len: usize, num_iso_packets: usize) -> TransferId {
        if buff_len == 0 || num_iso_packets < 2 {
            self.enque_iso_trb(bus_addr, buff_len)
        } else {
            self.enque_iso_multi(bus_addr, buff_len, num_iso_packets)
        }
    }

    fn enque_iso_trb(&mut self, bus_addr: u64, buff_len: usize) -> TransferId {
        let mut trb = Isoch::new();
        trb.set_data_buffer_pointer(bus_addr as _)
            .set_trb_transfer_length(buff_len as _)
            .set_interrupter_target(0)
            .set_interrupt_on_completion();
        trb.set_start_isoch_asap();
        // 创建Isoch TRB
        let trb = transfer::Allowed::Isoch(trb);
        self.enque_trb(trb)
    }
    fn enque_iso_multi(&mut self, bus_addr: u64, len: usize, num_iso_packets: usize) -> TransferId {
        let len = len as u64;
        let packet_size = if len == 0 {
            0
        } else {
            len.div_ceil(num_iso_packets as u64)
        };

        let mut id = TransferId(BusAddr(0));

        for i in 0..num_iso_packets {
            let i = i as u64;
            let offset = i * packet_size;
            if offset >= len {
                break; // 避免越界
            }
            let remaining = len - offset;
            let current_size = if remaining >= packet_size {
                packet_size
            } else {
                remaining
            };

            if current_size > 0 {
                let current_addr = bus_addr + offset;
                let is_last = (i == num_iso_packets as u64 - 1) || (offset + current_size >= len);

                if i == 0 {
                    // 第一个TRB必须是Isoch TRB
                    id = self.enque_iso_trb(current_addr, current_size as _);
                } else {
                    // Each subsequent packet is its own isoch TD with SIA so the
                    // controller schedules it at the next available service interval.
                    let mut trb = Isoch::new();
                    trb.set_data_buffer_pointer(current_addr as _)
                        .set_trb_transfer_length(current_size as _)
                        .set_interrupter_target(0)
                        .set_start_isoch_asap();
                    if is_last {
                        trb.set_interrupt_on_completion();
                    }
                    let trb = transfer::Allowed::Isoch(trb);
                    id = self.enque_trb(trb);
                }
            }
        }

        id
    }
}

impl EndpointOp for Endpoint {
    fn submit(
        &mut self,
        transfer: crate::backend::ty::transfer::Transfer,
    ) -> Result<crate::backend::ty::ep::TransferHandle<'_>, TransferError> {
        let mut data_bus_addr = 0;
        if transfer.buffer_len() > 0 {
            // let data_slice = transfer.dma_slice();
            if matches!(transfer.direction, Direction::Out) {
                // data_slice.confirm_write_all();
                transfer.confirm_write_all();
            }
            // data_bus_addr = data_slice.bus_addr();
            data_bus_addr = transfer.dma_addr();

            // 检查缓冲区起始地址是否在 dma_mask 范围内
            assert!(
                data_bus_addr <= self.kernel.dma_mask(),
                "DMA address 0x{:x} exceeds controller DMA mask 0x{:x} ({}-bit addressing)",
                data_bus_addr,
                self.kernel.dma_mask(),
                if self.kernel.dma_mask() == u32::MAX as u64 {
                    32
                } else {
                    64
                }
            );

            // 检查缓冲区结束地址是否在 dma_mask 范围内
            let buffer_end = data_bus_addr + transfer.buffer_len() as u64;
            assert!(
                buffer_end <= self.kernel.dma_mask(),
                "DMA buffer end 0x{:x} (start: 0x{:x}, len: {} bytes) exceeds controller DMA mask 0x{:x} ({}-bit addressing)",
                buffer_end,
                data_bus_addr,
                transfer.buffer_len(),
                self.kernel.dma_mask(),
                if self.kernel.dma_mask() == u32::MAX as u64 {
                    32
                } else {
                    64
                }
            );
        }

        let data_len = transfer.buffer_len();
        let dir = transfer.direction;
        let stream_id = match &transfer.kind {
            TransferKind::Bulk | TransferKind::Interrupt => transfer.stream_id,
            _ => 0,
        };

        let mut handle = TransferId(BusAddr(0));
        let mut ring_ptr = self.ring.bus_addr().raw();

        match &transfer.kind {
            TransferKind::Control(t) => {
                let bm_request_type = BmRequestType {
                    direction: transfer.direction,
                    request_type: t.request_type,
                    recipient: t.recipient,
                };

                let mut setup = transfer::SetupStage::default();
                setup
                    .set_request_type(bm_request_type.into())
                    .set_request(t.request.into())
                    .set_value(t.value)
                    .set_index(t.index)
                    .set_length(0)
                    .set_transfer_type(transfer::TransferType::No);

                let mut data = None;

                if transfer.buffer_len() > 0 {
                    setup
                        .set_transfer_type(dir.to_xhci_transfer_type())
                        .set_length(data_len as _);

                    let mut _data = transfer::DataStage::default();
                    _data
                        .set_data_buffer_pointer(data_bus_addr)
                        .set_trb_transfer_length(data_len as _)
                        .set_direction(transfer.direction.to_xhci_direction());
                    data = Some(_data);
                }

                let mut status = transfer::StatusStage::default();
                status.set_interrupt_on_completion();

                if matches!(transfer.direction, Direction::In) && transfer.buffer_len() > 0 {
                    status.clear_direction();
                } else {
                    status.set_direction();
                }

                self.ring.enque_transfer(setup.into());
                if let Some(data) = data {
                    self.ring.enque_transfer(data.into());
                }
                handle.0 = self.ring.enque_transfer(status.into());
            }
            TransferKind::Interrupt | TransferKind::Bulk => {
                let ring = self.transfer_ring_mut(stream_id)?;
                ring_ptr = ring.bus_addr().raw();
                handle = Self::enque_bulk_or_interrupt_on(ring, data_bus_addr, data_len);
            }
            TransferKind::Isochronous { num_pkgs } => {
                handle = self.enque_iso(data_bus_addr, data_len, *num_pkgs);
            }
        }
        self.transfers.insert(handle, transfer);
        debug_record_submit_stream(
            self.slot_id,
            self.dci.as_u8(),
            if matches!(dir, Direction::In) { 1 } else { 2 },
            stream_id,
            data_len as u32,
            handle.0.raw(),
            ring_ptr,
        );
        mb();
        self.doorbell(stream_id);

        Ok(TransferHandle::new(handle.0.raw(), self))
    }

    fn query_transfer(
        &mut self,
        id: u64,
    ) -> Option<Result<crate::backend::ty::transfer::Transfer, TransferError>> {
        let id = BusAddr(id);
        let stream_id = self
            .transfers
            .get(&TransferId(id))
            .map(|transfer| transfer.stream_id)
            .unwrap_or(0);
        let c = self.transfer_ring(stream_id)?.get_finished(id)?;
        let res = self.handle_transfer_completion(&c, id);
        Some(res)
    }

    fn register_cx(&self, id: u64, cx: &mut core::task::Context<'_>) {
        let id = BusAddr(id);
        let stream_id = self
            .transfers
            .get(&TransferId(id))
            .map(|transfer| transfer.stream_id)
            .unwrap_or(0);
        if let Some(ring) = self.transfer_ring(stream_id) {
            ring.register_cx(id, cx);
        }
    }

    fn new_transfer(
        &mut self,
        kind: TransferKind,
        direction: Direction,
        buff: Option<(NonNull<u8>, usize)>,
    ) -> Transfer {
        Transfer::new(&self.kernel, kind, direction, buff)
    }
}

pub(crate) trait EndpointDescriptorExt {
    fn endpoint_type(&self) -> xhci::context::EndpointType;
}

impl EndpointDescriptorExt for EndpointDescriptor {
    fn endpoint_type(&self) -> xhci::context::EndpointType {
        match self.transfer_type {
            descriptor::EndpointType::Control => xhci::context::EndpointType::Control,
            descriptor::EndpointType::Isochronous => match self.direction {
                usb_if::transfer::Direction::Out => xhci::context::EndpointType::IsochOut,
                usb_if::transfer::Direction::In => xhci::context::EndpointType::IsochIn,
            },
            descriptor::EndpointType::Bulk => match self.direction {
                usb_if::transfer::Direction::Out => xhci::context::EndpointType::BulkOut,
                usb_if::transfer::Direction::In => xhci::context::EndpointType::BulkIn,
            },
            descriptor::EndpointType::Interrupt => match self.direction {
                usb_if::transfer::Direction::Out => xhci::context::EndpointType::InterruptOut,
                usb_if::transfer::Direction::In => xhci::context::EndpointType::InterruptIn,
            },
        }
    }
}
