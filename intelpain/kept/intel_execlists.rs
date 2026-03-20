use core::ptr::NonNull;

use crate::gfx::intel::IntelGfxInfo;

const RCS_HW_BASE: usize = 0x2000;
const RING_MODE_GEN7: usize = RCS_HW_BASE + 0x29C;
const RING_ELSP: usize = RCS_HW_BASE + 0x230;
const RING_EXECLIST_STATUS_LO: usize = RCS_HW_BASE + 0x234;
const RING_EXECLIST_STATUS_HI: usize = RCS_HW_BASE + 0x238;
const RING_CONTEXT_CONTROL: usize = RCS_HW_BASE + 0x244;
const RING_CONTEXT_STATUS_PTR: usize = RCS_HW_BASE + 0x3A0;
const RING_EXECLIST_SQ_CONTENTS: usize = RCS_HW_BASE + 0x510;
const RING_EXECLIST_CONTROL: usize = RCS_HW_BASE + 0x550;
const FORCEWAKE_GEN11_RENDER: usize = 0xA278;
const FORCEWAKE_ACK_RENDER: usize = 0x0D84;
const MMIO_SUSPECT_PAGE: usize = 0xF000;
const MMIO_SAMPLE_LIMIT: usize = 0x20_000;
const MAX_NONZERO_SAMPLES: usize = 8;
const FORCEWAKE_KERNEL: u32 = 1 << 0;
const FORCEWAKE_KERNEL_FALLBACK: u32 = 1 << 15;
const FORCEWAKE_POLL_ITERS: usize = 20_000;

const GFX_RUN_LIST_ENABLE: u32 = 1 << 15;
const GFX_PPGTT_ENABLE: u32 = 1 << 9;
const GEN8_GFX_PPGTT_48B: u32 = 1 << 7;
const GEN11_GFX_DISABLE_LEGACY_MODE: u32 = 1 << 3;

pub struct IntelExeclistsProbe {
    mmio_base: NonNull<u8>,
    mmio_len: usize,
}

struct ForcewakeStatus {
    ack_before: u32,
    ack_after_set: u32,
    ack_after_clear: u32,
    set_ok: bool,
    clear_ok: bool,
    fallback_used: bool,
    mode_after_set: u32,
    elsp_after_set: u32,
    status_after_set: u32,
}

impl IntelExeclistsProbe {
    pub fn new(info: IntelGfxInfo) -> Option<Self> {
        if info.mmio_len <= (RING_CONTEXT_STATUS_PTR + 4) {
            return None;
        }
        Some(Self {
            mmio_base: info.mmio_base,
            mmio_len: info.mmio_len,
        })
    }

    #[inline]
    fn mmio_read32(&self, off: usize) -> u32 {
        let ptr = unsafe { self.mmio_base.as_ptr().add(off) as *const u32 };
        unsafe { core::ptr::read_volatile(ptr) }
    }

    #[inline]
    fn mmio_write32(&self, off: usize, value: u32) {
        let ptr = unsafe { self.mmio_base.as_ptr().add(off) as *mut u32 };
        unsafe { core::ptr::write_volatile(ptr, value) };
    }

    fn sample_mmio(&self, off: usize) -> u32 {
        if off + 4 > self.mmio_len {
            0
        } else {
            self.mmio_read32(off)
        }
    }

    #[inline]
    fn masked_bit_enable(bit: u32) -> u32 {
        bit | (bit << 16)
    }

    #[inline]
    fn masked_bit_disable(bit: u32) -> u32 {
        bit << 16
    }

    fn wait_ack(&self, mask: u32, expected: u32) -> (bool, u32, usize) {
        let mut last = self.sample_mmio(FORCEWAKE_ACK_RENDER);
        if (last & mask) == expected {
            return (true, last, 0);
        }

        let mut iter = 0usize;
        while iter < FORCEWAKE_POLL_ITERS {
            core::hint::spin_loop();
            last = self.sample_mmio(FORCEWAKE_ACK_RENDER);
            if (last & mask) == expected {
                return (true, last, iter + 1);
            }
            iter += 1;
        }

        (false, last, FORCEWAKE_POLL_ITERS)
    }

