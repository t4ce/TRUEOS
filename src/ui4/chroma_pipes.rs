//! Independent color settings for the two display pipes with a hardware cube.
//! Switching A/B changes only the editor view; edits target one selected pipe.
use super::chroma_color::Settings;

pub(super) const PIPE_COUNT: usize = 2;

pub(super) struct PipeBuckets {
    settings: [Settings; PIPE_COUNT],
    selected: usize,
}

impl Default for PipeBuckets {
    fn default() -> Self {
        Self {
            settings: [Settings::default(); PIPE_COUNT],
            selected: 0,
        }
    }
}

impl PipeBuckets {
    pub(super) fn selected(&self) -> usize {
        self.selected
    }

    pub(super) fn settings(&self, pipe: usize) -> &Settings {
        &self.settings[pipe]
    }

    pub(super) fn displayed_settings(&self) -> &Settings {
        self.settings(self.selected)
    }

    /// Merely selecting another bucket never copies settings or queues writes.
    pub(super) fn select(&mut self, pipe: usize) -> bool {
        if pipe >= PIPE_COUNT || pipe == self.selected {
            return false;
        }
        self.selected = pipe;
        true
    }

    pub(super) fn edit_slider(&mut self, slider: usize, value: f32) -> u8 {
        self.edit_slider_for(self.selected, slider, value)
    }

    /// A drag keeps its mouse-down target even if selection changes afterward.
    /// The return value is the changed pipe's bit, for per-pipe dirty tracking.
    pub(super) fn edit_slider_for(&mut self, pipe: usize, slider: usize, value: f32) -> u8 {
        if slider >= 5 || !value.is_finite() {
            return 0;
        }
        let value = match slider {
            1 | 2 => value.clamp(-1.0, 1.0),
            3 => value.clamp(0.0, 1.8),
            _ => value.clamp(0.0, 1.0),
        };
        self.edit(pipe, |settings| {
            let field = match slider {
                0 => &mut settings.yellow,
                1 => &mut settings.shadows,
                2 => &mut settings.highlights,
                3 => &mut settings.saturation,
                _ => &mut settings.strength,
            };
            let changed = *field != value;
            *field = value;
            changed
        })
    }

    pub(super) fn edit_curve(&mut self, knot: usize, value: f32) -> u8 {
        self.edit_curve_for(self.selected, knot, value)
    }

    pub(super) fn edit_curve_for(&mut self, pipe: usize, knot: usize, value: f32) -> u8 {
        if knot >= 7 || !value.is_finite() {
            return 0;
        }
        let value = value.clamp(-1.0, 1.0);
        self.edit(pipe, |settings| {
            if knot == 0 || knot == 6 {
                let changed = settings.sat_curve[0] != value || settings.sat_curve[6] != value;
                settings.sat_curve[0] = value;
                settings.sat_curve[6] = value;
                changed
            } else {
                let changed = settings.sat_curve[knot] != value;
                settings.sat_curve[knot] = value;
                changed
            }
        })
    }

