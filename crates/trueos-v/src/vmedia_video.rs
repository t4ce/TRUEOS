//! Hardware H.264 texture streaming. Shares the kernel's global playback cap
//! with shell video; no decoder address or decoded pixel copy crosses the ABI.
use crate::{bp_abi, vgpu::Device};
use alloc::sync::Arc;
fn command(op: u32, a: u64, b: u64, input: &[u8], output: &mut [u8]) -> i32 {
    unsafe {
        bp_abi::trueos_cabi_vmedia_video_command_v1(
            op,
            a,
            b,
            input.as_ptr(),
            input.len(),
            output.as_mut_ptr(),
            output.len(),
        )
    }
}
struct Stream {
    id: u32,
}
impl Drop for Stream {
    fn drop(&mut self) {
        let _ = command(5, self.id as u64, 0, &[], &mut []);
    }
}
/// A stream reserves one of the shared playback slots until this handle and
/// all acquired frames are dropped. Upload accepts AVC MP4 or Annex-B H.264.
/// MP4 preserves source presentation timing, colour metadata and B-frame order.
pub struct Video {
    stream: Arc<Stream>,
}
pub enum VideoPoll {
    Pending,
    Frame(VideoFrame),
    End,
}
impl Video {
    pub fn open(device: Device, encoded: &[u8], looping: bool) -> Result<Self, i32> {
        let id = command(0, device.raw(), encoded.len() as u64, &[looping as u8], &mut []);
        if id <= 0 {
            return Err(if id == 0 { -3 } else { id });
        }
        let stream = Arc::new(Stream { id: id as u32 });
        for (index, chunk) in encoded.chunks(3072).enumerate() {
            let rc = command(1, id as u64, (index * 3072) as u64, chunk, &mut []);
            if rc != 0 {
                return Err(rc);
            }
        }
        let rc = command(2, id as u64, 0, &[], &mut []);
        if rc != 0 {
            return Err(rc);
        }
        Ok(Self { stream })
    }
    /// Stream a browser-resolved HTTPS AVC MP4 through the kernel's existing
    /// online decoder into this owner's GPU texture ring. No Shell2 window.
    pub fn open_url(device: Device, url: &str, looping: bool) -> Result<Self, i32> {
        if !url.starts_with("https://")
            || url.len() > 3072
            || url
                .bytes()
                .any(|c| c.is_ascii_control() || c.is_ascii_whitespace())
        {
            return Err(-3);
        }
        let id = command(6, device.raw(), looping as u64, url.as_bytes(), &mut []);
        if id <= 0 {
            return Err(if id == 0 { -3 } else { id });
        }
        Ok(Self {
            stream: Arc::new(Stream { id: id as u32 }),
        })
    }

    /// Pause/resume without releasing the current texture or decoder slot.
    pub fn set_paused(&mut self, paused: bool) -> Result<(), i32> {
        let rc = command(7, self.stream.id as u64, paused as u64, &[], &mut []);
        if rc == 0 { Ok(()) } else { Err(rc) }
    }

    /// Nonblocking acquisition. Keep the current frame while Pending; replace
    /// it only after obtaining another frame. Holding every ring slot applies
    /// backpressure to decoding. Drop a frame after its render submission retires.
    pub fn poll(&mut self) -> Result<VideoPoll, i32> {
        let mut bytes = [0u8; 24];
        match command(3, self.stream.id as u64, 0, &[], &mut bytes) {
            0 => Ok(VideoPoll::Pending),
            2 => Ok(VideoPoll::End),
            1 => Ok(VideoPoll::Frame(VideoFrame {
                stream: self.stream.clone(),
                texture: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
                sequence: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
                width: u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
                height: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
            })),
            error => Err(error),
        }
    }
}
/// A read lease over a stable GPU texture. Multiple meshes can sample this ID
/// in one frame without consuming additional decoder slots.
pub struct VideoFrame {
    stream: Arc<Stream>,
    texture: u64,
    sequence: u64,
    width: u32,
    height: u32,
}
impl VideoFrame {
    pub fn texture_id(&self) -> super::TextureId {
        super::TextureId(self.texture)
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn extent(&self) -> [u32; 2] {
        [self.width, self.height]
    }
}
impl Drop for VideoFrame {
    fn drop(&mut self) {
        let _ = command(4, self.stream.id as u64, self.sequence, &[], &mut []);
    }
}

/// Native UI4 playback of a bounded live Annex-B H.264 stream. The first
/// access unit must include SPS, PPS and an IDR. Drop and reopen after loss or
/// backpressure, then resume at a fresh parameter-set/IDR access unit.
/// This owns its UI4 video window and never forwards remote input.
pub struct LiveVideo {
    id: u32,
}
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LiveVideoStatus {
    Waiting,
    /// The native window is prepared and hardware decoding has started.
    /// This is not a first-frame presentation acknowledgement.
    Playing,
    Ended,
}
impl LiveVideo {
    pub const MAX_ACCESS_UNIT_BYTES: usize = 4 * 1024 * 1024;

    /// Reserve a shared native playback slot. The first SPS sets the window
    /// extent. Supported fixed stream rates are 1 through 60 frames/second.
    pub fn open(fps: u16) -> Result<Self, i32> {
        let id = command(10, fps as u64, 0, &[], &mut []);
        if id > 0 { Ok(Self { id: id as u32 }) }
        else { Err(if id == 0 { -3 } else { id }) }
    }

    /// Upload one complete picture transactionally. Incomplete uploads never
    /// reach the decoder. Returns -4 when the 8-picture/4-MiB queue is full;
    /// the caller should reset and resynchronize at a fresh IDR.
    pub fn submit_access_unit(&mut self, annex_b: &[u8]) -> Result<(), i32> {
        if annex_b.is_empty() || annex_b.len() > Self::MAX_ACCESS_UNIT_BYTES { return Err(-3); }
        let rc = command(11, self.id as u64, annex_b.len() as u64, &[], &mut []);
        if rc != 0 { return Err(rc); }
        let result = (|| {
            for (index, chunk) in annex_b.chunks(3072).enumerate() {
                let rc = command(12, self.id as u64, (index * 3072) as u64, chunk, &mut []);
                if rc != 0 { return Err(rc); }
            }
            let rc = command(13, self.id as u64, 0, &[], &mut []);
            if rc == 0 { Ok(()) } else { Err(rc) }
        })();
        if result.is_err() { let _ = command(16, self.id as u64, 0, &[], &mut []); }
        result
    }

    pub fn status(&self) -> Result<LiveVideoStatus, i32> {
        match command(14, self.id as u64, 0, &[], &mut []) {
            0 => Ok(LiveVideoStatus::Waiting),
            1 => Ok(LiveVideoStatus::Playing),
            2 => Ok(LiveVideoStatus::Ended),
            error => Err(error),
        }
    }
}
impl Drop for LiveVideo {
    fn drop(&mut self) { let _ = command(15, self.id as u64, 0, &[], &mut []); }
}
