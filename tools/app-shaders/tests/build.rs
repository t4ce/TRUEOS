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

    // Exercise the encoder's admission boundary with the same metadata and
    // app-owned shader bytes used by the kernel, including its empty fallback.
    let contract_path = "src/intel/gpgpu/kernels/artifacts/adls/cpp/helio_retained_transform.contract.rs";
    let schema_path = "src/intel/gpgpu/artifacts/contract.rs";
    let metadata_path = "src/intel/gpgpu/artifacts/metadata.rs";
    let operation_path = "src/intel/gpgpu/operations/helio_retained_transform.rs";
    let encoder_path = "src/intel/gpgpu/rcs/helio_retained_transform.rs";
    let catalog_path = "src/intel/gpgpu/kernel_catalog.rs";
    let constants_path = "src/intel/gpgpu/rcs/constants.rs";
    for path in [contract_path, schema_path, metadata_path, operation_path, encoder_path, catalog_path, constants_path] {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    let schema = fs::read_to_string(root.join(schema_path)).unwrap();
    let mut transform = schema.split("impl GpgpuKernelAbiContract {").next().unwrap().to_string();
    transform += &fs::read_to_string(root.join(contract_path)).unwrap();
    let metadata = fs::read_to_string(root.join(metadata_path)).unwrap();
    for marker in ["struct UploadedKernelArtifact", "enum GpgpuArtifactAddressSpace"] {
        transform += "\n#[derive(Copy, Clone, Debug, Eq, PartialEq)]\n";
        transform += &item(&metadata, marker);
    }
    let operation = fs::read_to_string(root.join(operation_path)).unwrap();
    for marker in ["struct GpgpuHelioRetainedTransformArtifactMapping", "enum GpgpuHelioTransformError"] {
        transform += "\n#[derive(Copy, Clone, Debug, Eq, PartialEq)]\n";
        transform += &item(&operation, marker);
    }
    let catalog = fs::read_to_string(root.join(catalog_path)).unwrap();
    for name in ["HELIO_RETAINED_TRANSFORM_KERNEL_NAME", "HELIO_RETAINED_TRANSFORM_SOURCE_PATH",
        "HELIO_RETAINED_TRANSFORM_ADLS_BIN", "HELIO_RETAINED_TRANSFORM_ADLS_BIN_SHA256"] {
        transform += &constant(&catalog, name);
    }
    transform += &constant(&fs::read_to_string(root.join(constants_path)).unwrap(), "HELIO_RETAINED_TRANSFORM_ADLS_GPU");
    transform += &item(&fs::read_to_string(root.join(encoder_path)).unwrap(), "fn validate_helio_transform_artifact");
    fs::write(out.join("retained_transform.rs"), transform).unwrap();
}

fn constant(source: &str, name: &str) -> String {
    let marker = format!("const {name}:");
    let start = source.find(&marker).unwrap();
    let value = start + source[start..].find('=').unwrap();
    let end = value + source[value..].find(';').unwrap() + 1;
    format!("\n{}\n", &source[start..end])
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
