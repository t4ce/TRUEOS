use alloc::string::String;

#[derive(Clone, Debug)]
pub struct RemoteRestoreRequest {
    pub endpoint: String,
    pub vm_id: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteRestoreError {
    NotWired,
}

pub fn restore_from_remote(request: RemoteRestoreRequest) -> Result<usize, RemoteRestoreError> {
    let _ = (request.endpoint, request.vm_id);
    Err(RemoteRestoreError::NotWired)
}
