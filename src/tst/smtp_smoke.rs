// Oneshot SMTP boot reporter.
// Waits 5 s for the stack to settle, then mails the accumulated global log and
// a freshly generated TLB dump via smtp.mail.com:587 (STARTTLS).

extern crate alloc;

use alloc::string::String;
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
    crate::log!("smtp-smoke: waiting for NET_CONFIGURED\n");
    crate::r::readiness::wait_for(crate::r::readiness::NET_CONFIGURED).await;
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

                let log_text = String::from_utf8_lossy(&crate::globalog::snapshot()).into_owned();
                match send_report_mail(
                    &mut client,
                    "trueos global log",
                    "trueos-globalog",
                    &log_text,
                )
                .await
                {
                    Ok(()) => {
                        crate::log!("smtp-smoke: sent global log mail ({} bytes)\n", log_text.len())
                    }
                    Err(e) => crate::log!("smtp-smoke: global log mail failed {:?}\n", e),
                }

                let tlb_text = crate::shell2::build_tlb_dump_text();
                match crate::shell2::write_tlb_dump_to_default_path(tlb_text.as_bytes()).await {
                    Ok(()) => crate::log!(
                        "smtp-smoke: wrote fresh tlb dump to disk ({} bytes)\n",
                        tlb_text.len()
                    ),
                    Err(e) => crate::log!("smtp-smoke: tlb dump write failed {:?}\n", e),
                }

                match send_report_mail(
                    &mut client,
                    "trueos tlb dump",
                    "trueos-tlb-dump",
                    tlb_text.as_str(),
                )
                .await
                {
                    Ok(()) => {
                        crate::log!("smtp-smoke: sent tlb dump mail ({} bytes)\n", tlb_text.len())
                    }
                    Err(e) => crate::log!("smtp-smoke: tlb dump mail failed {:?}\n", e),
                }
            }

            let _ = client.quit(5_000).await;
        }
        Err(e) => {
            crate::log!("smtp-smoke: connect failed {:?}\n", e);
        }
    }

    crate::log!("smtp-smoke: done\n");
}

async fn send_report_mail(
    client: &mut crate::r::net::smtp::SmtpClient,
    subject: &str,
    message_tag: &str,
    body: &str,
) -> Result<(), crate::r::net::smtp::SmtpError> {
    let message = build_message(subject, message_tag, body);
    client
        .send_mail(SMOKE_FROM, &[SMOKE_TO], &message, SMOKE_TIMEOUT_MS)
        .await
}

fn build_message(subject: &str, message_tag: &str, body: &str) -> String {
    let from_domain = SMOKE_FROM.split('@').nth(1).unwrap_or("trueos.local");
    alloc::format!(
        "From: <{}>\nTo: <{}>\nSubject: {}\nDate: Mon, 01 Jan 2024 00:00:00 +0000\nMessage-ID: <{}@{}>\nMIME-Version: 1.0\nContent-Type: text/plain; charset=UTF-8\nContent-Transfer-Encoding: 8bit\n\n{}",
        SMOKE_FROM,
        SMOKE_TO,
        subject,
        message_tag,
        from_domain,
        body
    )
}
