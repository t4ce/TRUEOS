extern crate alloc;

use alloc::vec::Vec;

const MEDIA_DECODE_CACHE_PATH: &str = "media/demo_yelly.mp4";

pub(crate) enum MediaSource {
    Memory {
        source: &'static str,
        body: Vec<u8>,
    },
    CacheFile {
        source: &'static str,
        disk: crate::disc::block::DeviceHandle,
        path: &'static str,
        len: u64,
    },
}

impl MediaSource {
    pub(crate) fn source_name(&self) -> &'static str {
        match self {
            Self::Memory { source, .. } | Self::CacheFile { source, .. } => source,
        }
    }

    pub(crate) fn total_len(&self) -> u64 {
        match self {
            Self::Memory { body, .. } => body.len() as u64,
            Self::CacheFile { len, .. } => *len,
        }
    }

    pub(crate) fn body(&self) -> Option<&[u8]> {
        match self {
            Self::Memory { body, .. } => Some(body.as_slice()),
            Self::CacheFile { .. } => None,
        }
    }

    pub(crate) async fn read_range_into(
        &self,
        offset: u64,
        dst: &mut [u8],
    ) -> Result<bool, crate::disc::block::Error> {
        match self {
            Self::Memory { body, .. } => {
                let start =
                    usize::try_from(offset).map_err(|_| crate::disc::block::Error::OutOfBounds)?;
                let end = start
                    .checked_add(dst.len())
                    .ok_or(crate::disc::block::Error::OutOfBounds)?;
                let Some(slice) = body.get(start..end) else {
                    return Err(crate::disc::block::Error::OutOfBounds);
                };
                dst.copy_from_slice(slice);
                Ok(true)
            }
            Self::CacheFile {
                disk,
                path,
                len: total,
                ..
            } => {
                let end = offset
                    .checked_add(dst.len() as u64)
                    .ok_or(crate::disc::block::Error::OutOfBounds)?;
                if end > *total {
                    return Err(crate::disc::block::Error::OutOfBounds);
                }
                crate::r::stream::read_trueosfs_file_range_into_async(*disk, path, offset, dst)
                    .await
            }
        }
    }
}

pub(crate) async fn fetch_media_source_async() -> Option<MediaSource> {
    if !crate::logflag::INTEL_MEDIA_FS_CACHE_ENABLED {
        crate::log!(
            "intel/media: source unavailable path={} reason=fs-cache-disabled\n",
            MEDIA_DECODE_CACHE_PATH,
        );
        return None;
    }

    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::log!(
            "intel/media: source unavailable path={} reason=no-trueosfs-root\n",
            MEDIA_DECODE_CACHE_PATH,
        );
        return None;
    };
    let info = disk.info();
    crate::log!(
        "intel/media: cache probe path={} disk_id={} readonly={} block={} max_transfer={}\n",
        MEDIA_DECODE_CACHE_PATH,
        info.id.raw(),
        info.is_read_only() as u8,
        info.block_size,
        info.max_transfer_bytes,
    );
    match crate::r::fs::trueosfs::file_info_async(disk, MEDIA_DECODE_CACHE_PATH).await {
        Ok(Some(info)) if info.data_len != 0 => {
            crate::log!(
                "intel/media: cache hit path={} bytes={} source=file\n",
                MEDIA_DECODE_CACHE_PATH,
                info.data_len
            );
            return Some(MediaSource::CacheFile {
                source: "cache",
                disk,
                path: MEDIA_DECODE_CACHE_PATH,
                len: info.data_len,
            });
        }
        Ok(_) => {
            crate::log!("intel/media: cache miss path={}\n", MEDIA_DECODE_CACHE_PATH,);
        }
        Err(err) => {
            crate::log!(
                "intel/media: cache probe failed path={} err={:?}\n",
                MEDIA_DECODE_CACHE_PATH,
                err
            );
        }
    }
    None
}
