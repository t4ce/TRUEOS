use core::future::Future;

use crab_usb::{Device, Endpoint, err::USBError, usb_if};
use usb_if::{
    endpoint::{TransferCompletion, TransferRequest},
    err::TransferError,
};

pub(crate) type EndpointBulkIn = Endpoint;
pub(crate) type EndpointBulkOut = Endpoint;
pub(crate) type EndpointInterruptIn = Endpoint;
pub(crate) type EndpointIsoIn = Endpoint;

pub(crate) trait EndpointSubmitExt {
    fn submit_and_wait<'a, B>(
        &'a mut self,
        buffer: B,
    ) -> impl Future<Output = Result<usize, TransferError>> + 'a
    where
        B: EndpointSubmitBuffer<'a> + 'a;

    fn submit_iso_out_and_wait<'a>(
        &'a mut self,
        buffer: &'a [u8],
        packets: usize,
    ) -> impl Future<Output = Result<usize, TransferError>> + 'a;

    fn submit_iso_in_and_wait<'a>(
        &'a mut self,
        buffer: &'a mut [u8],
        packets: usize,
    ) -> impl Future<Output = Result<usize, TransferError>> + 'a;
}

pub(crate) trait EndpointSubmitBuffer<'a> {
    fn request(
        self,
        transfer_type: usb_if::descriptor::EndpointType,
    ) -> Result<TransferRequest, TransferError>;
}

impl<'a> EndpointSubmitBuffer<'a> for &'a mut [u8] {
    fn request(
        self,
        transfer_type: usb_if::descriptor::EndpointType,
    ) -> Result<TransferRequest, TransferError> {
        match transfer_type {
            usb_if::descriptor::EndpointType::Bulk => Ok(TransferRequest::bulk_in(self)),
            usb_if::descriptor::EndpointType::Interrupt => Ok(TransferRequest::interrupt_in(self)),
            usb_if::descriptor::EndpointType::Isochronous => {
                Ok(TransferRequest::iso_in(self, &[self.len()]))
            }
            usb_if::descriptor::EndpointType::Control => Err(TransferError::InvalidEndpoint),
        }
    }
}

impl<'a, const N: usize> EndpointSubmitBuffer<'a> for &'a mut [u8; N] {
    fn request(
        self,
        transfer_type: usb_if::descriptor::EndpointType,
    ) -> Result<TransferRequest, TransferError> {
        <&'a mut [u8] as EndpointSubmitBuffer<'a>>::request(self.as_mut_slice(), transfer_type)
    }
}

impl<'a> EndpointSubmitBuffer<'a> for &'a [u8] {
    fn request(
        self,
        transfer_type: usb_if::descriptor::EndpointType,
    ) -> Result<TransferRequest, TransferError> {
        match transfer_type {
            usb_if::descriptor::EndpointType::Bulk => Ok(TransferRequest::bulk_out(self)),
            usb_if::descriptor::EndpointType::Interrupt => Ok(TransferRequest::interrupt_out(self)),
            usb_if::descriptor::EndpointType::Isochronous => {
                Ok(TransferRequest::iso_out(self, &[self.len()]))
            }
            usb_if::descriptor::EndpointType::Control => Err(TransferError::InvalidEndpoint),
        }
    }
}

impl<'a, const N: usize> EndpointSubmitBuffer<'a> for &'a [u8; N] {
    fn request(
        self,
        transfer_type: usb_if::descriptor::EndpointType,
    ) -> Result<TransferRequest, TransferError> {
        <&'a [u8] as EndpointSubmitBuffer<'a>>::request(self.as_slice(), transfer_type)
    }
}

async fn completion_len(
    result: Result<TransferCompletion, TransferError>,
) -> Result<usize, TransferError> {
    result.map(|completion| completion.actual_length)
}

