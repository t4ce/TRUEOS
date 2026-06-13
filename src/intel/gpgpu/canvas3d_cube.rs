use super::{CANVAS3D_PROJECT_Q16_ONE, Canvas3dVec3Q16};

pub(crate) const CUBE_CORNER_COUNT: usize = 8;
pub(crate) const CUBE_EDGE_COUNT: usize = 12;
pub(crate) const CUBE_EDGE_SAMPLE_COUNT: usize = 1;
pub(crate) const CUBE_VERTEX_COUNT: usize =
    CUBE_CORNER_COUNT + CUBE_EDGE_COUNT * CUBE_EDGE_SAMPLE_COUNT;
pub(crate) const CUBE_INSTANCE_COUNT: usize = 1;
pub(crate) const TETRA_CORNER_COUNT: usize = 4;
pub(crate) const TETRA_EDGE_COUNT: usize = 6;
pub(crate) const TETRA_EDGE_SAMPLE_COUNT: usize = 1;
pub(crate) const TETRA_VERTEX_COUNT: usize =
    TETRA_CORNER_COUNT + TETRA_EDGE_COUNT * TETRA_EDGE_SAMPLE_COUNT;
pub(crate) const SEED_SCALE: i32 = 2;
pub(crate) const PRESENT_COLORS: [u32; CUBE_INSTANCE_COUNT] = [0xFFFF_3048];

const CUBE_EDGES: [(usize, usize); CUBE_EDGE_COUNT] = [
    (0, 1),
    (1, 3),
    (3, 2),
    (2, 0),
    (4, 5),
    (5, 7),
    (7, 6),
    (6, 4),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

const TETRA_EDGES: [(usize, usize); TETRA_EDGE_COUNT] =
    [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];

pub(crate) fn cube_translate_x_q16(frame: u32, half_q16: i32) -> i32 {
    translate_ping_pong_q16(frame, half_q16)
}

pub(crate) fn tetra_translate_y_q16(frame: u32, half_q16: i32) -> i32 {
    translate_ping_pong_q16(frame, half_q16)
}

pub(crate) fn cube_vertex(
    cube_index: usize,
    local_index: usize,
    seed_half_q16: i32,
) -> Canvas3dVec3Q16 {
    let half = seed_half_q16;
    let corners = [
        Canvas3dVec3Q16 {
            x: -half,
            y: -half,
            z: -half,
            pad: 0,
        },
        Canvas3dVec3Q16 {
            x: half,
            y: -half,
            z: -half,
            pad: 1,
        },
        Canvas3dVec3Q16 {
            x: -half,
            y: half,
            z: -half,
            pad: 2,
        },
        Canvas3dVec3Q16 {
            x: half,
            y: half,
            z: -half,
            pad: 3,
        },
        Canvas3dVec3Q16 {
            x: -half,
            y: -half,
            z: half,
            pad: 4,
        },
        Canvas3dVec3Q16 {
            x: half,
            y: -half,
            z: half,
            pad: 5,
        },
        Canvas3dVec3Q16 {
            x: -half,
            y: half,
            z: half,
            pad: 6,
        },
        Canvas3dVec3Q16 {
            x: half,
            y: half,
            z: half,
            pad: 7,
        },
    ];
    let mut vertex = if local_index < CUBE_CORNER_COUNT {
        corners[local_index]
    } else {
        let edge_sample = local_index - CUBE_CORNER_COUNT;
        let edge_index = edge_sample / CUBE_EDGE_SAMPLE_COUNT;
        let sample_index = edge_sample % CUBE_EDGE_SAMPLE_COUNT;
        let (a, b) = CUBE_EDGES[edge_index];
        let va = corners[a];
        let vb = corners[b];
        let step = (sample_index + 1) as i32;
        let denom = (CUBE_EDGE_SAMPLE_COUNT + 1) as i32;
        Canvas3dVec3Q16 {
            x: va.x + ((vb.x - va.x) * step) / denom,
            y: va.y + ((vb.y - va.y) * step) / denom,
            z: va.z + ((vb.z - va.z) * step) / denom,
            pad: local_index as i32,
        }
    };

    vertex.pad = (cube_index * CUBE_VERTEX_COUNT + local_index) as i32;
    vertex
}

pub(crate) fn tetra_vertex(
    local_index: usize,
    base_vertex: usize,
    half_q16: i32,
) -> Canvas3dVec3Q16 {
    let half = half_q16;
    let base_x = ((half as i64 * 56_756) / CANVAS3D_PROJECT_Q16_ONE as i64) as i32;
    let base_y = -half / 2;
    let corners = [
        Canvas3dVec3Q16 {
            x: 0,
            y: half,
            z: 0,
            pad: 0,
        },
        Canvas3dVec3Q16 {
            x: 0,
            y: base_y,
            z: half,
            pad: 1,
        },
        Canvas3dVec3Q16 {
            x: -base_x,
            y: base_y,
            z: -half / 2,
            pad: 2,
        },
        Canvas3dVec3Q16 {
            x: base_x,
            y: base_y,
            z: -half / 2,
            pad: 3,
        },
    ];
    let mut vertex = if local_index < TETRA_CORNER_COUNT {
        corners[local_index]
    } else {
        let edge_sample = local_index - TETRA_CORNER_COUNT;
        let edge_index = edge_sample / TETRA_EDGE_SAMPLE_COUNT;
        let sample_index = edge_sample % TETRA_EDGE_SAMPLE_COUNT;
        let (a, b) = TETRA_EDGES[edge_index];
        let va = corners[a];
        let vb = corners[b];
        let step = (sample_index + 1) as i32;
        let denom = (TETRA_EDGE_SAMPLE_COUNT + 1) as i32;
        Canvas3dVec3Q16 {
            x: va.x + ((vb.x - va.x) * step) / denom,
            y: va.y + ((vb.y - va.y) * step) / denom,
            z: va.z + ((vb.z - va.z) * step) / denom,
            pad: local_index as i32,
        }
    };

    vertex.pad = (base_vertex + local_index) as i32;
    vertex
}

pub(crate) fn present_color(vertex_index: usize, cube_visual_vertex_count: usize) -> u32 {
    if vertex_index < cube_visual_vertex_count {
        PRESENT_COLORS[0]
    } else {
        0xFF30_D8FF
    }
}

fn translate_ping_pong_q16(frame: u32, half_q16: i32) -> i32 {
    const PERIOD_FRAMES: u32 = 240;
    let half_period = PERIOD_FRAMES / 2;
    let phase = frame % PERIOD_FRAMES;
    let span = CANVAS3D_PROJECT_Q16_ONE;
    let offset = if phase < half_period {
        -half_q16 + ((span as i64 * phase as i64) / half_period as i64) as i32
    } else {
        half_q16 - ((span as i64 * (phase - half_period) as i64) / half_period as i64) as i32
    };
    offset.clamp(-half_q16, half_q16)
}
