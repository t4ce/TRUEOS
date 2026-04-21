use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};


use crate::r::ui2::{self, Ui2FontTier, Ui2Rect};

const UI2_WEATHER_TEX_ID: u32 = crate::tst_ui2_ids::Ui2DemoTexId::Weather.get();
const UI2_WEATHER_CONTENT_ID: u32 = crate::tst_ui2_ids::Ui2DemoContentId::Weather.get();
const UI2_WEATHER_WINDOW_TITLE: &str = "Frosch";
const UI2_WEATHER_VIEW_W: u32 = 520;
const UI2_WEATHER_VIEW_H: u32 = 260;
const UI2_WEATHER_WINDOW_X: f32 = 60.0;
const UI2_WEATHER_WINDOW_Y: f32 = 60.0;
const UI2_WEATHER_WINDOW_Z: i16 = 38;
const UI2_WEATHER_WINDOW_ALPHA: u8 = 0xFF;
const UI2_WEATHER_BG_RGBA: [u8; 4] = [0x1A, 0x1E, 0x26, 0xFF];
const UI2_WEATHER_HEADER_BG_RGBA: [u8; 4] = [0x22, 0x28, 0x34, 0xFF];
const UI2_WEATHER_ROW_EVEN_BG_RGBA: [u8; 4] = [0x1E, 0x24, 0x2E, 0xFF];
const UI2_WEATHER_ROW_ODD_BG_RGBA: [u8; 4] = [0x1A, 0x1E, 0x26, 0xFF];
const UI2_WEATHER_TEXT_RGBA: [u8; 4] = [0xEE, 0xF3, 0xF9, 0xFF];
const UI2_WEATHER_DIM_RGBA: [u8; 4] = [0x96, 0xA4, 0xB6, 0xFF];
const UI2_WEATHER_ACCENT_RGBA: [u8; 4] = [0x7F, 0xD1, 0xAE, 0xFF];
const UI2_WEATHER_FONT_TIER: Ui2FontTier = Ui2FontTier::OneX;
const UI2_WEATHER_FONT_SIZE_CASE: usize = UI2_WEATHER_FONT_TIER.size_case();
const UI2_WEATHER_PAD_X: usize = 8;
const UI2_WEATHER_PAD_Y: usize = 6;
const UI2_WEATHER_ROW_GAP_Y: usize = 2;

const WEATHER_CITY: &str = "Holzminden";
const WEATHER_API_KEY: &str = "9715912a7d8748d65bc3985b4a4274a0";

/// Map OpenWeatherMap icon code to a twemoji char.
/// OWM icons: 01d/n, 02d/n, 03d/n, 04d/n, 09d/n, 10d/n, 11d/n, 13d/n, 50d/n
fn owm_icon_to_twemoji(icon: &str) -> char {
    match icon {
        "01d" => '\u{2600}',          // sun
        "01n" => '\u{1F311}',         // new moon (dark)
        "02d" => '\u{26C5}',          // sun behind cloud
        "02n" => '\u{2601}',          // cloud
        "03d" | "03n" => '\u{2601}',  // cloud
        "04d" | "04n" => '\u{2601}',  // cloud (broken)
        "09d" | "09n" => '\u{1F327}', // cloud with rain
        "10d" => '\u{1F326}',         // sun behind rain cloud
        "10n" => '\u{1F327}',         // cloud with rain
        "11d" | "11n" => '\u{26A1}',  // lightning
        "13d" | "13n" => '\u{2744}',  // snowflake
        "50d" | "50n" => '\u{1F32B}', // fog
        _ => '\u{2601}',              // fallback: cloud
    }
}

/// Map hour (0..23) to nearest clock twemoji.
/// Twemoji clocks: U+1F550..U+1F55B (1-12 o'clock), U+1F55C..U+1F567 (1:30-12:30).
fn hour_to_clock_twemoji(hour: u32) -> char {
    let h12 = match hour % 12 {
        0 => 12u32,
        other => other,
    };
    // U+1F54F + h12 gives 1..12 o'clock faces
    char::from_u32(0x1F54F + h12).unwrap_or('\u{1F550}')
}

/// Is it daytime given a unix timestamp and timezone offset?
fn is_daytime_approx(dt: u64, sunrise: u64, sunset: u64) -> bool {
    dt >= sunrise && dt < sunset
}

/// Weekday abbreviation from a unix timestamp (UTC).
/// Uses the classic days-since-epoch formula (1970-01-01 was a Thursday).
fn weekday_abbrev(unix: u64) -> &'static str {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    let day_index = ((unix / 86400) % 7) as usize;
    DAYS[day_index]
}

