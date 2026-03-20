fn insert_signature_candidate(
    top: &mut [IntelDisplaySignatureCandidate; INTEL_DISPLAY_SIGNATURE_TOP_PAGES],
    cand: IntelDisplaySignatureCandidate,
) {
    if cand.score == 0 {
        return;
    }
    let mut slot = None;
    let mut idx = 0usize;
    while idx < top.len() {
        if cand.score > top[idx].score {
            slot = Some(idx);
            break;
        }
        idx += 1;
    }
    let Some(slot_idx) = slot else {
        return;
    };
    let mut move_idx = top.len() - 1;
    while move_idx > slot_idx {
        top[move_idx] = top[move_idx - 1];
        move_idx -= 1;
    }
    top[slot_idx] = cand;
}

fn log_signature_window(info: IntelGfxInfo, page: usize, label: &str) {
    crate::log!(
        "gfx-intel-scanout: signature-window label={} base=0x{:05X} dwords={}\n",
        label,
        page,
        INTEL_DISPLAY_SIGNATURE_WINDOW_DWORDS
    );
    let mut idx = 0usize;
    while idx < INTEL_DISPLAY_SIGNATURE_WINDOW_DWORDS {
        let off = page + idx.saturating_mul(4);
        let value = intel_mmio_read32(info, off);
        crate::log!(
            "gfx-intel-scanout: signature-mmio label={} off=0x{:05X} value=0x{:08X}\n",
            label,
            off,
            value
        );
        idx += 1;
    }
}

fn log_display_signature_sweep(info: IntelGfxInfo) {
    let mut top = [IntelDisplaySignatureCandidate::empty(); INTEL_DISPLAY_SIGNATURE_TOP_PAGES];
    let mut page = 0usize;
    while page + INTEL_DISPLAY_PAGE_STRIDE <= info.mmio_len {
        let mut cand = IntelDisplaySignatureCandidate {
            page,
            ..IntelDisplaySignatureCandidate::empty()
        };
        let mut off = 0usize;
        while off < INTEL_DISPLAY_PAGE_STRIDE {
            let mmio_off = page + off;
            let value = intel_mmio_read32(info, mmio_off);
            if value != 0 {
                cand.nonzero_dwords = cand.nonzero_dwords.saturating_add(1);
            }
            if cand.pipe_src_off == usize::MAX && plausible_pipe_src(value).is_some() {
                cand.pipe_src_off = mmio_off;
                cand.pipe_src_value = value;
                cand.score = cand.score.saturating_add(7);
            }
            if cand.stride_off == usize::MAX && plausible_scanout_stride(value) {
                cand.stride_off = mmio_off;
                cand.stride_value = value;
                cand.score = cand.score.saturating_add(5);
            }
            if cand.surf_off == usize::MAX
                && plausible_scanout_surface(value, info.aperture_bar_size)
            {
                cand.surf_off = mmio_off;
                cand.surf_value = value;
                cand.score = cand.score.saturating_add(6);
            }
            if cand.ctl_off == usize::MAX && (value & INTEL_PLANE_ENABLE) != 0 && value != u32::MAX
            {
                cand.ctl_off = mmio_off;
                cand.ctl_value = value;
                cand.score = cand.score.saturating_add(3);
            }
            off += 4;
        }
        if cand.pipe_src_off != usize::MAX && cand.stride_off != usize::MAX {
            cand.score = cand.score.saturating_add(4);
        }
        if cand.surf_off != usize::MAX && cand.stride_off != usize::MAX {
            cand.score = cand.score.saturating_add(3);
        }
        if cand.surf_off != usize::MAX && cand.ctl_off != usize::MAX {
            cand.score = cand.score.saturating_add(2);
        }
        insert_signature_candidate(&mut top, cand);
        page += INTEL_DISPLAY_PAGE_STRIDE;
    }

    crate::log!(
        "gfx-intel-scanout: signature-sweep begin mmio_len=0x{:X} aperture=0x{:X}\n",
        info.mmio_len,
        info.aperture_bar_size
    );
    let mut rank = 0usize;
    while rank < top.len() && top[rank].score != 0 {
        let cand = top[rank];
        let (pipe_w, pipe_h) = plausible_pipe_src(cand.pipe_src_value).unwrap_or((0, 0));
        crate::log!(
            "gfx-intel-scanout: signature-candidate rank={} page=0x{:05X} score={} nonzero={} pipe_src_off={} pipe_src=0x{:08X} size={}x{} stride_off={} stride=0x{:08X} surf_off={} surf=0x{:08X} ctl_off={} ctl=0x{:08X}\n",
            rank + 1,
            cand.page,
            cand.score,
            cand.nonzero_dwords,
            if cand.pipe_src_off == usize::MAX {
                -1isize
            } else {
                cand.pipe_src_off as isize
            },
            cand.pipe_src_value,
            pipe_w,
            pipe_h,
            if cand.stride_off == usize::MAX {
                -1isize
            } else {
                cand.stride_off as isize
            },
            cand.stride_value,
            if cand.surf_off == usize::MAX {
                -1isize
            } else {
                cand.surf_off as isize
            },
            cand.surf_value,
            if cand.ctl_off == usize::MAX {
                -1isize
            } else {
                cand.ctl_off as isize
            },
            cand.ctl_value
        );
        if rank < 3 {
            log_signature_window(info, cand.page, "signature-top");
        }
        rank += 1;
    }
    if rank == 0 {
        crate::log!("gfx-intel-scanout: signature-sweep found no plausible scanout pages\n");
    }
}

