mod firmware;

use sha2::{Digest, Sha256};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use trueos_fpga_abi::{
    ABI_VERSION, FIRMWARE_MANIFEST_MAGIC, FUNCTION_COUNT, FirmwareManifest, FunctionDescriptor,
    WorkPackage,
};

use firmware::{BindingKind, FUNCTIONS, FunctionSpec};

const Q8_DOT32_RTL: &str = include_str!("../../../src/compute/truega_q8_0_dot32.v");
const Q8_SCALE_SEQ_RTL: &str = include_str!("../../../src/compute/truega_q8_0_scale_q30_seq.v");
const Q8_BLOCK_SLOT_RTL: &str = include_str!("../../../src/compute/truega_q8_0_block_slot.v");
const Q8_GOLDEN_ARTIFACT: &[u8] = include_bytes!("../../../artifacts/lfm25_q8_block.golden.bin");
const LFM25_FFN_GOLDEN: &[u8] = include_bytes!("../../../artifacts/lfm25_layer0_ffn.golden.bin");
const LFM25_FFN_VECTORS: &[u8] =
    include_bytes!("../../../artifacts/lfm25_layer0_ffn.golden.bin.vectors");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GoldenQ8Block {
    activation: [u8; 34],
    weight: [u8; 34],
    dot: i32,
    term_q30: i64,
}

struct Config {
    rtl_out: PathBuf,
    manifest_out: PathBuf,
    rust_interface_out: PathBuf,
}

fn main() {
    let cfg = parse_args().unwrap_or_else(|error| {
        eprintln!("{error}");
        eprintln!(
            "usage: tga-gen [--rtl-out FILE] [--manifest-out FILE] [--rust-interface-out FILE]"
        );
        std::process::exit(2);
    });

    validate_catalogue().unwrap_or_else(|error| {
        eprintln!("invalid TRUEGA v1 function catalogue: {error}");
        std::process::exit(2);
    });

    let function_rtl = assembled_function_rtl();
    let firmware_hash: [u8; 32] = Sha256::digest(function_rtl.as_bytes()).into();
    let manifest = manifest_bytes(firmware_hash);
    let rtl = format!("{function_rtl}\n{}", emit_manifest_verilog(&manifest));
    let golden = parse_golden_q8_block().unwrap_or_else(|error| {
        eprintln!("invalid sealed Q8_0 runtime fixture: {error}");
        std::process::exit(2);
    });
    let rust_interface = emit_rust_interface(firmware_hash, golden);

    write_if_changed(&cfg.rtl_out, rtl.as_bytes());
    write_if_changed(&cfg.manifest_out, &manifest);
    write_if_changed(&cfg.rust_interface_out, rust_interface.as_bytes());

    println!(
        "generated rtl={} manifest={} rust_interface={} functions={} sha256={}",
        cfg.rtl_out.display(),
        cfg.manifest_out.display(),
        cfg.rust_interface_out.display(),
        FUNCTIONS.len(),
        hex(&firmware_hash)
    );
}

fn validate_catalogue() -> Result<(), String> {
    for (index, spec) in FUNCTIONS.iter().enumerate() {
        if spec.id as usize != index {
            return Err(format!("slot {index} has non-contiguous wire id {}", spec.id));
        }
        if spec.input_bytes as usize > trueos_fpga_abi::INLINE_INPUT_BYTES
            || spec.output_bytes as usize > trueos_fpga_abi::INLINE_OUTPUT_BYTES
        {
            return Err(format!(
                "slot {} uses input/output shape {}/{} beyond the 96-byte envelopes",
                spec.id, spec.input_bytes, spec.output_bytes
            ));
        }
        let expected_shape = match spec.binding {
            BindingKind::NoArgsU32 => (0, 4),
            BindingKind::BinaryU32 => (8, 4),
            BindingKind::Lfm25Q8Block => (68, 12),
        };
        if (spec.input_bytes, spec.output_bytes) != expected_shape {
            return Err(format!(
                "slot {} binding {:?} needs input/output shape {}/{}",
                spec.id, spec.binding, expected_shape.0, expected_shape.1
            ));
        }
    }
    Ok(())
}