impl EndpointSubmitExt for Endpoint {
    async fn submit_and_wait<'a, B>(&'a mut self, buffer: B) -> Result<usize, TransferError>
    where
        B: EndpointSubmitBuffer<'a> + 'a,
    {
        let request = buffer.request(self.info().transfer_type)?;
        completion_len(self.wait(request).await).await
    }

    async fn submit_iso_out_and_wait<'a>(
        &'a mut self,
        buffer: &'a [u8],
        packets: usize,
    ) -> Result<usize, TransferError> {
        let packet_count = packets.max(1);
        let base = buffer.len() / packet_count;
        let rem = buffer.len() % packet_count;
        let mut packet_lengths = alloc::vec![base; packet_count];
        if let Some(last) = packet_lengths.last_mut() {
            *last = last.saturating_add(rem);
        }
        completion_len(
            self.wait(TransferRequest::iso_out(buffer, &packet_lengths))
                .await,
        )
        .await
    }

    async fn submit_iso_in_and_wait<'a>(
        &'a mut self,
        buffer: &'a mut [u8],
        packets: usize,
    ) -> Result<usize, TransferError> {
        let packet_count = packets.max(1);
        let base = buffer.len() / packet_count;
        let rem = buffer.len() % packet_count;
        let mut packet_lengths = alloc::vec![base; packet_count];
        if let Some(last) = packet_lengths.last_mut() {
            *last = last.saturating_add(rem);
        }
        completion_len(
            self.wait(TransferRequest::iso_in(buffer, &packet_lengths))
                .await,
        )
        .await
    }
}

#[derive(Debug)]
pub(crate) enum InterfaceEndpointError {
    Usb(USBError),
    WrongKind { address: u8, expected: &'static str },
}

impl From<USBError> for InterfaceEndpointError {
    fn from(value: USBError) -> Self {
        Self::Usb(value)
    }
}

pub(crate) struct ClaimedInterface<'a> {
    device: &'a mut Device,
}

pub(crate) async fn claim_interface(
    device: &mut Device,
    interface_number: u8,
    alternate_setting: u8,
) -> Result<ClaimedInterface<'_>, USBError> {
    device
        .claim_interface(interface_number, alternate_setting)
        .await?;
    Ok(ClaimedInterface { device })
}

impl ClaimedInterface<'_> {
    pub(crate) fn device(&mut self) -> &mut Device {
        self.device
    }

    pub(crate) async fn endpoint_bulk_in(
        &mut self,
        address: u8,
    ) -> Result<EndpointBulkIn, InterfaceEndpointError> {
        let endpoint = self.device.endpoint(address)?;
        let info = endpoint.info();
        if info.transfer_type == usb_if::descriptor::EndpointType::Bulk
            && info.direction == usb_if::transfer::Direction::In
        {
            Ok(endpoint)
        } else {
            Err(InterfaceEndpointError::WrongKind {
                address,
                expected: "bulk-in",
            })
        }
    }

    pub(crate) async fn endpoint_bulk_out(
        &mut self,
        address: u8,
    ) -> Result<EndpointBulkOut, InterfaceEndpointError> {
        let endpoint = self.device.endpoint(address)?;
        let info = endpoint.info();
        if info.transfer_type == usb_if::descriptor::EndpointType::Bulk
            && info.direction == usb_if::transfer::Direction::Out
        {
            Ok(endpoint)
        } else {
            Err(InterfaceEndpointError::WrongKind {
                address,
                expected: "bulk-out",
            })
        }
    }

    pub(crate) async fn endpoint_isochronous_in(
        &mut self,
        address: u8,
    ) -> Result<EndpointIsoIn, InterfaceEndpointError> {
        let endpoint = self.device.endpoint(address)?;
        let info = endpoint.info();
        if info.transfer_type == usb_if::descriptor::EndpointType::Isochronous
            && info.direction == usb_if::transfer::Direction::In
        {
            Ok(endpoint)
        } else {
            Err(InterfaceEndpointError::WrongKind {
                address,
                expected: "iso-in",
            })
        }
    }

    pub(crate) async fn endpoint_interrupt_in(
        &mut self,
        address: u8,
    ) -> Result<EndpointInterruptIn, InterfaceEndpointError> {
        let endpoint = self.device.endpoint(address)?;
        let info = endpoint.info();
        if info.transfer_type == usb_if::descriptor::EndpointType::Interrupt
            && info.direction == usb_if::transfer::Direction::In
        {
            Ok(endpoint)
        } else {
            Err(InterfaceEndpointError::WrongKind {
                address,
                expected: "interrupt-in",
            })
        }
    }
}
