#[derive(Copy, Clone)]
struct CppAudioVisualizerBuffer {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for CppAudioVisualizerBuffer {}
unsafe impl Sync for CppAudioVisualizerBuffer {}

static CPP_AUDIO_VISUALIZER_BUFFER: Mutex<Option<CppAudioVisualizerBuffer>> = Mutex::new(None);

fn cpp_audio_visualizer_buffer_once() -> Option<CppAudioVisualizerBuffer> {
    if let Some(buffer) = *CPP_AUDIO_VISUALIZER_BUFFER.lock() {
        return Some(buffer);
    }
    let (phys, virt) = crate::dma::alloc(CPP_AUDIO_VISUALIZER_SNAPSHOT_BYTES, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, CPP_AUDIO_VISUALIZER_SNAPSHOT_BYTES);
    }
    super::dma_flush(virt, CPP_AUDIO_VISUALIZER_SNAPSHOT_BYTES);
    let buffer = CppAudioVisualizerBuffer {
        phys,
        gpu: CPP_AUDIO_VISUALIZER_SNAPSHOT_GPU,
        virt,
        bytes: CPP_AUDIO_VISUALIZER_SNAPSHOT_BYTES,
    };
    *CPP_AUDIO_VISUALIZER_BUFFER.lock() = Some(buffer);
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: cpp-audio-visualizer snapshot ready phys=0x{:X} gpu=0x{:X} bytes=0x{:X} fft=2048 mid_side=1 bands=64 waveform=128\n",
        buffer.phys,
        buffer.gpu,
        buffer.bytes,
    );
    Some(buffer)
}

fn cpp_audio_visualizer_write_snapshot(
    buffer: CppAudioVisualizerBuffer,
    snapshot: &crate::aud::audio_visualizer::AudioVisualizerFrame,
) {
    const MAGIC: u32 = 0x315A_5641;
    const VERSION: u32 = 1;
    const FEATURE_BASE: usize = 8;
    const WAVEFORM_BASE: usize = 32;
    const SPECTRUM_BASE: usize = 320;

    unsafe {
        core::ptr::write_bytes(buffer.virt, 0, buffer.bytes);
        let words = buffer.virt as *mut u32;
        core::ptr::write_volatile(words, MAGIC);
        core::ptr::write_volatile(words.add(1), VERSION);
        core::ptr::write_volatile(words.add(2), snapshot.sequence as u32);
        core::ptr::write_volatile(words.add(3), u32::from(snapshot.active));
        core::ptr::write_volatile(
            words.add(4),
            crate::aud::audio_visualizer::AUDIO_VISUALIZER_SAMPLE_RATE,
        );
        core::ptr::write_volatile(
            words.add(5),
            crate::aud::audio_visualizer::AUDIO_VISUALIZER_SPECTRUM_COUNT as u32,
        );
        core::ptr::write_volatile(
            words.add(6),
            crate::aud::audio_visualizer::AUDIO_VISUALIZER_WAVEFORM_COUNT as u32,
        );

        let features = [
            snapshot.rms_left,
            snapshot.rms_right,
            snapshot.peak,
            snapshot.stereo_width,
            snapshot.low,
            snapshot.mid,
            snapshot.high,
            snapshot.beat,
            snapshot.centroid,
            snapshot.flux,
            snapshot.tempo_phase,
            snapshot.signal,
        ];
        for (index, value) in features.into_iter().enumerate() {
            core::ptr::write_volatile(words.add(FEATURE_BASE + index), value.to_bits());
        }
        for index in 0..crate::aud::audio_visualizer::AUDIO_VISUALIZER_WAVEFORM_COUNT {
            core::ptr::write_volatile(
                words.add(WAVEFORM_BASE + index * 2),
                snapshot.waveform_left[index].to_bits(),
            );
            core::ptr::write_volatile(
                words.add(WAVEFORM_BASE + index * 2 + 1),
                snapshot.waveform_right[index].to_bits(),
            );
        }
        for (index, value) in snapshot.spectrum.iter().copied().enumerate() {
            core::ptr::write_volatile(words.add(SPECTRUM_BASE + index), value.to_bits());
        }
    }
    super::dma_flush(buffer.virt, buffer.bytes);
}

