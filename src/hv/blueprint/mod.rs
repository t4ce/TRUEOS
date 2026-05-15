mod prebind;

pub(crate) use prebind::{
    prebind_base_readiness, prebind_import_readiness, prebind_required_readiness,
};

include!("blueprint.rs");
