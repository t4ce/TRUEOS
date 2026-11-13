//! Small CPU-side fields over the 17³ color lattice. The resulting table is
//! used by both the hardware LUT and its point preview; no pixel framebuffer
//! work or per-frame allocation belongs here.
use super::chroma_color::{LUT_EDGE, LUT_ENTRIES};
use alloc::{boxed::Box, vec};

const TAU: f32 = core::f32::consts::TAU;
const LIFE_INTERVAL: f32 = 0.65;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(super) enum MotionMode {
    Orbs,
    Vortex,
    Water,
    Life,
}

impl MotionMode {
    pub(super) const ALL: [Self; 4] = [Self::Orbs, Self::Vortex, Self::Water, Self::Life];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Orbs => "ORBS",
            Self::Vortex => "VORTEX",
            Self::Water => "WATER",
            Self::Life => "LIFE 3D",
        }
    }

    pub(super) fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
}

pub(super) struct Motion {
    mode: Option<MotionMode>,
    phase: f32,
    activation: f32,
    water_position: [f32; 3],
    water_velocity: [f32; 3],
    last_gravity: [f32; 3],
    life: Box<[u8]>,
    next_life: Box<[u8]>,
    life_from: Box<[u8]>,
    life_to: Box<[u8]>,
    life_age: f32,
    generation: u32,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            mode: None,
            phase: 0.0,
            activation: 0.0,
            water_position: [0.0; 3],
            water_velocity: [0.0; 3],
            last_gravity: [0.0, -1.0, 0.0],
            life: vec![0; LUT_ENTRIES].into_boxed_slice(),
            next_life: vec![0; LUT_ENTRIES].into_boxed_slice(),
            life_from: vec![0; LUT_ENTRIES].into_boxed_slice(),
            life_to: vec![0; LUT_ENTRIES].into_boxed_slice(),
            life_age: 0.0,
            generation: 0,
        }
    }
}

impl Motion {
    pub(super) fn mode(&self) -> Option<MotionMode> {
        self.mode
    }

    /// Modes are exclusive. A repeated selection is a no-op; the service can
    /// explicitly select None when the active button is clicked again.
    pub(super) fn select(&mut self, mode: Option<MotionMode>) -> bool {
        if self.mode == mode {
            return false;
        }
        self.mode = mode;
        self.phase = 0.0;
        self.activation = 0.0;
        self.water_position = [0.0; 3];
        self.water_velocity = [0.0; 3];
        self.last_gravity = [0.0, -1.0, 0.0];
        self.life_age = 0.0;
        self.generation = 0;
        if mode == Some(MotionMode::Life) {
            self.seed_life();
            self.life_from.fill(0);
        }
        true
    }

