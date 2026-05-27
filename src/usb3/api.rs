use crab_usb::{
    Device, Endpoint,
    err::{TransferError, USBError},
    usb_if::{descriptor::EndpointType, endpoint::TransferRequest, transfer::Direction},
};

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
    ) -> Result<ClaimedEndpoint, InterfaceEndpointError> {
        self.endpoint(address, EndpointType::Bulk, Direction::In, "bulk-in")
            .await
    }

    pub(crate) async fn endpoint_bulk_out(
        &mut self,
        address: u8,
    ) -> Result<ClaimedEndpoint, InterfaceEndpointError> {
        self.endpoint(address, EndpointType::Bulk, Direction::Out, "bulk-out")
            .await
    }

    #[allow(dead_code)]
    pub(crate) async fn endpoint_isochronous_in(
        &mut self,
        address: u8,
    ) -> Result<ClaimedEndpoint, InterfaceEndpointError> {
        self.endpoint(address, EndpointType::Isochronous, Direction::In, "iso-in")
            .await
    }

    pub(crate) async fn endpoint_interrupt_in(
        &mut self,
        address: u8,
    ) -> Result<ClaimedEndpoint, InterfaceEndpointError> {
        self.endpoint(address, EndpointType::Interrupt, Direction::In, "interrupt-in")
            .await
    }

    async fn endpoint(
        &mut self,
        address: u8,
        transfer_type: EndpointType,
        direction: Direction,
        expected: &'static str,
    ) -> Result<ClaimedEndpoint, InterfaceEndpointError> {
        let endpoint = self.device.endpoint(address)?;
        let info = endpoint.info();
        if info.transfer_type != transfer_type || info.direction != direction {
            return Err(InterfaceEndpointError::WrongKind { address, expected });
        }
        Ok(ClaimedEndpoint {
            endpoint,
            transfer_type,
            direction,
        })
    }
}

pub(crate) struct ClaimedEndpoint {
    endpoint: Endpoint,
    transfer_type: EndpointType,
    direction: Direction,
}

impl ClaimedEndpoint {
    pub(crate) async fn submit_and_wait(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<usize, TransferError> {
        let request = match (self.transfer_type, self.direction) {
            (EndpointType::Interrupt, Direction::In) => TransferRequest::interrupt_in(buffer),
            (EndpointType::Bulk, Direction::In) => TransferRequest::bulk_in(buffer),
            (EndpointType::Bulk, Direction::Out) => TransferRequest::bulk_out(buffer),
            _ => return Err(TransferError::InvalidEndpoint),
        };
        self.endpoint
            .wait(request)
            .await
            .map(|done| done.actual_length)
    }
}
