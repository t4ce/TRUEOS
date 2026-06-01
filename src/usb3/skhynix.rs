use alloc::{boxed::Box, string::String, vec::Vec};
use core::{future::Future, task::Poll};

const USB_CLASS_MASS_STORAGE: u8 = 0x08;
const USB_SUBCLASS_SCSI: u8 = 0x06;
const USB_PROTO_UAS: u8 = 0x62;
const UAS_IO_TIMEOUT_MS: u64 = crate::allcaps::storage::USB_MASS_UAS_IO_TIMEOUT_MS;
const UAS_IU_COMMAND: u8 = 0x01;
const UAS_IU_STATUS: u8 = 0x03;
const UAS_IU_READ_READY: u8 = 0x06;
const UAS_IU_WRITE_READY: u8 = 0x07;
const UAS_STATUS_GOOD: u8 = 0x00;
const UAS_STREAM_ID_FIRST: u16 = 1;
const UAS_STREAM_ID_LAST: u16 = 1;
const UAS_XHCI_STREAMS_ENABLED: bool = true;
const UAS_XHCI_OUT_STREAMS_ENABLED: bool = false;
const SKHYNIX_UAS_MAX_TRANSFER_BYTES: usize = 8 * 1024 * 1024;
const SKHYNIX_UAS_LOG_TRANSFER_BYTES: usize = 512 * 1024;

struct SkhynixUasRuntime {
    device: super::crabusb::Device,
    command_out: super::crabusb::Endpoint,
    status_in: super::crabusb::Endpoint,
    data_in: super::crabusb::Endpoint,
    data_out: super::crabusb::Endpoint,
    target: UasTarget,
}

struct SkhynixUasBlockDevice {
    runtime: SkhynixUasRuntime,
    info: MassProbeInfo,
    next_tag: u16,
    poisoned: bool,
}

#[derive(Clone, Debug)]
struct MassBulkEndpoint {
    address: u8,
    max_packet_size: u16,
}

#[derive(Clone, Debug)]
struct UasCandidate {
    configuration_value: u8,
    interface_number: u8,
    alternate_setting: u8,
    bulk_in: Vec<MassBulkEndpoint>,
    bulk_out: Vec<MassBulkEndpoint>,
}

#[derive(Clone, Copy, Debug)]
struct UasTarget {
    configuration_value: u8,
    interface_number: u8,
    alternate_setting: u8,
    command_out: u8,
    status_in: u8,
    data_in: u8,
    data_out: u8,
    command_out_max_packet_size: u16,
    status_in_max_packet_size: u16,
    data_in_max_packet_size: u16,
    data_out_max_packet_size: u16,
}