fn scanout_plane(pipe: usize, plane_slot: usize) -> IntelScanoutPlane {
    let plane_base = INTEL_UNI_PLANE_BASE
        + pipe.saturating_mul(INTEL_UNI_PLANE_PIPE_STRIDE)
        + plane_slot.saturating_mul(INTEL_UNI_PLANE_SLOT_STRIDE);
    let (pipe_name, pipe_src_off, trans_ddi_func_ctl_off) = INTEL_SCANOUT_PIPES[pipe];
    IntelScanoutPlane {
        pipe_name,
        plane_slot: plane_slot + 1,
        ctl_off: plane_base,
        stride_off: plane_base + INTEL_UNI_PLANE_STRIDE_OFF,
        surf_off: plane_base + INTEL_UNI_PLANE_SURF_OFF,
        surf_live_off: plane_base + INTEL_UNI_PLANE_SURFLIVE_OFF,
        pipe_src_off,
        trans_ddi_func_ctl_off,
    }
}

fn log_display_region_sweep(info: IntelGfxInfo) {
    let mut logged = 0usize;
    let mut page = INTEL_DISPLAY_SWEEP_START;
    while page < INTEL_DISPLAY_SWEEP_END {
        let mut found = None;
        let mut off = 0usize;
        while off < INTEL_DISPLAY_PAGE_STRIDE {
            let value = intel_mmio_read32(info, page + off);
            if value != 0 {
                found = Some((off, value));
                break;
            }
            off += 4;
        }
        if let Some((first_off, value)) = found {
            crate::log!(
                "gfx-intel-scanout: display-page page=0x{:05X} first=0x{:03X} value=0x{:08X}\n",
                page,
                first_off,
                value
            );
            if logged < 4 {
                log_display_window(info, page + first_off);
            }
            logged += 1;
            if logged >= INTEL_DISPLAY_SWEEP_LOG_LIMIT {
                break;
            }
        }
        page += INTEL_DISPLAY_PAGE_STRIDE;
    }
    if logged == 0 {
        crate::log!(
            "gfx-intel-scanout: display-page sweep 0x{:05X}..0x{:05X} found no nonzero registers\n",
            INTEL_DISPLAY_SWEEP_START,
            INTEL_DISPLAY_SWEEP_END
        );
    }
}

