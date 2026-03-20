use alloc::{boxed::Box, string::String, vec::Vec};
use core::{future::Future, task::Poll};
use crab_usb::{EndpointBulkIn, EndpointBulkOut, err::TransferError, usb_if};
use embassy_time::{Duration as EmbassyDuration, Timer};
use usb_if::host::ControlSetup;
use usb_if::transfer::{Recipient, Request, RequestType};

use crate::disc::block;

const USB_CLASS_MASS_STORAGE: u8 = 0x08;
const USB_SUBCLASS_SCSI: u8 = 0x06;
const USB_PROTO_BULK_ONLY: u8 = 0x50;
const BOT_IO_RETRIES: usize = 8;
const BOT_IO_TIMEOUT_MS: u64 = 120;

#[derive(Copy, Clone, Debug)]
pub(crate) struct MassTarget {
    pub configuration_value: u8,
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub bulk_in: u8,
    pub bulk_out: u8,
    pub bulk_in_max_packet_size: u16,
    pub bulk_out_max_packet_size: u16,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
}

pub(crate) fn pick_mass_target(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Option<MassTarget> {
    let mut best: Option<(u32, MassTarget)> = None;

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                let mut bulk_in = None;
                let mut bulk_out = None;

                for ep in alt.endpoints.iter() {
                    if ep.transfer_type != usb_if::descriptor::EndpointType::Bulk {
                        continue;
                    }
                    match ep.direction {
                        usb_if::transfer::Direction::In if bulk_in.is_none() => {
                            bulk_in = Some((ep.address, ep.max_packet_size));
                        }
                        usb_if::transfer::Direction::Out if bulk_out.is_none() => {
                            bulk_out = Some((ep.address, ep.max_packet_size));
                        }
                        _ => {}
                    }
                }

                let (bulk_in_addr, bulk_in_mps) = bulk_in?;
                let (bulk_out_addr, bulk_out_mps) = bulk_out?;

                let mut score = 10u32;
                if alt.class == USB_CLASS_MASS_STORAGE {
                    score += 100;
                }
                if alt.subclass == USB_SUBCLASS_SCSI {
                    score += 50;
                }
                if alt.protocol == USB_PROTO_BULK_ONLY {
                    score += 50;
                }
                if alt.alternate_setting == 0 {
                    score += 10;
                }
                score += alt.endpoints.len() as u32;

                let target = MassTarget {
                    configuration_value: config.configuration_value,
                    interface_number: interface.interface_number,
                    alternate_setting: alt.alternate_setting,
                    bulk_in: bulk_in_addr,
                    bulk_out: bulk_out_addr,
                    bulk_in_max_packet_size: bulk_in_mps,
                    bulk_out_max_packet_size: bulk_out_mps,
                    class: alt.class,
                    subclass: alt.subclass,
                    protocol: alt.protocol,
                };

                match best {
                    Some((best_score, _)) if best_score >= score => {}
                    _ => best = Some((score, target)),
                }
            }
        }
    }

    best.map(|(_, target)| target).filter(|target| {
        target.class == USB_CLASS_MASS_STORAGE
            && target.subclass == USB_SUBCLASS_SCSI
            && target.protocol == USB_PROTO_BULK_ONLY
    })
}