    fn edit(&mut self, pipe: usize, update: impl FnOnce(&mut Settings) -> bool) -> u8 {
        let Some(settings) = self.settings.get_mut(pipe) else {
            return 0;
        };
        if update(settings) { 1 << pipe } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_a_pipe_changes_only_the_view_and_never_copies_settings() {
        let mut buckets = PipeBuckets::default();
        assert_eq!(buckets.selected(), 0);
        assert_eq!(buckets.edit_slider(0, 0.7), 1);
        let original = buckets.settings;
        assert!(buckets.select(1));
        assert_eq!(buckets.selected(), 1);
        assert_eq!(buckets.displayed_settings(), &Settings::default());
        assert_eq!(buckets.settings, original);
        assert!(!buckets.select(1));
        assert!(!buckets.select(2));
        assert_eq!(buckets.selected(), 1);
        assert!(buckets.select(0));
        assert_eq!(buckets.displayed_settings().yellow, 0.7);
        assert_eq!(buckets.settings, original);
    }

    #[test]
    fn edits_preserve_the_other_pipe_and_every_unrelated_property() {
        let mut buckets = PipeBuckets::default();
        assert_eq!(buckets.edit_slider(1, -0.5), 1);
        assert!(buckets.select(1));
        assert_eq!(buckets.edit_slider(2, 0.4), 2);
        assert_eq!(buckets.edit_slider(3, 0.9), 2);
        assert!(buckets.select(0));
        assert_eq!(buckets.edit_slider(4, 0.3), 1);
        assert_eq!(buckets.settings(0).shadows, -0.5);
        assert_eq!(buckets.settings(0).highlights, 0.0);
        assert_eq!(buckets.settings(0).saturation, 1.0);
        assert_eq!(buckets.settings(0).strength, 0.3);
        assert_eq!(buckets.settings(1).shadows, 0.0);
        assert_eq!(buckets.settings(1).highlights, 0.4);
        assert_eq!(buckets.settings(1).saturation, 0.9);
        assert_eq!(buckets.settings(1).strength, 1.0);
        assert_eq!(buckets.edit_slider(4, 0.3), 0);
    }

    #[test]
    fn drag_targets_remain_independent_of_later_selection_changes() {
        let mut buckets = PipeBuckets::default();
        let captured = buckets.selected();
        assert!(buckets.select(1));
        assert_eq!(buckets.edit_slider_for(captured, 0, 0.8), 1);
        assert_eq!(buckets.settings(0).yellow, 0.8);
        assert_eq!(buckets.settings(1).yellow, 0.0);
        assert_eq!(buckets.displayed_settings().yellow, 0.0);
        assert_eq!(buckets.edit_curve_for(captured, 2, -0.5), 1);
        assert_eq!(buckets.settings(0).sat_curve[2], -0.5);
        assert_eq!(buckets.settings(1).sat_curve[2], 0.0);
    }

    #[test]
    fn curve_edits_link_red_endpoints_on_only_the_target_pipe() {
        let mut buckets = PipeBuckets::default();
        assert_eq!(buckets.edit_curve(2, -0.4), 1);
        assert_eq!(buckets.edit_curve(6, 0.7), 1);
        assert_eq!(buckets.settings(0).sat_curve[0], 0.7);
        assert_eq!(buckets.settings(0).sat_curve[6], 0.7);
        assert_eq!(buckets.settings(0).sat_curve[2], -0.4);
        assert_eq!(buckets.settings(1).sat_curve, [0.0; 7]);
        assert!(buckets.select(1));
        assert_eq!(buckets.edit_curve(0, -0.8), 2);
        assert_eq!(buckets.settings(1).sat_curve[0], -0.8);
        assert_eq!(buckets.settings(1).sat_curve[6], -0.8);
        assert_eq!(buckets.settings(0).sat_curve[0], 0.7);
        assert_eq!(buckets.edit_curve(6, -0.8), 0);
    }

    #[test]
    fn invalid_edits_do_not_index_past_buckets_or_dirty_pipes() {
        let mut buckets = PipeBuckets::default();
        assert_eq!(buckets.edit_slider(9, 0.5), 0);
        assert_eq!(buckets.edit_curve(7, 0.5), 0);
        assert_eq!(buckets.edit_slider(0, f32::NAN), 0);
        assert_eq!(buckets.edit_curve(0, f32::INFINITY), 0);
        assert_eq!(buckets.edit_slider_for(2, 0, 0.5), 0);
        assert_eq!(buckets.edit_curve_for(usize::MAX, 0, 0.5), 0);
        assert_eq!(buckets.settings(0), &Settings::default());
        assert_eq!(buckets.settings(1), &Settings::default());
        assert_eq!(buckets.edit_slider(3, 999.0), 1);
        assert_eq!(buckets.settings(0).saturation, 1.8);
        assert_eq!(buckets.edit_curve(0, -999.0), 1);
        assert_eq!(buckets.settings(0).sat_curve[0], -1.0);
        assert_eq!(buckets.settings(0).sat_curve[6], -1.0);
    }
}
