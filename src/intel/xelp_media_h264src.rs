extern crate alloc;

use alloc::vec::Vec;

use super::xelp_media_matroska::{
    MatroskaH264Summary, ensure_sample_index as ensure_matroska_sample_index,
    get_sample_range as get_matroska_sample_range, looks_like_matroska,
    parse_h264_matroska_summary, parse_h264_matroska_summary_from_source,
    sample_count_known as matroska_sample_count_known,
};
use super::xelp_media_mp4::{
    AvccSummary, Mp4H264Summary, get_sample_range as get_mp4_sample_range, parse_h264_mp4_summary,
    parse_h264_mp4_summary_from_source,
};
use super::xelp_media_source::MediaSource;

pub(crate) enum H264SourceSummary {
    Mp4(Mp4H264Summary),
    Matroska(MatroskaH264Summary),
}

impl H264SourceSummary {
    pub(crate) fn container_name(&self) -> &'static str {
        match self {
            Self::Mp4(_) => "mp4",
            Self::Matroska(_) => "matroska",
        }
    }

    pub(crate) fn width(&self) -> u16 {
        match self {
            Self::Mp4(summary) => summary.width,
            Self::Matroska(summary) => summary.width,
        }
    }

    pub(crate) fn height(&self) -> u16 {
        match self {
            Self::Mp4(summary) => summary.height,
            Self::Matroska(summary) => summary.height,
        }
    }

    pub(crate) fn sample_count(&self) -> u32 {
        match self {
            Self::Mp4(summary) => summary.sample_count,
            Self::Matroska(summary) => {
                if matroska_sample_count_known(summary) {
                    summary.sample_count
                } else {
                    u32::try_from(summary.samples.len()).unwrap_or(u32::MAX)
                }
            }
        }
    }

    pub(crate) fn sample_count_known(&self) -> bool {
        match self {
            Self::Mp4(_) => true,
            Self::Matroska(summary) => matroska_sample_count_known(summary),
        }
    }

    pub(crate) fn first_sample(&self) -> &[u8] {
        match self {
            Self::Mp4(summary) => summary.first_sample.as_slice(),
            Self::Matroska(summary) => summary.first_sample.as_slice(),
        }
    }

    pub(crate) fn avcc(&self) -> &AvccSummary {
        match self {
            Self::Mp4(summary) => &summary.avcc,
            Self::Matroska(summary) => &summary.avcc,
        }
    }

    pub(crate) async fn load_sample(
        &mut self,
        source: &MediaSource,
        index: u32,
        scratch: &mut Vec<u8>,
    ) -> Option<usize> {
        let (offset, len) = match self {
            Self::Mp4(summary) => {
                let range = get_mp4_sample_range(summary, index)?;
                (range.offset, range.len)
            }
            Self::Matroska(summary) => {
                ensure_matroska_sample_index(summary, source, index).await.ok()?;
                let range = get_matroska_sample_range(summary, index)?;
                (range.offset, range.len)
            }
        };
        let len = usize::try_from(len).ok()?;
        scratch.resize(len, 0);
        if !source.read_range_into(offset, scratch.as_mut_slice()).await.ok()? {
            return None;
        }
        Some(len)
    }
}

pub(crate) async fn parse_h264_source_summary(
    source: &MediaSource,
) -> Result<H264SourceSummary, &'static str> {
    if let Some(bytes) = source.body() {
        if looks_like_matroska(bytes) {
            return parse_h264_matroska_summary(bytes)
                .map(H264SourceSummary::Matroska)
                .map_err(|_| "matroska");
        }
        return parse_h264_mp4_summary(bytes)
            .map(H264SourceSummary::Mp4)
            .map_err(|_| "mp4");
    }

    let probe_len = usize::try_from(source.total_len().min(16)).map_err(|_| "probe")?;
    let mut probe = [0u8; 16];
    if !source
        .read_range_into(0, &mut probe[..probe_len])
        .await
        .map_err(|_| "probe")?
    {
        return Err("probe");
    }
    if looks_like_matroska(&probe[..probe_len]) {
        return parse_h264_matroska_summary_from_source(source)
            .await
            .map(H264SourceSummary::Matroska)
            .map_err(|_| "matroska");
    }
    parse_h264_mp4_summary_from_source(source)
        .await
        .map(H264SourceSummary::Mp4)
        .map_err(|_| "mp4")
}