    fn try_forcewake_render(&self) -> ForcewakeStatus {
        let ack_before = self.sample_mmio(FORCEWAKE_ACK_RENDER);

        // Start from a known deasserted state for the render domain.
        self.mmio_write32(
            FORCEWAKE_GEN11_RENDER,
            Self::masked_bit_disable(FORCEWAKE_KERNEL | FORCEWAKE_KERNEL_FALLBACK),
        );
        let _ = self.wait_ack(FORCEWAKE_KERNEL | FORCEWAKE_KERNEL_FALLBACK, 0);

        self.mmio_write32(
            FORCEWAKE_GEN11_RENDER,
            Self::masked_bit_enable(FORCEWAKE_KERNEL),
        );
        let (mut set_ok, mut ack_after_set, _) = self.wait_ack(FORCEWAKE_KERNEL, FORCEWAKE_KERNEL);

        let mut fallback_used = false;
        if !set_ok {
            // Mirror i915's fallback kick for newer gens: toggle the fallback bit to
            // coax the forcewake ack state machine into responding.
            fallback_used = true;
            self.mmio_write32(
                FORCEWAKE_GEN11_RENDER,
                Self::masked_bit_disable(FORCEWAKE_KERNEL_FALLBACK),
            );
            let _ = self.wait_ack(FORCEWAKE_KERNEL_FALLBACK, 0);
            self.mmio_write32(
                FORCEWAKE_GEN11_RENDER,
                Self::masked_bit_enable(FORCEWAKE_KERNEL_FALLBACK),
            );
            let _ = self.wait_ack(FORCEWAKE_KERNEL_FALLBACK, FORCEWAKE_KERNEL_FALLBACK);
            let (retry_ok, retry_ack, _) = self.wait_ack(FORCEWAKE_KERNEL, FORCEWAKE_KERNEL);
            set_ok = retry_ok;
            ack_after_set = retry_ack;
            self.mmio_write32(
                FORCEWAKE_GEN11_RENDER,
                Self::masked_bit_disable(FORCEWAKE_KERNEL_FALLBACK),
            );
            let _ = self.wait_ack(FORCEWAKE_KERNEL_FALLBACK, 0);
        }

        let mode_after_set = self.sample_mmio(RING_MODE_GEN7);
        let elsp_after_set = self.sample_mmio(RING_ELSP);
        let status_after_set = self.sample_mmio(RING_EXECLIST_STATUS_LO);

        self.mmio_write32(
            FORCEWAKE_GEN11_RENDER,
            Self::masked_bit_disable(FORCEWAKE_KERNEL),
        );
        let (clear_ok, ack_after_clear, _) = self.wait_ack(FORCEWAKE_KERNEL, 0);

        ForcewakeStatus {
            ack_before,
            ack_after_set,
            ack_after_clear,
            set_ok,
            clear_ok,
            fallback_used,
            mode_after_set,
            elsp_after_set,
            status_after_set,
        }
    }