fn log_display_range_census(info: IntelGfxInfo) {
    for &(start, end, name) in INTEL_DISPLAY_CENSUS_RANGES {
        let mut page = start;
        let mut logged = 0usize;
        let mut run_logged = 0usize;
        let mut nonzero_pages = 0usize;
        let mut ffff_pages = 0usize;
        let mut zero_pages = 0usize;
        let mut run_class = "";
        let mut run_start = start;
        let mut run_pages = 0usize;
        crate::log!(
            "gfx-intel-scanout: census begin name={} start=0x{:05X} end=0x{:05X}\n",
            name,
            start,
            end
        );
        while page < end {
            let mut sample_or = 0u32;
            let mut sample_and = u32::MAX;
            let mut first_nonzero = None;
            let mut first_nonffff = None;
            let mut off = 0usize;
            while off < INTEL_DISPLAY_PAGE_STRIDE {
                let value = intel_mmio_read32(info, page + off);
                sample_or |= value;
                sample_and &= value;
                if first_nonzero.is_none() && value != 0 {
                    first_nonzero = Some((off, value));
                }
                if first_nonffff.is_none() && value != u32::MAX {
                    first_nonffff = Some((off, value));
                }
                off += 4;
            }

            let class = if sample_or == 0 {
                zero_pages += 1;
                "zero"
            } else if sample_and == u32::MAX {
                ffff_pages += 1;
                if logged < INTEL_DISPLAY_CENSUS_GROUP_LIMIT {
                    crate::log!(
                        "gfx-intel-scanout: census page=0x{:05X} class=ffff name={}\n",
                        page,
                        name
                    );
                    logged += 1;
                }
                "ffff"
            } else {
                nonzero_pages += 1;
                if logged < INTEL_DISPLAY_CENSUS_GROUP_LIMIT {
                    let (nz_off, nz_val) = first_nonzero.unwrap_or((0, 0));
                    let (nf_off, nf_val) = first_nonffff.unwrap_or((0, u32::MAX));
                    crate::log!(
                        "gfx-intel-scanout: census page=0x{:05X} class=mixed name={} or=0x{:08X} and=0x{:08X} first_nz=0x{:03X}/0x{:08X} first_nonffff=0x{:03X}/0x{:08X}\n",
                        page,
                        name,
                        sample_or,
                        sample_and,
                        nz_off,
                        nz_val,
                        nf_off,
                        nf_val
                    );
                    logged += 1;
                }
                "mixed"
            };

            if run_pages == 0 {
                run_class = class;
                run_start = page;
                run_pages = 1;
            } else if run_class == class {
                run_pages += 1;
            } else {
                if run_logged < INTEL_DISPLAY_CENSUS_RUN_LIMIT {
                    crate::log!(
                        "gfx-intel-scanout: census run name={} class={} start=0x{:05X} end=0x{:05X} pages={}\n",
                        name,
                        run_class,
                        run_start,
                        page,
                        run_pages
                    );
                    run_logged += 1;
                }
                run_class = class;
                run_start = page;
                run_pages = 1;
            }

            page += INTEL_DISPLAY_PAGE_STRIDE;
        }
        if run_pages != 0 && run_logged < INTEL_DISPLAY_CENSUS_RUN_LIMIT {
            crate::log!(
                "gfx-intel-scanout: census run name={} class={} start=0x{:05X} end=0x{:05X} pages={}\n",
                name,
                run_class,
                run_start,
                end,
                run_pages
            );
        }
        crate::log!(
            "gfx-intel-scanout: census end name={} mixed={} ffff={} zero={}\n",
            name,
            nonzero_pages,
            ffff_pages,
            zero_pages
        );
    }
}

fn log_display_window(info: IntelGfxInfo, center_off: usize) {
    let aligned = center_off & !0x1Fusize;
    let mut idx = 0usize;
    while idx < INTEL_DISPLAY_WINDOW_DWORDS {
        let off = aligned + idx.saturating_mul(4);
        let value = intel_mmio_read32(info, off);
        crate::log!(
            "gfx-intel-scanout: display-mmio off=0x{:05X} value=0x{:08X}\n",
            off,
            value
        );
        idx += 1;
    }
}