fn parse_args() -> Result<Config, String> {
    let mut rtl_out = PathBuf::from("src/generated/truega_functions.v");
    let mut manifest_out = PathBuf::from("artifacts/truega_firmware.manifest.bin");
    let mut rust_interface_out = PathBuf::from("../src/generated.rs");
    let mut args = env::args().skip(1);

    while let Some(argument) = args.next() {
        let destination = match argument.as_str() {
            "--rtl-out" => &mut rtl_out,
            "--manifest-out" => &mut manifest_out,
            "--rust-interface-out" => &mut rust_interface_out,
            "-h" | "--help" => return Err(String::new()),
            _ => return Err(format!("unknown argument {argument:?}")),
        };
        *destination = PathBuf::from(
            args.next()
                .ok_or_else(|| format!("{argument} needs a value"))?,
        );
    }

    Ok(Config {
        rtl_out,
        manifest_out,
        rust_interface_out,
    })
}

fn rename_rust_hdl_scalar_top(verilog: String) -> String {
    verilog
        .replace("top$", "truega_scalar_functions$")
        .replacen("module top(", "module truega_scalar_functions(", 1)
}

fn assembled_function_rtl() -> String {
    let scalar = rename_rust_hdl_scalar_top(firmware::generate());
    format!(
        "{scalar}\n{}\n\n// Exact native Q8_0 compute sources fused into this generated bundle.\n{}\n{}\n{}\n",
        emit_slot_wrapper_verilog(),
        Q8_DOT32_RTL,
        Q8_SCALE_SEQ_RTL,
        Q8_BLOCK_SLOT_RTL,
    )
}

fn emit_slot_wrapper_verilog() -> &'static str {
    r#"// Common clocked handoff for all three ahead-of-time function slots.
module truega_functions(
    input  wire         clk,
    input  wire         reset_n,
    input  wire         start,
    input  wire [15:0]  function_id,
    input  wire [767:0] input_data,
    input  wire [4:0]   led_state,
    output reg  [4:0]   next_led,
    output reg  [767:0] output_data,
    output reg  [15:0]  required_input_bytes,
    output reg  [15:0]  output_bytes,
    output reg          valid,
    output reg          busy,
    output reg          done,
    output reg          error
);
    wire [4:0] scalar_next_led;
    wire [31:0] scalar_result;
    wire [15:0] scalar_required_input_bytes;
    wire [15:0] scalar_output_bytes;
    wire scalar_valid;
    reg [15:0] active_function;
    reg q8_start;
    wire q8_busy;
    wire q8_done;
    wire signed [31:0] q8_dot;
    wire signed [63:0] q8_term_q30;
    wire q8_scale_error;

    truega_scalar_functions scalar_functions(
        .function_id(function_id),
        .arg0(input_data[31:0]),
        .arg1(input_data[63:32]),
        .led_state(led_state),
        .next_led(scalar_next_led),
        .result(scalar_result),
        .required_input_bytes(scalar_required_input_bytes),
        .output_bytes(scalar_output_bytes),
        .valid(scalar_valid)
    );

    truega_q8_0_block_slot q8_block_slot(
        .clk(clk),
        .reset_n(reset_n),
        .start_i(q8_start),
        .activation_block_i(input_data[271:0]),
        .weight_block_i(input_data[543:272]),
        .busy_o(q8_busy),
        .done_o(q8_done),
        .dot_o(q8_dot),
        .term_q30_o(q8_term_q30),
        .scale_error_o(q8_scale_error)
    );

    always @* begin
        required_input_bytes = 16'd0;
        output_bytes = 16'd0;
        valid = 1'b0;
        case (function_id)
            16'd0: begin
                required_input_bytes = 16'd0;
                output_bytes = 16'd4;
                valid = 1'b1;
            end
            16'd1: begin
                required_input_bytes = 16'd8;
                output_bytes = 16'd4;
                valid = 1'b1;
            end
            16'd2: begin
                required_input_bytes = 16'd68;
                output_bytes = 16'd12;
                valid = 1'b1;
            end
            default: begin end
        endcase
    end

    always @(posedge clk) begin
        if (!reset_n) begin
            active_function <= 16'd0;
            q8_start <= 1'b0;
            next_led <= 5'b00001;
            output_data <= 768'd0;
            busy <= 1'b0;
            done <= 1'b0;
            error <= 1'b0;
        end else begin
            q8_start <= 1'b0;
            done <= 1'b0;
            if (start && !busy) begin
                active_function <= function_id;
                output_data <= 768'd0;
                next_led <= led_state;
                error <= 1'b0;
                busy <= 1'b1;
                case (function_id)
                    16'd0, 16'd1: begin
                        output_data[31:0] <= scalar_result;
                        next_led <= scalar_next_led;
                        busy <= 1'b0;
                        done <= 1'b1;
                    end
                    16'd2: begin
                        q8_start <= 1'b1;
                    end
                    default: begin
                        busy <= 1'b0;
                        done <= 1'b1;
                        error <= 1'b1;
                    end
                endcase
            end else if (busy && active_function == 16'd2 && q8_done) begin
                output_data <= 768'd0;
                output_data[31:0] <= q8_dot;
                output_data[95:32] <= q8_term_q30;
                busy <= 1'b0;
                done <= 1'b1;
                error <= q8_scale_error;
            end
        end
    end
endmodule"#
}

