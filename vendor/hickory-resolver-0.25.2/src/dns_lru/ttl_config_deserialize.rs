use std::collections::HashMap;

use serde::Deserialize;

use hickory_proto::rr::RecordType;

use super::{TtlBounds, TtlConfig};

#[derive(Deserialize)]
pub(super) struct TtlConfigMap(HashMap<TtlConfigField, TtlBounds>);

impl From<TtlConfigMap> for TtlConfig {
    fn from(value: TtlConfigMap) -> Self {
        let mut default = TtlBounds::default();
        let mut by_query_type = HashMap::new();
        for (field, bounds) in value.0.into_iter() {
            match field {
                TtlConfigField::RecordType(record_type) => {
                    by_query_type.insert(record_type, bounds);
                }
                TtlConfigField::Default => default = bounds,
            }
        }
        Self {
            default,
            by_query_type,
        }
    }
}

#[derive(PartialEq, Eq, Hash, Deserialize)]
enum TtlConfigField {
    #[serde(rename = "default")]
    Default,
    #[serde(untagged)]
    RecordType(RecordType),
}
