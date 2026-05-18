const FETCH_PENDING_RC: i32 = -8;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Poll<T> {
    Pending,
    Ready(T),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BytesJob {
    op_id: u32,
}

impl BytesJob {
    pub fn start(url: &[u8]) -> Result<Self, i32> {
        let op_id = v::vfetch::fetch_bytes(url)?;
        Ok(Self { op_id })
    }

    pub fn poll_len(&self) -> Result<Poll<usize>, i32> {
        match v::vfetch::fetch_bytes_result_len(self.op_id) {
            Ok(len) => Ok(Poll::Ready(len)),
            Err(FETCH_PENDING_RC) => Ok(Poll::Pending),
            Err(rc) => Err(rc),
        }
    }

    pub fn discard(self) -> i32 {
        v::vfetch::fetch_bytes_discard(self.op_id)
    }
}
