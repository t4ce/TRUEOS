// Oneshot SMTP smoke test.
// Waits 5 s for the stack to settle, then attempts a single send via smtp.mail.com:587
// (STARTTLS). Credentials are hardcoded temporarily for the smoke run.

extern crate alloc;

use embassy_time::{Duration, Timer};

// ── temporary credentials ────────────────────────────────────────────────────
const SMOKE_USER: &str = "jonasb@post.com";
const SMOKE_PASS: &str = "Ttest1001";
const SMOKE_FROM: &str = "jonasb@post.com";
const SMOKE_TO: &str = "jonasbae@outlook.de";
// ─────────────────────────────────────────────────────────────────────────────

const SMOKE_TIMEOUT_MS: u32 = 20_000;

#[embassy_executor::task]
pub async fn smtp_smoke_task() {
    crate::log!("smtp-smoke: waiting 5 s for stack to settle\n");
    Timer::after(Duration::from_secs(5)).await;
    crate::log!("smtp-smoke: starting\n");

    match crate::r::net::smtp::SmtpClient::connect(SMOKE_TIMEOUT_MS).await {
        Ok(mut client) => {
            crate::log!("smtp-smoke: connected + STARTTLS ok\n");

            if let Err(e) = client
                .auth_login(SMOKE_USER, SMOKE_PASS, SMOKE_TIMEOUT_MS)
                .await
            {
                crate::log!("smtp-smoke: auth failed {:?}\n", e);
            } else {
                crate::log!("smtp-smoke: auth ok\n");

                let from_domain = SMOKE_FROM.split('@').nth(1).unwrap_or("trueos.local");
                let message = alloc::format!(
                    "From: <{}>\r\nTo: <{}>\r\nSubject: trueos smoke\r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\nMessage-ID: <trueos-smoke@{}>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: 7bit\r\n\r\nsmoke test from trueos kernel\r\n",
                    SMOKE_FROM,
                    SMOKE_TO,
                    from_domain
                );

                if let Err(e) = client
                    .send_mail(SMOKE_FROM, &[SMOKE_TO], &message, SMOKE_TIMEOUT_MS)
                    .await
                {
                    crate::log!("smtp-smoke: send_mail failed {:?}\n", e);
                } else {
                    crate::log!("smtp-smoke: send_mail ok\n");
                }
            }

            let _ = client.quit(5_000).await;
        }
        Err(e) => {
            crate::log!("smtp-smoke: connect failed {:?}\n", e);
        }
    }

    crate::log!("pop3-smoke: starting\n");
    match crate::r::net::pop3::Pop3Client::connect(SMOKE_TIMEOUT_MS).await {
        Ok(mut pop3) => {
            crate::log!("pop3-smoke: connected (implicit TLS)\n");
            if let Err(e) = pop3.login(SMOKE_USER, SMOKE_PASS, SMOKE_TIMEOUT_MS).await {
                crate::log!("pop3-smoke: auth failed {:?}\n", e);
            } else {
                crate::log!("pop3-smoke: auth ok\n");
                match pop3.stat(SMOKE_TIMEOUT_MS).await {
                    Ok((count, bytes)) => {
                        crate::log!("pop3-smoke: STAT ok count={} bytes={}\n", count, bytes);
                        let first_id = if count > 10 { count - 9 } else { 1 };
                        crate::log!("pop3-smoke: last {} mail captions\n", count.min(10));

                        if count == 0 {
                            crate::log!("pop3-smoke: mailbox empty\n");
                        } else {
                            for msg_id in first_id..=count {
                                match pop3.top(msg_id, 0, SMOKE_TIMEOUT_MS, 16 * 1024).await {
                                    Ok(raw) => {
                                        let text = core::str::from_utf8(&raw).unwrap_or("");
                                        let from = header_value(text, "from").unwrap_or("<none>");
                                        let subject =
                                            header_value(text, "subject").unwrap_or("<none>");
                                        crate::log!(
                                            "pop3-smoke: mail id={} from={} subject={}\n",
                                            msg_id,
                                            from,
                                            subject
                                        );
                                    }
                                    Err(e) => {
                                        crate::log!(
                                            "pop3-smoke: TOP failed id={} {:?}\n",
                                            msg_id,
                                            e
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        crate::log!("pop3-smoke: STAT failed {:?}\n", e);
                    }
                }
            }
            let _ = pop3.quit(5_000).await;
        }
        Err(e) => {
            crate::log!("pop3-smoke: connect failed {:?}\n", e);
        }
    }

    crate::log!("smtp-smoke: done\n");
}

fn header_value<'a>(mail_headers: &'a str, key: &str) -> Option<&'a str> {
    for line in mail_headers.lines() {
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(key) {
                return Some(v.trim());
            }
        }
    }
    None
}
