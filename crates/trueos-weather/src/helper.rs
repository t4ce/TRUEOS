// Implementation requires a math library in no_std.
// We rely on `libm` for trigonometric functions.

use libm::{acos, cos, floor, sin, tan};

const PI: f64 = core::f64::consts::PI;

pub struct SolarCalc;

impl SolarCalc {
    /// Returns the approximate sunrise time in hours from midnight (local solar time, UTC-ish if lon is 0).
    /// Returns value in hours (e.g. 6.5 = 06:30).
    /// Does not handle timezone conversion (caller must do that).
    pub fn get_sunrise(day_of_year: u16, latitude: f64, longitude: f64) -> f64 {
        Self::calculate_sun_time(day_of_year, latitude, longitude, true)
    }

    /// Returns the approximate sunset time in hours from midnight.
    pub fn get_sunset(day_of_year: u16, latitude: f64, longitude: f64) -> f64 {
        Self::calculate_sun_time(day_of_year, latitude, longitude, false)
    }

    fn calculate_sun_time(
        day_of_year: u16,
        latitude: f64,
        longitude: f64,
        is_sunrise: bool,
    ) -> f64 {
        // approximate declination
        let inner_val = (360.0 / 365.0) * ((day_of_year as f64) - 81.0);
        let declination = 23.45 * sin(Self::deg_to_rad(inner_val));

        let lat_rad = Self::deg_to_rad(latitude);
        let decl_rad = Self::deg_to_rad(declination);

        let mut cos_h = -tan(lat_rad) * tan(decl_rad);

        // Clamp cos_h to [-1.0, 1.0]
        if cos_h < -1.0 {
            cos_h = -1.0;
        }
        if cos_h > 1.0 {
            cos_h = 1.0;
        }

        let hour_angle = acos(cos_h);

        // Solar noon (approx)
        let solar_noon = 12.0 - (longitude / 15.0);

        let delta = Self::rad_to_deg(hour_angle) / 15.0;

        if is_sunrise {
            solar_noon - delta
        } else {
            solar_noon + delta
        }
    }

    fn deg_to_rad(degrees: f64) -> f64 {
        degrees * PI / 180.0
    }

    fn rad_to_deg(radians: f64) -> f64 {
        radians * 180.0 / PI
    }
}
