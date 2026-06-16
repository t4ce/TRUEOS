#![allow(dead_code)]

use super::Canvas3dVec3Q16;

pub(crate) const CORNER_COUNT: usize = 30;
pub(crate) const EDGE_COUNT: usize = 60;
pub(crate) const TRIANGLE_COUNT: usize = 20;
pub(crate) const PENTAGON_COUNT: usize = 12;
pub(crate) const FACE_COUNT: usize = TRIANGLE_COUNT + PENTAGON_COUNT;
pub(crate) const VERTEX_COUNT: usize = CORNER_COUNT + EDGE_COUNT;

pub(crate) const TRIANGLES: [(usize, usize, usize); TRIANGLE_COUNT] = [
    (0, 1, 3),
    (0, 2, 4),
    (1, 5, 7),
    (2, 6, 9),
    (3, 21, 23),
    (4, 22, 25),
    (5, 6, 8),
    (7, 10, 11),
    (8, 14, 15),
    (9, 12, 13),
    (10, 14, 16),
    (11, 23, 26),
    (12, 15, 17),
    (13, 25, 29),
    (16, 18, 20),
    (17, 19, 20),
    (18, 26, 27),
    (19, 28, 29),
    (21, 22, 24),
    (24, 27, 28),
];

pub(crate) const PENTAGONS: [(usize, usize, usize, usize, usize); PENTAGON_COUNT] = [
    (0, 1, 5, 6, 2),
    (0, 3, 21, 22, 4),
    (1, 3, 23, 11, 7),
    (2, 4, 25, 13, 9),
    (5, 7, 10, 14, 8),
    (6, 8, 15, 12, 9),
    (10, 11, 26, 18, 16),
    (12, 13, 29, 19, 17),
    (14, 15, 17, 20, 16),
    (18, 20, 19, 28, 27),
    (21, 23, 26, 27, 24),
    (22, 24, 28, 29, 25),
];

const CORNERS_Q16: [Canvas3dVec3Q16; CORNER_COUNT] = [
    Canvas3dVec3Q16 {
        x: 0,
        y: 0,
        z: 32768,
        pad: 0,
    },
    Canvas3dVec3Q16 {
        x: 16384,
        y: 10126,
        z: 26510,
        pad: 1,
    },
    Canvas3dVec3Q16 {
        x: -16384,
        y: 10126,
        z: 26510,
        pad: 2,
    },
    Canvas3dVec3Q16 {
        x: 16384,
        y: -10126,
        z: 26510,
        pad: 3,
    },
    Canvas3dVec3Q16 {
        x: -16384,
        y: -10126,
        z: 26510,
        pad: 4,
    },
    Canvas3dVec3Q16 {
        x: 10126,
        y: 26510,
        z: 16384,
        pad: 5,
    },
    Canvas3dVec3Q16 {
        x: -10126,
        y: 26510,
        z: 16384,
        pad: 6,
    },
    Canvas3dVec3Q16 {
        x: 26510,
        y: 16384,
        z: 10126,
        pad: 7,
    },
    Canvas3dVec3Q16 {
        x: 0,
        y: 32768,
        z: 0,
        pad: 8,
    },
    Canvas3dVec3Q16 {
        x: -26510,
        y: 16384,
        z: 10126,
        pad: 9,
    },
    Canvas3dVec3Q16 {
        x: 26510,
        y: 16384,
        z: -10126,
        pad: 10,
    },
    Canvas3dVec3Q16 {
        x: 32768,
        y: 0,
        z: 0,
        pad: 11,
    },
    Canvas3dVec3Q16 {
        x: -26510,
        y: 16384,
        z: -10126,
        pad: 12,
    },
    Canvas3dVec3Q16 {
        x: -32768,
        y: 0,
        z: 0,
        pad: 13,
    },
    Canvas3dVec3Q16 {
        x: 10126,
        y: 26510,
        z: -16384,
        pad: 14,
    },
    Canvas3dVec3Q16 {
        x: -10126,
        y: 26510,
        z: -16384,
        pad: 15,
    },
    Canvas3dVec3Q16 {
        x: 16384,
        y: 10126,
        z: -26510,
        pad: 16,
    },
    Canvas3dVec3Q16 {
        x: -16384,
        y: 10126,
        z: -26510,
        pad: 17,
    },
    Canvas3dVec3Q16 {
        x: 16384,
        y: -10126,
        z: -26510,
        pad: 18,
    },
    Canvas3dVec3Q16 {
        x: -16384,
        y: -10126,
        z: -26510,
        pad: 19,
    },
    Canvas3dVec3Q16 {
        x: 0,
        y: 0,
        z: -32768,
        pad: 20,
    },
    Canvas3dVec3Q16 {
        x: 10126,
        y: -26510,
        z: 16384,
        pad: 21,
    },
    Canvas3dVec3Q16 {
        x: -10126,
        y: -26510,
        z: 16384,
        pad: 22,
    },
    Canvas3dVec3Q16 {
        x: 26510,
        y: -16384,
        z: 10126,
        pad: 23,
    },
    Canvas3dVec3Q16 {
        x: 0,
        y: -32768,
        z: 0,
        pad: 24,
    },
    Canvas3dVec3Q16 {
        x: -26510,
        y: -16384,
        z: 10126,
        pad: 25,
    },
    Canvas3dVec3Q16 {
        x: 26510,
        y: -16384,
        z: -10126,
        pad: 26,
    },
    Canvas3dVec3Q16 {
        x: 10126,
        y: -26510,
        z: -16384,
        pad: 27,
    },
    Canvas3dVec3Q16 {
        x: -10126,
        y: -26510,
        z: -16384,
        pad: 28,
    },
    Canvas3dVec3Q16 {
        x: -26510,
        y: -16384,
        z: -10126,
        pad: 29,
    },
];

