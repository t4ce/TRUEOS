use alloc::{boxed::Box, vec::Vec as AllocVec};
use crab_usb::{Device, DeviceInfo, EndpointBulkIn, EndpointBulkOut, USBHost};
use embassy_executor::Spawner;
use heapless::Vec;
use spin::Mutex;

use crate::disc::block;

use super::api::claim_interface;
use super::mass;

const SKHYNIX_GREEN_VID: u16 = 0x152E;
const SKHYNIX_GREEN_PID: u16 = 0x7001;
const MAX_ACTIVE_GREEN_PROBES: usize = 4;
const GREEN_MAX_TRANSFER_BYTES: u64 = 1024 * 1024;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ActiveGreenProbe {
    controller_id: u32,
    slot_id: u32,
}

static ACTIVE_GREEN_PROBES: Mutex<Vec<ActiveGreenProbe, MAX_ACTIVE_GREEN_PROBES>> =
    Mutex::new(Vec::new());

struct GreenSkhynixBlockDevice {
    _device: Device,
    command_out: EndpointBulkOut,
    status_in: EndpointBulkIn,
    data_in: EndpointBulkIn,
    _data_out: EndpointBulkOut,
    block_size: u32,
    block_count: u64,
    next_tag: u32,
}

unsafe impl Send for GreenSkhynixBlockDevice {}

impl GreenSkhynixBlockDevice {
    fn next_command_tag(&mut self) -> u32 {
        let tag = self.next_tag.max(1);
        self.next_tag = self.next_tag.wrapping_add(1).max(1);
        tag
    }

    fn validate_read(&self, lba: u64, blocks: usize, dst_len: usize) -> block::Result<usize> {
        let bs = self.block_size as usize;
        if bs == 0 {
            return Err(block::Error::InvalidParam);
        }
        if blocks == 0 {
            return if dst_len == 0 {
                Ok(0)
            } else {
                Err(block::Error::InvalidParam)
            };
        }
        let bytes = blocks.checked_mul(bs).ok_or(block::Error::InvalidParam)?;
        if dst_len != bytes {
            return Err(block::Error::InvalidParam);
        }
        let end = lba
            .checked_add(blocks as u64)
            .ok_or(block::Error::OutOfBounds)?;
        if end > self.block_count || end > u64::from(u32::MAX) + 1 {
            return Err(block::Error::OutOfBounds);
        }
        if bytes as u64 > GREEN_MAX_TRANSFER_BYTES {
            return Err(block::Error::InvalidParam);
        }
        Ok(bytes)
    }

    async fn read_blocks_into_green(
        &mut self,
        lba: u64,
        blocks: usize,
        dst: &mut [u8],
    ) -> block::Result<()> {
        let bs = self.block_size as usize;
        let _bytes = self.validate_read(lba, blocks, dst.len())?;
        if blocks == 0 {
            return Ok(());
        }

        let max_blocks_by_bytes = (GREEN_MAX_TRANSFER_BYTES as usize / bs).max(1);
        let max_blocks = max_blocks_by_bytes.min(u16::MAX as usize).max(1);
        let mut cur_lba = lba;
        let mut remaining = blocks;
        let mut off = 0usize;

        while remaining != 0 {
            let blocks_here = remaining.min(max_blocks);
            let bytes_here = blocks_here * bs;
            let tag = self.next_command_tag();
            mass::read_blocks_uas_skhynix(
                &mut self.command_out,
                &mut self.status_in,
                &mut self.data_in,
                cur_lba as u32,
                blocks_here as u16,
                &mut dst[off..off + bytes_here],
                tag,
            )
            .await
            .map_err(|err| {
                crate::log!(
                    "crabusb: skhynix-green read err lba={} blocks={} tag=0x{:08X} err={:?}\n",
                    cur_lba,
                    blocks_here,
                    tag,
                    err
                );
                block::Error::Io
            })?;

            cur_lba = cur_lba.saturating_add(blocks_here as u64);
            remaining = remaining.saturating_sub(blocks_here);
            off = off.saturating_add(bytes_here);
        }

        Ok(())
    }
}

impl block::BlockDevice for GreenSkhynixBlockDevice {
    fn block_size_bytes(&self) -> u32 {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn max_transfer_bytes(&self) -> u64 {
        GREEN_MAX_TRANSFER_BYTES
    }

    fn read_blocks<'a>(
        &'a mut self,
        lba: u64,
        blocks: usize,
    ) -> block::BoxFuture<'a, block::Result<AllocVec<u8>>> {
        Box::pin(async move {
            let bs = self.block_size as usize;
            let bytes = blocks.checked_mul(bs).ok_or(block::Error::InvalidParam)?;
            let mut out = alloc::vec![0u8; bytes];
            self.read_blocks_into_green(lba, blocks, out.as_mut_slice()).await?;
            Ok(out)
        })
    }

    fn read_blocks_into<'a>(
        &'a mut self,
        lba: u64,
        blocks: usize,
        dst: &'a mut [u8],
    ) -> block::BoxFuture<'a, block::Result<()>> {
        Box::pin(async move { self.read_blocks_into_green(lba, blocks, dst).await })
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
    let _ = spawner;

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
            "crabusb: skhynix-green {:04X}:{:04X} proof=uas-target status=missing TODO=raw-pipe-usage-refinement no_block_register=true\n",
            vendor_id,
            product_id
        );
        return true;
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

    let mut interface =
        match claim_interface(&mut device, target.interface_number, target.alternate_setting).await
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

    let label = alloc::format!("skhynix-green-{:04X}:{:04X}", vendor_id, product_id);
    let desc = block::DeviceDescriptor::new(block::DeviceKind::Unknown)
        .with_label(label)
        .mark_read_only();
    let handle = block::register_device(
        desc,
        GreenSkhynixBlockDevice {
            _device: device,
            command_out,
            status_in,
            data_in,
            _data_out: data_out,
            block_size: probe.block_size.max(1),
            block_count: probe.block_count.max(1),
            next_tag: 0x4752_0001,
        },
    );

    crate::log!(
        "crabusb: skhynix-green {:04X}:{:04X} proof=green-disk status=registered disk={} label=skhynix-green read_only=1 max_xfer={} no_legacy_mass=true\n",
        vendor_id,
        product_id,
        handle.id(),
        GREEN_MAX_TRANSFER_BYTES
    );

    true
}