pub(super) async fn start_green_uas(mut pooled: super::dev_gears::PooledUsbDevice) {
    let candidates = collect_uas_candidates(pooled.device.configurations());
    crate::log!(
        "crabusb: skhynix-green {:04x}:{:04x} proof=transport-plan uas_candidates={}\n",
        pooled.vendor_id,
        pooled.product_id,
        candidates.len()
    );

    let Some(target) = pick_skhynix_uas_target(pooled.vendor_id, pooled.product_id, &candidates)
    else {
        crate::log!(
            "crabusb: skhynix-green {:04x}:{:04x} proof=uas-target status=missing\n",
            pooled.vendor_id,
            pooled.product_id
        );
        return;
    };

    crate::log!(
        "crabusb: skhynix-green {:04x}:{:04x} proof=uas-target if#{} alt={} cfg={} cmd_out=0x{:02x}/{} status_in=0x{:02x}/{} data_in=0x{:02x}/{} data_out=0x{:02x}/{}\n",
        pooled.vendor_id,
        pooled.product_id,
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

    if let Err(err) = pooled
        .device
        .set_configuration(target.configuration_value)
        .await
    {
        crate::log!(
            "crabusb: skhynix-green {:04x}:{:04x} proof=set-config cfg={} status=failed err={:?}\n",
            pooled.vendor_id,
            pooled.product_id,
            target.configuration_value,
            err
        );
        return;
    }
    crate::log!(
        "crabusb: skhynix-green {:04x}:{:04x} proof=set-config cfg={} status=ok\n",
        pooled.vendor_id,
        pooled.product_id,
        target.configuration_value
    );

    if let Err(err) = pooled
        .device
        .claim_interface(target.interface_number, target.alternate_setting)
        .await
    {
        crate::log!(
            "crabusb: skhynix-green {:04x}:{:04x} proof=claim if#{} alt={} status=failed err={:?}\n",
            pooled.vendor_id,
            pooled.product_id,
            target.interface_number,
            target.alternate_setting,
            err
        );
        return;
    }
    crate::log!(
        "crabusb: skhynix-green {:04x}:{:04x} proof=claim if#{} alt={} status=ok\n",
        pooled.vendor_id,
        pooled.product_id,
        target.interface_number,
        target.alternate_setting
    );

    let command_out = match pooled.device.endpoint(target.command_out) {
        Ok(endpoint) => endpoint,
        Err(err) => {
            crate::log!(
                "crabusb: skhynix-green {:04x}:{:04x} proof=endpoints cmd_out=false err={:?}\n",
                pooled.vendor_id,
                pooled.product_id,
                err
            );
            return;
        }
    };
    let status_in = match pooled.device.endpoint(target.status_in) {
        Ok(endpoint) => endpoint,
        Err(err) => {
            crate::log!(
                "crabusb: skhynix-green {:04x}:{:04x} proof=endpoints status_in=false err={:?}\n",
                pooled.vendor_id,
                pooled.product_id,
                err
            );
            return;
        }
    };
    let data_in = match pooled.device.endpoint(target.data_in) {
        Ok(endpoint) => endpoint,
        Err(err) => {
            crate::log!(
                "crabusb: skhynix-green {:04x}:{:04x} proof=endpoints data_in=false err={:?}\n",
                pooled.vendor_id,
                pooled.product_id,
                err
            );
            return;
        }
    };
    let data_out = match pooled.device.endpoint(target.data_out) {
        Ok(endpoint) => endpoint,
        Err(err) => {
            crate::log!(
                "crabusb: skhynix-green {:04x}:{:04x} proof=endpoints data_out=false err={:?}\n",
                pooled.vendor_id,
                pooled.product_id,
                err
            );
            return;
        }
    };

    let mut command_out = command_out;
    let mut status_in = status_in;
    let mut data_in = data_in;
    let data_out = data_out;

    let probe_info = match probe_mass_uas_skhynix(&mut command_out, &mut status_in, &mut data_in)
        .await
    {
        Ok(info) => {
            crate::log!(
                "crabusb: skhynix-green {:04x}:{:04x} proof=scsi-probe status=ok bs={} blocks={} vendor='{}' product='{}'\n",
                pooled.vendor_id,
                pooled.product_id,
                info.block_size,
                info.block_count,
                info.vendor.as_str(),
                info.product.as_str()
            );
            info
        }
        Err(err) => {
            crate::log!(
                "crabusb: skhynix-green {:04x}:{:04x} proof=scsi-probe status=failed err={:?}\n",
                pooled.vendor_id,
                pooled.product_id,
                err
            );
            return;
        }
    };

    let runtime = SkhynixUasRuntime {
        device: pooled.device,
        command_out,
        status_in,
        data_in,
        data_out,
        target,
    };

    let label =
        alloc::format!("USB UAS {} {}", probe_info.vendor.as_str(), probe_info.product.as_str());
    let descriptor =
        crate::disc::block::DeviceDescriptor::new(crate::disc::block::DeviceKind::Unknown)
            .with_label(label)
            .with_serial(alloc::format!(
                "usb-uas-{:04x}:{:04x}-slot{}",
                pooled.vendor_id,
                pooled.product_id,
                pooled.id
            ));
    let block_device = SkhynixUasBlockDevice {
        runtime,
        info: probe_info,
        next_tag: UAS_STREAM_ID_FIRST,
        poisoned: false,
    };
    let handle = crate::disc::block::register_device_with_worker(descriptor, block_device);
    crate::log!(
        "crabusb: skhynix-green {:04x}:{:04x} proof=block-register status=ok disc={} read_only=false max_xfer={}\n",
        pooled.vendor_id,
        pooled.product_id,
        handle.id(),
        SKHYNIX_UAS_MAX_TRANSFER_BYTES
    );

    crate::log!(
        "crabusb: skhynix-green {:04x}:{:04x} proof=endpoints cmd_out=true status_in=true data_in=true data_out=true owner=block\n",
        pooled.vendor_id,
        pooled.product_id
    );
}

fn collect_uas_candidates(
    configs: &[super::crabusb::usb_if::descriptor::ConfigurationDescriptor],
) -> Vec<UasCandidate> {
    let mut out = Vec::new();

    for config in configs {
        for interface in &config.interfaces {
            for alt in &interface.alt_settings {
                if alt.class != USB_CLASS_MASS_STORAGE
                    || alt.subclass != USB_SUBCLASS_SCSI
                    || alt.protocol != USB_PROTO_UAS
                {
                    continue;
                }

                let mut bulk_in = Vec::new();
                let mut bulk_out = Vec::new();
                for ep in &alt.endpoints {
                    if ep.transfer_type != super::crabusb::usb_if::descriptor::EndpointType::Bulk {
                        continue;
                    }

                    let item = MassBulkEndpoint {
                        address: ep.address,
                        max_packet_size: ep.max_packet_size,
                    };
                    match ep.direction {
                        super::crabusb::usb_if::transfer::Direction::In => bulk_in.push(item),
                        super::crabusb::usb_if::transfer::Direction::Out => bulk_out.push(item),
                    }
                }

                if !bulk_in.is_empty() && !bulk_out.is_empty() {
                    out.push(UasCandidate {
                        configuration_value: config.configuration_value,
                        interface_number: interface.interface_number,
                        alternate_setting: alt.alternate_setting,
                        bulk_in,
                        bulk_out,
                    });
                }
            }
        }
    }

    out
}

fn pick_skhynix_uas_target(
    vendor_id: u16,
    product_id: u16,
    candidates: &[UasCandidate],
) -> Option<UasTarget> {
    if vendor_id != 0x152e || product_id != 0x7001 {
        return None;
    }

    for uas in candidates {
        let command_out = uas.bulk_out.iter().find(|ep| ep.address == 0x04)?;
        let status_in = uas.bulk_in.iter().find(|ep| ep.address == 0x83)?;
        let data_out = uas.bulk_out.iter().find(|ep| ep.address == 0x02)?;
        let data_in = uas.bulk_in.iter().find(|ep| ep.address == 0x81)?;
        return Some(UasTarget {
            configuration_value: uas.configuration_value,
            interface_number: uas.interface_number,
            alternate_setting: uas.alternate_setting,
            command_out: command_out.address,
            status_in: status_in.address,
            data_in: data_in.address,
            data_out: data_out.address,
            command_out_max_packet_size: command_out.max_packet_size,
            status_in_max_packet_size: status_in.max_packet_size,
            data_in_max_packet_size: data_in.max_packet_size,
            data_out_max_packet_size: data_out.max_packet_size,
        });
    }

    None
}

#[derive(Clone, Debug)]
struct MassProbeInfo {
    block_size: u32,
    block_count: u64,
    vendor: String,
    product: String,
}

#[derive(Clone, Copy, Debug)]
enum MassProbeError {
    Transport(&'static str),
    ShortData,
    Csw,
}

async fn with_timeout_or_none<F: Future>(fut: F, timeout_ms: u64) -> Option<F::Output> {
    embassy_time::with_timeout(embassy_time::Duration::from_millis(timeout_ms), fut)
        .await
        .ok()
}

async fn endpoint_wait_submitted(
    endpoint: &mut super::crabusb::Endpoint,
    id: super::crabusb::usb_if::endpoint::RequestId,
    timeout_label: &'static str,
    transfer_label: &'static str,
) -> Result<usize, MassProbeError> {
    let mut timeout = core::pin::pin!(embassy_time::Timer::after(
        embassy_time::Duration::from_millis(UAS_IO_TIMEOUT_MS),
    ));

    core::future::poll_fn(|cx| {
        if let Poll::Ready(result) = endpoint.poll_request(id, cx) {
            return Poll::Ready(
                result
                    .map(|completion| completion.actual_length)
                    .map_err(|_| MassProbeError::Transport(transfer_label)),
            );
        }
        if timeout.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(MassProbeError::Transport(timeout_label)));
        }
        Poll::Pending
    })
    .await
}

