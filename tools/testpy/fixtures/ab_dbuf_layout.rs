#![allow(dead_code)]
// Model only device discovery and native-panel handoff. Allocations, cursor
// reservation and policy functions below are copied from production by runner.
#[derive(Clone, Copy)]
struct PipeInfo {
    slot: usize,
}
const PIPES: [PipeInfo; 4] = [
    PipeInfo { slot: 0 },
    PipeInfo { slot: 1 },
    PipeInfo { slot: 2 },
    PipeInfo { slot: 3 },
];
#[derive(Clone, Copy)]
struct State {
    device: Option<intel::Dev>,
    native: bool,
}
std::thread_local! {
    static STATE: std::cell::Cell<State> = const { std::cell::Cell::new(State {
        device: Some(intel::Dev { device_id: 0x4680 }), native: false,
    }) };
}
mod intel {
    // @CURSOR_RESERVATION@
    #[derive(Clone, Copy)]
    pub struct Dev {
        pub device_id: u16,
    }
    pub fn claimed_device() -> Option<Dev> {
        crate::STATE.with(|state| state.get().device)
    }
    pub mod tgl_native_panel {
        pub fn ui4_handoff_active() -> bool {
            crate::STATE.with(|state| state.get().native)
        }
    }
}
#[path = "@NATIVE_LAYOUT_PATH@"]
mod tgl_ui4_layout;
// @PRODUCTION_ITEMS@

fn configure(device: Option<u16>, native: bool) {
    STATE.with(|state| {
        state.set(State {
            device: device.map(|device_id| intel::Dev { device_id }),
            native,
        })
    });
}

#[test]
fn desktop_ab_allocations_are_disjoint_and_keep_both_cursor_reservations_free() {
    configure(Some(0x4680), false);
    assert!(uses_ab_dbuf_layout(intel::Dev { device_id: 0x4680 }));
    let expected_a = [
        (1024, 1228),
        (1229, 1433),
        (1434, 1638),
        (1639, 1843),
        (1844, 2031),
    ];
    let expected_b = [(0, 204), (205, 409), (410, 614), (615, 819), (820, 1007)];
    for slot in 0..5 {
        let a = plane_dbuf_range(PIPES[0], slot).unwrap();
        let b = plane_dbuf_range(PIPES[1], slot).unwrap();
        assert_eq!(a, expected_a[slot]);
        assert_eq!(b, expected_b[slot]);
        assert_eq!(a.1 - a.0, b.1 - b.0);
        for (start, end) in [a, b] {
            assert!(start <= end);
            for (cursor_start, cursor_end) in [(1008, 1023), (2032, 2047)] {
                assert!(end < cursor_start || start > cursor_end);
            }
        }
    }
    for a in expected_a {
        for b in expected_b {
            assert!(a.0 > b.1);
        }
    }
    for pair in expected_a.windows(2) {
        assert_eq!(pair[0].1 + 1, pair[1].0);
    }
    for pair in expected_b.windows(2) {
        assert_eq!(pair[0].1 + 1, pair[1].0);
    }
    assert_eq!(expected_a.iter().map(|(a, b)| b - a + 1).sum::<u16>(), 1008);
    assert_eq!(expected_b.iter().map(|(a, b)| b - a + 1).sum::<u16>(), 1008);
}

#[test]
fn desktop_a_reserves_s2_before_a_capture_route_exists() {
    configure(Some(0x4680), false);
    // There is deliberately no WD state in the allocator's dependencies:
    // allocation is established before the first B capture starts fetching.
    let before = core::array::from_fn::<_, 5, _>(|slot| plane_dbuf_range(PIPES[0], slot));
    assert_eq!(before[0], Some((1024, 1228)));
    assert_eq!(before[4], Some((1844, 2031)));
    assert_eq!(
        core::array::from_fn::<_, 5, _>(|slot| plane_dbuf_range(PIPES[0], slot)),
        before
    );
}

#[test]
fn native_tiger_lake_keeps_its_existing_two_slice_layout() {
    // The native experiment aliases its Dev ID to4680. Handoff still wins.
    configure(Some(0x4680), true);
    assert!(!uses_ab_dbuf_layout(intel::Dev { device_id: 0x4680 }));
    assert!(native_ui4_for_pipe(PIPES[0]));
    assert!(!native_ui4_for_pipe(PIPES[1]));
    for slot in 0..8 {
        assert_eq!(
            plane_dbuf_range(PIPES[0], slot),
            tgl_ui4_layout::dbuf_range(slot)
        );
    }
    assert_eq!(plane_dbuf_range(PIPES[0], 0), Some((0, 274)));
    assert_eq!(plane_dbuf_range(PIPES[0], 3), Some((1024, 1298)));
    assert_eq!(plane_dbuf_range(PIPES[1], 0), Some((0, 204)));
}

#[test]
fn other_devices_and_absent_claim_keep_the_legacy_s1_policy() {
    for device in [Some(0x9a49), Some(0xa780), Some(0x46d1), Some(0x1234), None] {
        configure(device, false);
        if let Some(device_id) = device {
            assert!(!uses_ab_dbuf_layout(intel::Dev { device_id }));
        }
        for pipe in PIPES {
            assert_eq!(plane_dbuf_range(pipe, 0), Some((0, 204)));
            assert_eq!(plane_dbuf_range(pipe, 4), Some((820, 1007)));
        }
    }
}

#[test]
fn invalid_plane_slots_never_alias_a_valid_dbuf_range() {
    for native in [false, true] {
        configure(Some(0x4680), native);
        for pipe in PIPES {
            for slot in [7, 8, 99, usize::MAX] {
                assert_eq!(plane_dbuf_range(pipe, slot), None);
            }
        }
        assert_eq!(plane_dbuf_range(PIPES[1], 5), None);
        assert_eq!(plane_dbuf_range(PIPES[1], 6), None);
        if !native {
            assert_eq!(plane_dbuf_range(PIPES[0], 5), None);
        }
    }
}
