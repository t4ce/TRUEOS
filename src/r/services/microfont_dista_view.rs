//! Pure controls and source-cache identity for the visible Meso atlas.
//! Quality changes polygon detail; AA changes only raster sampling density.

const SEGMENTS: [usize; 5] = [4, 8, 12, 16, 24];
const SCALES: [u32; 5] = [1, 2, 4, 8, 16];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Config {
    pub(super) quality_index: usize,
    pub(super) size_index: usize,
    pub(super) aa: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { quality_index: 3, size_index: 4, aa: false }
    }
}

impl Config {
    pub(super) const fn segments(self) -> usize { SEGMENTS[self.quality_index] }
    pub(super) const fn scale(self) -> u32 { SCALES[self.size_index] }
    pub(super) const fn width(self) -> u32 { 16 * 6 * self.scale() }
    pub(super) const fn height(self) -> u32 { 6 * 11 * self.scale() }
    pub(super) const fn sample_scale(self) -> u32 { if self.aa { 2 } else { 1 } }
    pub(super) const fn source_scale(self) -> u32 { self.scale() * self.sample_scale() }

    /// Raster sources can be shared by e.g. 4x with AA and 8x without AA.
    /// The displayed Config remains separate because their final images differ.
    pub(super) const fn source_key(self) -> (usize, u32) {
        (self.segments(), self.source_scale())
    }

    /// Inscribed-circle chord error at the logical display size, in pixels.
    /// Supersampling changes coverage, not this geometric approximation bound.
    pub(super) fn max_error_px(self) -> f64 {
        let radius = microfont::FontShape::INFLUENCE_WIDTH
            * libm::sqrt(-2.0 * libm::log(microfont::FontShape::Meso.body_threshold()));
        radius * f64::from(self.scale())
            * (1.0 - libm::cos(core::f64::consts::PI / self.segments() as f64))
    }

    /// The owner routes only deliberate key presses here. Returning false means
    /// no frame update is needed, including repeated selection and size clamps.
    pub(super) fn apply_key(&mut self, key: char) -> bool {
        let previous = *self;
        match key {
            'q' | 'Q' => self.quality_index = (self.quality_index + 1) % SEGMENTS.len(),
            'a' | 'A' => self.aa = !self.aa,
            '1'..='5' => self.size_index = key as usize - '1' as usize,
            '+' => self.size_index = (self.size_index + 1).min(SCALES.len() - 1),
            '-' => self.size_index = self.size_index.saturating_sub(1),
            _ => {}
        }
        *self != previous
    }
}
