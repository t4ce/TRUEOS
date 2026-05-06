use core::ptr::NonNull;

use core::task::{Context, Poll};

use usb_if::{err::TransferError, transfer::Direction};

use crate::backend::ty::{
    ep::{DetachedTransfer, TransferHandle},
    transfer::TransferKind,
};

use super::EndpointBase;

pub struct EndpointBulkIn {
    pub(crate) raw: EndpointBase,
}

impl EndpointBulkIn {
    pub async fn submit_and_wait(&mut self, buff: &mut [u8]) -> Result<usize, TransferError> {
        let t = self.submit(buff)?.await?;
        let n = t.transfer_len;
        Ok(n)
    }

    pub fn submit(&mut self, buff: &mut [u8]) -> Result<TransferHandle<'_>, TransferError> {
        self.submit_on_stream(0, buff)
    }

    pub async fn submit_on_stream_and_wait(
        &mut self,
        stream_id: u16,
        buff: &mut [u8],
    ) -> Result<usize, TransferError> {
        let t = self.submit_on_stream(stream_id, buff)?.await?;
        let n = t.transfer_len;
        Ok(n)
    }

    pub fn submit_on_stream(
        &mut self,
        stream_id: u16,
        buff: &mut [u8],
    ) -> Result<TransferHandle<'_>, TransferError> {
        let buff = if buff.is_empty() {
            None
        } else {
            Some((NonNull::new(buff.as_mut_ptr()).unwrap(), buff.len()))
        };

        let mut transfer = self
            .raw
            .new_transfer(TransferKind::Bulk, Direction::In, buff);
        transfer.stream_id = stream_id;

        self.raw.submit(transfer)
    }

    /// Submit a stream transfer without holding an endpoint borrow in the returned ticket.
    ///
    /// # Safety
    ///
    /// The caller must keep `buff` alive, at a stable address, and exclusively owned until
    /// `poll_detached` returns `Ready` for the returned ticket.
    pub unsafe fn submit_on_stream_detached(
        &mut self,
        stream_id: u16,
        buff: &mut [u8],
    ) -> Result<DetachedTransfer, TransferError> {
        let buff = if buff.is_empty() {
            None
        } else {
            Some((NonNull::new(buff.as_mut_ptr()).unwrap(), buff.len()))
        };

        let mut transfer = self
            .raw
            .new_transfer(TransferKind::Bulk, Direction::In, buff);
        transfer.stream_id = stream_id;

        unsafe { self.raw.submit_detached(transfer) }
    }

    pub fn poll_detached(
        &mut self,
        ticket: DetachedTransfer,
        cx: &mut Context<'_>,
    ) -> Poll<Result<usize, TransferError>> {
        self.raw.poll_detached(ticket, cx)
    }
}

impl From<EndpointBase> for EndpointBulkIn {
    fn from(raw: EndpointBase) -> Self {
        Self { raw }
    }
}

pub struct EndpointBulkOut {
    pub(crate) raw: EndpointBase,
}

impl EndpointBulkOut {
    pub async fn submit_and_wait(&mut self, buff: &[u8]) -> Result<usize, TransferError> {
        let t = self.submit(buff)?.await?;
        let n = t.transfer_len;
        Ok(n)
    }

    pub fn submit(&mut self, buff: &[u8]) -> Result<TransferHandle<'_>, TransferError> {
        self.submit_on_stream(0, buff)
    }

    pub async fn submit_on_stream_and_wait(
        &mut self,
        stream_id: u16,
        buff: &[u8],
    ) -> Result<usize, TransferError> {
        let t = self.submit_on_stream(stream_id, buff)?.await?;
        let n = t.transfer_len;
        Ok(n)
    }

    pub fn submit_on_stream(
        &mut self,
        stream_id: u16,
        buff: &[u8],
    ) -> Result<TransferHandle<'_>, TransferError> {
        let buff = if buff.is_empty() {
            None
        } else {
            Some((NonNull::new(buff.as_ptr() as *mut u8).unwrap(), buff.len()))
        };
        let mut transfer = self
            .raw
            .new_transfer(TransferKind::Bulk, Direction::Out, buff);
        transfer.stream_id = stream_id;

        self.raw.submit(transfer)
    }

    /// Submit a stream transfer without holding an endpoint borrow in the returned ticket.
    ///
    /// # Safety
    ///
    /// The caller must keep `buff` alive, at a stable address, and immutable until
    /// `poll_detached` returns `Ready` for the returned ticket.
    pub unsafe fn submit_on_stream_detached(
        &mut self,
        stream_id: u16,
        buff: &[u8],
    ) -> Result<DetachedTransfer, TransferError> {
        let buff = if buff.is_empty() {
            None
        } else {
            Some((NonNull::new(buff.as_ptr() as *mut u8).unwrap(), buff.len()))
        };
        let mut transfer = self
            .raw
            .new_transfer(TransferKind::Bulk, Direction::Out, buff);
        transfer.stream_id = stream_id;

        unsafe { self.raw.submit_detached(transfer) }
    }

    pub fn poll_detached(
        &mut self,
        ticket: DetachedTransfer,
        cx: &mut Context<'_>,
    ) -> Poll<Result<usize, TransferError>> {
        self.raw.poll_detached(ticket, cx)
    }
}

impl From<EndpointBase> for EndpointBulkOut {
    fn from(raw: EndpointBase) -> Self {
        Self { raw }
    }
}
