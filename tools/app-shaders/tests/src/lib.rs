extern crate alloc;
#[path = "../../../../src/gpu/app_shaders.rs"]
pub(crate) mod app_shaders;
mod gpu { pub(crate) use crate::app_shaders; }
#[allow(dead_code, unfulfilled_lint_expectations)]
mod metadata {
    include!(concat!(env!("OUT_DIR"), "/types.rs"));
    pub(crate) mod patch { include!(concat!(env!("OUT_DIR"), "/patch.rs")); }
    pub(crate) mod world { include!(concat!(env!("OUT_DIR"), "/world.rs")); }
    pub(crate) mod figure { include!(concat!(env!("OUT_DIR"), "/figure.rs")); }
    pub(crate) mod cloud { include!(concat!(env!("OUT_DIR"), "/cloud.rs")); }
    pub(crate) mod pbr { include!(concat!(env!("OUT_DIR"), "/pbr.rs")); }
    pub(crate) mod maxpix { include!(concat!(env!("OUT_DIR"), "/maxpix.rs")); }
    pub(crate) mod adjacency { include!(concat!(env!("OUT_DIR"), "/adjacency.rs")); }
}


#[cfg(test)]
mod tests {
    use super::app_shaders;
    fn bundle(owner: &str) -> Vec<u8> {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../..").join(owner).join("shaders/trueos/app-shaders.bundle");
        std::fs::read(path).unwrap()
    }
    #[test]
    fn downloaded_bundles_populate_aligned_shared_storage_once() {
        for owner in ["Cubes", "TRUEOS-Picasso", "voxy"] {
            let package = bundle(owner);
            app_shaders::register_section(&package).unwrap();
            app_shaders::register_section(&package).unwrap();
        }
        for id in 1..=10 { assert!(app_shaders::present(id)); }
        let cube = app_shaders::words(4, 0).unwrap();
        assert_eq!(cube.len(), 188);
        assert_eq!(cube.as_ptr() as usize % 4, 0);
        let pointer = cube.as_ptr();
        app_shaders::register_section(&bundle("Cubes")).unwrap();
        assert_eq!(app_shaders::words(4, 0).unwrap().as_ptr(), pointer);
        assert_eq!(app_shaders::words(4, 1).unwrap().len(), 4596);
        assert_eq!(app_shaders::bytes(9, 0).unwrap().len(), 206768);
        assert_eq!(&app_shaders::bytes(9, 0).unwrap()[..4], b"\x7fELF");
        assert_eq!(app_shaders::bytes(8, 0).unwrap().len(), 44742);
        assert!(app_shaders::words(8, 0).is_none());
        assert!(app_shaders::bytes(4, 0).is_none());
        assert!(app_shaders::words(0, 0).is_none());
        assert!(app_shaders::words(11, 0).is_none());
        for pipeline in [crate::metadata::patch::pipeline(), crate::metadata::world::pipeline(),
            crate::metadata::figure::pipeline(), crate::metadata::cloud::pipeline(),
            crate::metadata::pbr::pipeline(), crate::metadata::maxpix::pipeline()] {
            assert_eq!(pipeline.vs.code.len() * 4, pipeline.vs.meta.kernel.code_size_bytes as usize);
            assert_eq!(pipeline.ps.code.len() * 4, pipeline.ps.meta.kernel.code_size_bytes as usize);
            assert!(!pipeline.vs.code.is_empty());
        }
        assert!(crate::metadata::patch::matches(crate::metadata::patch::pipeline()));
        assert_eq!(crate::metadata::patch::tess_control().len(), 4596);
        assert_eq!(crate::metadata::patch::tess_eval().len(), 3064);
        assert_eq!(crate::metadata::adjacency::line_adjacency_geometry_shader().code.len(), 96);
        assert_eq!(crate::metadata::adjacency::triangle_adjacency_geometry_shader().code.len(), 128);

    }
    #[test]
    fn rejects_corrupt_unknown_and_truncated_downloads() {
        let mut package = bundle("Cubes");
        *package.last_mut().unwrap() ^= 1;
        assert_eq!(app_shaders::register_section(&package), Err("app-shader-sha256-mismatch"));
        let package = bundle("Cubes");
        assert_eq!(app_shaders::register_section(&package[..package.len()-1]), Err("app-shader-truncated"));
        let mut package = bundle("Cubes");
        package[8..12].copy_from_slice(&99u32.to_le_bytes());
        assert_eq!(app_shaders::register_section(&package), Err("app-shader-untrusted-id"));
        assert_eq!(app_shaders::register_section(b"not-a-package"), Err("app-shader-magic"));
    }
}
