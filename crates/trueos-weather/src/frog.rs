extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use embassy_executor::task;
use heapless::String as HString;

use crate::shell::CommandAction;
use crate::shell::cmd::registry::{ParsedArgs, ShellCommandCtx};

const FROG_LATITUDE: f64 = 51.832427;
const FROG_LONGITUDE: f64 = 9.456766;

/*ApiKey "9715912a7d8748d65bc3985b4a4274a0"
Longitude  9.456766
Latitude  51.832427 */
pub(crate) fn cmd_frog(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let Some(api_key) = args.and_then(|a| a.get_str(0)) else {
        ctx.io.write_str("frog: usage frog <api_key>\r\n");
        return CommandAction::None;
    };

    let mut key: HString<128> = HString::new();
    for ch in api_key.chars() {
        if key.push(ch).is_err() {
            ctx.io
                .write_str("frog: api_key too long (max 128 chars)\r\n");
            return CommandAction::None;
        }
    }

    let Some(slot_id) = crate::matrix::alloc_slot("frog geo") else {
        ctx.io.write_str("frog: matrix full\r\n");
        return CommandAction::None;
    };

    if ctx.spawner.spawn(frog_job(slot_id, key)).is_err() {
        let _ = crate::matrix::free_slot(slot_id);
        ctx.io.write_str("frog: spawn failed\r\n");
        return CommandAction::None;
    }

    ctx.io.write_fmt(format_args!(
        "frog: started §{} (lat={}, lon={})\r\n",
        slot_id + 1,
        FROG_LATITUDE,
        FROG_LONGITUDE
    ));
    crate::matrix::refresh_matrix_symbols(ctx.io, *ctx.term_cols);
    CommandAction::None
}

#[task]
pub async fn frog_job(slot_id: u8, api_key: HString<128>) {
    crate::matrix::push_line(slot_id, "frog: requesting openweather reverse-geo");
    let url =
        trueos_weather::oc3::openweather_geo_url(FROG_LATITUDE, FROG_LONGITUDE, api_key.as_str());

    match crate::v::net::json::get_json(url.as_str()).await {
        Ok(raw) => {
            let pretty = pretty_json(raw.as_str());
            let blob: Vec<u8> = pretty.into_bytes();
            let _ = crate::matrix::set_blob_owned_with_preview(slot_id, blob);
            crate::matrix::push_line(slot_id, "frog: response ready");
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
        }
        Err(e) => {
            let msg = alloc::format!("frog: request failed: {:?}", e);
            crate::matrix::push_line(slot_id, msg.as_str());
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
        }
    }
}

fn pretty_json(input: &str) -> String {
    let mut out = String::new();
    let mut depth: usize = 0;
    let mut in_string = false;
    let mut escaped = false;

    for ch in input.chars() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                out.push(ch);
            }
            '{' | '[' => {
                out.push(ch);
                depth = depth.saturating_add(1);
                out.push('\n');
                push_indent(&mut out, depth);
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                out.push('\n');
                push_indent(&mut out, depth);
                out.push(ch);
            }
            ',' => {
                out.push(',');
                out.push('\n');
                push_indent(&mut out, depth);
            }
            ':' => {
                out.push(':');
                out.push(' ');
            }
            c if c.is_whitespace() => {}
            _ => out.push(ch),
        }
    }

    out
}

#[inline]
fn push_indent(out: &mut String, depth: usize) {
    for _ in 0..(depth * 2) {
        out.push(' ');
    }
}
