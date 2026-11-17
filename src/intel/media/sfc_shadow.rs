//! Opt-in probe. One supported AVC picture writes an unpublished RGBA surface
//! alongside its normal NV12 decode target for a post-retirement comparison.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::intel::ppgtt::{PpgttRange, SparsePpgtt, build_sparse_ppgtt_for_ranges};

use super::engine::{MediaBitstreamBacking, MediaEngineDescriptor, MediaGpuWindowLayout};
use super::h264_cmd::{AVC_CMD_OFFSET_SURFACE_STATE, AvcLongFormatIdrPlan, MFX_SURFACE_STATE_DW0};
use super::sfc_cmd::{self, Csc, Geometry};

// Private PPGTT only. Keep the probe outside the standard decoder windows.
const SHADOW_GPU: u64 = 0x1_0000_0000;
const PAGE_BYTES: usize = 4096;
// Tiger Lake PRM Vol. 2c, MFX_SFC_LOCK_STATUS (VCS0 and VCS2).
const SFC_LOCK_STATUS: usize = 0x890;
const SFC_IN_USE: u32 = 1;
static ATTEMPTED: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone)]
pub(super) struct Picture {
    geometry: Geometry,
    nv12_pitch: usize,
    uv_offset: usize,
    csc: Csc,
}

impl Picture {
    pub(super) fn for_avc(plan: &AvcLongFormatIdrPlan) -> Option<Self> {
        if !plan.picture.idr_pic
            || !plan.picture.frame_mbs_only
            || plan.slice_count != 1
            || plan.slice.disable_deblocking_filter_idc == 1
        {
            return None;
        }
        let geometry = Geometry::new(plan.picture.coded_width(), plan.picture.coded_height())?;
        let surface = plan.resources.dest_surface;
        Some(Self {
            geometry,
            nv12_pitch: surface.pitch_bytes,
            uv_offset: surface.uv_offset,
            csc: Csc::new(
                plan.picture.video_full_range,
                plan.picture.matrix_coefficients,
            ),
        })
    }
}

struct Dma {
    virt: *mut u8,
    bytes: usize,
}

impl Drop for Dma {
    fn drop(&mut self) {
        crate::dma::dealloc(self.virt, self.bytes);
    }
}

// The picture service owns this across yields. No other task can read or
// mutate the allocation, and the GPU has its own private address space.
unsafe impl Send for Dma {}

pub(super) struct Probe {
    ppgtt: SparsePpgtt,
    dma: Dma,
    picture: Picture,
    commands: Vec<u32>,
}

impl Probe {
    pub(super) fn prepare(
        dev: crate::intel::Dev,
        engine: MediaEngineDescriptor,
        windows: MediaGpuWindowLayout,
        backing: MediaBitstreamBacking,
        commands: &[u32],
        picture: Picture,
    ) -> Option<Self> {
        if !matches!(dev.device_id, 0x4680 | 0xa780)
            || !matches!(engine.ring_base, 0x1c_0000 | 0x1d_0000)
            || !engine.capabilities.sfc
            || commands.get(AVC_CMD_OFFSET_SURFACE_STATE) != Some(&MFX_SURFACE_STATE_DW0)
            || ATTEMPTED.swap(true, Ordering::AcqRel)
        {
            return None;
        }

        let geometry = picture.geometry;
        let bytes = geometry.rgba_bytes + geometry.avs_bytes;
        let (phys, virt) = crate::dma::alloc_ppgtt(bytes, PAGE_BYTES)?;
        let dma = Dma { virt, bytes };
        // Alpha must change on every sampled destination pixel. A distinctive
        // initial value also makes an untouched target easy to diagnose.
        unsafe { core::ptr::write_bytes(virt, 0xa5, bytes) };
        crate::intel::dma_flush(virt, bytes);

        let mut ppgtt = build_sparse_ppgtt_for_ranges(&[
            PpgttRange {
                gpu: windows.batch_gpu_addr,
                phys: backing.batch_phys,
                bytes: backing.batch_bytes,
            },
            PpgttRange {
                gpu: windows.result_gpu_addr,
                phys: backing.result_phys,
                bytes: backing.result_bytes,
            },
            PpgttRange {
                gpu: windows.bitstream_gpu_addr,
                phys: backing.bitstream_phys,
                bytes: backing.bitstream_bytes,
            },
            PpgttRange {
                gpu: windows.output_surface_gpu_addr,
                phys: backing.output_surface_phys,
                bytes: backing.output_surface_bytes,
            },
            PpgttRange {
                gpu: windows.avc_scratch_gpu_addr,
                phys: backing.avc_scratch_phys,
                bytes: backing.avc_scratch_bytes,
            },
        ])?;
        ppgtt.map_scanout_range(PpgttRange {
            gpu: SHADOW_GPU,
            phys,
            bytes,
        })?;

        let sfc = sfc_cmd::encode(
            geometry,
            SHADOW_GPU,
            SHADOW_GPU + geometry.rgba_bytes as u64,
            picture.csc,
        )?;
        let mut combined = Vec::new();
        combined
            .try_reserve_exact(commands.len().checked_add(sfc.len())?)
            .ok()?;
        let (before, after) = commands.split_at(AVC_CMD_OFFSET_SURFACE_STATE);
        combined.extend_from_slice(before);
        combined.extend_from_slice(&sfc);
        combined.extend_from_slice(after);
        crate::log_important!(target: "intel-media";
            "intel/avc-sfc: plan engine={} size={}x{} pitch={} rgba_bytes={} avs_bytes={} packet_dwords={}\n",
            engine.name, geometry.width, geometry.height, geometry.pitch,
            geometry.rgba_bytes, geometry.avs_bytes, sfc.len());
        Some(Self {
            ppgtt,
            dma,
            picture,
            commands: combined,
        })
    }