fn log_display_dense_window(info: IntelGfxInfo, center_off: usize, label: &str) {
    let aligned = center_off & !(INTEL_DISPLAY_PAGE_STRIDE - 1);
    crate::log!(
        "gfx-intel-scanout: dense-window label={} base=0x{:05X} dwords={}\n",
        label,
        aligned,
        INTEL_DISPLAY_DENSE_WINDOW_DWORDS
    );
    let mut idx = 0usize;
    while idx < INTEL_DISPLAY_DENSE_WINDOW_DWORDS {
        let off = aligned + idx.saturating_mul(4);
        let value = intel_mmio_read32(info, off);
        crate::log!(
            "gfx-intel-scanout: dense-mmio label={} off=0x{:05X} value=0x{:08X}\n",
            label,
            off,
            value
        );
        idx += 1;
    }
}

fn log_display_dense_windows(info: IntelGfxInfo) {
    for &(center, label) in INTEL_DISPLAY_DENSE_CENTERS {
        log_display_dense_window(info, center, label);
    }
}

fn log_display_extra_dense_window(info: IntelGfxInfo, start_off: usize, label: &str) {
    let aligned = start_off & !(INTEL_DISPLAY_PAGE_STRIDE - 1);
    crate::log!(
        "gfx-intel-scanout: extra-dense-window label={} start=0x{:05X} dwords={}\n",
        label,
        start_off,
        INTEL_DISPLAY_EXTRA_DENSE_WINDOW_DWORDS
    );
    let mut idx = 0usize;
    while idx < INTEL_DISPLAY_EXTRA_DENSE_WINDOW_DWORDS {
        let off = aligned + idx.saturating_mul(4);
        let value = intel_mmio_read32(info, off);
        crate::log!(
            "gfx-intel-scanout: extra-dense-mmio label={} off=0x{:05X} value=0x{:08X}\n",
            label,
            off,
            value
        );
        idx += 1;
    }
}

fn log_display_extra_dense_windows(info: IntelGfxInfo) {
    for &(start, label) in INTEL_DISPLAY_EXTRA_DENSE_WINDOWS {
        log_display_extra_dense_window(info, start, label);
    }
}

fn log_display_power_probe(info: IntelGfxInfo) {
    let phy_misc_a = intel_mmio_read32(info, INTEL_ICL_PHY_MISC_A);
    let phy_misc_b = intel_mmio_read32(info, INTEL_ICL_PHY_MISC_B);
    let tx_bmu = intel_mmio_read32(info, INTEL_DISPIO_CR_TX_BMU_CR0);
    let de_pll_ctl = intel_mmio_read32(info, INTEL_BXT_DE_PLL_CTL);
    let de_pll_enable = intel_mmio_read32(info, INTEL_BXT_DE_PLL_ENABLE);
    let dc_state_en = intel_mmio_read32(info, INTEL_DC_STATE_EN);
    let dc_state_debug = intel_mmio_read32(info, INTEL_DC_STATE_DEBUG);
    let hotplug = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let gt_disp_pwron = intel_mmio_read32(info, INTEL_GT_DISP_PWRON);

    crate::log!(
        "gfx-intel-scanout: power-probe phy_misc_a=0x{:08X} phy_misc_b=0x{:08X} tx_bmu=0x{:08X} de_pll_ctl=0x{:08X} de_pll_enable=0x{:08X} dc_state_en=0x{:08X} dc_state_debug=0x{:08X} hotplug=0x{:08X} gt_disp_pwron=0x{:08X}\n",
        phy_misc_a,
        phy_misc_b,
        tx_bmu,
        de_pll_ctl,
        de_pll_enable,
        dc_state_en,
        dc_state_debug,
        hotplug,
        gt_disp_pwron
    );
}

fn log_hdmi_port_probe(info: IntelGfxInfo) {
    let hotplug_en = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let trans_a = intel_mmio_read32(info, INTEL_TRANS_A_DDI_FUNC_CTL);
    let trans_b = intel_mmio_read32(info, INTEL_TRANS_B_DDI_FUNC_CTL);
    let trans_c = intel_mmio_read32(info, INTEL_TRANS_C_DDI_FUNC_CTL);
    let trans_d = intel_mmio_read32(info, INTEL_TRANS_D_DDI_FUNC_CTL);
    let pipe_a = intel_mmio_read32(info, INTEL_PIPE_A_SRC);
    let pipe_b = intel_mmio_read32(info, INTEL_PIPE_B_SRC);
    let pipe_c = intel_mmio_read32(info, INTEL_PIPE_C_SRC);
    let pipe_d = intel_mmio_read32(info, INTEL_PIPE_D_SRC);
    crate::log!(
        "gfx-intel-scanout: hdmi-probe hotplug_en=0x{:08X} trans_a=0x{:08X} trans_b=0x{:08X} trans_c=0x{:08X} trans_d=0x{:08X} pipe_a=0x{:08X} pipe_b=0x{:08X} pipe_c=0x{:08X} pipe_d=0x{:08X}\n",
        hotplug_en,
        trans_a,
        trans_b,
        trans_c,
        trans_d,
        pipe_a,
        pipe_b,
        pipe_c,
        pipe_d
    );
}

