extern crate alloc;

use crate::r::net::ws::WsConnection;
use crate::r::net::wss::WssConnection;

#[embassy_executor::task]
pub async fn boot_ws_smoke_task() {
    crate::log!("ws-smoke: starting\n");

    let mut plain_ok = false;
    let mut secure_ok = false;

    // Insecure. Use a host that still supports a real ws:// upgrade on port 80.
    match WsConnection::connect("ws://websocket-echo.com/").await {
        Ok(mut ws) => {
            crate::log!("ws-smoke: connected plain\n");
            if let Err(e) = ws.send("Hello TRUEOS") {
                crate::log!("ws-smoke: send plain failed {:?}\n", e);
            } else {
                // Simple poll for response
                for _ in 0..10 {
                    if let Some(msg) = ws.recv() {
                        crate::log!("ws-smoke: recv plain: {}\n", msg);
                        plain_ok = true;
                        break;
                    }
                    embassy_time::Timer::after(embassy_time::Duration::from_millis(100)).await;
                }
            }
        }
        Err(e) => {
            crate::log!("ws-smoke: plain connect failed {:?}\n", e);
        }
    }

    // Secure (required). Prefer websocket-echo; fall back to postman and echo.websocket.org.
    let mut wss_res = WssConnection::connect("wss://websocket-echo.com/").await;
    if wss_res.is_err() {
        wss_res = WssConnection::connect("wss://ws.postman-echo.com/raw").await;
    }
    if wss_res.is_err() {
        wss_res = WssConnection::connect("wss://echo.websocket.org/").await;
    }

    match wss_res {
        Ok(mut wss) => {
            crate::log!("ws-smoke: connected secure\n");
            if let Err(e) = wss.send("Hello Secure TRUEOS") {
                crate::log!("ws-smoke: secure send failed {:?}\n", e);
            } else {
                // Simple poll for response
                for _ in 0..10 {
                    if let Some(msg) = wss.recv() {
                        crate::log!("ws-smoke: recv secure: {}\n", msg);
                        secure_ok = true;
                        break;
                    }
                    embassy_time::Timer::after(embassy_time::Duration::from_millis(100)).await;
                }
            }
        }
        Err(e) => {
            crate::log!("ws-smoke: secure connect failed {:?}\n", e);
        }
    }

    crate::log!(
        "ws-smoke: summary plain={} secure={}\n",
        if plain_ok { "ok" } else { "fail" },
        if secure_ok { "ok" } else { "fail" },
    );
    crate::log!("ws-smoke: done\n");
}