async fn uas_send_command(
    command_out: &mut super::crabusb::Endpoint,
    cmd: &'static str,
    cdb: &[u8],
    tag: u16,
) -> Result<(), MassProbeError> {
    let iu = make_uas_command_iu(tag, cdb);
    let id = command_out
        .submit(super::crabusb::usb_if::endpoint::TransferRequest::bulk_out(&iu))
        .map_err(|_| MassProbeError::Transport("uas-command-submit"))?;
    if crate::logflag::USB_MASS_UAS_TRACE_LOGS || cmd == "write10" {
        crate::log!(
            "crabusb: skhynix-green proof=uas-command-submit cmd={} tag=0x{:04x} request={}\n",
            cmd,
            tag,
            id.raw()
        );
    }
    let sent =
        match endpoint_wait_submitted(command_out, id, "uas-command-timeout", "uas-command-out")
            .await
        {
            Ok(sent) => sent,
            Err(err) => {
                let _ = command_out.cancel(id);
                if cmd == "read10" || cmd == "write10" {
                    crate::log!(
                        "crabusb: skhynix-green proof=uas-command cmd={} tag=0x{:04x} request={} status=failed err={:?}\n",
                        cmd,
                        tag,
                        id.raw(),
                        err
                    );
                }
                return Err(err);
            }
        };
    if crate::logflag::USB_MASS_UAS_TRACE_LOGS || cmd == "write10" {
        crate::log!(
            "crabusb: skhynix-green proof=uas-command cmd={} tag=0x{:04x} sent={}\n",
            cmd,
            tag,
            sent
        );
    }
    if sent != iu.len() {
        return Err(MassProbeError::ShortData);
    }
    Ok(())
}