fn log_display_focus_windows(info: IntelGfxInfo) {
    for &center in &[
        INTEL_BXT_DE_PLL_ENABLE,
        INTEL_PORT_HOTPLUG_EN,
        INTEL_TRANS_A_DDI_FUNC_CTL,
        INTEL_TRANS_B_DDI_FUNC_CTL,
        INTEL_TRANS_C_DDI_FUNC_CTL,
        INTEL_TRANS_D_DDI_FUNC_CTL,
        INTEL_GT_DISP_PWRON,
    ] {
        crate::log!("gfx-intel-scanout: focus-window center=0x{:05X}\n", center);
        log_display_window(info, center);
    }
}

fn arm_display_power_smoke(info: IntelGfxInfo) -> bool {
    let orig_pwron = intel_mmio_read32(info, INTEL_GT_DISP_PWRON);
    let req_pwron = orig_pwron | INTEL_GT_DISP_PWRON_REQ;
    let wrote = intel_mmio_write32(info, INTEL_GT_DISP_PWRON, req_pwron);
    let rb_pwron = intel_mmio_read32(info, INTEL_GT_DISP_PWRON);
    let hotplug = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let de_pll_enable = intel_mmio_read32(info, INTEL_BXT_DE_PLL_ENABLE);
    let phy_misc_a = intel_mmio_read32(info, INTEL_ICL_PHY_MISC_A);
    let latched = wrote && (rb_pwron & INTEL_GT_DISP_PWRON_REQ) != 0;
    crate::log!(
        "gfx-intel-scanout: disp-pwron-smoke orig=0x{:08X} req=0x{:08X} rb=0x{:08X} hotplug=0x{:08X} de_pll_enable=0x{:08X} phy_misc_a=0x{:08X} latched={}\n",
        orig_pwron,
        req_pwron,
        rb_pwron,
        hotplug,
        de_pll_enable,
        phy_misc_a,
        latched as u8
    );
    crate::log!(
        "gfx-intel-scanout: post-pwron-window center=0x{:05X}\n",
        INTEL_GT_DISP_PWRON
    );
    log_display_window(info, INTEL_GT_DISP_PWRON);
    latched
}

fn hotplug_write_smoke(info: IntelGfxInfo) -> bool {
    let orig = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let test = orig | INTEL_PORT_HOTPLUG_TEST_BIT;
    let wrote = intel_mmio_write32(info, INTEL_PORT_HOTPLUG_EN, test);
    let rb = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let _ = intel_mmio_write32(info, INTEL_PORT_HOTPLUG_EN, orig);
    let restored = intel_mmio_read32(info, INTEL_PORT_HOTPLUG_EN);
    let latched = wrote && (rb & INTEL_PORT_HOTPLUG_TEST_BIT) != 0;
    crate::log!(
        "gfx-intel-scanout: hotplug-smoke orig=0x{:08X} test=0x{:08X} rb=0x{:08X} restore=0x{:08X} latched={}\n",
        orig,
        test,
        rb,
        restored,
        latched as u8
    );
    crate::log!(
        "gfx-intel-scanout: post-hotplug-window center=0x{:05X}\n",
        INTEL_PORT_HOTPLUG_EN
    );
    log_display_window(info, INTEL_PORT_HOTPLUG_EN);
    latched
}

