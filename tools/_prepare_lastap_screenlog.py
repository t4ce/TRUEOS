#!/usr/bin/env python3
"""One-shot source wiring; removed from the final PR tree after validation."""
from pathlib import Path
import re
ROOT = Path(__file__).resolve().parents[1]
paths = ['src/log_os.rs', 'src/workers.rs', 'src/r/services/mod.rs',
         'src/r/services/spawn_service.rs', 'src/intel/tgl_native_panel.rs', 'src/intel/display.rs']
sources = {p: (ROOT / p).read_text() for p in paths}

def once(source, old, new):
    assert source.count(old) == 1, (old, source.count(old))
    return source.replace(old, new, 1)

def prepend(source, name, body):
    hits = list(re.finditer(r'\bfn ' + re.escape(name) + r'\(', source))
    assert len(hits) == 1, name
    opening = source.index('{\n', hits[0].start()) + 2
    return source[:opening] + body + source[opening:]

p = 'src/log_os.rs'
s = sources[p]
s = once(s, '''        head: usize,
        len: usize,
    }

    impl TcpLogRing''', '''        head: usize,
        len: usize,
        // Independent of TCP's unread length: the screen is a second reader.
        written: u64,
    }

    impl TcpLogRing''')
s = once(s, '''                head: 0,
                len: 0,
            }
        }

        #[inline]
        fn write_bytes''', '''                head: 0,
                len: 0,
                written: 0,
            }
        }

        #[inline]
        fn write_bytes''')
s = once(s, '''        fn write_bytes(&mut self, bytes: &[u8]) {
''', '''        fn write_bytes(&mut self, bytes: &[u8]) {
            self.written = self.written.saturating_add(bytes.len() as u64);
''')
s = once(s, '''    }

    static RING: Mutex<TcpLogRing>''', '''        /// Copy retained bytes without advancing TCP's destructive drain.
        /// Even TCP-drained bytes remain readable until the writer wraps.
        fn copy_since(&self, cursor: &mut u64, out: &mut [u8]) -> (usize, u64) {
            let oldest = self.written.saturating_sub(MAX_BYTES as u64);
            let lost = oldest.saturating_sub(*cursor);
            let start_seq = (*cursor).max(oldest).min(self.written);
            let behind = (self.written - start_seq) as usize;
            let take = out.len().min(behind);
            let start = (self.head + MAX_BYTES - behind) % MAX_BYTES;
            let first = take.min(MAX_BYTES - start);
            out[..first].copy_from_slice(&self.buf[start..start + first]);
            out[first..take].copy_from_slice(&self.buf[..take - first]);
            *cursor = start_seq + take as u64;
            (take, lost)
        }
    }

    static RING: Mutex<TcpLogRing>''')
s = once(s, '''    #[trueos_executor::task]
    pub async fn logtotcp_task()''', '''    /// Short nonblocking snapshot for the last-AP screen service. Formatting,
    /// acceptance and TCP draining are unchanged; drawing happens after unlock.
    pub(crate) fn copy_for_screen(cursor: &mut u64, out: &mut [u8]) -> Option<(usize, u64)> {
        let ring = RING.try_lock()?;
        Some(ring.copy_since(cursor, out))
    }

    #[trueos_executor::task]
    pub async fn logtotcp_task()''')
sources[p] = s

p = 'src/workers.rs'
s = sources[p]
s = once(s, '''// Media capture/encode now use the ordinary BSP executor. Every background
// topology slot remains available to VM hulls and general worker lanes.''', '''// Media capture/encode stay on BSP. The laptop's CPU screen-log profile
// reuses the former LastAP isolation policy, enabled before AP registration.''')
s = once(s, '/// AP media reservation.', '/// AP diagnostic-service reservation.')
s = once(s, '''pub fn is_general_background_worker_slot(cpu_slot: u32) -> bool {
    is_background_worker_slot(cpu_slot)
}''', '''/// The former RDP encoder policy: topology identity, never registration order.
pub fn last_ap_service_slot() -> Option<u32> {
    if !crate::r::services::microfont_log_service::enabled() {
        return None;
    }
    let slot = topology_core_slot_count().checked_sub(1)?;
    if slot < FIRST_BACKGROUND_SLOT as usize || slot >= WORKER_SLOT_LIMIT {
        return None;
    }
    Some(slot as u32)
}

pub fn is_last_ap_service_slot(cpu_slot: u32) -> bool {
    last_ap_service_slot() == Some(cpu_slot)
}

pub fn last_ap_service_worker() -> Option<(u32, u8, WorkerSpawner)> {
    let slot = last_ap_service_slot()?;
    Some((slot, core_kind_for_slot(slot), spawner_for_slot(slot)?))
}

pub fn is_general_background_worker_slot(cpu_slot: u32) -> bool {
    is_background_worker_slot(cpu_slot) && !is_last_ap_service_slot(cpu_slot)
}''')
s = once(s, '''        let background = topology_slots.saturating_sub(first_app_slot as usize);
        return background.max(1);''', '''        let background = topology_slots.saturating_sub(first_app_slot as usize);
        let reserved = usize::from(
            last_ap_service_slot().is_some_and(|slot| slot as usize >= first_app_slot as usize),
        );
        return background.saturating_sub(reserved).max(1);''')
s = once(s, '''    (first_app_slot..registered_slot_end().max(first_app_slot))
        .filter(|slot| is_slot_registered(*slot))''', '''    (first_app_slot..registered_slot_end().max(first_app_slot))
        .filter(|slot| is_slot_registered(*slot) && !is_last_ap_service_slot(*slot))''')
