#![no_std]

extern crate alloc;
pub mod config;
pub mod lang;
pub mod helper;
pub mod oc3;

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WetterMinute {
    pub dt: u64,
    #[serde(rename = "precipitation")]
    pub precipitation: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WetterAktuell {
    pub dt: u64,
    pub sunrise: u64,
    pub sunset: u64,
    pub temp: f64,
    pub feels_like: f64,
    pub pressure: i32,
    pub humidity: i32,
    pub dew_point: f64,
    pub uvi: f64,
    pub clouds: i32,
    pub visibility: i32,
    pub wind_speed: f64,
    pub wind_deg: i32,
    #[serde(default)]
    pub wind_gust: Option<f64>,
    pub weather: Vec<WetterInfo>,
    #[serde(default)]
    pub rain: Option<RegenDaten>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegenDaten {
    #[serde(rename = "1h")]
    pub letzte_stunde: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WetterInfo {
    pub id: i32,
    pub main: String,
    pub description: String,
    pub icon: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WetterStunde {
    pub dt: u64,
    pub temp: f64,
    pub feels_like: f64,
    pub pressure: i32,
    pub humidity: i32,
    pub dew_point: f64,
    pub uvi: f64,
    pub clouds: i32,
    pub visibility: i32,
    pub wind_speed: f64,
    pub wind_deg: i32,
    #[serde(default)]
    pub wind_gust: Option<f64>,
    pub weather: Vec<WetterInfo>,
    pub pop: f64, // Probability of precipitation
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WetterTag {
    pub dt: u64,
    pub sunrise: u64,
    pub sunset: u64,
    pub moonrise: u64,
    pub moonset: u64,
    pub moon_phase: f64,
    pub summary: String,
    pub temp: TemperaturDaten,
    pub feels_like: GefuehlteTemperaturDaten,
    pub pressure: i32,
    pub humidity: i32,
    pub dew_point: f64,
    pub wind_speed: f64,
    pub wind_deg: i32,
    #[serde(default)]
    pub wind_gust: Option<f64>,
    pub weather: Vec<WetterInfo>,
    pub clouds: i32,
    pub pop: f64,
    pub uvi: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TemperaturDaten {
    pub day: f64,
    pub min: f64,
    pub max: f64,
    pub night: f64,
    pub eve: f64,
    pub morn: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GefuehlteTemperaturDaten {
    pub day: f64,
    pub night: f64,
    pub eve: f64,
    pub morn: f64,
}

// This structure matches OpenWeatherAntwort from the input
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenWeatherResponse {
    pub lat: f64,
    pub lon: f64,
    pub timezone: String,
    pub timezone_offset: i32,
    #[serde(default)]
    pub current: Option<WetterAktuell>,
    #[serde(default)]
    pub minutely: Option<Vec<WetterMinute>>,
    #[serde(default)]
    pub hourly: Option<Vec<WetterStunde>>,
    #[serde(default)]
    pub daily: Option<Vec<WetterTag>>,
}