fn signature_candidate_surface_smoke(info: IntelGfxInfo) {
    let ctl = intel_mmio_read32(info, INTEL_SIGNATURE_SMOKE_CTL_OFF);
    let surf = intel_mmio_read32(info, INTEL_SIGNATURE_SMOKE_SURF_OFF);
    let pipe_src = intel_mmio_read32(info, INTEL_SIGNATURE_SMOKE_PIPE_SRC_OFF);
    let stride = intel_mmio_read32(info, INTEL_SIGNATURE_SMOKE_STRIDE_OFF);
    let (width, height) = plausible_pipe_src(pipe_src).unwrap_or((0, 0));
    let surf_ok = plausible_scanout_surface(surf, info.aperture_bar_size);
    let stride_ok = plausible_scanout_stride(stride);
    let ctl_ok = ctl == INTEL_PLANE_ENABLE;
    if !ctl_ok || !surf_ok || !stride_ok || width == 0 || height == 0 {
        crate::log!(
            "gfx-intel-scanout: signature-smoke skip ctl=0x{:08X} surf=0x{:08X} pipe_src=0x{:08X} size={}x{} stride=0x{:08X} ctl_ok={} surf_ok={} stride_ok={}\n",
            ctl,
            surf,
            pipe_src,
            width,
            height,
            stride,
            ctl_ok as u8,
            surf_ok as u8,
            stride_ok as u8
        );
        return;
    }

    let test_surf = if (surf as u64).saturating_add(0x1000) < info.aperture_bar_size {
        surf.saturating_add(0x1000)
    } else if surf >= 0x1000 {
        surf.saturating_sub(0x1000)
    } else {
        surf
    };
    if test_surf == surf || !plausible_scanout_surface(test_surf, info.aperture_bar_size) {
        crate::log!(
            "gfx-intel-scanout: signature-smoke skip surf=0x{:08X} no alternate in aperture=0x{:X}\n",
            surf,
            info.aperture_bar_size
        );
        return;
    }

    let wrote = intel_mmio_write32(info, INTEL_SIGNATURE_SMOKE_SURF_OFF, test_surf);
    let rb = intel_mmio_read32(info, INTEL_SIGNATURE_SMOKE_SURF_OFF);
    let _ = intel_mmio_write32(info, INTEL_SIGNATURE_SMOKE_SURF_OFF, surf);
    let restored = intel_mmio_read32(info, INTEL_SIGNATURE_SMOKE_SURF_OFF);
    let latched = wrote && rb == test_surf && restored == surf;
    crate::log!(
        "gfx-intel-scanout: signature-smoke page=0x82000 ctl=0x{:08X} pipe_src=0x{:08X} size={}x{} stride=0x{:08X} surf orig=0x{:08X} test=0x{:08X} rb=0x{:08X} restore=0x{:08X} latched={}\n",
        ctl,
        pipe_src,
        width,
        height,
        stride,
        surf,
        test_surf,
        rb,
        restored,
        latched as u8
    );
}

fn probe_scanout_surface(info: IntelGfxInfo) -> Option<IntelScanoutSurface> {
    let mut found = None;
    let mut nonzero_pipes = 0usize;
    let mut nonzero_planes = 0usize;
    let mut enabled_planes = 0usize;
    for pipe in 0..INTEL_SCANOUT_PIPES.len() {
        let plane0 = scanout_plane(pipe, 0);
        let pipe_src = intel_mmio_read32(info, plane0.pipe_src_off);
        let trans_ddi = intel_mmio_read32(info, plane0.trans_ddi_func_ctl_off);
        let (pipe_w, pipe_h) = decode_pipe_src(pipe_src);
        if pipe_src != 0 || trans_ddi != 0 {
            nonzero_pipes += 1;
        }
        for plane_slot in 0..4 {
            let plane = scanout_plane(pipe, plane_slot);
            let ctl = intel_mmio_read32(info, plane.ctl_off);
            let stride = intel_mmio_read32(info, plane.stride_off) as usize;
            let surf = intel_mmio_read32(info, plane.surf_off);
            let surf_live = intel_mmio_read32(info, plane.surf_live_off);
            let enabled = (ctl & INTEL_PLANE_ENABLE) != 0;
            if enabled {
                enabled_planes += 1;
            }
            if ctl != 0 || stride != 0 || surf != 0 || surf_live != 0 {
                nonzero_planes += 1;
                crate::log!(
                    "gfx-intel-scanout: plane-live {}{} ctl=0x{:08X} stride=0x{:08X} surf=0x{:08X} surf_live=0x{:08X} enabled={}\n",
                    plane.pipe_name,
                    plane.plane_slot,
                    ctl,
                    stride as u32,
                    surf,
                    surf_live,
                    enabled as u8
                );
            }
            if !enabled || surf == 0 || stride < 64 || pipe_w == 0 || pipe_h == 0 {
                continue;
            }
            found = Some(IntelScanoutSurface {
                plane,
                ctl,
                stride,
                surf,
                surf_live,
                width: pipe_w,
                height: pipe_h,
            });
            break;
        }
        if found.is_some() {
            break;
        }
    }
    if found.is_none() && (nonzero_pipes != 0 || nonzero_planes != 0 || enabled_planes != 0) {
        crate::log!(
            "gfx-intel-scanout: plane-scan summary nonzero_pipes={} nonzero_planes={} enabled_planes={}\n",
            nonzero_pipes,
            nonzero_planes,
            enabled_planes
        );
    }
    found
}