sources[p] = s

p = 'src/r/services/mod.rs'
sources[p] = once(sources[p], 'pub mod media_service;\n', 'pub mod media_service;\npub(crate) mod microfont_log_service;\n')

p = 'src/r/services/spawn_service.rs'
s = sources[p]
s = once(s, '    LOGTOTCP_STARTED,\n', '    LOGTOTCP_STARTED,\n    MICROFONT_LOG_STARTED,\n')
s = once(s, 'fn spawn_logtotcp(spawner: Spawner) -> SpawnAttempt {', '''fn microfont_log_gate() -> bool {
    super::microfont_log_service::enabled()
        && crate::workers::last_ap_service_worker().is_some()
}

fn spawn_microfont_log(_spawner: Spawner) -> SpawnAttempt {
    let Some((slot, _kind, lastap_spawner)) = crate::workers::last_ap_service_worker() else {
        return SpawnAttempt::Skipped;
    };
    match super::microfont_log_service::microfont_log_task(slot) {
        Ok(token) => {
            lastap_spawner.spawn(token);
            SpawnAttempt::Spawned
        }
        Err(error) => SpawnAttempt::Failed(error),
    }
}

fn ui4_slot4_service_gate() -> bool {
    ui4_compositor_gate()
        && !super::microfont_log_service::owns_plane(0, crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT)
}

fn spawn_logtotcp(spawner: Spawner) -> SpawnAttempt {''')
s = once(s, 'const TASK_COUNT: usize = 76\n', 'const TASK_COUNT: usize = 77\n')
s = once(s, 'static TASKS: [TaskSpec; TASK_COUNT] = [\n', '''static TASKS: [TaskSpec; TASK_COUNT] = [
    // Only the retained surface and exact LastAP executor are required.
    // In particular this has no NET, filesystem, GuC or UI4-ready dependency.
    TaskSpec::enabled_gated(
        "microfont-log", 0, microfont_log_gate, &MICROFONT_LOG_STARTED, spawn_microfont_log,
    ),
''')
s = once(s, '''        "ui4-slot4-service",
        0,
        ui4_compositor_gate,''', '''        "ui4-slot4-service",
        0,
        ui4_slot4_service_gate,''')
sources[p] = s

p = 'src/intel/tgl_native_panel.rs'
s = sources[p]
s = once(s, '''    if readback_ok {
        // PIPE_SRC now describes the actual native panel.''', '''    if readback_ok {
        // Reuse the proven allocation for a transparent CPU log layer. This
        // publishes LastAP's reservation before secondary CPUs are started.
        // Only the service writes these permanently retained pages afterwards.
        let _ = unsafe {
            crate::r::services::microfont_log_service::install(
                crate::r::services::microfont_log_service::Surface {
                    phys: frame.phys, gpu: SURFACE_GPU, virt: frame.virt,
                    byte_len: FRAME_BYTES, width: WIDTH, height: HEIGHT,
                    pitch_bytes: PITCH_BYTES,
                },
            )
        };
        // PIPE_SRC now describes the actual native panel.''')
sources[p] = s

p = 'src/intel/display.rs'
s = sources[p]
s = once(s, 'use crate::intel::types::Rgba8;\n', 'use crate::intel::types::Rgba8;\nuse crate::r::services::microfont_log_service as screenlog;\n')
s = once(s, 'fn bootstrap_ui4_rgba8_plane_stack_once(dev: crate::intel::Dev, primary: PrimarySurface) -> bool {', '''fn microfont_log_boot_surface(pipe: PipeInfo, slot: usize, width: u32, height: u32) -> Option<OverlaySurface> {
    if !screenlog::owns_plane(pipe.slot, slot) { return None; }
    let s = screenlog::surface()?;
    if s.width != width || s.height != height { return None; }
    Some(OverlaySurface {
        width, height, pitch_bytes: s.pitch_bytes, byte_len: s.byte_len,
        phys: s.phys, virt: s.virt as *mut u8, gpu: s.gpu,
        pipe, plane_slot: slot, buffer_index: 0,
    })
}

fn bootstrap_ui4_rgba8_plane_stack_once(dev: crate::intel::Dev, primary: PrimarySurface) -> bool {''')
s = once(s, '''            ensure_overlay_surface_for_pipe(dev, pipe, slot, primary.width, primary.height)
        else {''', '''            microfont_log_boot_surface(pipe, slot, primary.width, primary.height)
                .or_else(|| ensure_overlay_surface_for_pipe(dev, pipe, slot, primary.width, primary.height))
        else {''')
s = once(s, '''    for surface in overlay_surfaces.iter().flatten().copied() {
        mark_overlay_surface_front(surface);
    }''', '''    for surface in overlay_surfaces.iter().flatten().copied() {
        if !screenlog::owns_plane(surface.pipe.slot, surface.plane_slot) {
            mark_overlay_surface_front(surface);
        }
    }''')
s = prepend(s, 'ensure_overlay_surface_for_pipe', '''    // The service front is not a UI4 pool allocation, even after it is full.
    if screenlog::owns_plane(pipe.slot, plane_slot) { return None; }
''')
s = prepend(s, 'queue_ui4_plane_surface_flip', '''    if screenlog::owns_plane(0, crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT)
        && plane_base == PIPES[0].plane(crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT).base()
    {
        return PlaneSurfaceFlipQueueResult::Rejected;
    }
''')
sources[p] = s

for path, source in sources.items():
    (ROOT / path).write_text(source)
    print('updated', path, flush=True)