    /// Returns whether output bytes changed. Large or non-finite time steps
    /// cannot destabilize the spring or fast-forward the cellular automaton.
    pub(super) fn step(
        &mut self,
        dt: f32,
        gravity: [f32; 3],
        base: &[u32; LUT_ENTRIES],
        output: &mut [u32; LUT_ENTRIES],
    ) -> bool {
        let Some(mode) = self.mode else {
            let changed = output != base;
            output.copy_from_slice(base);
            return changed;
        };
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        self.phase = (self.phase + dt * 0.35) % TAU;
        self.activation = (self.activation + dt).min(1.0);
        let gain = smooth(self.activation);

        // Trigonometry belongs outside the lattice loop. All fields then do
        // constant arithmetic per node; Life alone periodically visits neighbors.
        let phase = self.phase;
        let centers = [
            [
                0.5 + 0.29 * libm::sinf(phase),
                0.5 + 0.28 * libm::sinf(phase * 2.0 + 1.0),
                0.5 + 0.25 * libm::cosf(phase + 0.7),
            ],
            [
                0.5 + 0.28 * libm::cosf(phase + 2.0),
                0.5 + 0.26 * libm::sinf(phase + 3.1),
                0.5 + 0.29 * libm::sinf(phase * 2.0 + 2.8),
            ],
            [
                0.5 + 0.26 * libm::sinf(phase * 2.0 + 4.0),
                0.5 + 0.28 * libm::cosf(phase + 4.4),
                0.5 + 0.27 * libm::cosf(phase * 2.0 + 0.2),
            ],
        ];
        let rotations: [[f32; 2]; LUT_EDGE] = core::array::from_fn(|green| {
            let y = green as f32 / (LUT_EDGE - 1) as f32;
            let angle = phase + (y - 0.5) * 1.4;
            [libm::sinf(angle), libm::cosf(angle)]
        });
        let wave: [f32; LUT_EDGE] = core::array::from_fn(|axis| {
            libm::sinf(phase * 4.0 + axis as f32 / (LUT_EDGE - 1) as f32 * TAU)
        });
        if mode == MotionMode::Water && dt > 0.0 {
            let gravity = normalized_gravity(gravity);
            for axis in 0..3 {
                self.water_velocity[axis] += (gravity[axis] - self.last_gravity[axis]) * 0.08;
                let target = gravity[axis] * 0.075;
                self.water_velocity[axis] += ((target - self.water_position[axis]) * 12.0
                    - self.water_velocity[axis] * 3.8)
                    * dt;
                self.water_velocity[axis] = self.water_velocity[axis].clamp(-0.3, 0.3);
                self.water_position[axis] =
                    (self.water_position[axis] + self.water_velocity[axis] * dt).clamp(-0.14, 0.14);
            }
            self.last_gravity = gravity;
        } else if mode == MotionMode::Life {
            self.life_age += dt;
            if self.life_age >= LIFE_INTERVAL {
                self.life_age -= LIFE_INTERVAL;
                self.advance_life();
            }
        }
        let life_blend = smooth(self.life_age / LIFE_INTERVAL);
        let mut changed = false;
        for (index, (&anchor, output)) in base.iter().zip(output.iter_mut()).enumerate() {
            let red = index / (LUT_EDGE * LUT_EDGE);
            let green = index / LUT_EDGE % LUT_EDGE;
            let blue = index % LUT_EDGE;
            let lattice = [red, green, blue];
            let point = lattice.map(|axis| axis_code(axis) as f32 / 1023.0);
            let delta = match mode {
                MotionMode::Orbs => {
                    let mut delta = [0.0; 3];
                    for (orb, center) in centers.iter().enumerate() {
                        let distance =
                            core::array::from_fn::<_, 3, _>(|axis| point[axis] - center[axis]);
                        let squared = distance.iter().map(|d| d * d).sum::<f32>();
                        let falloff = (1.0 - squared / 0.20).max(0.0);
                        let weight = falloff * falloff * if orb == 1 { -0.34 } else { 0.34 };
                        for axis in 0..3 {
                            delta[axis] += distance[axis] * weight;
                        }
                    }
                    delta.map(|value| value.clamp(-0.10, 0.10))
                }
                MotionMode::Vortex => {
                    let x = point[0] - 0.5;
                    let z = point[2] - 0.5;
                    let [sine, cosine] = rotations[green];
                    let radius = (1.0 - (x * x + z * z) * 1.8).max(0.0);
                    let weight = radius * (0.11 + point[1] * 0.09);
                    [
                        ((x * cosine - z * sine - x) * weight).clamp(-0.12, 0.12),
                        wave[green] * radius * 0.015,
                        ((x * sine + z * cosine - z) * weight).clamp(-0.12, 0.12),
                    ]
                }
                MotionMode::Water => {
                    let ripple = wave[red] * wave[blue] * (0.25 + 0.75 * point[1]);
                    [
                        self.water_position[0] * (0.4 + point[1] * 0.6) + ripple * 0.018,
                        self.water_position[1] * (0.3 + point[1] * 0.7) + ripple * 0.025,
                        self.water_position[2] * (0.4 + point[1] * 0.6) - ripple * 0.018,
                    ]
                }
                MotionMode::Life => {
                    let from = life_offset(lattice, self.life_from[index]);
                    let to = life_offset(lattice, self.life_to[index]);
                    core::array::from_fn(|axis| from[axis] + (to[axis] - from[axis]) * life_blend)
                }
            };
            let channels = unpack(anchor);
            let result = core::array::from_fn(|axis| {
                // Smooth fields pin channel endpoints instead of crushing all
                // blacks/whites. Life deliberately uses exact lattice steps.
                let envelope = if mode == MotionMode::Life {
                    1.0
                } else {
                    4.0 * point[axis] * (1.0 - point[axis])
                };
                channels[axis] + delta[axis] * gain * envelope
            });
            let packed = pack(result);
            changed |= *output != packed;
            *output = packed;
        }
        changed
    }

    fn seed_life(&mut self) {
        for index in 0..LUT_ENTRIES {
            let hash = hash(index as u32 ^ self.generation.wrapping_mul(0x9e37_79b9));
            self.life[index] = u8::from(hash % 5 == 0);
            self.life_to[index] = if self.life[index] != 0 {
                1 + (hash % 6) as u8
            } else {
                0
            };
        }
    }