async fn uas_drain_status_grace(
    status_in: &mut super::crabusb::Endpoint,
    cmd: &'static str,
    tag: u16,
    timeout_label: &'static str,
) -> Result<(), MassProbeError> {
    let mut status = [0u8; 96];
    let got = with_timeout_or_none(
        status_in.wait(uas_bulk_in_request(&mut status, tag)),
        UAS_IO_TIMEOUT_MS,
    )
    .await
    .ok_or(MassProbeError::Transport(timeout_label))?
    .map_err(|_| MassProbeError::Transport("uas-status-in"))?
    .actual_length;
    if got < 4 {
        return Err(MassProbeError::ShortData);
    }
    validate_uas_status(cmd, &status[..got.min(status.len())], tag)
}

fn uas_bulk_in_request(buffer: &mut [u8], tag: u16) -> super::crabusb::usb_if::endpoint::TransferRequest {
    if UAS_XHCI_STREAMS_ENABLED {
        super::crabusb::usb_if::endpoint::TransferRequest::bulk_in_on_stream(buffer, tag)
    } else {
        super::crabusb::usb_if::endpoint::TransferRequest::bulk_in(buffer)
    }
}

fn uas_bulk_out_request(buffer: &[u8], tag: u16) -> super::crabusb::usb_if::endpoint::TransferRequest {
    if UAS_XHCI_OUT_STREAMS_ENABLED {
        super::crabusb::usb_if::endpoint::TransferRequest::bulk_out_on_stream(buffer, tag)
    } else {
        super::crabusb::usb_if::endpoint::TransferRequest::bulk_out(buffer)
    }
}

async fn uas_command_in(
    command_out: &mut super::crabusb::Endpoint,
    status_in: &mut super::crabusb::Endpoint,
    data_in: &mut super::crabusb::Endpoint,
    cmd: &'static str,
    cdb: &[u8],
    data: &mut [u8],
    tag: u16,
) -> Result<usize, MassProbeError> {
    let mut ready_iu = [0u8; 16];
    enum FirstInCompletion {
        Status(Result<usize, MassProbeError>),
        Data(Result<usize, MassProbeError>),
        Timeout,
    }
    enum InOutcome {
        Done(usize),
        DrainStatus(usize),
    }

    let outcome = {
        let ready_id = status_in
            .submit(uas_bulk_in_request(&mut ready_iu, tag))
            .map_err(|_| MassProbeError::Transport("uas-status-submit"))?;
        let data_id = data_in
            .submit(uas_bulk_in_request(data, tag))
            .map_err(|_| MassProbeError::Transport("uas-data-submit"))?;
        if crate::logflag::USB_MASS_UAS_TRACE_LOGS
            || (cmd == "read10" && data.len() >= SKHYNIX_UAS_LOG_TRANSFER_BYTES)
        {
            crate::log!(
                "crabusb: skhynix-green proof=uas-read phase=prepost cmd={} tag=0x{:04x} status_req={} data_req={} bytes={}\n",
                cmd,
                tag,
                ready_id.raw(),
                data_id.raw(),
                data.len()
            );
        }

        if let Err(err) = uas_send_command(command_out, cmd, cdb, tag).await {
            let _ = status_in.cancel(ready_id);
            let _ = data_in.cancel(data_id);
            return Err(err);
        }

        let mut timeout = core::pin::pin!(embassy_time::Timer::after(
            embassy_time::Duration::from_millis(UAS_IO_TIMEOUT_MS,)
        ));
        let first = core::future::poll_fn(|cx| {
            if let Poll::Ready(result) = data_in.poll_request(data_id, cx) {
                return Poll::Ready(FirstInCompletion::Data(
                    result
                        .map(|completion| completion.actual_length)
                        .map_err(|_| MassProbeError::Transport("uas-data-in")),
                ));
            }
            if let Poll::Ready(result) = status_in.poll_request(ready_id, cx) {
                return Poll::Ready(FirstInCompletion::Status(
                    result
                        .map(|completion| completion.actual_length)
                        .map_err(|_| MassProbeError::Transport("uas-status-in")),
                ));
            }
            if timeout.as_mut().poll(cx).is_ready() {
                return Poll::Ready(FirstInCompletion::Timeout);
            }
            Poll::Pending
        })
        .await;

        match first {
            FirstInCompletion::Data(got) => {
                let got = got?;
                let ready_got = endpoint_wait_submitted(
                    status_in,
                    ready_id,
                    "uas-status-timeout",
                    "uas-status-in",
                )
                .await?;
                if ready_got < 4 {
                    return Err(MassProbeError::ShortData);
                }
                let ready = &ready_iu[..ready_got.min(ready_iu.len())];
                let ready_id = ready[0];
                let ready_tag = parse_uas_tag(ready).unwrap_or(0);
                if ready_id == UAS_IU_STATUS && ready_tag == tag {
                    validate_uas_status(cmd, ready, tag)?;
                    Ok(InOutcome::Done(got))
                } else if ready_id == UAS_IU_READ_READY && ready_tag == tag {
                    Ok(InOutcome::DrainStatus(got))
                } else {
                    Err(MassProbeError::Csw)
                }
            }
            FirstInCompletion::Status(ready_got) => {
                let ready_got = ready_got?;
                if ready_got < 4 {
                    return Err(MassProbeError::ShortData);
                }
                let ready = &ready_iu[..ready_got.min(ready_iu.len())];
                let ready_id = ready[0];
                let ready_tag = parse_uas_tag(ready).unwrap_or(0);
                if ready_id == UAS_IU_STATUS && ready_tag == tag {
                    validate_uas_status(cmd, ready, tag)?;
                    let got = endpoint_wait_submitted(
                        data_in,
                        data_id,
                        "uas-data-timeout",
                        "uas-data-in",
                    )
                    .await?;
                    Ok(InOutcome::Done(got))
                } else if ready_id == UAS_IU_READ_READY && ready_tag == tag {
                    let got = endpoint_wait_submitted(
                        data_in,
                        data_id,
                        "uas-data-timeout",
                        "uas-data-in",
                    )
                    .await?;
                    Ok(InOutcome::DrainStatus(got))
                } else {
                    Err(MassProbeError::Csw)
                }
            }
            FirstInCompletion::Timeout => {
                crate::log!(
                    "crabusb: skhynix-green proof=uas-read phase=timeout cmd={} tag=0x{:04x} status_req={} data_req={}\n",
                    cmd,
                    tag,
                    ready_id.raw(),
                    data_id.raw()
                );
                let _ = status_in.cancel(ready_id);
                let _ = data_in.cancel(data_id);
                Err(MassProbeError::Transport("uas-in-timeout"))
            }
        }
    }?;

    match outcome {
        InOutcome::Done(got) => Ok(got),
        InOutcome::DrainStatus(got) => {
            uas_drain_status_grace(status_in, cmd, tag, "uas-read-status-timeout").await?;
            Ok(got)
        }
    }
}

