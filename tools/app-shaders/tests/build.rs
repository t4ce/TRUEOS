use std::{env,fs,path::PathBuf};
fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let shader = fs::read_to_string(root.join("src/intel/shader.rs")).unwrap();
    let types = shader.split("#[path = \"../../crates/trueos-shader/generated_patch_cube.rs\"]").next().unwrap();
    fs::write(out.join("types.rs"), types).unwrap();
    for (name,path) in [
        ("patch", "crates/trueos-shader/generated_patch_cube.rs"),
        ("world", "crates/trueos-shader/generated_voxy_headless_tgl.rs"),
        ("figure", "crates/trueos-shader/generated_voxy_figure_gen12.rs"),
        ("cloud", "crates/trueos-shader/generated_voxy_flat_cloud.rs"),
        ("pbr", "picasso/picasso-retained-pbr-forward/pipeline.rs"),
        ("maxpix", "picasso/maxpix-tornado/pipeline.rs"),
        ("adjacency", "crates/trueos-shader/generated_adjacency_gs.rs"),
    ] {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
        // This harness supplies package registration explicitly and tests the
        // production accessors, independently of the older capture fixtures.
        let source=fs::read_to_string(root.join(path)).unwrap().replace("#[cfg(test)]", "#[cfg(any())]");
        fs::write(out.join(format!("{name}.rs")), source).unwrap();
    }
    println!("cargo:rerun-if-changed={}", root.join("src/intel/shader.rs").display());
}
