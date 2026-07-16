#[path = "runtime_config_parse.rs"]
mod parse;

pub(crate) use parse::RuntimeConfig;

use v::sync::LazyLock;

pub(crate) const PATH: &str = "trueos/lsdconf.xdg";
const MAX_BYTES: usize = 64 * 1024;

static LATCHED: LazyLock<RuntimeConfig> = LazyLock::new(load);

pub(crate) fn latched() -> &'static RuntimeConfig {
    LazyLock::force(&LATCHED)
}

fn load() -> RuntimeConfig {
    match v::vfs::read_file_utf8(PATH.as_bytes()) {
        Ok(text) if text.len() <= MAX_BYTES => RuntimeConfig::parse(text.as_str()),
        _ => RuntimeConfig::default(),
    }
}