fn write_scanout_test_pattern(base: *mut u8, stride: usize, width: usize, height: usize) {
    for y in 0..height {
        let row_ptr = unsafe { base.add(y.saturating_mul(stride)) as *mut u32 };
        let row = unsafe { core::slice::from_raw_parts_mut(row_ptr, width) };
        for (x, px) in row.iter_mut().enumerate() {
            let band = (x.saturating_mul(6)) / width.max(1);
            let mut color = match band {
                0 => 0x00002020,
                1 => 0x00FF3030,
                2 => 0x0030FF30,
                3 => 0x003080FF,
                4 => 0x00F0E040,
                _ => 0x00F8F8F8,
            };
            if x < 4 || y < 4 || x + 4 >= width || y + 4 >= height {
                color = 0x00FFFFFF;
            }
            let diag0 = x.saturating_mul(height.max(1)) / width.max(1);
            let diag1 = (width.saturating_sub(1).saturating_sub(x)).saturating_mul(height.max(1))
                / width.max(1);
            if y.abs_diff(diag0) <= 2 || y.abs_diff(diag1) <= 2 {
                color = 0x00000000;
            }
            if y > height / 3
                && y < (height / 3).saturating_mul(2)
                && x > width / 3
                && x < (width / 3).saturating_mul(2)
            {
                color = 0x00000000;
            }
            *px = color;
        }
    }
}

fn prepare_direct_demo_surface(info: IntelGfxInfo) -> Option<(u32, usize, usize, usize)> {
    if info.aperture_bar_phys == 0 || info.aperture_bar_size == 0 {
        crate::log!(
            "gfx-intel-scanout: direct-demo aperture unavailable bar2=0x{:X} size=0x{:X}\n",
            info.aperture_bar_phys,
            info.aperture_bar_size
        );
        return None;
    }

    let surf = INTEL_DIRECT_DEMO_SURF_OFF;
    let stride = INTEL_DIRECT_DEMO_STRIDE as usize;
    let width = INTEL_DIRECT_DEMO_WIDTH.min((stride / 4).max(1));
    let height = INTEL_DIRECT_DEMO_HEIGHT;
    let bytes = height.saturating_mul(stride);
    if (surf as u64).saturating_add(bytes as u64) > info.aperture_bar_size {
        crate::log!(
            "gfx-intel-scanout: direct-demo surf=0x{:08X} stride=0x{:X} bytes=0x{:X} exceeds aperture=0x{:X}\n",
            surf,
            stride,
            bytes,
            info.aperture_bar_size
        );
        return None;
    }

    let phys = info.aperture_bar_phys.saturating_add(surf as u64);
    let Ok(mapped) = crate::pci::mmio::map_mmio_region_exact(phys, bytes) else {
        crate::log!(
            "gfx-intel-scanout: direct-demo aperture map failed phys=0x{:X} bytes=0x{:X}\n",
            phys,
            bytes
        );
        return None;
    };

    write_scanout_test_pattern(mapped.as_ptr(), stride, width, height);
    let sample0 = unsafe { core::ptr::read_volatile(mapped.as_ptr() as *const u32) };
    let sample1 = unsafe { core::ptr::read_volatile(mapped.as_ptr().add(4) as *const u32) };
    crate::log!(
        "gfx-intel-scanout: direct-demo surface ready surf=0x{:08X} stride=0x{:X} size={}x{} phys=0x{:X} sample0=0x{:08X} sample1=0x{:08X}\n",
        surf,
        stride,
        width,
        height,
        phys,
        sample0,
        sample1
    );
    Some((surf, stride, width, height))
}