async fn uas_command_out(
    command_out: &mut super::crabusb::Endpoint,
    status_in: &mut super::crabusb::Endpoint,
    data_out: &mut super::crabusb::Endpoint,
    cmd: &'static str,
    cdb: &[u8],
    data: &[u8],
    tag: u16,
) -> Result<usize, MassProbeError> {
    let mut status_iu = [0u8; 96];
    let status_id = status_in
        .submit(uas_bulk_in_request(&mut status_iu, tag))
        .map_err(|_| MassProbeError::Transport("uas-status-submit"))?;
    if cmd == "write10" {
        crate::log!(
            "crabusb: skhynix-green proof=uas-write phase=pre-submit-status cmd={} tag=0x{:04x} status_req={} bytes={}\n",
            cmd,
            tag,
            status_id.raw(),
            data.len()
        );
    }

    if let Err(err) = uas_send_command(command_out, cmd, cdb, tag).await {
        let _ = status_in.cancel(status_id);
        return Err(err);
    }
    if cmd == "write10" {
        crate::log!(
            "crabusb: skhynix-green proof=uas-write phase=command-sent cmd={} tag=0x{:04x}\n",
            cmd,
            tag
        );
    }

    let data_id = data_out
        .submit(uas_bulk_out_request(data, tag))
        .map_err(|_| MassProbeError::Transport("uas-data-submit"))?;
    if cmd == "write10" {
        crate::log!(
            "crabusb: skhynix-green proof=uas-write phase=post-command-data-submit cmd={} tag=0x{:04x} data_req={} bytes={}\n",
            cmd,
            tag,
            data_id.raw(),
            data.len()
        );
    }

    let ready_got = match endpoint_wait_submitted(
        status_in,
        status_id,
        "uas-write-ready-timeout",
        "uas-status-in",
    )
    .await
    {
        Ok(got) => got,
        Err(err) => {
            let _ = data_out.cancel(data_id);
            return Err(err);
        }
    };
    if ready_got < 4 {
        let _ = data_out.cancel(data_id);
        return Err(MassProbeError::ShortData);
    }
    let ready = &status_iu[..ready_got.min(status_iu.len())];
    let ready_id = ready[0];
    let ready_tag = parse_uas_tag(ready).unwrap_or(0);
    if cmd == "write10" {
        crate::log!(
            "crabusb: skhynix-green proof=uas-write phase=status-ready cmd={} tag=0x{:04x} iu=0x{:02x} iu_tag=0x{:04x} raw_len={}\n",
            cmd,
            tag,
            ready_id,
            ready_tag,
            ready_got
    );
    }
    if ready_id == UAS_IU_STATUS && ready_tag == tag {
        let _ = data_out.cancel(data_id);
        validate_uas_status(cmd, ready, tag)?;
        return Err(MassProbeError::Csw);
    }
    if ready_id != UAS_IU_WRITE_READY || ready_tag != tag {
        let _ = data_out.cancel(data_id);
        return Err(MassProbeError::Csw);
    }

    let sent = endpoint_wait_submitted(data_out, data_id, "uas-data-timeout", "uas-data-out")
        .await?;

    uas_drain_status_grace(status_in, cmd, tag, "uas-write-status-timeout").await?;

    if cmd == "write10" {
        crate::log!(
            "crabusb: skhynix-green proof=uas-write phase=data-sent cmd={} tag=0x{:04x} bytes={}\n",
            cmd,
            tag,
            sent
        );
    }
    if sent != data.len() {
        return Err(MassProbeError::ShortData);
    }

    if cmd == "write10" {
        crate::log!(
            "crabusb: skhynix-green proof=uas-write phase=status-ok cmd={} tag=0x{:04x}\n",
            cmd,
            tag
        );
    }
    Ok(sent)
}