fn weather_line_height() -> usize {
    usize::from(ui2::ui2_font_native_line_height_px(UI2_WEATHER_FONT_TIER).max(1))
}

fn weather_measure_width(text: &str) -> usize {
    ui2::ui2_font_measure_text(UI2_WEATHER_FONT_TIER, text)
        .width_px
        .max(1) as usize
}

fn fill_rect_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgba: [u8; 4],
) {
    let end_y = y.saturating_add(h).min(dst_height);
    let end_x = x.saturating_add(w).min(dst_width);
    for row in y.min(dst_height)..end_y {
        for col in x.min(dst_width)..end_x {
            let idx = (row * dst_width + col) * 4;
            dst[idx] = rgba[0];
            dst[idx + 1] = rgba[1];
            dst[idx + 2] = rgba[2];
            dst[idx + 3] = rgba[3];
        }
    }
}

fn render_text_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    atlases: &ui2::Ui2FontCpuAtlases,
    x: usize,
    y: usize,
    text: &str,
    rgba: [u8; 4],
) {
    let max_width_px = dst_width.saturating_sub(x);
    let _ = ui2::ui2_font_blit_text_rgba(
        dst,
        dst_width,
        dst_height,
        atlases,
        UI2_WEATHER_FONT_TIER,
        x,
        y,
        max_width_px,
        text,
        rgba,
    );
}

#[derive(Clone, Debug)]
struct GeoResult {
    name: String,
    country: String,
    lat: f64,
    lon: f64,
}

#[derive(Clone, Debug)]
struct WeatherRow {
    icon_char: char,
    weekday: &'static str,
    summary: String,
    temp_line: String,
}

#[derive(Clone, Debug)]
struct WeatherSnapshot {
    header: String,
    rows: Vec<WeatherRow>,
}

fn parse_geo_response(raw: &str) -> Option<GeoResult> {
    let root: serde_json::Value = serde_json::from_str(raw).ok()?;
    let arr = root.as_array()?;
    let first = arr.first()?;
    let name = first.get("name")?.as_str()?.to_string();
    let country = first.get("country")?.as_str()?.to_string();
    let lat = first.get("lat")?.as_f64()?;
    let lon = first.get("lon")?.as_f64()?;
    Some(GeoResult {
        name,
        country,
        lat,
        lon,
    })
}

fn build_weather_snapshot(
    geo: &GeoResult,
    response: &trueos_weather::OpenWeatherResponse,
) -> WeatherSnapshot {
    let tz_offset = response.timezone_offset as i64;

    // Use real wall-clock time, apply timezone offset for local time
    let now_utc = crate::time::unix_time_seconds().unwrap_or_else(crate::time::uptime_seconds);
    let local_secs = (now_utc as i64).saturating_add(tz_offset);
    let day_secs = ((local_secs % 86400) + 86400) % 86400;
    let hour = (day_secs / 3600) as u32;
    let minute = ((day_secs % 3600) / 60) as u32;
    let clock_ch = hour_to_clock_twemoji(hour);

    // Determine day/night from today's sunrise/sunset
    let (sunrise, sunset) = response
        .daily
        .as_ref()
        .and_then(|d| d.first())
        .map(|day| (day.sunrise, day.sunset))
        .unwrap_or((0, 0));
    let daytime = is_daytime_approx(now_utc, sunrise, sunset);
    let sun_moon_ch = if daytime { '\u{2600}' } else { '\u{1F319}' };

    let header = format!(
        "{} {}  {} {:02}:{:02} {}  {:.4} {:.4}  {} {}",
        geo.country,
        geo.name,
        clock_ch,
        hour,
        minute,
        sun_moon_ch,
        geo.lat,
        geo.lon,
        response.timezone.as_str(),
        if tz_offset >= 0 {
            format!("+{}", tz_offset / 3600)
        } else {
            format!("{}", tz_offset / 3600)
        }
    );

    let mut rows = Vec::new();
    if let Some(daily) = response.daily.as_ref() {
        for day in daily.iter() {
            let icon_code = day
                .weather
                .first()
                .map(|w| w.icon.as_str())
                .unwrap_or("03d");
            let icon_char = owm_icon_to_twemoji(icon_code);
            let weekday = weekday_abbrev(day.dt);
            let summary = day.summary.clone();
            let k2c = |k: f64| libm::round(k - 273.15) as i32;
            let temp_line = format!(
                "    morn {}/{}  day {}/{}  eve {}/{}  night {}/{}",
                k2c(day.temp.morn),
                k2c(day.feels_like.morn),
                k2c(day.temp.day),
                k2c(day.feels_like.day),
                k2c(day.temp.eve),
                k2c(day.feels_like.eve),
                k2c(day.temp.night),
                k2c(day.feels_like.night),
            );
            rows.push(WeatherRow {
                icon_char,
                weekday,
                summary,
                temp_line,
            });
        }
    }

    WeatherSnapshot { header, rows }
}

