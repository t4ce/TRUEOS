//! Direct YAML semantics probe for the TRUEOS std-backed kernel crate set.

extern crate alloc;
extern crate std;

use alloc::vec::Vec;
use serde::Deserialize;

#[derive(Deserialize)]
struct BlueprintFrontmatterProbe {
    name: alloc::string::String,
    tools: Vec<alloc::string::String>,
    enabled: bool,
}

pub(crate) fn log_boot_probe() {
    crate::log!("yaml_probe: wired serde_yaml 0.9.34 std wrapper beside serde_json\n");

    let typed = match serde_yaml::from_str::<BlueprintFrontmatterProbe>(
        "name: localcoder\n\
         tools:\n\
           - grep\n\
           - lsp\n\
         enabled: true\n",
    ) {
        Ok(value) => value,
        Err(err) => {
            crate::log!("yaml_probe: failure frontmatter.typed err={}\n", err);
            return;
        }
    };

    if typed.name != "localcoder"
        || typed.tools.len() != 2
        || typed.tools.first().map(|tool| tool.as_str()) != Some("grep")
        || !typed.enabled
    {
        crate::log!("yaml_probe: failure frontmatter.typed value_mismatch\n");
        return;
    }
    crate::log!("yaml_probe: success frontmatter.typed\n");

    let value = match serde_yaml::from_str::<serde_yaml::Value>("kind: framework\ncount: 3\n") {
        Ok(value) => value,
        Err(err) => {
            crate::log!("yaml_probe: failure value.map err={}\n", err);
            return;
        }
    };

    let kind_ok = value.get("kind").and_then(serde_yaml::Value::as_str) == Some("framework");
    let count_ok = value.get("count").and_then(serde_yaml::Value::as_i64) == Some(3);

    if kind_ok && count_ok {
        crate::log!("yaml_probe: success value.map\n");
    } else {
        crate::log!("yaml_probe: failure value.map kind_ok={} count_ok={}\n", kind_ok, count_ok);
    }
}
