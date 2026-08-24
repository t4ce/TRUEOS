/// Exact physical UI4 producer description consumed by the HelioC plan
/// preflight. It deliberately has no tenant GPU address field.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct HelioCloudSurfaceDesc {
    pub(crate) phys: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct HelioCloudDispatchPlan {
    pub(crate) source_volume_gpu: u64,
    pub(crate) destination_volume_gpu: u64,
    pub(crate) sim_params_gpu: u64,
    pub(crate) render_params_gpu: u64,
    pub(crate) dispatch_groups: [u32; 3],
}

/// Immutable lowering result for one bounded Cloud frame. The native encoder
/// may consume this plan after the broker lock is dropped; this type performs
/// no mapping, submission, telemetry, or allocation.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct HelioCloudFramePlan {
    pub(crate) dispatches: [HelioCloudDispatchPlan; 2],
    pub(crate) dispatch_count: u8,
    pub(crate) final_volume_gpu: u64,
    pub(crate) surface: HelioCloudSurfaceDesc,
}

const HELIOC_SIM_PARAMS_GPU: u64 = 0x0900_0000;
const HELIOC_RENDER_PARAMS_GPU: u64 = 0x0901_0000;
const HELIOC_DISPATCH_GROUPS: [u32; 3] = [24, 12, 24];

const HELIOC_PAGE_BYTES: usize = 4096;
const HELIOC_VOLUME_PAGE_COUNT: usize = HELIOC_VOLUME_RGBA16F_BYTES / HELIOC_PAGE_BYTES;
const HELIOC_PARAM_PAGE_COUNT: usize = 1;

fn helioc_exact_pages_valid(pages: &[u64], expected_count: usize) -> bool {
    pages.len() == expected_count
        && pages
            .iter()
            .all(|phys| *phys != 0 && phys.is_multiple_of(HELIOC_PAGE_BYTES as u64))
}

fn helioc_pages_are_disjoint(resources: [&[u64]; 4]) -> bool {
    for (index, pages) in resources.iter().enumerate() {
        for page in pages.iter().copied() {
            if resources[..index].iter().any(|prior| prior.contains(&page)) {
                return false;
            }
        }
        for (offset, page) in pages.iter().copied().enumerate() {
            if pages[offset + 1..].contains(&page) {
                return false;
            }
        }
    }
    true
}

fn helioc_surface_valid(surface: HelioCloudSurfaceDesc) -> bool {
    if surface.phys == 0
        || !surface.phys.is_multiple_of(HELIOC_PAGE_BYTES as u64)
        || surface.bytes == 0
        || !surface.bytes.is_multiple_of(HELIOC_PAGE_BYTES)
        || surface.width == 0
        || surface.height == 0
        || surface.pitch_bytes < surface.width.saturating_mul(4)
    {
        return false;
    }
    let Some(last_row) = (surface.height as usize - 1)
        .checked_mul(surface.pitch_bytes as usize)
    else {
        return false;
    };
    let Some(row_bytes) = (surface.width as usize).checked_mul(4) else {
        return false;
    };
    last_row
        .checked_add(row_bytes)
        .is_some_and(|extent| extent <= surface.bytes)
}

const fn helioc_volume_gpu(selector: u8) -> u64 {
    if selector == 0 {
        HELIOC_RCS_GPU_VA_VOLUME_A_BASE
    } else {
        HELIOC_RCS_GPU_VA_VOLUME_B_BASE
    }
}