fn weather_content_size(snapshot: &WeatherSnapshot) -> (u32, u32) {
    let line_height = weather_line_height();
    let line_step = line_height.saturating_add(UI2_WEATHER_ROW_GAP_Y);
    let mut max_width = weather_measure_width(snapshot.header.as_str());
    for row in snapshot.rows.iter() {
        let line = format!("{}  {}  {}", row.icon_char, row.weekday, row.summary);
        max_width = max_width.max(weather_measure_width(line.as_str()));
        max_width = max_width.max(weather_measure_width(row.temp_line.as_str()));
    }
    let total_lines = 1 + snapshot.rows.len().max(1) * 2;
    let content_w = max_width
        .saturating_add(UI2_WEATHER_PAD_X * 2)
        .max(UI2_WEATHER_VIEW_W as usize);
    let content_h = total_lines
        .saturating_mul(line_step)
        .saturating_add(UI2_WEATHER_PAD_Y * 2)
        .max(UI2_WEATHER_VIEW_H as usize);
    (content_w as u32, content_h as u32)
}

fn compose_weather_rgba(
    atlases: &ui2::Ui2FontCpuAtlases,
    snapshot: &WeatherSnapshot,
    content_w: u32,
    content_h: u32,
) -> Vec<u8> {
    let dst_width = content_w as usize;
    let dst_height = content_h as usize;
    let mut rgba = vec![0u8; dst_width.saturating_mul(dst_height).saturating_mul(4)];
    fill_rect_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        0,
        0,
        dst_width,
        dst_height,
        UI2_WEATHER_BG_RGBA,
    );

    let line_height = weather_line_height();
    let line_step = line_height.saturating_add(UI2_WEATHER_ROW_GAP_Y);

    // Header row background
    let header_block_h = line_step.saturating_add(UI2_WEATHER_PAD_Y);
    fill_rect_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        0,
        0,
        dst_width,
        header_block_h.min(dst_height),
        UI2_WEATHER_HEADER_BG_RGBA,
    );

    let mut y = UI2_WEATHER_PAD_Y;
    render_text_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        atlases,
        UI2_WEATHER_PAD_X,
        y,
        snapshot.header.as_str(),
        UI2_WEATHER_ACCENT_RGBA,
    );
    y = y.saturating_add(line_step);

    if snapshot.rows.is_empty() {
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            UI2_WEATHER_PAD_X,
            y,
            "Loading weather data...",
            UI2_WEATHER_DIM_RGBA,
        );
        return rgba;
    }

    for (idx, row) in snapshot.rows.iter().enumerate() {
        let row_bg = if (idx & 1) == 0 {
            UI2_WEATHER_ROW_EVEN_BG_RGBA
        } else {
            UI2_WEATHER_ROW_ODD_BG_RGBA
        };
        fill_rect_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            0,
            y.saturating_sub(1),
            dst_width,
            line_step.saturating_mul(2).saturating_add(2),
            row_bg,
        );
        let line = format!("{}  {}  {}", row.icon_char, row.weekday, row.summary);
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            UI2_WEATHER_PAD_X,
            y,
            line.as_str(),
            UI2_WEATHER_TEXT_RGBA,
        );
        y = y.saturating_add(line_step);
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            UI2_WEATHER_PAD_X,
            y,
            row.temp_line.as_str(),
            UI2_WEATHER_DIM_RGBA,
        );
        y = y.saturating_add(line_step);
    }

    rgba
}

fn present_snapshot(
    surface: &ui2::Ui2SurfaceWindow,
    atlases: &ui2::Ui2FontCpuAtlases,
    snapshot: &WeatherSnapshot,
) {
    let (content_w, content_h) = weather_content_size(snapshot);
    let rgba = compose_weather_rgba(atlases, snapshot, content_w, content_h);
    let _ = surface.bind_hosted_scroll_state(UI2_WEATHER_CONTENT_ID, content_w, content_h);
    let _ = crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
        surface.tex_id(),
        content_w,
        content_h,
        rgba.as_slice(),
        surface.window_id(),
        "ui2-weather-present",
    );
}