    pub(super) fn pml4_phys(&self) -> u64 {
        self.ppgtt.pml4_phys()
    }

    pub(super) fn commands(&self) -> &[u32] {
        &self.commands
    }

    pub(super) fn lock_released(dev: crate::intel::Dev, engine: MediaEngineDescriptor) -> bool {
        crate::intel::mmio_read(dev, engine.ring_base + SFC_LOCK_STATUS) & SFC_IN_USE == 0
    }

    /// Call only after the video flush, GuC deregistration, and SFC unlock.
    /// Compare a grid including the edges and report alpha and RGB separately.
    pub(super) fn compare(self, decode_id: u32, nv12: *mut u8, nv12_bytes: usize) {
        let Picture {
            geometry,
            nv12_pitch,
            uv_offset,
            csc,
            ..
        } = self.picture;
        let chroma_row = uv_offset / nv12_pitch;
        let tiles_per_row = nv12_pitch / 128;
        let offset = super::engine::media_ytile_offset;
        let last_uv = offset(
            geometry.width - 1,
            chroma_row + geometry.height / 2 - 1,
            tiles_per_row,
        );
        if last_uv >= nv12_bytes {
            crate::log_error!(target: "intel-media";
                "intel/avc-sfc: compare id={} error=nv12-bounds\n", decode_id);
            return;
        }
        crate::intel::dma_flush(nv12, nv12_bytes);
        crate::intel::dma_flush(self.dma.virt, geometry.rgba_bytes);
        let nv12 = unsafe { core::slice::from_raw_parts(nv12, nv12_bytes) };
        let rgba = unsafe { core::slice::from_raw_parts(self.dma.virt, geometry.rgba_bytes) };

        const GRID: usize = 32;
        let mut bad_alpha = 0;
        let mut over_four = 0;
        let mut max_delta = 0;
        for row in 0..GRID {
            let y = row * (geometry.height - 1) / (GRID - 1);
            for col in 0..GRID {
                let x = col * (geometry.width - 1) / (GRID - 1);
                let luma = nv12[offset(x, y, tiles_per_row)];
                let uv = offset(x & !1, chroma_row + y / 2, tiles_per_row);
                let expected = csc.reference_rgb(luma, nv12[uv], nv12[uv + 1]);
                let base = y * geometry.pitch + x * 4;
                let actual = &rgba[base..base + 4];
                bad_alpha += usize::from(actual[3] != 255);
                let delta = expected
                    .into_iter()
                    .zip(actual)
                    .map(|(a, &b)| a.abs_diff(b))
                    .max()
                    .unwrap_or(0);
                max_delta = max_delta.max(delta);
                over_four += usize::from(delta > 4);
            }
        }
        crate::log_important!(target: "intel-media";
            "intel/avc-sfc: compare id={} samples={} bad_alpha={} rgb_delta_gt4={} rgb_max_delta={} published=0\n",
            decode_id, GRID * GRID, bad_alpha, over_four, max_delta);
    }
}
