//! Per-pipe animation state. Base grades stay independent of animated outputs;
//! each output is regenerated at most 30 times per second and can be coalesced
//! while the hardware's previous LUT is still pending.
use alloc::{boxed::Box, vec};

use super::{
    chroma_color::{LUT_ENTRIES, Settings},
    chroma_motion::{Motion, MotionMode},
    chroma_pipes::PIPE_COUNT,
};

const STEP_SECONDS: f32 = 1.0 / 30.0;
type Table = Box<[u32; LUT_ENTRIES]>;

pub(super) struct Animations {
    base: [Table; PIPE_COUNT],
    output: [Table; PIPE_COUNT],
    motion: [Motion; PIPE_COUNT],
    gravity: [[f32; 3]; PIPE_COUNT],
    elapsed: [f32; PIPE_COUNT],
    refresh: u8,
}

impl Animations {
    pub(super) fn new(settings: [Settings; PIPE_COUNT]) -> Self {
        let base: [Table; PIPE_COUNT] = core::array::from_fn(|pipe| {
            let mut table: Table = vec![0; LUT_ENTRIES]
                .into_boxed_slice()
                .try_into()
                .expect("chroma-lut-length");
            settings[pipe].build_table(&mut table);
            table
        });
        let output = core::array::from_fn(|pipe| {
            let mut table: Table = vec![0; LUT_ENTRIES]
                .into_boxed_slice()
                .try_into()
                .expect("chroma-lut-length");
            table.copy_from_slice(base[pipe].as_slice());
            table
        });
        Self {
            base,
            output,
            motion: core::array::from_fn(|_| Motion::default()),
            gravity: [[0.0, -1.0, 0.0]; PIPE_COUNT],
            elapsed: [0.0; PIPE_COUNT],
            refresh: 0,
        }
    }

    pub(super) fn table(&self, pipe: usize) -> &[u32; LUT_ENTRIES] {
        &self.output[pipe]
    }

    pub(super) fn mode(&self, pipe: usize) -> Option<MotionMode> {
        self.motion[pipe].mode()
    }

    pub(super) fn select_mode(&mut self, pipe: usize, mode: Option<MotionMode>) -> bool {
        let Some(motion) = self.motion.get_mut(pipe) else {
            return false;
        };
        if !motion.select(mode) {
            return false;
        }
        self.elapsed[pipe] = 0.0;
        self.refresh |= 1 << pipe;
        true
    }

    pub(super) fn set_base(&mut self, pipe: usize, settings: &Settings) {
        settings.build_table(&mut self.base[pipe]);
        self.refresh |= 1 << pipe;
    }

    pub(super) fn set_gravity(&mut self, pipe: usize, gravity: [f32; 3]) {
        if gravity.iter().all(|v| v.is_finite()) {
            self.gravity[pipe] = gravity;
        }
    }

    pub(super) fn any_active(&self) -> bool {
        self.motion.iter().any(|motion| motion.mode().is_some())
    }

    /// An inactive, unseen output can pause its simulation until the pipe is
    /// enabled. A selected inactive pipe can still show the live cube preview.
    pub(super) fn runnable(&self, preview: Option<usize>, inactive: u8) -> bool {
        (0..PIPE_COUNT).any(|pipe| {
            self.mode(pipe).is_some() && (inactive & (1 << pipe) == 0 || preview == Some(pipe))
        })
    }

