use crate::v::net::ws::WsConnection;
use crate::v::net::wss::WssConnection;

#[embassy_executor::task]
pub async fn boot_ws_smoke_task() {
    crate::log!("ws-smoke: starting\n");

    // Insecure
    match WsConnection::connect("ws://echo.websocket.events").await {
        Ok(mut ws) => {
             crate::log!("ws-smoke: connected plain\n");
             if let Err(e) = ws.send("Hello TRUEOS") {
                  crate::log!("ws-smoke: send plain failed {:?}\n", e);
             } else {
                 // Simple poll for response
                 for _ in 0..10 {
                     if let Some(msg) = ws.recv() {
                        crate::log!("ws-smoke: recv plain: {}\n", msg);
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

    // Secure
    match WssConnection::connect("wss://echo.websocket.events").await {
        Ok(mut wss) => {
            crate::log!("ws-smoke: connected secure\n");
            if let Err(e) = wss.send("Hello Secure TRUEOS") {
                 crate::log!("ws-smoke: secure send failed {:?}\n", e);
            } else {
                 // Simple poll for response
                 for _ in 0..10 {
                     if let Some(msg) = wss.recv() {
                        crate::log!("ws-smoke: recv secure: {}\n", msg);
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
    
    crate::log!("ws-smoke: done\n");
}