/// Validate exact Cloud backing and derive the fixed Helio VA ping-pong
/// sequence. `current_volume` is broker-owned state; no tenant GPU VA is
/// accepted or copied into the result.
pub(crate) fn plan_helioc_frame(
    volume_a_pages: &[u64],
    volume_b_pages: &[u64],
    sim_params_pages: &[u64],
    render_params_pages: &[u64],
    surface: HelioCloudSurfaceDesc,
    simulation_steps: u32,
    current_volume: u8,
) -> Option<HelioCloudFramePlan> {
    if simulation_steps > 2 || current_volume > 1 {
        return None;
    }
    if !helioc_exact_pages_valid(volume_a_pages, HELIOC_VOLUME_PAGE_COUNT)
        || !helioc_exact_pages_valid(volume_b_pages, HELIOC_VOLUME_PAGE_COUNT)
        || !helioc_exact_pages_valid(sim_params_pages, HELIOC_PARAM_PAGE_COUNT)
        || !helioc_exact_pages_valid(render_params_pages, HELIOC_PARAM_PAGE_COUNT)
        || !helioc_pages_are_disjoint([
            volume_a_pages,
            volume_b_pages,
            sim_params_pages,
            render_params_pages,
        ])
        || !helioc_surface_valid(surface)
    {
        return None;
    }

    let first_source = helioc_volume_gpu(current_volume);
    let first_destination = helioc_volume_gpu(current_volume ^ 1);
    let second = HelioCloudDispatchPlan {
        source_volume_gpu: first_destination,
        destination_volume_gpu: first_source,
        sim_params_gpu: HELIOC_SIM_PARAMS_GPU,
        render_params_gpu: HELIOC_RENDER_PARAMS_GPU,
        dispatch_groups: HELIOC_DISPATCH_GROUPS,
    };
    let first = HelioCloudDispatchPlan {
        source_volume_gpu: first_source,
        destination_volume_gpu: first_destination,
        sim_params_gpu: HELIOC_SIM_PARAMS_GPU,
        render_params_gpu: HELIOC_RENDER_PARAMS_GPU,
        dispatch_groups: HELIOC_DISPATCH_GROUPS,
    };
    let dispatches = if simulation_steps == 2 {
        [first, second]
    } else {
        [first, first]
    };
    let final_volume_gpu = if simulation_steps & 1 == 0 {
        first_source
    } else {
        first_destination
    };
    Some(HelioCloudFramePlan {
        dispatches,
        dispatch_count: simulation_steps as u8,
        final_volume_gpu,
        surface,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: u64 = 0x1000;
    const VOLUME_PAGE: u64 = 0x20_0000;

    fn pages(count: usize, base: u64) -> [u64; HELIOC_VOLUME_PAGE_COUNT] {
        let mut result = [0; HELIOC_VOLUME_PAGE_COUNT];
        assert_eq!(count, result.len());
        for (index, page) in result.iter_mut().enumerate() {
            *page = base + index as u64 * PAGE;
        }
        result
    }

    fn surface() -> HelioCloudSurfaceDesc {
        HelioCloudSurfaceDesc {
            phys: 0x40_0000,
            bytes: 1920 * 1080 * 4,
            width: 1920,
            height: 1080,
            pitch_bytes: 1920 * 4,
        }
    }

    fn plan(steps: u32, selector: u8) -> HelioCloudFramePlan {
        let a = pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE);
        let b = pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE * 2);
        plan_helioc_frame(&a, &b, &[PAGE], &[PAGE * 2], surface(), steps, selector).unwrap()
    }

    #[test]
    fn zero_steps_samples_selected_volume_without_dispatch() {
        let plan = plan(0, 1);
        assert_eq!(plan.dispatch_count, 0);
        assert_eq!(plan.final_volume_gpu, HELIOC_RCS_GPU_VA_VOLUME_B_BASE);
    }

    #[test]
    fn one_step_flips_once() {
        let plan = plan(1, 0);
        assert_eq!(plan.dispatch_count, 1);
        assert_eq!(
            plan.dispatches[0],
            HelioCloudDispatchPlan {
                source_volume_gpu: HELIOC_RCS_GPU_VA_VOLUME_A_BASE,
                destination_volume_gpu: HELIOC_RCS_GPU_VA_VOLUME_B_BASE,
                sim_params_gpu: HELIOC_SIM_PARAMS_GPU,
                render_params_gpu: HELIOC_RENDER_PARAMS_GPU,
                dispatch_groups: HELIOC_DISPATCH_GROUPS,
            }
        );
        assert_eq!(plan.final_volume_gpu, HELIOC_RCS_GPU_VA_VOLUME_B_BASE);
    }

    #[test]
    fn two_steps_returns_to_original_volume() {
        let plan = plan(2, 1);
        assert_eq!(plan.dispatch_count, 2);
        assert_eq!(plan.dispatches[0].source_volume_gpu, HELIOC_RCS_GPU_VA_VOLUME_B_BASE);
        assert_eq!(plan.dispatches[1].destination_volume_gpu, HELIOC_RCS_GPU_VA_VOLUME_B_BASE);
        assert_eq!(plan.final_volume_gpu, HELIOC_RCS_GPU_VA_VOLUME_B_BASE);
    }

    #[test]
    fn exact_shapes_alignment_and_surface_are_fail_closed() {
        let a = pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE);
        let b = pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE * 2);
        assert!(plan_helioc_frame(&a, &b, &[PAGE], &[PAGE * 2], surface(), 2, 0).is_some());
        assert!(plan_helioc_frame(&a[..a.len() - 1], &b, &[PAGE], &[PAGE * 2], surface(), 0, 0).is_none());
        assert!(plan_helioc_frame(&a, &b, &[PAGE + 1], &[PAGE * 2], surface(), 0, 0).is_none());
        assert!(plan_helioc_frame(&a, &b, &[PAGE], &[PAGE * 2], surface(), 3, 0).is_none());
        assert!(plan_helioc_frame(&a, &b, &[PAGE], &[PAGE * 2], surface(), 0, 2).is_none());
        let mut bad_surface = surface();
        bad_surface.pitch_bytes -= 4;
        assert!(plan_helioc_frame(&a, &b, &[PAGE], &[PAGE * 2], bad_surface, 0, 0).is_none());
        bad_surface = surface();
        bad_surface.phys += 1;
        assert!(plan_helioc_frame(&a, &b, &[PAGE], &[PAGE * 2], bad_surface, 0, 0).is_none());
    }

    #[test]
    fn rejects_physical_page_aliases_across_cloud_resources() {
        let a = pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE);
        let mut b = pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE * 2);
        b[0] = a[0];
        assert!(plan_helioc_frame(&a, &b, &[PAGE], &[PAGE * 2], surface(), 0, 0).is_none());
        assert!(
            plan_helioc_frame(
                &a,
                &pages(HELIOC_VOLUME_PAGE_COUNT, VOLUME_PAGE * 2),
                &[a[1]],
                &[PAGE * 2],
                surface(),
                0,
                0
            )
            .is_none()
        );
    }

    #[test]
    fn every_dispatch_uses_fixed_parameters_and_groups() {
        let frame = plan(2, 0);
        for dispatch in frame.dispatches {
            assert_eq!(dispatch.sim_params_gpu, HELIOC_SIM_PARAMS_GPU);
            assert_eq!(dispatch.render_params_gpu, HELIOC_RENDER_PARAMS_GPU);
            assert_eq!(dispatch.dispatch_groups, HELIOC_DISPATCH_GROUPS);
        }
    }
}
