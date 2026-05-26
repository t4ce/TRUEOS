use crab_usb::{
    Device, Endpoint,
    err::{TransferError, USBError},
    usb_if::{
        descriptor::EndpointType,
        endpoint::TransferRequest,
        transfer::Direction,
    },
};

pub(crate) enum InterfaceEndpointError {
    WrongKind { address: u8, expected: &'static str },
    Usb(USBError),
}

pub(crate) struct ClaimedInterface<'a> {
    device: &'a mut Device,
}

impl ClaimedInterface<'_> {
    pub(crate) fn device(&mut self) -> &mut Device {
        self.device
    }

    pub(crate) async fn endpoint_interrupt_in(
        &mut self,
        address: u8,
    ) -> Result<ClaimedEndpoint, InterfaceEndpointError> {
        self.endpoint(address, EndpointType::Interrupt, Direction::In, "interrupt-in")
            .await
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

    async fn endpoint(
        &mut self,
        address: u8,
        transfer_type: EndpointType,
        direction: Direction,
        expected: &'static str,
    ) -> Result<ClaimedEndpoint, InterfaceEndpointError> {
        let endpoint = self.device.endpoint(address).map_err(InterfaceEndpointError::Usb)?;
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
        self.endpoint.wait(request).await.map(|done| done.actual_length)
    }
}

pub(crate) async fn claim_interface(
    device: &mut Device,
    interface: u8,
    alternate: u8,
) -> Result<ClaimedInterface<'_>, USBError> {
    device.claim_interface(interface, alternate).await?;
    Ok(ClaimedInterface { device })
}