#[derive(Clone, Debug)]
pub(crate) struct MassProbeInfo {
    pub max_lun: u8,
    pub block_size: u32,
    pub block_count: u64,
    pub vendor: String,
    pub product: String,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum MassProbeError {
    Transport(&'static str),
    ShortData {
        cmd: &'static str,
        got: usize,
        need: usize,
    },
    Csw {
        cmd: &'static str,
        sig: u32,
        tag: u32,
        expected_tag: u32,
        status: u8,
    },
}

fn make_cbw(tag: u32, data_len: u32, flags: u8, lun: u8, cdb: &[u8]) -> [u8; 31] {
    let mut cbw = [0u8; 31];
    cbw[0..4].copy_from_slice(&0x4342_5355u32.to_le_bytes());
    cbw[4..8].copy_from_slice(&tag.to_le_bytes());
    cbw[8..12].copy_from_slice(&data_len.to_le_bytes());
    cbw[12] = flags;
    cbw[13] = lun;
    let cdb_len = cdb.len().min(16) as u8;
    cbw[14] = cdb_len;
    cbw[15..15 + usize::from(cdb_len)].copy_from_slice(&cdb[..usize::from(cdb_len)]);
    cbw
}

async fn with_timeout_or_none<F: Future>(fut: F, timeout_ms: u64) -> Option<F::Output> {
    let mut fut = core::pin::pin!(fut);
    let mut timeout = core::pin::pin!(Timer::after(EmbassyDuration::from_millis(timeout_ms)));

    core::future::poll_fn(|cx| {
        if let Poll::Ready(out) = fut.as_mut().poll(cx) {
            return Poll::Ready(Some(out));
        }
        if timeout.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

async fn read_and_validate_csw(
    bulk_in: &mut EndpointBulkIn,
    cmd: &'static str,
    expected_tag: u32,
) -> Result<(), MassProbeError> {
    let mut csw = [0u8; 13];
    let mut csw_got = 0usize;
    for _ in 0..BOT_IO_RETRIES {
        csw_got = with_timeout_or_none(bulk_in.submit_and_wait(&mut csw), BOT_IO_TIMEOUT_MS)
            .await
            .ok_or(MassProbeError::Transport("csw-timeout"))?
            .map_err(|_| MassProbeError::Transport("csw-in"))?;
        if csw_got != 0 {
            break;
        }
    }
    if csw_got != csw.len() {
        return Err(MassProbeError::ShortData {
            cmd,
            got: csw_got,
            need: csw.len(),
        });
    }

    let sig = u32::from_le_bytes([csw[0], csw[1], csw[2], csw[3]]);
    let csw_tag = u32::from_le_bytes([csw[4], csw[5], csw[6], csw[7]]);
    let status = csw[12];
    if sig != 0x5342_5355 || csw_tag != expected_tag || status != 0 {
        return Err(MassProbeError::Csw {
            cmd,
            sig,
            tag: csw_tag,
            expected_tag,
            status,
        });
    }

    Ok(())
}

async fn bot_command_in(
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    cmd: &'static str,
    lun: u8,
    cdb: &[u8],
    data: &mut [u8],
    tag: u32,
) -> Result<usize, MassProbeError> {
    let cbw = make_cbw(tag, data.len() as u32, 0x80, lun, cdb);
    let mut sent = 0usize;
    for _ in 0..BOT_IO_RETRIES {
        sent = with_timeout_or_none(bulk_out.submit_and_wait(&cbw), BOT_IO_TIMEOUT_MS)
            .await
            .ok_or(MassProbeError::Transport("cbw-timeout"))?
            .map_err(|_| MassProbeError::Transport("cbw-out"))?;
        if sent != 0 {
            break;
        }
    }
    if sent != cbw.len() {
        return Err(MassProbeError::ShortData {
            cmd,
            got: sent,
            need: cbw.len(),
        });
    }

    let mut got = 0usize;
    for _ in 0..BOT_IO_RETRIES {
        got = with_timeout_or_none(bulk_in.submit_and_wait(data), BOT_IO_TIMEOUT_MS)
            .await
            .ok_or(MassProbeError::Transport("data-timeout"))?
            .map_err(|_| MassProbeError::Transport("data-in"))?;
        if got != 0 {
            break;
        }
    }
    read_and_validate_csw(bulk_in, cmd, tag).await?;
    Ok(got)
}

fn decode_ascii_field(field: &[u8]) -> String {
    let mut out = String::new();
    for &b in field {
        if (0x20..=0x7E).contains(&b) {
            out.push(b as char);
        } else {
            out.push(' ');
        }
    }
    String::from(out.trim())
}

pub(crate) async fn probe_mass_bot(
    device: &mut crab_usb::Device,
    bulk_out: &mut EndpointBulkOut,
    bulk_in: &mut EndpointBulkIn,
    interface_number: u8,
) -> Result<MassProbeInfo, MassProbeError> {
    let mut max_lun_buf = [0u8; 1];
    let max_lun = match device
        .control_in(
            ControlSetup {
                request_type: RequestType::Class,
                recipient: Recipient::Interface,
                request: Request::Other(0xFE),
                value: 0,
                index: interface_number as u16,
            },
            &mut max_lun_buf,
        )
        .await
    {
        Ok(read) if read >= 1 => max_lun_buf[0],
        Ok(_) => 0,
        Err(TransferError::Stall) => 0,
        Err(_) => return Err(MassProbeError::Transport("get-max-lun")),
    };

    let lun = 0u8;
    let mut inquiry = [0u8; 36];
    let inquiry_cdb = [0x12, 0, 0, 0, inquiry.len() as u8, 0];
    let inquiry_read = bot_command_in(
        bulk_out,
        bulk_in,
        "inquiry",
        lun,
        &inquiry_cdb,
        &mut inquiry,
        0x544F_4E51,
    )
    .await?;
    if inquiry_read < 32 {
        return Err(MassProbeError::ShortData {
            cmd: "inquiry",
            got: inquiry_read,
            need: 32,
        });
    }

    let mut read_capacity = [0u8; 8];
    let read_capacity_cdb = [0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let read_capacity_read = bot_command_in(
        bulk_out,
        bulk_in,
        "read-capacity10",
        lun,
        &read_capacity_cdb,
        &mut read_capacity,
        0x544F_4E52,
    )
    .await?;
    if read_capacity_read < read_capacity.len() {
        return Err(MassProbeError::ShortData {
            cmd: "read-capacity10",
            got: read_capacity_read,
            need: read_capacity.len(),
        });
    }

    let last_lba = u32::from_be_bytes([
        read_capacity[0],
        read_capacity[1],
        read_capacity[2],
        read_capacity[3],
    ]);
    let block_size = u32::from_be_bytes([
        read_capacity[4],
        read_capacity[5],
        read_capacity[6],
        read_capacity[7],
    ]);
    if block_size == 0 {
        return Err(MassProbeError::ShortData {
            cmd: "read-capacity10",
            got: 0,
            need: 1,
        });
    }

    let block_count = u64::from(last_lba) + 1;
    let vendor = decode_ascii_field(&inquiry[8..16]);
    let product = decode_ascii_field(&inquiry[16..32]);

    Ok(MassProbeInfo {
        max_lun,
        block_size,
        block_count,
        vendor,
        product,
    })
}

struct UsbMassGeometryPlaceholderDevice {
    block_size: u32,
    block_count: u64,
}

impl block::BlockDevice for UsbMassGeometryPlaceholderDevice {
    fn block_size_bytes(&self) -> u32 {
        self.block_size
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn read_blocks<'a>(
        &'a mut self,
        _lba: u64,
        _blocks: usize,
    ) -> block::BoxFuture<'a, block::Result<Vec<u8>>> {
        Box::pin(async { Err(block::Error::NotSupported) })
    }
}

pub(crate) fn register_mass_geometry_placeholder(
    vendor_id: u16,
    product_id: u16,
    block_size: u32,
    block_count: u64,
) -> block::DeviceHandle {
    let label = alloc::format!("usbms-{:04X}:{:04X}", vendor_id, product_id);
    let desc = block::DeviceDescriptor::new(block::DeviceKind::Unknown).with_label(label);
    block::register_device(
        desc,
        UsbMassGeometryPlaceholderDevice {
            block_size: block_size.max(1),
            block_count: block_count.max(1),
        },
    )
}
