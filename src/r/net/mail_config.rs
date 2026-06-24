extern crate alloc;

use alloc::string::String;
use serde::{Deserialize, Serialize};
use spin::Mutex;

pub const MAIL_CONFIG_PATH: &str = "mail/config.json";
const MAIL_CONFIG_PASSWORD_PLACEHOLDER: &str = "ENTER_MAIL_PASSWORD_HERE";

static MAIL_RUNTIME_OVERRIDE: Mutex<Option<RuntimeMailConfig>> = Mutex::new(None);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RuntimeMailConfig {
    #[serde(default)]
    pub smtp_user: String,
    #[serde(default)]
    pub smtp_pass: String,
    #[serde(default)]
    pub from: Option<String>,
}

impl RuntimeMailConfig {
    pub fn defaults() -> Self {
        Self {
            smtp_user: String::from(crate::allports::mail::ACCOUNT_EMAIL),
            smtp_pass: String::new(),
            from: Some(String::from(crate::allports::mail::ACCOUNT_EMAIL)),
        }
    }

    pub fn merge_with_defaults(mut self) -> Self {
        let defaults = Self::defaults();
        if self.smtp_user.trim().is_empty() {
            self.smtp_user = defaults.smtp_user;
        }
        if self
            .from
            .as_deref()
            .map(|from| from.trim().is_empty())
            .unwrap_or(true)
        {
            self.from = defaults.from;
        }
        self
    }

    pub fn password_is_placeholder(&self) -> bool {
        self.smtp_pass.trim().is_empty()
            || self.smtp_pass.contains("ENTER_")
            || self.smtp_pass == "password"
    }
}

fn primary_root() -> Result<crate::disc::block::DeviceHandle, &'static str> {
    crate::r::fs::trueosfs::primary_root_handle().ok_or("mail root unavailable")
}

async fn ensure_mail_dir(disk: crate::disc::block::DeviceHandle) -> Result<(), &'static str> {
    match crate::r::fs::trueosfs::file_in_async(disk, "mail/.keep", &[]).await {
        Ok(true) => Ok(()),
        Ok(false) => Err("mail dir create refused"),
        Err(_) => Err("mail dir create failed"),
    }
}

async fn write_template(disk: crate::disc::block::DeviceHandle) -> Result<(), &'static str> {
    ensure_mail_dir(disk).await?;
    let template = serde_json::json!({
        "smtp_user": crate::allports::mail::ACCOUNT_EMAIL,
        "smtp_pass": MAIL_CONFIG_PASSWORD_PLACEHOLDER,
        "from": crate::allports::mail::ACCOUNT_EMAIL,
        "smtp_host": crate::allports::mail::SMTP_HOST,
        "smtp_port": crate::allports::mail::SMTP_PORT,
        "pop3_host": crate::allports::mail::POP3_HOST,
        "pop3_port": crate::allports::mail::POP3_PORT,
        "note": "Fill smtp_pass at runtime. TRUEOS keeps mail provider defaults in allports.rs but does not ship a password."
    });
    let bytes =
        serde_json::to_vec_pretty(&template).map_err(|_| "config template serialize failed")?;
    match crate::r::fs::trueosfs::file_in_async(disk, MAIL_CONFIG_PATH, bytes.as_slice()).await {
        Ok(true) => Ok(()),
        Ok(false) => Err("config template write refused"),
        Err(_) => Err("config template write failed"),
    }
}

pub async fn load_runtime_config() -> Result<RuntimeMailConfig, &'static str> {
    if let Some(config) = MAIL_RUNTIME_OVERRIDE.lock().clone() {
        return Ok(config.merge_with_defaults());
    }

    let disk = primary_root()?;
    match crate::r::fs::trueosfs::file_out_async(disk, MAIL_CONFIG_PATH).await {
        Ok(Some(bytes)) => serde_json::from_slice::<RuntimeMailConfig>(bytes.as_slice())
            .map(|config| config.merge_with_defaults())
            .map_err(|_| "bad mail config"),
        Ok(None) => {
            let _ = write_template(disk).await;
            Ok(RuntimeMailConfig::defaults())
        }
        Err(_) => Err("config read failed"),
    }
}

pub async fn save_runtime_config(config: &RuntimeMailConfig) -> Result<(), &'static str> {
    let disk = primary_root()?;
    ensure_mail_dir(disk).await?;
    let bytes = serde_json::to_vec_pretty(config).map_err(|_| "config serialize failed")?;
    match crate::r::fs::trueosfs::file_in_async(disk, MAIL_CONFIG_PATH, bytes.as_slice()).await {
        Ok(true) => Ok(()),
        Ok(false) => Err("config write refused"),
        Err(_) => Err("config write failed"),
    }
}

pub fn set_runtime_override(
    smtp_user: &str,
    smtp_pass: &str,
    from: Option<&str>,
) -> Result<(), &'static str> {
    let mut config = RuntimeMailConfig {
        smtp_user: if smtp_user.trim().is_empty() {
            String::from(crate::allports::mail::ACCOUNT_EMAIL)
        } else {
            String::from(smtp_user.trim())
        },
        smtp_pass: String::from(smtp_pass.trim()),
        from: from
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(String::from)
            .or_else(|| Some(String::from(crate::allports::mail::ACCOUNT_EMAIL))),
    }
    .merge_with_defaults();

    if config.password_is_placeholder() {
        return Err("mail password missing");
    }

    config.smtp_pass = String::from(config.smtp_pass.trim());
    *MAIL_RUNTIME_OVERRIDE.lock() = Some(config);
    Ok(())
}

pub fn runtime_password_configured() -> bool {
    MAIL_RUNTIME_OVERRIDE
        .lock()
        .as_ref()
        .map(|config| !config.password_is_placeholder())
        .unwrap_or(false)
}

pub async fn runtime_smtp_account() -> Result<(String, String, String), &'static str> {
    let config = load_runtime_config().await?;
    if config.password_is_placeholder() {
        return Err("mail password missing");
    }
    let from = config
        .from
        .clone()
        .unwrap_or_else(|| config.smtp_user.clone());
    Ok((config.smtp_user, config.smtp_pass, from))
}

pub fn redacted_status(config: &RuntimeMailConfig) -> serde_json::Value {
    serde_json::json!({
        "smtp_user": config.smtp_user,
        "from": config.from,
        "passwordConfigured": !config.password_is_placeholder(),
        "smtp_host": crate::allports::mail::SMTP_HOST,
        "smtp_port": crate::allports::mail::SMTP_PORT,
        "pop3_host": crate::allports::mail::POP3_HOST,
        "pop3_port": crate::allports::mail::POP3_PORT,
    })
}
