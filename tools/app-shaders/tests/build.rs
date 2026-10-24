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
        ("triangle", "crates/trueos-shader/generated_triangle.rs"),
        ("mask", "crates/trueos-shader/generated_microfont_mask.rs"),
    ] {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
        // This harness supplies package registration explicitly and tests the
        // production accessors, independently of the older capture fixtures.
        let source=fs::read_to_string(root.join(path)).unwrap().replace("#[cfg(test)]", "#[cfg(any())]");
        fs::write(out.join(format!("{name}.rs")), source).unwrap();
    }
    let resources = fs::read_to_string(root.join("src/intel/render/resources.rs")).unwrap();
    let state = fs::read_to_string(root.join("src/intel/render/state.rs")).unwrap();
    let mut upload = String::from("#[derive(Copy, Clone)]\n");
    upload += &item(&state, "struct TriangleShaderStageLayout");
    upload += "\n#[derive(Copy, Clone)]\n";
    upload += &item(&state, "struct TriangleShaderLayout");
    upload += "\n#[derive(Copy, Clone)]\n";
    upload += &item(&resources, "struct StageUploadRange");
    upload += "\nconst TRIANGLE_PS_COLOR_WORDS: [usize; 4] = [1,3,5,7];\n";
    for name in ["upload_triangle_shader_pipeline_at", "optional_stage_range", "stage_range",
        "upload_stage_code", "stage_end", "ranges_overlap", "stage_error",
        "encode_compacted_float_immediate", "specialize_uploaded_triangle_ps_color"] {
        upload += &item(&resources, &format!("fn {name}"));
        upload += "\n";
    }
    fs::write(out.join("upload.rs"), upload).unwrap();
    fs::write(out.join("patch_layout.rs"), "pub(crate) ".to_string() + &item(&shader, "fn patch_cube_upload_layout")).unwrap();
    println!("cargo:rerun-if-changed={}", root.join("src/intel/render/resources.rs").display());
    println!("cargo:rerun-if-changed={}", root.join("src/intel/shader.rs").display());
}

fn item(source: &str, marker: &str) -> String {
    let start=source.find(marker).unwrap();
    let open=start+source[start..].find('{').unwrap();
    let mut depth=1;
    let mut end=open+1;
    while depth>0 {
        match source.as_bytes()[end] { b'{' => depth+=1, b'}' => depth-=1, _ => {} }
        end+=1;
    }
    source[start..end].to_string()
}