    /// Synchronous 3D Life, B5/S456, with all 26 adjacent cells counted and
    /// dead space beyond the cube. An extinct colony is deterministically reseeded.
    fn advance_life(&mut self) {
        self.life_from.copy_from_slice(&self.life_to);
        let mut population = 0usize;
        for red in 0..LUT_EDGE {
            for green in 0..LUT_EDGE {
                for blue in 0..LUT_EDGE {
                    let index = (red * LUT_EDGE + green) * LUT_EDGE + blue;
                    let mut neighbors = 0;
                    for r in red.saturating_sub(1)..=(red + 1).min(LUT_EDGE - 1) {
                        for g in green.saturating_sub(1)..=(green + 1).min(LUT_EDGE - 1) {
                            for b in blue.saturating_sub(1)..=(blue + 1).min(LUT_EDGE - 1) {
                                let neighbor = (r * LUT_EDGE + g) * LUT_EDGE + b;
                                if neighbor != index {
                                    neighbors += self.life[neighbor];
                                }
                            }
                        }
                    }
                    let alive = if self.life[index] != 0 {
                        (4..=6).contains(&neighbors)
                    } else {
                        neighbors == 5
                    };
                    self.next_life[index] = u8::from(alive);
                    population += usize::from(alive);
                }
            }
        }
        core::mem::swap(&mut self.life, &mut self.next_life);
        self.generation = self.generation.wrapping_add(1);
        if population == 0 {
            self.seed_life();
        } else {
            for index in 0..LUT_ENTRIES {
                self.life_to[index] = if self.life[index] != 0 {
                    1 + (hash(index as u32 ^ self.generation.wrapping_mul(0x9e37_79b9)) % 6) as u8
                } else {
                    0
                };
            }
        }
    }
}