    pub fn log_registers(&self, info: IntelGfxInfo) {
        let sample_0000 = self.sample_mmio(0x0000);
        let sample_0004 = self.sample_mmio(0x0004);
        let sample_1000 = self.sample_mmio(0x1000);
        let sample_2000 = self.sample_mmio(RCS_HW_BASE);
        let sample_2100 = self.sample_mmio(RCS_HW_BASE + 0x100);
        let sample_f000 = self.sample_mmio(MMIO_SUSPECT_PAGE);
        let sample_f004 = self.sample_mmio(MMIO_SUSPECT_PAGE + 0x4);
        let sample_fw_req = self.sample_mmio(FORCEWAKE_GEN11_RENDER);
        let sample_fw_ack = self.sample_mmio(FORCEWAKE_ACK_RENDER);
        let forcewake = self.try_forcewake_render();

        let mut sample_nonzero = 0usize;
        let mut sample_or = 0u32;
        let mut first_nonzero_off = usize::MAX;
        let mut nonzero_offsets = [0usize; MAX_NONZERO_SAMPLES];
        let mut nonzero_values = [0u32; MAX_NONZERO_SAMPLES];
        let mut nonzero_logged = 0usize;
        let sample_limit = self.mmio_len.min(MMIO_SAMPLE_LIMIT);
        let mut off = 0usize;
        while off + 4 <= sample_limit {
            let value = self.mmio_read32(off);
            sample_or |= value;
            if value != 0 {
                sample_nonzero += 1;
                if first_nonzero_off == usize::MAX {
                    first_nonzero_off = off;
                }
                if nonzero_logged < MAX_NONZERO_SAMPLES {
                    nonzero_offsets[nonzero_logged] = off;
                    nonzero_values[nonzero_logged] = value;
                    nonzero_logged += 1;
                }
            }
            off += 0x1000;
        }
        let first_nonzero = if first_nonzero_off == usize::MAX {
            0xFFFF_FFFFusize
        } else {
            first_nonzero_off
        };

        let mode = self.mmio_read32(RING_MODE_GEN7);
        let elsp = self.mmio_read32(RING_ELSP);
        let status_lo = self.mmio_read32(RING_EXECLIST_STATUS_LO);
        let status_hi = self.mmio_read32(RING_EXECLIST_STATUS_HI);
        let ctx_ctrl = self.mmio_read32(RING_CONTEXT_CONTROL);
        let ctx_status_ptr = self.mmio_read32(RING_CONTEXT_STATUS_PTR);
        let sq_contents = self.mmio_read32(RING_EXECLIST_SQ_CONTENTS);
        let execlist_ctrl = self.mmio_read32(RING_EXECLIST_CONTROL);

        crate::log!(
            "gfx-intel: execlists probe {:02X}:{:02X}.{} device=0x{:04X} mmio_len=0x{:X} sample0000=0x{:08X} sample0004=0x{:08X} sample1000=0x{:08X} sample2000=0x{:08X} sample2100=0x{:08X} samplef000=0x{:08X} samplef004=0x{:08X} fw_req=0x{:08X} fw_ack=0x{:08X} sample_nonzero={} sample_or=0x{:08X} first_nonzero=0x{:X} nz0=0x{:X}:0x{:08X} nz1=0x{:X}:0x{:08X} nz2=0x{:X}:0x{:08X} nz3=0x{:X}:0x{:08X} nz4=0x{:X}:0x{:08X} nz5=0x{:X}:0x{:08X} nz6=0x{:X}:0x{:08X} nz7=0x{:X}:0x{:08X} fw_before=0x{:08X} fw_after_set=0x{:08X} fw_after_clear=0x{:08X} fw_set_ok={} fw_clear_ok={} fw_fallback={} mode_after_set=0x{:08X} elsp_after_set=0x{:08X} status_after_set=0x{:08X} mode=0x{:08X} elsp=0x{:08X} status_lo=0x{:08X} status_hi=0x{:08X} ctx_ctrl=0x{:08X} ctx_status_ptr=0x{:08X} sq=0x{:08X} exec_ctl=0x{:08X} legacy_disabled={} run_list={} ppgtt={} ppgtt48={}\n",
            info.bus,
            info.slot,
            info.function,
            info.device_id,
            self.mmio_len,
            sample_0000,
            sample_0004,
            sample_1000,
            sample_2000,
            sample_2100,
            sample_f000,
            sample_f004,
            sample_fw_req,
            sample_fw_ack,
            sample_nonzero,
            sample_or,
            first_nonzero,
            nonzero_offsets[0],
            nonzero_values[0],
            nonzero_offsets[1],
            nonzero_values[1],
            nonzero_offsets[2],
            nonzero_values[2],
            nonzero_offsets[3],
            nonzero_values[3],
            nonzero_offsets[4],
            nonzero_values[4],
            nonzero_offsets[5],
            nonzero_values[5],
            nonzero_offsets[6],
            nonzero_values[6],
            nonzero_offsets[7],
            nonzero_values[7],
            forcewake.ack_before,
            forcewake.ack_after_set,
            forcewake.ack_after_clear,
            forcewake.set_ok as u8,
            forcewake.clear_ok as u8,
            forcewake.fallback_used as u8,
            forcewake.mode_after_set,
            forcewake.elsp_after_set,
            forcewake.status_after_set,
            mode,
            elsp,
            status_lo,
            status_hi,
            ctx_ctrl,
            ctx_status_ptr,
            sq_contents,
            execlist_ctrl,
            ((mode & GEN11_GFX_DISABLE_LEGACY_MODE) != 0) as u8,
            ((mode & GFX_RUN_LIST_ENABLE) != 0) as u8,
            ((mode & GFX_PPGTT_ENABLE) != 0) as u8,
            ((mode & GEN8_GFX_PPGTT_48B) != 0) as u8,
        );
    }
}