const EDGES: [(usize, usize); EDGE_COUNT] = [
    (0, 1),
    (0, 2),
    (0, 3),
    (0, 4),
    (5, 6),
    (5, 1),
    (5, 7),
    (5, 8),
    (6, 2),
    (6, 9),
    (6, 8),
    (1, 3),
    (1, 7),
    (2, 4),
    (2, 9),
    (7, 10),
    (7, 11),
    (9, 12),
    (9, 13),
    (14, 15),
    (14, 16),
    (14, 10),
    (14, 8),
    (15, 17),
    (15, 12),
    (15, 8),
    (16, 18),
    (16, 10),
    (17, 19),
    (17, 12),
    (20, 16),
    (20, 17),
    (20, 18),
    (20, 19),
    (21, 22),
    (21, 3),
    (21, 23),
    (21, 24),
    (22, 4),
    (22, 25),
    (22, 24),
    (3, 23),
    (4, 25),
    (23, 26),
    (26, 11),
    (10, 11),
    (27, 28),
    (27, 18),
    (27, 26),
    (27, 24),
    (28, 19),
    (28, 29),
    (28, 24),
    (18, 26),
    (19, 29),
    (25, 29),
    (25, 13),
    (23, 11),
    (12, 13),
    (29, 13),
];

pub(crate) fn write_vertices(dst: *mut Canvas3dVec3Q16, seed_scale: i32) {
    unsafe {
        for (index, vertex) in CORNERS_Q16.iter().copied().enumerate() {
            let mut vertex = scale_seed_vertex(vertex, seed_scale);
            vertex.pad = index as i32;
            core::ptr::write_volatile(dst.add(index), vertex);
        }
        for (edge_index, &(a, b)) in EDGES.iter().enumerate() {
            let va = scale_seed_vertex(CORNERS_Q16[a], seed_scale);
            let vb = scale_seed_vertex(CORNERS_Q16[b], seed_scale);
            let out_index = CORNER_COUNT + edge_index;
            core::ptr::write_volatile(
                dst.add(out_index),
                Canvas3dVec3Q16 {
                    x: va.x + ((vb.x - va.x) / 2),
                    y: va.y + ((vb.y - va.y) / 2),
                    z: va.z + ((vb.z - va.z) / 2),
                    pad: out_index as i32,
                },
            );
        }
    }
}

pub(crate) fn corner(index: usize, seed_scale: i32) -> Canvas3dVec3Q16 {
    if index >= CORNER_COUNT {
        return Canvas3dVec3Q16::default();
    }
    scale_seed_vertex(CORNERS_Q16[index], seed_scale)
}

pub(crate) fn present_color(vertex_index: usize) -> u32 {
    if vertex_index < CORNER_COUNT {
        const CORNER_COLORS: [u32; 6] = [
            0xFFFF_4FD8,
            0xFFFF_D050,
            0xFF53_FFD2,
            0xFF60_9CFF,
            0xFFE8_7CFF,
            0xFFFFFFFF,
        ];
        CORNER_COLORS[vertex_index % CORNER_COLORS.len()]
    } else {
        0xFF6C_B7FF
    }
}

fn scale_seed_vertex(vertex: Canvas3dVec3Q16, seed_scale: i32) -> Canvas3dVec3Q16 {
    Canvas3dVec3Q16 {
        x: vertex.x.saturating_mul(seed_scale),
        y: vertex.y.saturating_mul(seed_scale),
        z: vertex.z.saturating_mul(seed_scale),
        pad: vertex.pad,
    }
}
