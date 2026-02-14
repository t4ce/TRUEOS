extern crate alloc;

use crate::v::net::ws::WsConnection;
use crate::v::net::wss::WssConnection;

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

    /*
    static OPENAI_DEMO_KEY: &str = "***";
    let auth_header = alloc::format!("Authorization: Bearer {}", OPENAI_DEMO_KEY);
    let temp_headers = [
        auth_header.as_str(),
        "OpenAI-Beta: realtime=v1",
    ];
    let headers = &temp_headers[..];
    let openai_res = WssConnection::connect_with_headers(
        "wss://api.openai.com/v1/realtime?model=gpt-realtime-2025-08-28",
        headers
    ).await;
    match openai_res {
        Ok(mut wss) => {
            crate::log!("ws-smoke: connected to openai (unexpected success with dummy key!)\n");
            
            // Send a user message asking for the time
            let event = r#"{
                "type": "conversation.item.create",
                "item": {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": "Hello? What time is it?"
                        }
                    ]
                }
            }"#;

            if let Err(e) = wss.send(event) {
                 crate::log!("ws-smoke: openai send failed {:?}\n", e);
            } else {
                 // Trigger the response generation immediately after
                 let _ = wss.send(r#"{"type": "response.create"}"#);

                 // Poll for details/audio
                 let mut audio_packets = 0;
                 for _ in 0..50 {
                     if let Some(msg) = wss.recv() {
                         if msg.contains("response.audio.delta") {
                             audio_packets += 1;
                             crate::log!("ws-smoke: recv audio delta #{}\n", audio_packets);
                         } else if msg.contains("response.audio_transcript.delta") {
                             crate::log!("ws-smoke: recv transcript delta: {}\n", msg);
                         } else {
                            crate::log!("ws-smoke: recv openai control: {}\n", msg);
                         }
                         
                         // Don't break immediately, let it stream a bit
                         if audio_packets > 5 { break; }
                     }
                     embassy_time::Timer::after(embassy_time::Duration::from_millis(50)).await;
                 }
            }
        }
        Err(e) => {
             crate::log!("ws-smoke: openai connect result: {:?} (likely expected due to invalid key)\n", e);
        }
    }
    */

    crate::log!(
        "ws-smoke: summary plain={} secure={}\n",
        if plain_ok { "ok" } else { "fail" },
        if secure_ok { "ok" } else { "fail" },
    );
    crate::log!("ws-smoke: done\n");
}