    pub(super) fn advance(&mut self, elapsed: f32, preview: Option<usize>, inactive: u8) -> u8 {
        let elapsed = if elapsed.is_finite() {
            elapsed.clamp(0.0, 0.1)
        } else {
            0.0
        };
        let mut changed = 0;
        for pipe in 0..PIPE_COUNT {
            let bit = 1 << pipe;
            let active = self.mode(pipe).is_some();
            let runnable = active && (inactive & bit == 0 || preview == Some(pipe));
            if runnable {
                self.elapsed[pipe] += elapsed;
            }
            let steps = (self.elapsed[pipe] / STEP_SECONDS) as u32;
            let refresh = self.refresh & bit != 0;
            if !refresh && steps == 0 {
                continue;
            }
            let dt = steps as f32 * STEP_SECONDS;
            self.elapsed[pipe] = (self.elapsed[pipe] - dt).max(0.0);
            if self.motion[pipe].step(
                dt,
                self.gravity[pipe],
                &self.base[pipe],
                &mut self.output[pipe],
            ) {
                changed |= bit;
            }
            self.refresh &= !bit;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Animations {
        Animations::new([Settings::default(); PIPE_COUNT])
    }

    #[test]
    fn one_exclusive_mode_per_pipe_and_switching_never_changes_base() {
        let mut s = scene();
        let original_a = s.table(0).to_vec();
        let original_b = s.table(1).to_vec();
        assert!(!s.any_active());
        assert!(s.select_mode(0, Some(MotionMode::Orbs)));
        assert!(!s.select_mode(0, Some(MotionMode::Orbs)));
        assert!(s.select_mode(0, Some(MotionMode::Vortex)));
        assert_eq!(s.mode(0), Some(MotionMode::Vortex));
        assert_eq!(s.mode(1), None);
        for _ in 0..30 {
            s.advance(STEP_SECONDS, Some(0), 0);
        }
        assert_eq!(s.base[0].as_slice(), original_a);
        assert_eq!(s.table(1).as_slice(), original_b);
        assert!(s.select_mode(1, Some(MotionMode::Life)));
        assert_eq!(s.mode(0), Some(MotionMode::Vortex));
        assert_eq!(s.mode(1), Some(MotionMode::Life));
        assert!(!s.select_mode(2, Some(MotionMode::Water)));
    }

    #[test]
    fn edit_during_animation_then_stop_restores_the_new_base_exactly() {
        let mut s = scene();
        s.select_mode(0, Some(MotionMode::Orbs));
        for _ in 0..30 {
            s.advance(STEP_SECONDS, Some(0), 0);
        }
        let settings = Settings {
            saturation: 0.6,
            yellow: 0.4,
            ..Settings::default()
        };
        s.set_base(0, &settings);
        s.advance(STEP_SECONDS, Some(0), 0);
        let expected = s.base[0].to_vec();
        assert_ne!(s.table(0).as_slice(), expected);
        s.select_mode(0, None);
        assert_eq!(s.advance(0.0, Some(0), 0), 1);
        assert_eq!(s.table(0).as_slice(), expected);
        assert_eq!(s.advance(STEP_SECONDS, Some(0), 0), 0);
    }

    #[test]
    fn closed_editor_keeps_active_outputs_moving_but_pauses_inactive_unseen_pipes() {
        let mut s = scene();
        s.select_mode(0, Some(MotionMode::Orbs));
        s.select_mode(1, Some(MotionMode::Water));
        s.advance(0.0, None, 2);
        let frozen_b = s.table(1).to_vec();
        let original_a = s.table(0).to_vec();
        for _ in 0..30 {
            s.advance(STEP_SECONDS, None, 2);
        }
        assert_ne!(s.table(0).as_slice(), original_a);
        assert_eq!(s.table(1).as_slice(), frozen_b);
        assert!(s.runnable(None, 2));
        s.select_mode(0, None);
        assert!(!s.runnable(None, 2));
        assert!(s.runnable(Some(1), 2));
        for _ in 0..30 {
            s.advance(STEP_SECONDS, Some(1), 2);
        }
        assert_ne!(s.table(1).as_slice(), frozen_b);
    }

    #[test]
    fn subframe_ticks_and_idle_state_do_not_rebuild_tables() {
        let mut s = scene();
        assert_eq!(s.advance(STEP_SECONDS, None, 0), 0);
        s.select_mode(0, Some(MotionMode::Vortex));
        s.advance(0.0, Some(0), 0);
        assert_eq!(s.advance(STEP_SECONDS * 0.4, Some(0), 0), 0);
        assert_eq!(s.advance(f32::NAN, Some(0), 0), 0);
        assert_eq!(s.advance(-1.0, Some(0), 0), 0);
    }
}