fn try_direct_plane_demo(info: IntelGfxInfo) -> bool {
    let Some((surf, stride, width, height)) = prepare_direct_demo_surface(info) else {
        return false;
    };
    let pipe_src = (((height.saturating_sub(1)) as u32) << 16) | ((width.saturating_sub(1)) as u32);
    let mut armed = false;

    for pipe in 0..INTEL_SCANOUT_PIPES.len() {
        let plane = scanout_plane(pipe, 0);
        let orig_ctl = intel_mmio_read32(info, plane.ctl_off);
        let orig_stride = intel_mmio_read32(info, plane.stride_off);
        let orig_surf = intel_mmio_read32(info, plane.surf_off);
        let orig_pipe_src = intel_mmio_read32(info, plane.pipe_src_off);
        let orig_ddi = intel_mmio_read32(info, plane.trans_ddi_func_ctl_off);

        let _ = intel_mmio_write32(info, plane.pipe_src_off, pipe_src);
        let rb_pipe_src = intel_mmio_read32(info, plane.pipe_src_off);
        let _ = intel_mmio_write32(info, plane.stride_off, stride as u32);
        let rb_stride = intel_mmio_read32(info, plane.stride_off);
        let _ = intel_mmio_write32(info, plane.surf_off, surf);
        let rb_surf = intel_mmio_read32(info, plane.surf_off);
        let _ = intel_mmio_write32(info, plane.ctl_off, INTEL_PLANE_ENABLE);
        let rb_ctl = intel_mmio_read32(info, plane.ctl_off);

        let pipe_stuck = rb_pipe_src == pipe_src;
        let stride_stuck = rb_stride == stride as u32;
        let surf_stuck = rb_surf == surf;
        let ctl_stuck = rb_ctl == INTEL_PLANE_ENABLE;

        crate::log!(
            "gfx-intel-scanout: direct-demo attempt pipe={} plane={} pipe_src orig=0x{:08X} rb=0x{:08X} stride orig=0x{:08X} rb=0x{:08X} surf orig=0x{:08X} rb=0x{:08X} ctl orig=0x{:08X} rb=0x{:08X} ddi=0x{:08X} stuck pipe={} stride={} surf={} ctl={}\n",
            plane.pipe_name,
            plane.plane_slot,
            orig_pipe_src,
            rb_pipe_src,
            orig_stride,
            rb_stride,
            orig_surf,
            rb_surf,
            orig_ctl,
            rb_ctl,
            orig_ddi,
            pipe_stuck as u8,
            stride_stuck as u8,
            surf_stuck as u8,
            ctl_stuck as u8
        );

        if pipe_stuck && stride_stuck && surf_stuck {
            crate::log!(
                "gfx-intel-scanout: direct-demo armed pipe={} plane={} surf=0x{:08X} stride=0x{:X} size={}x{}\n",
                plane.pipe_name,
                plane.plane_slot,
                surf,
                stride,
                width,
                height
            );
            armed = true;
            break;
        }

        let _ = intel_mmio_write32(info, plane.ctl_off, orig_ctl);
        let _ = intel_mmio_write32(info, plane.surf_off, orig_surf);
        let _ = intel_mmio_write32(info, plane.stride_off, orig_stride);
        let _ = intel_mmio_write32(info, plane.pipe_src_off, orig_pipe_src);
    }

    if !armed {
        crate::log!(
            "gfx-intel-scanout: direct-demo no candidate plane latched raw scanout state\n"
        );
    }
    armed
}

