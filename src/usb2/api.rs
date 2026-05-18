use crab_usb::{Device, Endpoint, err::USBError, usb_if};

pub(crate) type EndpointBulkIn = Endpoint;
pub(crate) type EndpointBulkOut = Endpoint;
pub(crate) type EndpointInterruptIn = Endpoint;
pub(crate) type EndpointIsoIn = Endpoint;

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