async fn probe_mass_uas_skhynix(
    command_out: &mut super::crabusb::Endpoint,
    status_in: &mut super::crabusb::Endpoint,
    data_in: &mut super::crabusb::Endpoint,
) -> Result<MassProbeInfo, MassProbeError> {
    let mut inquiry = [0u8; 36];
    let inquiry_cdb = cdb_inquiry(inquiry.len() as u16);
    let inquiry_read =
        uas_command_in(command_out, status_in, data_in, "inquiry", &inquiry_cdb, &mut inquiry, 1)
            .await?;
    if inquiry_read < 32 {
        return Err(MassProbeError::ShortData);
    }
    crate::log!(
        "crabusb: mass uas-skhynix inquiry removable={} pdt=0x{:02x}\n",
        (inquiry[1] & 0x80) != 0,
        inquiry[0] & 0x1f
    );

    let mut read_capacity = [0u8; 8];
    let capacity_cdb = cdb_read_capacity_10();
    let capacity_read = uas_command_in(
        command_out,
        status_in,
        data_in,
        "read-capacity10",
        &capacity_cdb,
        &mut read_capacity,
        2,
    )
    .await?;
    if capacity_read < read_capacity.len() {
        return Err(MassProbeError::ShortData);
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
        return Err(MassProbeError::ShortData);
    }

    Ok(MassProbeInfo {
        block_size,
        block_count: u64::from(last_lba) + 1,
        vendor: decode_ascii_field(&inquiry[8..16]),
        product: decode_ascii_field(&inquiry[16..32]),
    })
}

impl SkhynixUasBlockDevice {
    fn ensure_not_poisoned(&self) -> crate::disc::block::Result<()> {
        if self.poisoned {
            return Err(crate::disc::block::Error::NotReady);
        }
        Ok(())
    }

    fn poison_on_timeout(&mut self, op: &'static str, err: MassProbeError) {
        if !mass_probe_error_is_timeout(err) {
            return;
        }
        self.poisoned = true;
        crate::log!(
            "crabusb: skhynix-green proof=runtime-poisoned op={} err={:?} action=block-device-not-ready\n",
            op,
            err
        );
    }

    fn alloc_tag(&mut self) -> u16 {
        if !(UAS_STREAM_ID_FIRST..=UAS_STREAM_ID_LAST).contains(&self.next_tag) {
            self.next_tag = UAS_STREAM_ID_FIRST;
        }
        let tag = self.next_tag;
        self.next_tag = if self.next_tag >= UAS_STREAM_ID_LAST {
            UAS_STREAM_ID_FIRST
        } else {
            self.next_tag + 1
        };
        tag
    }

    fn validate_span(&self, lba: u64, blocks: usize) -> crate::disc::block::Result<usize> {
        if blocks == 0 {
            return Ok(0);
        }
        let block_size = usize::try_from(self.info.block_size)
            .map_err(|_| crate::disc::block::Error::InvalidParam)?;
        let end = lba
            .checked_add(blocks as u64)
            .ok_or(crate::disc::block::Error::OutOfBounds)?;
        if end > self.info.block_count {
            return Err(crate::disc::block::Error::OutOfBounds);
        }
        blocks
            .checked_mul(block_size)
            .ok_or(crate::disc::block::Error::InvalidParam)
    }