fn smooth(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

fn hash(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^ (value >> 16)
}

fn axis_code(axis: usize) -> u32 {
    if axis == LUT_EDGE - 1 {
        1023
    } else {
        axis as u32 * 64
    }
}

fn life_offset(lattice: [usize; 3], code: u8) -> [f32; 3] {
    let mut offset = [0.0; 3];
    if code == 0 {
        return offset;
    }
    let direction = usize::from(code - 1) % 6;
    let axis = direction / 2;
    let coordinate = lattice[axis];
    let positive = if coordinate == 0 {
        true
    } else if coordinate == LUT_EDGE - 1 {
        false
    } else {
        direction % 2 == 0
    };
    let neighbor = if positive {
        coordinate + 1
    } else {
        coordinate - 1
    };
    offset[axis] = (axis_code(neighbor) as f32 - axis_code(coordinate) as f32) / 1023.0;
    offset
}

fn normalized_gravity(gravity: [f32; 3]) -> [f32; 3] {
    let gravity = gravity.map(|value| {
        if value.is_finite() {
            value.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    });
    let magnitude = libm::sqrtf(gravity.iter().map(|value| value * value).sum());
    if magnitude < 0.001 {
        [0.0, -1.0, 0.0]
    } else {
        gravity.map(|value| value / magnitude)
    }
}

fn unpack(value: u32) -> [f32; 3] {
    [value >> 20, value >> 10, value].map(|channel| (channel & 1023) as f32 / 1023.0)
}

fn pack(value: [f32; 3]) -> u32 {
    let [red, green, blue] = value.map(|channel| (channel.clamp(0.0, 1.0) * 1023.0 + 0.5) as u32);
    (red << 20) | (green << 10) | blue
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> Box<[u32; LUT_ENTRIES]> {
        let mut table: Box<[u32; LUT_ENTRIES]> =
            vec![0; LUT_ENTRIES].into_boxed_slice().try_into().unwrap();
        for (index, entry) in table.iter_mut().enumerate() {
            let r = index / (LUT_EDGE * LUT_EDGE);
            let g = index / LUT_EDGE % LUT_EDGE;
            let b = index % LUT_EDGE;
            *entry = (axis_code(r) << 20) | (axis_code(g) << 10) | axis_code(b);
        }
        table
    }

    #[test]
    fn modes_are_exclusive_and_off_restores_the_exact_base() {
        let base = identity();
        let mut output = base.clone();
        let mut motion = Motion::default();
        assert_eq!(motion.mode(), None);
        assert!(!motion.step(0.016, [0.0, -1.0, 0.0], &base, &mut output));
        for mode in MotionMode::ALL {
            assert!(motion.select(Some(mode)));
            assert!(!motion.select(Some(mode)));
            assert_eq!(motion.mode(), Some(mode));
            for _ in 0..100 {
                motion.step(0.016, [0.3, -0.9, 0.1], &base, &mut output);
            }
            assert_ne!(output, base, "{} must deform nodes", mode.label());
        }
        assert!(motion.select(None));
        assert!(motion.step(0.016, [0.0; 3], &base, &mut output));
        assert_eq!(output, base);
        assert!(!motion.step(0.016, [0.0; 3], &base, &mut output));
        assert_eq!(MotionMode::from_index(4), None);
    }

    #[test]
    fn bounded_output_and_step_validation_hold_for_every_mode() {
        let base = identity();
        for mode in MotionMode::ALL {
            let mut motion = Motion::default();
            let mut output = base.clone();
            motion.select(Some(mode));
            for dt in [0.0, -1.0, f32::NAN, f32::INFINITY] {
                assert!(!motion.step(dt, [f32::NAN; 3], &base, &mut output));
                assert_eq!(output, base);
            }
            for tick in 0..300 {
                motion.step(1e9, [1.0, -1.0, 1.0], &base, &mut output);
                assert!(output.iter().all(|entry| entry & 0xc000_0000 == 0));
                if tick == 0 {
                    assert_eq!(motion.activation, 0.1);
                }
            }
            assert!(output.iter().zip(base.iter()).any(|(a, b)| a != b));
        }
    }

    #[test]
    fn fields_keep_running_and_have_different_shapes() {
        let base = identity();
        let mut outputs = Vec::new();
        for mode in MotionMode::ALL {
            let mut motion = Motion::default();
            let mut output = base.clone();
            motion.select(Some(mode));
            for _ in 0..140 {
                motion.step(0.016, [0.0, -1.0, 0.0], &base, &mut output);
            }
            let before = output.clone();
            for _ in 0..50 {
                motion.step(0.016, [0.0, -1.0, 0.0], &base, &mut output);
            }
            assert_ne!(output, before, "{} must continue moving", mode.label());
            outputs.push(output);
        }
        for a in 0..outputs.len() {
            for b in 0..a {
                assert_ne!(outputs[a], outputs[b]);
            }
        }
    }

    #[test]
    fn water_responds_to_gravity_and_pipe_states_are_independent() {
        let base = identity();
        let mut upright = Motion::default();
        let mut tilted = Motion::default();
        upright.select(Some(MotionMode::Water));
        tilted.select(Some(MotionMode::Water));
        let mut a = base.clone();
        let mut b = base.clone();
        for _ in 0..150 {
            upright.step(0.016, [0.0, -1.0, 0.0], &base, &mut a);
            tilted.step(0.016, [0.8, -0.2, 0.3], &base, &mut b);
        }
        assert_ne!(a, b);
        let phase = upright.phase;
        tilted.select(Some(MotionMode::Orbs));
        tilted.step(0.1, [0.0; 3], &base, &mut b);
        assert_eq!(upright.phase, phase);
        assert_eq!(upright.mode(), Some(MotionMode::Water));
    }

    #[test]
    fn editing_the_base_recomputes_without_advancing_or_resetting_motion() {
        let base = identity();
        let mut output = base.clone();
        let mut motion = Motion::default();
        motion.select(Some(MotionMode::Water));
        for _ in 0..90 {
            motion.step(0.016, [0.3, -0.9, 0.1], &base, &mut output);
        }
        let phase = motion.phase;
        let position = motion.water_position;
        let velocity = motion.water_velocity;
        let gravity = motion.last_gravity;
        let mut changed_base = base.clone();
        changed_base.fill((600 << 20) | (400 << 10) | 200);
        assert!(motion.step(0.0, [1.0, 0.0, 0.0], &changed_base, &mut output));
        assert_eq!(motion.phase, phase);
        assert_eq!(motion.water_position, position);
        assert_eq!(motion.water_velocity, velocity);
        assert_eq!(motion.last_gravity, gravity);
        assert_eq!(motion.mode(), Some(MotionMode::Water));
        motion.select(None);
        motion.step(0.0, [0.0; 3], &changed_base, &mut output);
        assert_eq!(output, changed_base);
    }

    #[test]
    fn life_counts_26_neighbors_and_updates_synchronously() {
        let mut motion = Motion::default();
        motion.select(Some(MotionMode::Life));
        motion.life.fill(0);
        let middle = |r: usize, g: usize, b: usize| (r * LUT_EDGE + g) * LUT_EDGE + b;
        // Five diagonal/axial neighbors make a dead central node live.
        for (r, g, b) in [(7, 7, 7), (7, 8, 9), (9, 8, 8), (8, 7, 9), (9, 9, 9)] {
            motion.life[middle(r, g, b)] = 1;
        }
        let center = middle(8, 8, 8);
        assert_eq!(motion.life[center], 0);
        motion.advance_life();
        assert_eq!(motion.life[center], 1);
        assert_eq!(motion.generation, 1);
        assert!((1..=6).contains(&motion.life_to[center]));
        let before = motion.life.clone();
        motion.advance_life();
        assert_ne!(motion.life, before);
        for code in 1..=6 {
            let offset = life_offset([8; 3], code);
            assert_eq!(offset.iter().filter(|v| **v != 0.0).count(), 1);
            assert!((offset.iter().map(|v| v.abs()).sum::<f32>() - 64.0 / 1023.0).abs() < 1e-7);
        }
    }
}