fn parse_golden_q8_block() -> Result<GoldenQ8Block, String> {
    const BYTES: usize = 336;
    const SEAL_OFFSET: usize = 0xC0;
    const INPUT_OFFSET: usize = 0x100;
    const OUTPUT_OFFSET: usize = 0x144;

    let bytes = Q8_GOLDEN_ARTIFACT;
    if bytes.len() != BYTES
        || bytes.get(..8) != Some(b"TGAQ8B01")
        || artifact_u16(bytes, 0x08)? != 1
        || artifact_u16(bytes, 0x0A)? != 256
        || artifact_u16(bytes, 0x0C)? != 68
        || artifact_u16(bytes, 0x0E)? != 12
        || artifact_u32(bytes, 0x10)? != 1
        || artifact_u16(bytes, 0x14)? != 4
        || bytes[0x16] != 0
        || bytes[0x17] != 3
        || artifact_u32(bytes, 0x18)? != 0
        || artifact_u32(bytes, 0x1C)? != 0
        || artifact_u32(bytes, 0xE0)? != 0x048C_9000
        || artifact_u32(bytes, 0xE4)? as usize != INPUT_OFFSET
        || artifact_u32(bytes, 0xE8)? as usize != OUTPUT_OFFSET
        || artifact_u16(bytes, 0xEC)? != 0
        || artifact_u16(bytes, 0xEE)? != 1
        || bytes[0xF0..0x100] != [0; 16]
    {
        return Err("header or canonical coordinate mismatch".into());
    }

    let ffn_hash: [u8; 32] = Sha256::digest(LFM25_FFN_GOLDEN).into();
    let vector_hash: [u8; 32] = Sha256::digest(LFM25_FFN_VECTORS).into();
    if bytes[0x20..0x40] != ffn_hash
        || bytes[0x40..0x60] != trueos_fpga_abi::lfm25::PINNED_NATIVE_IMAGE_SHA256
        || bytes[0x60..0x80] != trueos_fpga_abi::lfm25::generated::MODEL_CONTRACT_SHA256
        || bytes[0x80..0xA0] != vector_hash
    {
        return Err("provenance hash mismatch".into());
    }

    let payload_hash: [u8; 32] = Sha256::digest(&bytes[INPUT_OFFSET..BYTES]).into();
    if bytes[0xA0..0xC0] != payload_hash {
        return Err("payload hash mismatch".into());
    }
    let mut seal_view = bytes.to_vec();
    seal_view[SEAL_OFFSET..SEAL_OFFSET + 32].fill(0);
    let seal: [u8; 32] = Sha256::digest(&seal_view).into();
    if bytes[SEAL_OFFSET..SEAL_OFFSET + 32] != seal {
        return Err("self-seal mismatch".into());
    }

    let input: [u8; 68] = bytes[INPUT_OFFSET..OUTPUT_OFFSET].try_into().unwrap();
    let output: [u8; 12] = bytes[OUTPUT_OFFSET..BYTES].try_into().unwrap();
    let activation = input[..34].try_into().unwrap();
    let weight = input[34..].try_into().unwrap();
    let dot = i32::from_le_bytes(output[..4].try_into().unwrap());
    let term_q30 = i64::from_le_bytes(output[4..].try_into().unwrap());
    Ok(GoldenQ8Block {
        activation,
        weight,
        dot,
        term_q30,
    })
}