#[embassy_executor::task]
pub async fn ui2_weather_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-weather-demo");
    let Some(atlases) = ui2::ui2_font_decode_cpu_atlases(UI2_WEATHER_FONT_SIZE_CASE) else {
        return;
    };

    let content_w = UI2_WEATHER_VIEW_W;
    let content_h = UI2_WEATHER_VIEW_H;
    let Some(surface) = ui2::Ui2SurfaceWindow::from_existing_texture_with_size(
        UI2_WEATHER_WINDOW_TITLE,
        Ui2Rect {
            x: UI2_WEATHER_WINDOW_X,
            y: UI2_WEATHER_WINDOW_Y,
            w: UI2_WEATHER_VIEW_W as f32,
            h: UI2_WEATHER_VIEW_H as f32,
        },
        UI2_WEATHER_WINDOW_Z,
        UI2_WEATHER_WINDOW_ALPHA,
        UI2_WEATHER_TEX_ID,
        true,
        content_w,
        content_h,
    ) else {
        crate::log!("ui2-weather: window creation failed\n");
        return;
    };
    let _ = surface.bind_spawn_task("ui2-weather-demo");

    // Scrollbar on right side and top
    let _ = ui2::set_window_vertical_scrollbar_side(
        surface.window_id(),
        ui2::Ui2WindowVerticalScrollbarSide::Right,
    );
    let _ = ui2::set_window_horizontal_scrollbar_side(
        surface.window_id(),
        ui2::Ui2WindowHorizontalScrollbarSide::Top,
    );

    // Show initial "loading" state
    let loading_snapshot = WeatherSnapshot {
        header: format!("{}  Loading...", WEATHER_CITY),
        rows: Vec::new(),
    };
    present_snapshot(&surface, &atlases, &loading_snapshot);

    // Step 1: Forward geocoding (use https, not the http GEO_URL constant)
    let geo_url = format!(
        "https://api.openweathermap.org/geo/1.0/direct?q={}&limit=1&appid={}",
        WEATHER_CITY, WEATHER_API_KEY
    );

    let geo = match crate::r::net::json::get_json(geo_url.as_str()).await {
        Ok(raw) => parse_geo_response(raw.as_str()),
        Err(e) => {
            crate::log!("ui2-weather: geo request failed: {:?}\n", e);
            None
        }
    };

    let Some(geo) = geo else {
        let err_snapshot = WeatherSnapshot {
            header: format!("{}  Geocoding failed", WEATHER_CITY),
            rows: Vec::new(),
        };
        present_snapshot(&surface, &atlases, &err_snapshot);
        return;
    };

    crate::log!(
        "ui2-weather: geo ok: {} {} lat={} lon={}\n",
        geo.country,
        geo.name,
        geo.lat,
        geo.lon
    );

    // Show geo result while fetching weather
    let geo_snapshot = WeatherSnapshot {
        header: format!(
            "{} {}  {:.4} {:.4}  fetching weather...",
            geo.country, geo.name, geo.lat, geo.lon
        ),
        rows: Vec::new(),
    };
    present_snapshot(&surface, &atlases, &geo_snapshot);

    // Step 2: Fetch daily weather (exclude current, minutely, hourly, alerts)
    let weather_url = format!(
        "{}?lat={}&lon={}&exclude=current,minutely,hourly,alerts&appid={}",
        trueos_weather::config::ONECALL_URL,
        geo.lat,
        geo.lon,
        WEATHER_API_KEY
    );

    let weather_response = match crate::r::net::json::get_json(weather_url.as_str()).await {
        Ok(raw) => trueos_weather::oc3::decode_onecall_raw_safe(raw.as_str()).ok(),
        Err(e) => {
            crate::log!("ui2-weather: onecall request failed: {:?}\n", e);
            None
        }
    };

    let Some(response) = weather_response else {
        let err_snapshot = WeatherSnapshot {
            header: format!(
                "{} {}  {:.4} {:.4}  weather fetch failed",
                geo.country, geo.name, geo.lat, geo.lon
            ),
            rows: Vec::new(),
        };
        present_snapshot(&surface, &atlases, &err_snapshot);
        return;
    };

    let snapshot = build_weather_snapshot(&geo, &response);
    crate::log!("ui2-weather: {} daily rows\n", snapshot.rows.len());
    present_snapshot(&surface, &atlases, &snapshot);

    // Periodically re-fetch (every hour)
    loop {
        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-weather-demo", 3_600_000).await {
            break;
        }

        let weather_response = match crate::r::net::json::get_json(weather_url.as_str()).await {
            Ok(raw) => trueos_weather::oc3::decode_onecall_raw_safe(raw.as_str()).ok(),
            Err(_) => None,
        };

        if let Some(response) = weather_response {
            let snapshot = build_weather_snapshot(&geo, &response);
            present_snapshot(&surface, &atlases, &snapshot);
        }
    }
}
