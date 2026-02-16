extern crate alloc;

use alloc::format;
use alloc::string::String;

use crate::config::{GEO_REVERSE_URL, ONECALL_URL, OVERVIEW_URL};

pub fn openweather_geo_url(latitude: f64, longitude: f64, api_key: &str) -> String {
    format!(
        "{}?lat={}&lon={}&limit=1&appid={}",
        GEO_REVERSE_URL, latitude, longitude, api_key
    )
}

pub fn openweather_onecall_url(
    latitude: f64,
    longitude: f64,
    units: &str,
    lang: &str,
    api_key: &str,
) -> String {
    format!(
        "{}?lat={}&lon={}&units={}&lang={}&appid={}",
        ONECALL_URL, latitude, longitude, units, lang, api_key
    )
}

pub fn openweather_onecall_metric_de_url(latitude: f64, longitude: f64, api_key: &str) -> String {
    openweather_onecall_url(latitude, longitude, "metric", "de", api_key)
}

pub fn openweather_onecall_overview_url(latitude: f64, longitude: f64, api_key: &str) -> String {
    format!(
        "{}?lat={}&lon={}&appid={}",
        OVERVIEW_URL, latitude, longitude, api_key
    )
}