fn submit_cpp_audio_visualizer_rgba8(
    dst: GpgpuRgba8Surface,
    audio: CppAudioVisualizerBuffer,
    params: CppAudioVisualizerRgba8Params,
) -> DirectRcsDispatchOutcome {
    if !dst.is_valid() || dst.width == 0 || dst.height == 0 || !params.time_seconds.is_finite() {
        return DirectRcsDispatchOutcome::default();
    }

    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: cpp-audio-visualizer submit rejected reason=no-claimed-device\n"
        );
        return DirectRcsDispatchOutcome::default();
    };
    let Some(upload) = upload_cpp_audio_visualizer_rgba8_kernel() else {
        return DirectRcsDispatchOutcome::default();
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return DirectRcsDispatchOutcome::default();
    };

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let audio_ok =
        kernel_ok && direct_rcs_map_ppgtt_kernel(state, audio.gpu, audio.phys, audio.bytes);
    let dst_ok = audio_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ok
        && direct_rcs_encode_cpp_audio_visualizer_rgba8_batch(
            state,
            upload,
            params,
            audio.bytes,
            dst.bytes,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let observed = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            CPP_AUDIO_VISUALIZER_POST_MARKER_SLOT,
            CPP_AUDIO_VISUALIZER_POST_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };

    if observed != CPP_AUDIO_VISUALIZER_POST_MARKER {
        if submitted {
            quarantine_direct_rcs_context("cpp-audio-visualizer-marker-timeout");
        }
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: cpp-audio-visualizer failed forcewake={} mapped={} ppgtt={} kernel={} audio={} dst={} batch={} submitted={} observed=0x{:08X} want=0x{:08X} extent={}x{} lanes={} artifact={} kernel_gpu=0x{:X} audio_gpu=0x{:X} dst_gpu=0x{:X}\n",
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ok as u8,
            audio_ok as u8,
            dst_ok as u8,
            batch_ok as u8,
            submitted as u8,
            observed,
            CPP_AUDIO_VISUALIZER_POST_MARKER,
            params.dst_width,
            params.dst_height,
            params.dst_width.div_ceil(2).saturating_mul(params.dst_height),
            CPP_AUDIO_VISUALIZER_RGBA8_ADLS_ARTIFACT.name,
            upload.gpu,
            audio.gpu,
            dst.gpu,
        );
        return DirectRcsDispatchOutcome {
            submitted,
            observed,
        };
    }

    DirectRcsDispatchOutcome {
        submitted,
        observed,
    }
}

/// Render one live post-mix PCM snapshot into an arbitrary UI4 RGBA8 frame.
///
/// The artifact is one C++ kernel and shades two horizontal pixels per lane,
/// making the walker count exactly half of a conventional full-pixel launch.
pub(crate) fn cpp_audio_visualizer_rgba8_surface_full(
    dst: GpgpuRgba8Surface,
    time_seconds: f32,
    frame: u32,
    snapshot: &crate::aud::audio_visualizer::AudioVisualizerFrame,
) -> GpgpuRgba8KernelResult {
    let start_tick = direct_rcs_now_tick();
    let Some(audio) = cpp_audio_visualizer_buffer_once() else {
        return GpgpuRgba8KernelResult::default();
    };
    cpp_audio_visualizer_write_snapshot(audio, snapshot);
    let params = CppAudioVisualizerRgba8Params {
        audio_gpu: audio.gpu,
        dst_gpu: dst.gpu,
        dst_pitch_bytes: dst.pitch_bytes,
        dst_width: dst.width,
        dst_height: dst.height,
        time_seconds,
        frame,
        flags: 1,
    };
    let outcome = submit_cpp_audio_visualizer_rgba8(dst, audio, params);
    let ok = outcome.observed == CPP_AUDIO_VISUALIZER_POST_MARKER;
    GpgpuRgba8KernelResult {
        ok,
        submitted: outcome.submitted,
        marker: outcome.observed,
        submit_ms: direct_rcs_elapsed_ms_since(start_tick),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}
