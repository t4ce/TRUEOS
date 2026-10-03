//! The real table-population code exercised with explicit page-table fixtures.
#![allow(dead_code)]

mod memory {
    #[derive(Copy, Clone)]
    pub struct GuestPage(pub [u64; 512]);

    pub mod carrier {
        mod tables;

        const STACK: u64 = 0x400000;
        const COMM: u64 = 0x20400000;
        const BASE: u64 = 0x6000;
        const NX: u64 = 1 << 63;

        fn guest() -> [u64; 512] {
            core::array::from_fn(|index| 0x100000 + index as u64 * 4096 | 3 | NX)
        }

        fn build(root: &[u64; 512], pdpt: &[u64; 512], pd: &[u64; 512]) -> [super::GuestPage; 3] {
            let mut pages = [super::GuestPage([0; 512]); 3];
            tables::populate(
                &mut pages,
                BASE,
                root,
                &guest(),
                STACK,
                8 * 1024 * 1024,
                COMM,
                8192,
                |physical| match physical {
                    0x2000 => Ok(*pdpt),
                    0x3000 => Ok(*pd),
                    _ => panic!("unexpected source page {physical:x}"),
                },
            )
            .unwrap();
            pages
        }

        #[test]
        fn clone_keeps_all_other_host_branches_and_only_replaces_stack_comm() {
            let mut root = [0; 512];
            root[0] = 0x2003;
            root[256] = 0x9003;
            root[508] = 0xa003; // ordinary shared guarded-stack branch
            let mut pdpt = [0; 512];
            pdpt[0] = 0x3003;
            pdpt[17] = 0x12003;
            let pd = core::array::from_fn(|i| 0x40000000 + i as u64 * 0x200000 | 0x183);
            let root_before = root;
            let pdpt_before = pdpt;
            let pd_before = pd;
            let pages = build(&root, &pdpt, &pd);
            assert_eq!(pages[0].0[0], 0x7003);
            assert_eq!(pages[0].0[256..], root[256..]);
            assert_eq!(pages[1].0[0], 0x8003);
            assert_eq!(pages[1].0[1..], pdpt[1..]);
            for index in 0..512 {
                let overwritten = (2..6).contains(&index) || index == 258;
                assert_eq!(
                    pages[2].0[index],
                    if overwritten {
                        guest()[index]
                    } else {
                        pd[index]
                    }
                );
            }
            assert_eq!(root, root_before);
            assert_eq!(pdpt, pdpt_before);
            assert_eq!(pd, pd_before);
        }

        #[test]
        fn empty_host_low_branch_is_created_without_reading_physical_zero() {
            let mut pages = [super::GuestPage([0; 512]); 3];
            tables::populate(
                &mut pages,
                BASE,
                &[0; 512],
                &guest(),
                STACK,
                8 * 1024 * 1024,
                COMM,
                8192,
                |_| panic!("absent branch read"),
            )
            .unwrap();
            assert_eq!(pages[2].0[1], 0);
            assert_eq!(pages[2].0[2], guest()[2]);
            assert_eq!(pages[2].0[258], guest()[258]);
        }

        #[test]
        fn one_gib_leaf_is_split_preserving_pat_global_and_permissions() {
            let mut root = [0; 512];
            root[0] = 0x2003;
            let mut pdpt = [0; 512];
            pdpt[0] = 0x80000000 | 0x1183 | NX;
            let pages = build(&root, &pdpt, &[0; 512]);
            assert_eq!(pages[2].0[0], 0x80000000 | 0x1183 | NX);
            assert_eq!(pages[2].0[511], 0xbfe00000 | 0x1183 | NX);
            assert_eq!(pages[2].0[2], guest()[2]);
        }

        #[test]
        fn replacing_parents_preserves_unrelated_effective_nx_readonly() {
            let mut root = [0; 512];
            root[0] = 0x2001 | NX;
            let mut pdpt = [0; 512];
            pdpt[0] = 0x3003;
            pdpt[17] = 0x12003;
            let pd = [0x90183; 512];
            let pages = build(&root, &pdpt, &pd);
            assert_eq!(pages[1].0[17], 0x12001 | NX);
            assert_eq!(pages[2].0[1], 0x90181 | NX);
            assert_eq!(pages[2].0[2], guest()[2]);
        }

        #[test]
        fn missing_guest_branch_and_unrepresentable_geometry_reject() {
            let mut pages = [super::GuestPage([0; 512]); 3];
            assert_eq!(
                tables::populate(
                    &mut pages,
                    BASE,
                    &[0; 512],
                    &[0; 512],
                    STACK,
                    8 * 1024 * 1024,
                    COMM,
                    8192,
                    |_| unreachable!()
                ),
                Err("thread carrier guest branch absent")
            );
            assert_eq!(
                tables::populate(
                    &mut pages,
                    BASE,
                    &[0; 512],
                    &guest(),
                    STACK,
                    1 << 30,
                    COMM,
                    8192,
                    |_| unreachable!()
                ),
                Err("thread carrier table geometry")
            );
        }
    }
}
