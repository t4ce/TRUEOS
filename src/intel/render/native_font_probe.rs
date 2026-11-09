/// Result of the explicit, display-independent native Microfont VS/PS probe.
/// A retired command stream alone cannot pass: all 256 R8 pixels are checked.
#[derive(Debug)]
pub(crate) struct NativeMicrofontR8Proof {
    pub(crate) physical_vendor: u16,
    pub(crate) physical_device: u16,
    pub(crate) physical_revision: u8,
    pub(crate) effective_device: u16,
    pub(crate) effective_revision: u8,
    pub(crate) completed_draws: usize,
    pub(crate) release_sequence: u64,
    pub(crate) gpu_poll_us: u64,
    pub(crate) pixels: native_font_probe_contract::Pixels,
    pub(crate) resources_released: bool,
}

impl NativeMicrofontR8Proof {
    pub(crate) const fn passed(&self) -> bool {
        self.completed_draws == 1
            && self.release_sequence != 0
            && self.pixels.passed()
            && self.resources_released
    }
}

/// Run on an explicit diagnostic request or the physical laptop's temporary
/// font-first boot preflight. This uses the production font
/// pipeline, push constants, R8 render-target and Render0 GuC retirement path.
/// It neither selects a font fallback nor touches a display plane. Success
/// does not validate native line AA, a complete alphabet, or display scanout.
pub(crate) fn probe_native_microfont_r8() -> Result<NativeMicrofontR8Proof, &'static str> {
    use native_font_probe_contract as contract;
    let dev = crate::intel::claimed_device().ok_or("no-device")?;
    let physical = crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 0);
    let revision = crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 8) as u8;
    crate::log_important!(target: "render";
        "font-r8-probe: start physical={:04X}:{:04X} rev={:02X} effective=8086:{:04X} rev={:02X} target=16x16 format=r8 artifact=microfont-mask-adls scope=explicit-quad-readback display_write=0\n",
        physical as u16, (physical >> 16) as u16, revision, dev.device_id, dev.revision_id);
    let mesh = create_resident_triangle_mesh(&contract::VERTICES, &contract::INDICES)?;
    let constants = match allocate_resident_render_buffer(64) {
        Ok(buffer) => buffer,
        Err(error) => {
            let _ = release_resident_triangle_mesh(&mesh);
            return Err(error);
        }
    };
    let mut mask = match create_resident_coverage_mask(contract::EXTENT, contract::EXTENT) {
        Ok(mask) => mask,
        Err(error) => {
            let _ = release_resident_render_buffer(&constants);
            let _ = release_resident_triangle_mesh(&mesh);
            return Err(error);
        }
    };
    let mut inputs_retired = true;
    let result = (|| {
        for (index, word) in contract::parameters().iter().enumerate() {
            if !constants.write(index * 4, &word.to_le_bytes()) {
                return Err("font-r8-probe-constants");
            }
        }
        constants.flush();
        let draw = ResidentSceneDraw {
            mesh: &mesh,
            depth_flags: None,
            rgba: [255; 4],
            sampled_texture: None,
            fragment_contract: ResidentSceneFragmentContract::MicrofontMask(constants.gpu_base()),
            viewport_translation_px: [0.0; 2],
            topology: ResidentScenePrimitiveTopology::TriangleList,
            point_width_px: 0,
            line_width_px: 0.0,
            clip_adjacency: false,
        };
        // The target owns its quarantine state. Mesh/constants have manual
        // lifetimes, so retain them unless this call proves full retirement.
        inputs_retired = false;
        let frame = render_resident_coverage_mask(&[draw], &mut mask)?;
        inputs_retired = frame.frame_complete;
        if let Some(error) = frame.completion_error() {
            return Err(error);
        }
        let surface = mask.surface();
        let release = frame
            .release_fence
            .filter(|release| release.matches(surface.phys, surface.bytes))
            .ok_or("font-r8-probe-release")?;
        let source = crate::phys::phys_to_virt(surface.phys as usize) as *mut u8;
        if !crate::intel::dma_flush_strided_row_spans(&[crate::intel::DmaFlushRows::new(
            source,
            contract::EXTENT as usize,
            surface.pitch_bytes as usize,
            contract::EXTENT as usize,
        )]) {
            return Err("font-r8-probe-readback-cache");
        }
        let mut pixels = [0; (contract::EXTENT * contract::EXTENT) as usize];
        for y in 0..contract::EXTENT as usize {
            for x in 0..contract::EXTENT as usize {
                pixels[y * contract::EXTENT as usize + x] = unsafe {
                    core::ptr::read_volatile(source.add(y * surface.pitch_bytes as usize + x))
                };
            }
        }
        Ok(NativeMicrofontR8Proof {
            physical_vendor: physical as u16,
            physical_device: (physical >> 16) as u16,
            physical_revision: revision,
            effective_device: dev.device_id,
            effective_revision: dev.revision_id,
            completed_draws: frame.completed_draws,
            release_sequence: release.sequence(),
            gpu_poll_us: frame.gpu_poll_us,
            pixels: contract::classify(&pixels, contract::EXTENT as usize)
                .ok_or("font-r8-probe-readback-shape")?,
            resources_released: false,
        })
    })();
    // Render0 input release only serializes unmapping; it does not establish
    // execution retirement. Never unmap/free these inputs after uncertainty.
    let mask_released = release_resident_coverage_mask(mask);
    let constants_released = inputs_retired && release_resident_render_buffer(&constants);
    let mesh_released = inputs_retired && release_resident_triangle_mesh(&mesh);
    if !(mask_released && constants_released && mesh_released) {
        crate::log_error!(target: "render";
            "font-r8-probe: cleanup mask={} constants={} mesh={} action=retain-unretired-resources\n",
            mask_released, constants_released, mesh_released);
    }
    result.map(|mut report| {
        report.resources_released = mask_released && constants_released && mesh_released;
        report
    })
}