fn artifact_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| format!("truncated u16 at {offset:#x}"))?;
    Ok(u16::from_le_bytes(value.try_into().unwrap()))
}

fn artifact_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| format!("truncated u32 at {offset:#x}"))?;
    Ok(u32::from_le_bytes(value.try_into().unwrap()))
}

fn descriptor(spec: FunctionSpec) -> FunctionDescriptor {
    FunctionDescriptor {
        id: spec.id,
        input_bytes: spec.input_bytes,
        output_bytes: spec.output_bytes,
        flags: 0,
        symbol_hash: fnv1a64(spec.signature.as_bytes()),
    }
}

fn manifest_bytes(firmware_hash: [u8; 32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(core::mem::size_of::<FirmwareManifest>());
    bytes.extend_from_slice(&FIRMWARE_MANIFEST_MAGIC.to_le_bytes());
    bytes.extend_from_slice(&ABI_VERSION.to_le_bytes());
    bytes.extend_from_slice(&(FUNCTION_COUNT as u16).to_le_bytes());
    bytes.extend_from_slice(&(core::mem::size_of::<WorkPackage>() as u32).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&firmware_hash);
    for spec in FUNCTIONS {
        let function = descriptor(spec);
        bytes.extend_from_slice(&function.id.to_le_bytes());
        bytes.extend_from_slice(&function.input_bytes.to_le_bytes());
        bytes.extend_from_slice(&function.output_bytes.to_le_bytes());
        bytes.extend_from_slice(&function.flags.to_le_bytes());
        bytes.extend_from_slice(&function.symbol_hash.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 32]);
    assert_eq!(bytes.len(), core::mem::size_of::<FirmwareManifest>());
    bytes
}

fn emit_manifest_verilog(manifest: &[u8]) -> String {
    assert_eq!(manifest.len(), core::mem::size_of::<FirmwareManifest>());
    let mut verilog = String::from(
        "// Read-only build manifest paired with the generated host interface.\n\
module truega_firmware_manifest(word_index,data);\n\
    input wire [4:0] word_index;\n\
    output reg [31:0] data;\n\
    always @(*) begin\n\
        data = 32'h00000000;\n\
        case (word_index)\n",
    );
    for (index, bytes) in manifest.chunks_exact(4).enumerate() {
        let word = u32::from_le_bytes(bytes.try_into().unwrap());
        writeln!(verilog, "            5'd{index}: data = 32'h{word:08X};").unwrap();
    }
    verilog.push_str(
        "            default: data = 32'h00000000;\n\
        endcase\n\
    end\n\
endmodule\n",
    );
    verilog
}

fn emit_rust_interface(firmware_hash: [u8; 32], golden: GoldenQ8Block) -> String {
    let mut rust = String::new();
    rust.push_str("// @generated by TRUEGA tools/tga-gen; do not hand-edit.\n");
    rust.push_str(
        "// This is ordinary host Rust metadata. It contains no HDL/compiler runtime.\n\n",
    );
    rust.push_str("use super::{FirmwareManifest, FunctionDescriptor, FunctionId};\n\n");
    for spec in FUNCTIONS {
        writeln!(rust, "pub const {}: FunctionId = FunctionId::SLOT_{};", spec.rust_name, spec.id)
            .unwrap();
    }
    writeln!(
        rust,
        "pub const HEARTBEAT_REPLY: u32 = 0x{:08X}; // \"TGAT\"",
        firmware::HEARTBEAT_REPLY
    )
    .unwrap();
    rust.push_str("pub const FIRMWARE_RTL_SHA256: [u8; 32] = [\n    ");
    for (index, byte) in firmware_hash.iter().enumerate() {
        write!(rust, "0x{byte:02x},").unwrap();
        if index % 16 == 15 {
            rust.push('\n');
            if index + 1 != firmware_hash.len() {
                rust.push_str("    ");
            }
        } else {
            rust.push(' ');
        }
    }
    rust.push_str("];\n\n");
    rust.push_str("pub const FUNCTIONS: [FunctionDescriptor; 3] = [\n");
    for spec in FUNCTIONS {
        rust.push_str("    FunctionDescriptor {\n");
        writeln!(rust, "        id: {}.raw(),", spec.rust_name).unwrap();
        writeln!(rust, "        input_bytes: {},", spec.input_bytes).unwrap();
        writeln!(rust, "        output_bytes: {},", spec.output_bytes).unwrap();
        rust.push_str("        flags: 0,\n");
        writeln!(rust, "        symbol_hash: 0x{:016x},", fnv1a64(spec.signature.as_bytes()))
            .unwrap();
        rust.push_str("    },\n");
    }
    rust.push_str("];\n\n");
    rust.push_str("pub const FIRMWARE_MANIFEST: FirmwareManifest = FirmwareManifest {\n");
    rust.push_str("    magic: super::FIRMWARE_MANIFEST_MAGIC,\n");
    rust.push_str("    abi_version: super::ABI_VERSION,\n");
    rust.push_str("    function_count: super::FUNCTION_COUNT as u16,\n");
    rust.push_str("    work_package_bytes: core::mem::size_of::<super::WorkPackage>() as u32,\n");
    rust.push_str("    flags: 0,\n");
    rust.push_str("    rtl_sha256: FIRMWARE_RTL_SHA256,\n");
    rust.push_str("    functions: FUNCTIONS,\n");
    rust.push_str("    reserved: [0; 32],\n");
    rust.push_str("};\n\n");
    for spec in FUNCTIONS {
        writeln!(rust, "pub mod {} {{", spec.rust_module).unwrap();
        match spec.binding {
            BindingKind::NoArgsU32 | BindingKind::BinaryU32 => {
                rust.push_str("    use super::{FunctionId, result_u32};\n\n");
            }
            BindingKind::Lfm25Q8Block => {
                rust.push_str("    use super::FunctionId;\n\n");
            }
        }
        writeln!(rust, "    pub const ID: FunctionId = super::{};", spec.rust_name).unwrap();
        writeln!(rust, "    pub const INPUT_BYTES: usize = {};", spec.input_bytes).unwrap();
        writeln!(rust, "    pub const OUTPUT_BYTES: usize = {};", spec.output_bytes).unwrap();
        match spec.binding {
            BindingKind::NoArgsU32 => {
                rust.push_str("    pub const fn encode() -> [u8; 0] {\n        []\n    }\n");
                rust.push_str(
                    "    pub fn decode(bytes: &[u8]) -> Option<u32> {\n        result_u32(bytes)\n    }\n",
                );
            }
            BindingKind::BinaryU32 => {
                rust.push_str("    pub fn encode(a: u32, b: u32) -> [u8; 8] {\n");
                rust.push_str("        let mut bytes = [0; 8];\n");
                rust.push_str("        bytes[..4].copy_from_slice(&a.to_le_bytes());\n");
                rust.push_str("        bytes[4..].copy_from_slice(&b.to_le_bytes());\n");
                rust.push_str("        bytes\n");
                rust.push_str("    }\n");
                rust.push_str(
                    "    pub fn decode(bytes: &[u8]) -> Option<u32> {\n        result_u32(bytes)\n    }\n",
                );
            }
            BindingKind::Lfm25Q8Block => {
                rust.push_str("    pub const Q8_0_BLOCK_BYTES: usize = 34;\n\n");
                rust.push_str("    #[derive(Copy, Clone, Debug, Eq, PartialEq)]\n");
                rust.push_str("    pub struct Q8BlockResult {\n");
                rust.push_str("        pub dot: i32,\n");
                rust.push_str("        pub term_q30: i64,\n");
                rust.push_str("    }\n\n");
                rust.push_str(
                    "    pub fn encode(\n        activation: &[u8; Q8_0_BLOCK_BYTES],\n        weight: &[u8; Q8_0_BLOCK_BYTES],\n    ) -> [u8; INPUT_BYTES] {\n",
                );
                rust.push_str("        let mut bytes = [0; INPUT_BYTES];\n");
                rust.push_str("        bytes[..Q8_0_BLOCK_BYTES].copy_from_slice(activation);\n");
                rust.push_str("        bytes[Q8_0_BLOCK_BYTES..].copy_from_slice(weight);\n");
                rust.push_str("        bytes\n");
                rust.push_str("    }\n\n");
                rust.push_str("    pub fn decode(bytes: &[u8]) -> Option<Q8BlockResult> {\n");
                rust.push_str(
                    "        Some(Q8BlockResult {\n            dot: i32::from_le_bytes(bytes.get(..4)?.try_into().ok()?),\n            term_q30: i64::from_le_bytes(bytes.get(4..12)?.try_into().ok()?),\n        })\n",
                );
                rust.push_str("    }\n\n");
                emit_rust_byte_array(
                    &mut rust,
                    "    pub const GOLDEN_ACTIVATION: [u8; Q8_0_BLOCK_BYTES] = ",
                    &golden.activation,
                );
                emit_rust_byte_array(
                    &mut rust,
                    "    pub const GOLDEN_WEIGHT: [u8; Q8_0_BLOCK_BYTES] = ",
                    &golden.weight,
                );
                writeln!(
                    rust,
                    "    pub const GOLDEN_RESULT: Q8BlockResult = Q8BlockResult {{\n        dot: {},\n        term_q30: {},\n    }};",
                    golden.dot, golden.term_q30
                )
                .unwrap();
            }
        }
        rust.push_str("}\n\n");
    }
    rust.push_str("pub fn binary_u32_args(a: u32, b: u32) -> [u8; 8] {\n");
    rust.push_str("    add_u32::encode(a, b)\n");
    rust.push_str("}\n\n");
    rust.push_str("pub fn result_u32(bytes: &[u8]) -> Option<u32> {\n");
    rust.push_str("    Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))\n");
    rust.push_str("}\n");
    rust
}

fn emit_rust_byte_array(rust: &mut String, declaration: &str, bytes: &[u8]) {
    rust.push_str(declaration);
    rust.push_str("[\n        ");
    for (index, byte) in bytes.iter().enumerate() {
        write!(rust, "0x{byte:02x},").unwrap();
        if index % 15 == 14 || index + 1 == bytes.len() {
            rust.push('\n');
            if index + 1 != bytes.len() {
                rust.push_str("        ");
            }
        } else {
            rust.push(' ');
        }
    }
    rust.push_str("    ];\n");
}

fn write_if_changed(path: &Path, contents: &[u8]) {
    if fs::read(path).ok().as_deref() == Some(contents) {
        return;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create generated artifact directory");
    }
    fs::write(path, contents).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut index = 0;
    while index < bytes.len() {
        hash ^= bytes[index] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        index += 1;
    }
    hash
}

fn hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(value, "{byte:02x}").unwrap();
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOP_VHDL: &str = include_str!("../../../src/top.vhd");
    const BOARD_CST: &str = include_str!("../../../src/min_pci_led.cst");
    const BOARD_SDC: &str = include_str!("../../../src/min_pci_led.sdc");
    const PCIE_PLL: &str = include_str!("../../../src/gowin_pll/gowin_pll.v");

    #[test]
    fn binary_manifest_matches_shared_abi_layout() {
        let bytes = manifest_bytes([0xA5; 32]);
        assert_eq!(bytes.len(), core::mem::size_of::<FirmwareManifest>());
        assert_eq!(&bytes[..4], &FIRMWARE_MANIFEST_MAGIC.to_le_bytes());
        assert_eq!(&bytes[4..6], &ABI_VERSION.to_le_bytes());
        assert_eq!(&bytes[6..8], &(FUNCTION_COUNT as u16).to_le_bytes());
    }

    #[test]
    fn generated_interface_has_exactly_three_slots() {
        let golden = parse_golden_q8_block().unwrap();
        let interface = emit_rust_interface([0; 32], golden);
        assert!(interface.contains("HEARTBEAT"));
        assert!(interface.contains("ADD_U32"));
        assert!(interface.contains("LFM25_Q8_BLOCK"));
        assert!(interface.contains("pub const FIRMWARE_MANIFEST: FirmwareManifest"));
        assert!(interface.contains("pub mod led_step_heartbeat"));
        assert!(interface.contains("pub mod add_u32"));
        assert!(interface.contains("pub mod lfm25_q8_block"));
        assert!(interface.contains("pub struct Q8BlockResult"));
        assert!(interface.contains("pub const GOLDEN_ACTIVATION"));
        assert_eq!(FUNCTIONS.len(), 3);
    }

    #[test]
    fn v1_catalogue_is_contiguous_and_physically_supported() {
        validate_catalogue().unwrap();
        assert_eq!(FUNCTIONS.map(|function| function.id), [0, 1, 2]);
        assert_eq!(FUNCTIONS.map(|function| function.input_bytes), [0, 8, 68]);
        assert_eq!(FUNCTIONS.map(|function| function.output_bytes), [4, 4, 12]);
    }

    #[test]
    fn sealed_q8_block_fixture_is_canonical() {
        let golden = parse_golden_q8_block().unwrap();
        assert_eq!(golden.activation[..4], [0x30, 0x18, 0x0d, 0xa8]);
        assert_eq!(golden.weight[..4], [0xb9, 0x0c, 0x7a, 0x14]);
        assert_eq!(golden.dot, -14_901);
        assert_eq!(golden.term_q30, -9_429_888);
    }

    #[test]
    fn generated_rtl_binds_compute_and_common_handoff() {
        let rtl = assembled_function_rtl();
        assert!(rtl.contains("module truega_functions("));
        assert!(rtl.contains("input  wire [767:0] input_data"));
        assert!(rtl.contains("module truega_q8_0_dot32"));
        assert!(rtl.contains("module truega_q8_0_scale_q30_seq"));
        assert!(rtl.contains("module truega_q8_0_block_slot"));
        assert!(rtl.contains("output reg          busy"));
        assert!(rtl.contains("output reg          done"));
    }

    #[test]
    fn generated_manifest_rom_contains_the_binary_manifest_words() {
        let manifest = manifest_bytes([0xA5; 32]);
        let verilog = emit_manifest_verilog(&manifest);
        assert!(verilog.contains("module truega_firmware_manifest"));
        assert!(verilog.contains("5'd0: data = 32'h4D465754;"));
        assert!(verilog.contains("5'd4: data = 32'hA5A5A5A5;"));
        assert!(verilog.contains("5'd31: data = 32'h00000000;"));
    }

    #[test]
    fn gowin_completion_uses_the_high_dword_lanes() {
        // IPUG1020 Figure 3-1 numbers TL dwords from [255:224] downward.
        // A four-dword completion therefore occupies the high half with F0 valid.
        for required in [
            "tx_pending_data(255 downto 224) <= dw0;",
            "tx_pending_data(223 downto 192) <= dw1;",
            "tx_pending_data(191 downto 160) <= dw2;",
            "tx_pending_data(159 downto 128) <= byte_swap32(data_in);",
            "tx_pending_valid <= \"11110000\";",
        ] {
            assert!(TOP_VHDL.contains(required), "missing Gowin TL layout: {required}");
        }
        assert!(!TOP_VHDL.contains("tx_pending_valid <= \"00001111\";"));
    }

    #[test]
    fn gowin_completion_appends_function_zero_to_bus_device() {
        // IPUG1020 defines tl_cfg_busdev as Bus[12:5], Device[4:0]. PCIe's
        // 16-bit Completer ID adds Function[2:0] after those thirteen bits.
        assert!(TOP_VHDL.contains("dw1(31 downto 16) := tl_cfg_busdev & \"000\";"));
        assert!(!TOP_VHDL.contains("dw1(31 downto 16) := \"000\" & tl_cfg_busdev;"));
    }

    #[test]
    fn tang_mega_pro_generates_the_required_tlp_clock() {
        assert!(BOARD_CST.contains("IO_LOC \"clk\" P16;"));
        assert!(BOARD_SDC.contains("create_clock -name board_clk -period 20.000"));
        assert!(PCIE_PLL.contains("defparam PLL_inst.FCLKIN = \"50\";"));
        assert!(PCIE_PLL.contains(".DIV_MODE(\"2\")"));
        assert!(TOP_VHDL.contains("PCIE_Controller_Top_pcie_tl_clk_i        => tlp_clk,"));
        assert!(!TOP_VHDL.contains("PCIE_Controller_Top_pcie_tl_clk_i        => clk,"));
    }

    #[test]
    fn completion_is_held_until_the_controller_accepts_it() {
        assert!(TOP_VHDL.contains(
            "tl_tx_valid <= tx_pending_valid when tx_pending = '1' else (others => '0');"
        ));
        assert!(TOP_VHDL.contains("if (tx_pending = '1') and (tl_tx_wait = '0') then"));
        assert!(!TOP_VHDL.contains("next_tx_valid := tx_pending_valid;"));
    }

    #[test]
    fn bar_payloads_are_swapped_at_the_gowin_tlp_boundary() {
        assert!(TOP_VHDL.contains("payload_out := byte_swap32(payload);"));
        assert!(TOP_VHDL.contains("tx_pending_data(159 downto 128) <= byte_swap32(data_in);"));
    }

    #[test]
    fn final_image_gives_leds_to_the_fused_heartbeat_function() {
        assert!(TOP_VHDL.contains("signal debug_led_mode : std_logic := '0';"));
        assert!(TOP_VHDL.contains("debug_led_mode <= '0';"));
        assert!(TOP_VHDL.contains("signal led_reg : std_logic_vector(4 downto 0) := \"00001\";"));
        assert!(TOP_VHDL.contains("led_reg <= function_next_led;"));
        assert!(TOP_VHDL.contains("usr_led0 <= not led_reg(0);"));
        assert!(!TOP_VHDL.contains("signal debug_led_mode : std_logic := '1';"));
    }

    #[test]
    fn final_image_exposes_the_generated_manifest_read_only() {
        assert!(TOP_VHDL.contains("component truega_firmware_manifest is"));
        assert!(
            TOP_VHDL.contains("constant BAR0_FIRMWARE_MANIFEST_BASE_DW : integer := 16#200# / 4;")
        );
        assert!(TOP_VHDL.contains("read_data_dw := firmware_manifest_word;"));
    }

    #[test]
    fn final_image_physically_backs_both_96_byte_envelopes() {
        assert!(TOP_VHDL.contains("type call_data_arr_t is array (0 to 23)"));
        assert!(TOP_VHDL.contains(
            "call_input_words(addr_index - BAR0_CALL_BASE_DW - CALL_INPUT_WORD) <= payload_dw;"
        ));
        assert!(TOP_VHDL.contains(
            "call_output_words(addr_index - BAR0_CALL_BASE_DW - CALL_OUTPUT_WORD) <= payload_dw;"
        ));
        assert!(TOP_VHDL.contains("elsif (call_active = '1') and (function_done = '1') then"));
        assert!(TOP_VHDL.contains("function_start <= '1';"));
    }
}