    async fn read_blocks_inner(
        &mut self,
        lba: u64,
        blocks: usize,
        dst: &mut [u8],
    ) -> crate::disc::block::Result<()> {
        let bytes = self.validate_span(lba, blocks)?;
        if bytes != dst.len() {
            return Err(crate::disc::block::Error::InvalidParam);
        }
        if blocks == 0 {
            return Ok(());
        }
        if lba > u32::MAX as u64 || blocks > u16::MAX as usize {
            return Err(crate::disc::block::Error::InvalidParam);
        }
        self.ensure_not_poisoned()?;

        let trace_transfer =
            crate::logflag::USB_MASS_UAS_TRACE_LOGS && bytes >= SKHYNIX_UAS_LOG_TRANSFER_BYTES;
        let cdb = cdb_read_10(lba as u32, blocks as u16);
        let tag = self.alloc_tag();
        if trace_transfer {
            crate::log_trace!(target: "usb";
                "crabusb: skhynix-green proof=block-read cmd=read10 lba={} blocks={} bytes={} tag=0x{:04x} status=start\n",
                lba,
                blocks,
                bytes,
                tag
            );
        }
        let got = match uas_command_in(
            &mut self.runtime.command_out,
            &mut self.runtime.status_in,
            &mut self.runtime.data_in,
            "read10",
            &cdb,
            dst,
            tag,
        )
        .await
        {
            Ok(got) => got,
            Err(err) => {
                crate::log!(
                    "crabusb: skhynix-green proof=block-read cmd=read10 lba={} blocks={} bytes={} tag=0x{:04x} status=failed err={:?}\n",
                    lba,
                    blocks,
                    bytes,
                    tag,
                    err
                );
                self.poison_on_timeout("read10", err);
                return Err(mass_probe_to_block_error(err));
            }
        };
        if got != bytes {
            crate::log!(
                "crabusb: skhynix-green proof=block-read cmd=read10 lba={} blocks={} bytes={} tag=0x{:04x} status=short got={} expected={}\n",
                lba,
                blocks,
                bytes,
                tag,
                got,
                bytes
            );
            return Err(crate::disc::block::Error::Corrupted);
        }
        if trace_transfer {
            crate::log_trace!(target: "usb";
                "crabusb: skhynix-green proof=block-read cmd=read10 lba={} blocks={} bytes={} tag=0x{:04x} status=ok\n",
                lba,
                blocks,
                bytes,
                tag
            );
        }
        Ok(())
    }

    async fn write_blocks_inner(&mut self, lba: u64, buf: &[u8]) -> crate::disc::block::Result<()> {
        let block_size = usize::try_from(self.info.block_size)
            .map_err(|_| crate::disc::block::Error::InvalidParam)?;
        if block_size == 0 || !buf.len().is_multiple_of(block_size) {
            return Err(crate::disc::block::Error::InvalidParam);
        }
        let blocks = buf.len() / block_size;
        let bytes = self.validate_span(lba, blocks)?;
        if bytes != buf.len() {
            return Err(crate::disc::block::Error::InvalidParam);
        }
        if blocks == 0 {
            return Ok(());
        }
        if lba > u32::MAX as u64 || blocks > u16::MAX as usize {
            return Err(crate::disc::block::Error::InvalidParam);
        }
        self.ensure_not_poisoned()?;

        let cdb = cdb_write_10(lba as u32, blocks as u16);
        let tag = self.alloc_tag();
        crate::log!(
            "crabusb: skhynix-green proof=block-write cmd=write10 lba={} blocks={} bytes={} tag=0x{:04x} status=start\n",
            lba,
            blocks,
            bytes,
            tag
        );
        let sent = match uas_command_out(
            &mut self.runtime.command_out,
            &mut self.runtime.status_in,
            &mut self.runtime.data_out,
            "write10",
            &cdb,
            buf,
            tag,
        )
        .await
        {
            Ok(sent) => sent,
            Err(err) => {
                crate::log!(
                    "crabusb: skhynix-green proof=block-write cmd=write10 lba={} blocks={} tag=0x{:04x} status=failed err={:?}\n",
                    lba,
                    blocks,
                    tag,
                    err
                );
                self.poison_on_timeout("write10", err);
                return Err(mass_probe_to_block_error(err));
            }
        };
        if sent != bytes {
            crate::log!(
                "crabusb: skhynix-green proof=block-write cmd=write10 lba={} blocks={} tag=0x{:04x} status=short sent={} expected={}\n",
                lba,
                blocks,
                tag,
                sent,
                bytes
            );
            return Err(crate::disc::block::Error::Corrupted);
        }
        Ok(())
    }
}

impl crate::disc::block::BlockDevice for SkhynixUasBlockDevice {
    fn block_size_bytes(&self) -> u32 {
        self.info.block_size
    }

    fn block_count(&self) -> u64 {
        self.info.block_count
    }

