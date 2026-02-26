extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::config::{GEO_REVERSE_URL, ONECALL_URL, OVERVIEW_URL};
use crate::lang::{DEFAULT_GERMAN_LANGUAGE_CODE, normalized_language_code};
use serde_json::Value;

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
    let lang = normalized_language_code(lang);
    format!(
        "{}?lat={}&lon={}&units={}&lang={}&appid={}",
        ONECALL_URL, latitude, longitude, units, lang, api_key
    )
}

pub fn openweather_onecall_metric_de_url(latitude: f64, longitude: f64, api_key: &str) -> String {
    openweather_onecall_url(
        latitude,
        longitude,
        "metric",
        DEFAULT_GERMAN_LANGUAGE_CODE,
        api_key,
    )
}

pub fn openweather_onecall_overview_url(latitude: f64, longitude: f64, api_key: &str) -> String {
    format!(
        "{}?lat={}&lon={}&appid={}",
        OVERVIEW_URL, latitude, longitude, api_key
    )
}

#[derive(Clone, Debug)]
pub enum RawDecodeError {
    InvalidJson,
}

pub fn decode_onecall_raw_safe(
    raw_json: &str,
) -> Result<crate::OpenWeatherResponse, RawDecodeError> {
    let root: Value = serde_json::from_str(raw_json).map_err(|_| RawDecodeError::InvalidJson)?;
    Ok(map_onecall_root(&root))
}

pub fn decode_onecall_overview_raw_safe(raw_json: &str) -> Result<Option<String>, RawDecodeError> {
    let root: Value = serde_json::from_str(raw_json).map_err(|_| RawDecodeError::InvalidJson)?;
    Ok(root
        .get("weather_overview")
        .and_then(|v| v.as_str())
        .map(String::from))
}

pub fn encode_model_json(model: &crate::OpenWeatherResponse) -> Result<String, serde_json::Error> {
    serde_json::to_string(model)
}

pub fn encode_model_json_pretty(
    model: &crate::OpenWeatherResponse,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(model)
}

fn map_onecall_root(root: &Value) -> crate::OpenWeatherResponse {
    crate::OpenWeatherResponse {
        lat: get_f64(root, "lat"),
        lon: get_f64(root, "lon"),
        timezone: get_str(root, "timezone"),
        timezone_offset: get_i32(root, "timezone_offset"),
        current: root.get("current").and_then(map_current),
        minutely: root.get("minutely").and_then(map_minutely),
        hourly: root.get("hourly").and_then(map_hourly),
        daily: root.get("daily").and_then(map_daily),
    }
}

fn map_current(v: &Value) -> Option<crate::WetterAktuell> {
    let obj = v.as_object()?;
    Some(crate::WetterAktuell {
        dt: get_u64_obj(obj, "dt"),
        sunrise: get_u64_obj(obj, "sunrise"),
        sunset: get_u64_obj(obj, "sunset"),
        temp: get_f64_obj(obj, "temp"),
        feels_like: get_f64_obj(obj, "feels_like"),
        pressure: get_i32_obj(obj, "pressure"),
        humidity: get_i32_obj(obj, "humidity"),
        dew_point: get_f64_obj(obj, "dew_point"),
        uvi: get_f64_obj(obj, "uvi"),
        clouds: get_i32_obj(obj, "clouds"),
        visibility: get_i32_obj(obj, "visibility"),
        wind_speed: get_f64_obj(obj, "wind_speed"),
        wind_deg: get_i32_obj(obj, "wind_deg"),
        wind_gust: obj.get("wind_gust").and_then(|x| x.as_f64()),
        weather: map_weather_list_obj(obj, "weather"),
        rain: obj.get("rain").and_then(map_rain),
    })
}

fn map_minutely(v: &Value) -> Option<Vec<crate::WetterMinute>> {
    let arr = v.as_array()?;
    Some(
        arr.iter()
            .filter_map(|it| {
                let o = it.as_object()?;
                Some(crate::WetterMinute {
                    dt: get_u64_obj(o, "dt"),
                    precipitation: get_f64_obj(o, "precipitation"),
                })
            })
            .collect(),
    )
}

fn map_hourly(v: &Value) -> Option<Vec<crate::WetterStunde>> {
    let arr = v.as_array()?;
    Some(
        arr.iter()
            .filter_map(|it| {
                let o = it.as_object()?;
                Some(crate::WetterStunde {
                    dt: get_u64_obj(o, "dt"),
                    temp: get_f64_obj(o, "temp"),
                    feels_like: get_f64_obj(o, "feels_like"),
                    pressure: get_i32_obj(o, "pressure"),
                    humidity: get_i32_obj(o, "humidity"),
                    dew_point: get_f64_obj(o, "dew_point"),
                    uvi: get_f64_obj(o, "uvi"),
                    clouds: get_i32_obj(o, "clouds"),
                    visibility: get_i32_obj(o, "visibility"),
                    wind_speed: get_f64_obj(o, "wind_speed"),
                    wind_deg: get_i32_obj(o, "wind_deg"),
                    wind_gust: o.get("wind_gust").and_then(|x| x.as_f64()),
                    weather: map_weather_list_obj(o, "weather"),
                    pop: get_f64_obj(o, "pop"),
                })
            })
            .collect(),
    )
}