    fn read_blocks<'a>(
        &'a mut self,
        lba: u64,
        blocks: usize,
    ) -> crate::disc::block::BoxFuture<'a, crate::disc::block::Result<Vec<u8>>> {
        Box::pin(async move {
            let bytes = self.validate_span(lba, blocks)?;
            let mut out = vec![0u8; bytes];
            self.read_blocks_inner(lba, blocks, &mut out).await?;
            Ok(out)
        })
    }

    fn read_blocks_into<'a>(
        &'a mut self,
        lba: u64,
        blocks: usize,
        dst: &'a mut [u8],
    ) -> crate::disc::block::BoxFuture<'a, crate::disc::block::Result<()>> {
        Box::pin(async move { self.read_blocks_inner(lba, blocks, dst).await })
    }

    fn write_blocks<'a>(
        &'a mut self,
        lba: u64,
        buf: &'a [u8],
    ) -> crate::disc::block::BoxFuture<'a, crate::disc::block::Result<()>> {
        Box::pin(async move { self.write_blocks_inner(lba, buf).await })
    }

    fn dma_alignment_bytes(&self) -> u32 {
        64
    }

    fn max_transfer_bytes(&self) -> u64 {
        SKHYNIX_UAS_MAX_TRANSFER_BYTES as u64
    }

    fn supports_write(&self) -> bool {
        true
    }
}

fn mass_probe_error_is_timeout(err: MassProbeError) -> bool {
    matches!(
        err,
        MassProbeError::Transport("uas-command-timeout")
            | MassProbeError::Transport("uas-in-timeout")
            | MassProbeError::Transport("uas-read-ready-timeout")
            | MassProbeError::Transport("uas-status-timeout")
            | MassProbeError::Transport("uas-read-status-timeout")
            | MassProbeError::Transport("uas-write-ready-timeout")
            | MassProbeError::Transport("uas-write-status-timeout")
            | MassProbeError::Transport("uas-data-timeout")
            | MassProbeError::Transport("uas-out-timeout")
    )
}

fn mass_probe_to_block_error(err: MassProbeError) -> crate::disc::block::Error {
    if mass_probe_error_is_timeout(err) {
        return crate::disc::block::Error::Timeout;
    }

    match err {
        MassProbeError::ShortData | MassProbeError::Csw => crate::disc::block::Error::Corrupted,
        MassProbeError::Transport(_) => crate::disc::block::Error::Io,
    }
}

fn make_uas_command_iu(tag: u16, cdb: &[u8]) -> [u8; 32] {
    let mut iu = [0u8; 32];
    iu[0] = UAS_IU_COMMAND;
    iu[2..4].copy_from_slice(&tag.to_be_bytes());
    let cdb_len = cdb.len().min(16);
    iu[16..16 + cdb_len].copy_from_slice(&cdb[..cdb_len]);
    iu
}

fn parse_uas_tag(iu: &[u8]) -> Option<u16> {
    if iu.len() < 4 {
        return None;
    }
    Some(u16::from_be_bytes([iu[2], iu[3]]))
}

fn validate_uas_status(
    cmd: &'static str,
    iu: &[u8],
    expected_tag: u16,
) -> Result<(), MassProbeError> {
    if iu.len() < 10 {
        return Err(MassProbeError::ShortData);
    }
    let iu_id = iu[0];
    let tag = parse_uas_tag(iu).unwrap_or(0);
    let status = iu[6];
    if iu_id != UAS_IU_STATUS || tag != expected_tag || status != UAS_STATUS_GOOD {
        crate::log!(
            "crabusb: skhynix-green proof=uas-status cmd={} expected_tag=0x{:04x} iu=0x{:02x} tag=0x{:04x} status=0x{:02x} raw_len={} result=bad\n",
            cmd,
            expected_tag,
            iu_id,
            tag,
            status,
            iu.len()
        );
        return Err(MassProbeError::Csw);
    }
    Ok(())
}

fn cdb_inquiry(allocation_len: u16) -> [u8; 6] {
    [0x12, 0, 0, 0, allocation_len.min(0xff) as u8, 0]
}

fn cdb_read_capacity_10() -> [u8; 10] {
    [0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0]
}

fn cdb_read_10(lba: u32, blocks: u16) -> [u8; 10] {
    let lba = lba.to_be_bytes();
    let blocks = blocks.to_be_bytes();
    [
        0x28, 0, lba[0], lba[1], lba[2], lba[3], 0, blocks[0], blocks[1], 0,
    ]
}

fn cdb_write_10(lba: u32, blocks: u16) -> [u8; 10] {
    let lba = lba.to_be_bytes();
    let blocks = blocks.to_be_bytes();
    [
        0x2A, 0, lba[0], lba[1], lba[2], lba[3], 0, blocks[0], blocks[1], 0,
    ]
}

fn decode_ascii_field(field: &[u8]) -> String {
    let mut out = String::new();
    for &b in field {
        if (0x20..=0x7e).contains(&b) {
            out.push(b as char);
        } else {
            out.push(' ');
        }
    }
    String::from(out.trim())
}