fn map_daily(v: &Value) -> Option<Vec<crate::WetterTag>> {
    let arr = v.as_array()?;
    Some(
        arr.iter()
            .filter_map(|it| {
                let o = it.as_object()?;
                Some(crate::WetterTag {
                    dt: get_u64_obj(o, "dt"),
                    sunrise: get_u64_obj(o, "sunrise"),
                    sunset: get_u64_obj(o, "sunset"),
                    moonrise: get_u64_obj(o, "moonrise"),
                    moonset: get_u64_obj(o, "moonset"),
                    moon_phase: get_f64_obj(o, "moon_phase"),
                    summary: get_string_obj(o, "summary"),
                    temp: map_temp(o.get("temp")),
                    feels_like: map_feels_like(o.get("feels_like")),
                    pressure: get_i32_obj(o, "pressure"),
                    humidity: get_i32_obj(o, "humidity"),
                    dew_point: get_f64_obj(o, "dew_point"),
                    wind_speed: get_f64_obj(o, "wind_speed"),
                    wind_deg: get_i32_obj(o, "wind_deg"),
                    wind_gust: o.get("wind_gust").and_then(|x| x.as_f64()),
                    weather: map_weather_list_obj(o, "weather"),
                    clouds: get_i32_obj(o, "clouds"),
                    pop: get_f64_obj(o, "pop"),
                    uvi: get_f64_obj(o, "uvi"),
                })
            })
            .collect(),
    )
}

fn map_rain(v: &Value) -> Option<crate::RegenDaten> {
    let o = v.as_object()?;
    Some(crate::RegenDaten {
        letzte_stunde: o.get("1h").and_then(|x| x.as_f64()).unwrap_or(0.0),
    })
}

fn map_temp(v: Option<&Value>) -> crate::TemperaturDaten {
    let o = v.and_then(|x| x.as_object());
    crate::TemperaturDaten {
        day: get_f64_opt_obj(o, "day"),
        min: get_f64_opt_obj(o, "min"),
        max: get_f64_opt_obj(o, "max"),
        night: get_f64_opt_obj(o, "night"),
        eve: get_f64_opt_obj(o, "eve"),
        morn: get_f64_opt_obj(o, "morn"),
    }
}

fn map_feels_like(v: Option<&Value>) -> crate::GefuehlteTemperaturDaten {
    let o = v.and_then(|x| x.as_object());
    crate::GefuehlteTemperaturDaten {
        day: get_f64_opt_obj(o, "day"),
        night: get_f64_opt_obj(o, "night"),
        eve: get_f64_opt_obj(o, "eve"),
        morn: get_f64_opt_obj(o, "morn"),
    }
}

fn map_weather_list_obj(obj: &serde_json::Map<String, Value>, key: &str) -> Vec<crate::WetterInfo> {
    obj.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|it| {
                    let o = it.as_object()?;
                    Some(crate::WetterInfo {
                        id: get_i32_obj(o, "id"),
                        main: get_string_obj(o, "main"),
                        description: get_string_obj(o, "description"),
                        icon: get_string_obj(o, "icon"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[inline]
fn get_f64(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(|x| x.as_f64()).unwrap_or(0.0)
}

#[inline]
fn get_i32(v: &Value, key: &str) -> i32 {
    v.get(key)
        .and_then(|x| x.as_i64())
        .and_then(|n| i32::try_from(n).ok())
        .unwrap_or(0)
}

#[inline]
fn get_str(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(String::from)
        .unwrap_or_default()
}

#[inline]
fn get_u64_obj(obj: &serde_json::Map<String, Value>, key: &str) -> u64 {
    obj.get(key).and_then(|x| x.as_u64()).unwrap_or(0)
}

#[inline]
fn get_i32_obj(obj: &serde_json::Map<String, Value>, key: &str) -> i32 {
    obj.get(key)
        .and_then(|x| x.as_i64())
        .and_then(|n| i32::try_from(n).ok())
        .unwrap_or(0)
}

#[inline]
fn get_f64_obj(obj: &serde_json::Map<String, Value>, key: &str) -> f64 {
    obj.get(key).and_then(|x| x.as_f64()).unwrap_or(0.0)
}

#[inline]
fn get_string_obj(obj: &serde_json::Map<String, Value>, key: &str) -> String {
    obj.get(key)
        .and_then(|x| x.as_str())
        .map(String::from)
        .unwrap_or_default()
}

#[inline]
fn get_f64_opt_obj(obj: Option<&serde_json::Map<String, Value>>, key: &str) -> f64 {
    obj.and_then(|o| o.get(key))
        .and_then(|x| x.as_f64())
        .unwrap_or(0.0)
}
