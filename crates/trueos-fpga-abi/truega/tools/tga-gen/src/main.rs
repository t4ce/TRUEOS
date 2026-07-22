mod firmware;

use sha2::{Digest, Sha256};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use trueos_fpga_abi::{
    FirmwareManifest, FunctionDescriptor, WorkPackage, ABI_VERSION, FIRMWARE_MANIFEST_MAGIC,
    FUNCTION_COUNT,
};

use firmware::{BindingKind, FunctionSpec, FUNCTIONS};

const Q8_DOT32_RTL: &str = include_str!("../../../src/compute/truega_q8_0_dot32.v");
const Q8_SCALE_SEQ_RTL: &str = include_str!("../../../src/compute/truega_q8_0_scale_q30_seq.v");
const Q8_BLOCK_SLOT_RTL: &str = include_str!("../../../src/compute/truega_q8_0_block_slot.v");
const Q8_ROW_BLOCK_SLOT_RTL: &str =
    include_str!("../../../src/compute/truega_q8_0_row_block_slot.v");
const Q8_CACHED_PAIR_SLOT_RTL: &str =
    include_str!("../../../src/compute/truega_q8_0_cached_pair_slot.v");
const LFM25_SILU_SLOT_RTL: &str = include_str!("../../../src/compute/truega_lfm25_silu_q30_slot.v");
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GoldenQ8Row {
    activations: [[u8; 34]; 32],
    dots: [i32; 32],
    terms_q30: [i64; 32],
    row_q30: i64,
    fp_q30: i64,
    fp_bound_q30: i64,
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
    let golden_row = parse_golden_gate_row().unwrap_or_else(|error| {
        eprintln!("invalid sealed layer-0 gate-row fixture: {error}");
        std::process::exit(2);
    });
    let rust_interface = emit_rust_interface(firmware_hash, golden, golden_row);

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
            BindingKind::Lfm25Q8RowBlock => (72, 20),
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
        "{scalar}\n{}\n\n// Exact native Q8_0/FFN compute sources fused into this generated bundle.\n{}\n{}\n{}\n{}\n{}\n{}\n",
        emit_slot_wrapper_verilog(),
        Q8_DOT32_RTL,
        Q8_SCALE_SEQ_RTL,
        Q8_BLOCK_SLOT_RTL,
        Q8_ROW_BLOCK_SLOT_RTL,
        Q8_CACHED_PAIR_SLOT_RTL,
        LFM25_SILU_SLOT_RTL,
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
    wire signed [63:0] q8_row_q30;
    wire q8_scale_error;
    reg silu_start;
    wire silu_busy;
    wire silu_done;
    wire silu_error;
    wire signed [63:0] silu_result_q30;
    reg active_silu;

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

    truega_q8_0_cached_pair_slot #(
        .CACHED_PAIR_ENABLE(1)
    ) q8_row_block_slot(
        .clk(clk),
        .reset_n(reset_n),
        .start_i(q8_start),
        .control_i(input_data[31:0]),
        .activation_block_i(input_data[303:32]),
        .weight_block_i(input_data[575:304]),
        .busy_o(q8_busy),
        .done_o(q8_done),
        .dot_o(q8_dot),
        .term_q30_o(q8_term_q30),
        .row_q30_o(q8_row_q30),
        .error_o(q8_scale_error)
    );

    truega_lfm25_silu_q30_slot #(
        .SILU_ENABLE(1)
    ) silu_slot(
        .clk(clk),
        .reset_n(reset_n),
        .start_i(silu_start),
        .gate_q30_i(input_data[95:32]),
        .up_q30_i(input_data[159:96]),
        .busy_o(silu_busy),
        .done_o(silu_done),
        .error_o(silu_error),
        .result_q30_o(silu_result_q30)
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
                required_input_bytes = 16'd72;
                output_bytes = 16'd20;
                valid = 1'b1;
            end
            default: begin end
        endcase
    end

    always @(posedge clk) begin
        if (!reset_n) begin
            active_function <= 16'd0;
            q8_start <= 1'b0;
            silu_start <= 1'b0;
            active_silu <= 1'b0;
            next_led <= 5'b00001;
            output_data <= 768'd0;
            busy <= 1'b0;
            done <= 1'b0;
            error <= 1'b0;
        end else begin
            q8_start <= 1'b0;
            silu_start <= 1'b0;
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
                        active_silu <= input_data[3];
                        if (input_data[3])
                            silu_start <= 1'b1;
                        else
                            q8_start <= 1'b1;
                    end
                    default: begin
                        busy <= 1'b0;
                        done <= 1'b1;
                        error <= 1'b1;
                    end
                endcase
            end else if (busy && active_function == 16'd2 && active_silu && silu_done) begin
                output_data <= 768'd0;
                output_data[159:96] <= silu_result_q30;
                busy <= 1'b0;
                done <= 1'b1;
                error <= silu_error;
            end else if (busy && active_function == 16'd2 && !active_silu && q8_done) begin
                output_data <= 768'd0;
                output_data[31:0] <= q8_dot;
                output_data[95:32] <= q8_term_q30;
                output_data[159:96] <= q8_row_q30;
                busy <= 1'b0;
                done <= 1'b1;
                error <= q8_scale_error;
            end
        end
    end
    wire unused_silu_busy = silu_busy;
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

fn parse_golden_gate_row() -> Result<GoldenQ8Row, String> {
    let text = core::str::from_utf8(LFM25_FFN_VECTORS)
        .map_err(|_| "golden vectors are not UTF-8".to_string())?;
    let mut activations = [[0u8; 34]; 32];
    let mut dots = [0i32; 32];
    let mut terms_q30 = [0i64; 32];
    let mut row_q30 = 0i64;
    let mut fp_q30 = None;
    let mut fp_bound_q30 = None;
    let mut count = 0usize;

    for line in text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 12 {
            return Err(format!("row vector has {} fields, expected 12", fields.len()));
        }
        let row: usize = fields[0].parse().map_err(|_| "invalid row index")?;
        if row != 0 {
            break;
        }
        if count >= 32 {
            return Err("gate row contains more than 32 blocks".into());
        }
        let block: usize = fields[1].parse().map_err(|_| "invalid block index")?;
        let first: u8 = fields[2].parse().map_err(|_| "invalid first flag")?;
        let last: u8 = fields[3].parse().map_err(|_| "invalid last flag")?;
        if block != count || first != u8::from(count == 0) || last != u8::from(count == 31) {
            return Err(format!("non-canonical row control at block {count}"));
        }

        let activation_scale = u16::from_str_radix(fields[4], 16)
            .map_err(|_| format!("invalid activation scale at block {count}"))?;
        activations[count][..2].copy_from_slice(&activation_scale.to_le_bytes());
        activations[count][2..].copy_from_slice(&parse_hex_32(fields[6])?);
        dots[count] = fields[8]
            .parse()
            .map_err(|_| format!("invalid dot at block {count}"))?;
        terms_q30[count] = u64::from_str_radix(fields[9], 16)
            .map_err(|_| format!("invalid Q30 term at block {count}"))?
            as i64;
        row_q30 = row_q30
            .checked_add(terms_q30[count])
            .ok_or_else(|| "gate-row Q30 accumulator overflow".to_string())?;
        let block_fp = u64::from_str_radix(fields[10], 16)
            .map_err(|_| format!("invalid F32 reference at block {count}"))?
            as i64;
        let block_bound: i64 = fields[11]
            .parse()
            .map_err(|_| format!("invalid F32 bound at block {count}"))?;
        if fp_q30
            .replace(block_fp)
            .is_some_and(|previous| previous != block_fp)
            || fp_bound_q30
                .replace(block_bound)
                .is_some_and(|previous| previous != block_bound)
        {
            return Err("gate-row reference/bound changes between blocks".into());
        }
        count += 1;
    }

    if count != 32 {
        return Err(format!("gate row has {count} blocks, expected 32"));
    }
    let fp_q30 = fp_q30.ok_or_else(|| "missing gate-row F32 reference".to_string())?;
    let fp_bound_q30 = fp_bound_q30.ok_or_else(|| "missing gate-row F32 bound".to_string())?;
    if (row_q30 - fp_q30).abs() > fp_bound_q30 {
        return Err("gate-row accumulated result exceeds frozen F32 bound".into());
    }
    Ok(GoldenQ8Row {
        activations,
        dots,
        terms_q30,
        row_q30,
        fp_q30,
        fp_bound_q30,
    })
}

fn parse_hex_32(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 {
        return Err(format!("Q8 quant payload has {} hex digits, expected 64", value.len()));
    }
    let mut bytes = [0u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        // The vector file prints a Verilog [255:0] literal most-significant byte
        // first. The work-package/native block uses byte lane zero first.
        let text_index = (31 - index) * 2;
        *byte = u8::from_str_radix(&value[text_index..text_index + 2], 16)
            .map_err(|_| "invalid Q8 quant payload".to_string())?;
    }
    Ok(bytes)
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

fn emit_rust_interface(
    firmware_hash: [u8; 32],
    golden: GoldenQ8Block,
    golden_row: GoldenQ8Row,
) -> String {
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
            BindingKind::Lfm25Q8RowBlock => {
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
            BindingKind::Lfm25Q8RowBlock => {
                rust.push_str("    pub const Q8_0_BLOCK_BYTES: usize = 34;\n");
                rust.push_str("    pub const GATE_ROW0_BLOCKS: usize = 32;\n");
                rust.push_str("    pub const WIDE_ROW_BLOCKS: usize = 144;\n");
                rust.push_str("    pub const GATE_ROW0_NATIVE_OFFSET: u64 = 0x048c9000;\n\n");
                rust.push_str("    #[derive(Copy, Clone, Debug, Eq, PartialEq)]\n");
                rust.push_str("    pub struct Q8RowBlockResult {\n");
                rust.push_str("        pub dot: i32,\n");
                rust.push_str("        pub term_q30: i64,\n");
                rust.push_str("        pub row_q30: i64,\n");
                rust.push_str("    }\n\n");
                rust.push_str(
                    "    pub fn encode_projection(\n        first: bool,\n        last: bool,\n        wide: bool,\n        block_index: u8,\n        activation: &[u8; Q8_0_BLOCK_BYTES],\n        weight: &[u8; Q8_0_BLOCK_BYTES],\n    ) -> [u8; INPUT_BYTES] {\n",
                );
                rust.push_str("        let mut bytes = [0; INPUT_BYTES];\n");
                rust.push_str("        let control = u32::from(first) | (u32::from(last) << 1) | (u32::from(wide) << 2) | (u32::from(block_index) << 8);\n");
                rust.push_str("        bytes[..4].copy_from_slice(&control.to_le_bytes());\n");
                rust.push_str(
                    "        bytes[4..4 + Q8_0_BLOCK_BYTES].copy_from_slice(activation);\n",
                );
                rust.push_str("        bytes[4 + Q8_0_BLOCK_BYTES..].copy_from_slice(weight);\n");
                rust.push_str("        bytes\n");
                rust.push_str("    }\n\n");
                rust.push_str("    pub fn encode(\n        first: bool,\n        last: bool,\n        block_index: u8,\n        activation: &[u8; Q8_0_BLOCK_BYTES],\n        weight: &[u8; Q8_0_BLOCK_BYTES],\n    ) -> [u8; INPUT_BYTES] {\n");
                rust.push_str("        encode_projection(first, last, false, block_index, activation, weight)\n");
                rust.push_str("    }\n\n");
                rust.push_str("    pub fn encode_activation_cache(wide: bool, block_index: u8, activation: &[u8; Q8_0_BLOCK_BYTES]) -> [u8; INPUT_BYTES] {\n");
                rust.push_str("        let mut bytes = [0; INPUT_BYTES];\n");
                rust.push_str("        let control = (u32::from(wide) << 2) | (1 << 4) | (u32::from(block_index) << 8);\n");
                rust.push_str("        bytes[..4].copy_from_slice(&control.to_le_bytes());\n");
                rust.push_str("        bytes[4..4 + Q8_0_BLOCK_BYTES].copy_from_slice(activation);\n");
                rust.push_str("        bytes\n");
                rust.push_str("    }\n\n");
                rust.push_str("    pub fn encode_cached_pair(first: bool, last: bool, wide: bool, block_index: u8, weight0: &[u8; Q8_0_BLOCK_BYTES], weight1: &[u8; Q8_0_BLOCK_BYTES]) -> [u8; INPUT_BYTES] {\n");
                rust.push_str("        let mut bytes = [0; INPUT_BYTES];\n");
                rust.push_str("        let control = u32::from(first) | (u32::from(last) << 1) | (u32::from(wide) << 2) | (1 << 5) | (u32::from(block_index) << 8);\n");
                rust.push_str("        bytes[..4].copy_from_slice(&control.to_le_bytes());\n");
                rust.push_str("        bytes[4..4 + Q8_0_BLOCK_BYTES].copy_from_slice(weight0);\n");
                rust.push_str("        bytes[4 + Q8_0_BLOCK_BYTES..].copy_from_slice(weight1);\n");
                rust.push_str("        bytes\n");
                rust.push_str("    }\n\n");
                rust.push_str("    pub fn encode_single(activation: &[u8; Q8_0_BLOCK_BYTES], weight: &[u8; Q8_0_BLOCK_BYTES]) -> [u8; INPUT_BYTES] {\n");
                rust.push_str("        encode(true, true, 0, activation, weight)\n");
                rust.push_str("    }\n\n");
                rust.push_str(
                    "    pub fn encode_silu(gate_q30: i64, up_q30: i64) -> [u8; INPUT_BYTES] {\n",
                );
                rust.push_str("        let mut bytes = [0; INPUT_BYTES];\n");
                rust.push_str("        bytes[..4].copy_from_slice(&8u32.to_le_bytes());\n");
                rust.push_str("        bytes[4..12].copy_from_slice(&gate_q30.to_le_bytes());\n");
                rust.push_str("        bytes[12..20].copy_from_slice(&up_q30.to_le_bytes());\n");
                rust.push_str("        bytes\n");
                rust.push_str("    }\n\n");
                rust.push_str("    pub fn decode(bytes: &[u8]) -> Option<Q8RowBlockResult> {\n");
                rust.push_str(
                    "        Some(Q8RowBlockResult {\n            dot: i32::from_le_bytes(bytes.get(..4)?.try_into().ok()?),\n            term_q30: i64::from_le_bytes(bytes.get(4..12)?.try_into().ok()?),\n            row_q30: i64::from_le_bytes(bytes.get(12..20)?.try_into().ok()?),\n        })\n",
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
                    "    pub const GOLDEN_RESULT: Q8RowBlockResult = Q8RowBlockResult {{\n        dot: {},\n        term_q30: {},\n        row_q30: {},\n    }};",
                    golden.dot,
                    golden.term_q30,
                    golden.term_q30,
                )
                .unwrap();
                emit_rust_q8_row(&mut rust, &golden_row);
            }
        }
        rust.push_str("}\n\n");
    }
    rust.push_str("pub use lfm25_ffn_step as lfm25_q8_row_block;\n\n");
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

fn emit_rust_q8_row(rust: &mut String, golden: &GoldenQ8Row) {
    rust.push_str(
        "    pub const GOLDEN_GATE_ROW0_ACTIVATIONS: [[u8; Q8_0_BLOCK_BYTES]; GATE_ROW0_BLOCKS] = [\n",
    );
    for activation in &golden.activations {
        rust.push_str("        [");
        for byte in activation {
            write!(rust, "0x{byte:02x},").unwrap();
        }
        rust.push_str("],\n");
    }
    rust.push_str("    ];\n");

    rust.push_str("    pub const GOLDEN_GATE_ROW0_DOTS: [i32; GATE_ROW0_BLOCKS] = [\n        ");
    for (index, value) in golden.dots.iter().enumerate() {
        write!(rust, "{value},").unwrap();
        if index % 8 == 7 {
            rust.push('\n');
            if index + 1 != golden.dots.len() {
                rust.push_str("        ");
            }
        } else {
            rust.push(' ');
        }
    }
    rust.push_str("    ];\n");

    rust.push_str(
        "    pub const GOLDEN_GATE_ROW0_TERMS_Q30: [i64; GATE_ROW0_BLOCKS] = [\n        ",
    );
    for (index, value) in golden.terms_q30.iter().enumerate() {
        write!(rust, "{value},").unwrap();
        if index % 4 == 3 {
            rust.push('\n');
            if index + 1 != golden.terms_q30.len() {
                rust.push_str("        ");
            }
        } else {
            rust.push(' ');
        }
    }
    rust.push_str("    ];\n");
    writeln!(
        rust,
        "    pub const GOLDEN_GATE_ROW0_Q30: i64 = {};\n    pub const GOLDEN_GATE_ROW0_FP_Q30: i64 = {};\n    pub const GOLDEN_GATE_ROW0_FP_BOUND_Q30: i64 = {};",
        golden.row_q30, golden.fp_q30, golden.fp_bound_q30,
    )
    .unwrap();
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
    const ROW_STREAMER_RTL: &str = include_str!("../../../src/compute/truega_lfm25_row_streamer.v");
    const GOWIN_PROJECT: &str = include_str!("../../../min_pci_led.gprj");
    const PCIE_CONTROLLER_IPC: &str =
        include_str!("../../../src/serdes/pcie_controller/pcie_controller.ipc");
    const SERDES_IPC: &str = include_str!("../../../src/serdes/serdes.ipc");

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
        let golden_row = parse_golden_gate_row().unwrap();
        let interface = emit_rust_interface([0; 32], golden, golden_row);
        assert!(interface.contains("HEARTBEAT"));
        assert!(interface.contains("ADD_U32"));
        assert!(interface.contains("LFM25_FFN_STEP"));
        assert!(interface.contains("pub const FIRMWARE_MANIFEST: FirmwareManifest"));
        assert!(interface.contains("pub mod led_step_heartbeat"));
        assert!(interface.contains("pub mod add_u32"));
        assert!(interface.contains("pub mod lfm25_ffn_step"));
        assert!(interface.contains("pub use lfm25_ffn_step as lfm25_q8_row_block"));
        assert!(interface.contains("pub struct Q8RowBlockResult"));
        assert!(interface.contains("pub fn encode_activation_cache"));
        assert!(interface.contains("pub fn encode_cached_pair"));
        assert!(interface.contains("pub const GOLDEN_ACTIVATION"));
        assert!(interface.contains("pub const GOLDEN_GATE_ROW0_ACTIVATIONS"));
        assert_eq!(FUNCTIONS.len(), 3);
    }

    #[test]
    fn v1_catalogue_is_contiguous_and_physically_supported() {
        validate_catalogue().unwrap();
        assert_eq!(FUNCTIONS.map(|function| function.id), [0, 1, 2]);
        assert_eq!(FUNCTIONS.map(|function| function.input_bytes), [0, 8, 72]);
        assert_eq!(FUNCTIONS.map(|function| function.output_bytes), [4, 4, 20]);
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
    fn sealed_gate_row_fixture_is_canonical() {
        let row = parse_golden_gate_row().unwrap();
        assert_eq!(row.activations[0][..4], [0x30, 0x18, 0x0d, 0xa8]);
        assert_eq!(row.dots[0], -14_901);
        assert_eq!(row.terms_q30[0], -9_429_888);
        assert_eq!(row.row_q30, 29_481_209);
        assert_eq!(row.fp_q30, 29_481_200);
        assert_eq!(row.fp_bound_q30, 2_148);
    }

    #[test]
    fn generated_rtl_binds_compute_and_common_handoff() {
        let rtl = assembled_function_rtl();
        assert!(rtl.contains("module truega_functions("));
        assert!(rtl.contains("input  wire [767:0] input_data"));
        assert!(rtl.contains("module truega_q8_0_dot32"));
        assert!(rtl.contains(
            "{{5{product_reg[15]}}, product_reg}"
        ));
        assert!(rtl.contains("accumulator_next = accumulator + registered_product_extended"));
        assert!(rtl.contains("module truega_q8_0_scale_q30_seq"));
        assert!(rtl.contains("module truega_q8_0_block_slot"));
        assert!(rtl.contains("module truega_q8_0_row_block_slot"));
        assert!(rtl.contains("module truega_q8_0_cached_pair_slot"));
        assert!(rtl.contains("module truega_lfm25_silu_q30_slot"));
        assert!(rtl.contains(".CACHED_PAIR_ENABLE(1)"));
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
    fn receive_snapshot_is_not_clock_enabled_by_transaction_state() {
        let marker = "Register the hard-IP receive pins on every TLP clock.";
        let start = TOP_VHDL.find(marker).expect("missing unconditional RX snapshot");
        let finish = TOP_VHDL[start..]
            .find("end process;")
            .map(|offset| start + offset)
            .expect("unterminated RX snapshot process");
        let process = &TOP_VHDL[start..finish];
        for required in [
            "rx_snapshot_data <= tl_rx_data;",
            "rx_snapshot_valid <= tl_rx_valid;",
            "rx_snapshot_bardec <= tl_rx_bardec;",
        ] {
            assert!(process.contains(required), "missing RX register: {required}");
        }
        assert!(!process.contains("transaction_pending"));
        assert!(!process.contains("capture_pending"));
    }

    #[test]
    fn bar0_and_bar2_addresses_are_decoded_in_their_own_apertures() {
        for required in [
            "if transaction_bardec(0) = '1' then",
            "addr_index := to_integer(unsigned(addr_dw(7 downto 0)));",
            "elsif hit_write and (transaction_bardec(0) = '1') then",
            "if hit_write and (transaction_bardec(2) = '1') then",
        ] {
            assert!(TOP_VHDL.contains(required), "missing BAR-relative decode: {required}");
        }
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
        assert!(TOP_VHDL.contains("bar_read_manifest_data_dw <= firmware_manifest_word;"));
    }

    #[test]
    fn final_image_physically_backs_both_96_byte_envelopes() {
        assert!(TOP_VHDL.contains("type call_data_arr_t is array (0 to 23)"));
        assert!(TOP_VHDL.contains(
            "call_input_words(addr_index - BAR0_CALL_BASE_DW - CALL_INPUT_WORD) <= payload_dw;"
        ));
        assert!(TOP_VHDL.contains(
            "bar_read_call_output_data_dw <= call_output_words(to_integer(unsigned(bar_read_word_index)));"
        ));
        assert!(TOP_VHDL.contains(
            "call_output_words(i) <= function_output_data((i + 1) * 32 - 1 downto i * 32);"
        ));
        assert!(!TOP_VHDL.contains(
            "call_output_words(addr_index - BAR0_CALL_BASE_DW - CALL_OUTPUT_WORD) <= payload_dw;"
        ));
        assert!(TOP_VHDL.contains("elsif (call_active = '1') and (function_done = '1') then"));
        assert!(TOP_VHDL.contains("function_start <= '1';"));
    }

    #[test]
    fn bar_read_mux_is_banked_and_registered_before_tx() {
        for required in [
            "signal bar_read_select_pending : std_logic := '0';",
            "signal bar_read_data_select_pending : std_logic := '0';",
            "signal bar_read_completion_pending : std_logic := '0';",
            "bar_read_bank <= BAR_READ_BANK_CALL_HEADER;",
            "bar_read_bank <= BAR_READ_BANK_CALL_INPUT;",
            "bar_read_bank <= BAR_READ_BANK_CALL_OUTPUT;",
            "bar_read_bank <= BAR_READ_BANK_MANIFEST;",
            "bar_read_bank <= BAR_READ_BANK_STREAM;",
            "bar_read_bank <= BAR_READ_BANK_DEBUG;",
            "bar_read_selected_bank <= bar_read_bank;",
            "bar_read_data_dw <= read_data_dw;",
            "queue_cpld(bar_read_req_id, bar_read_req_tag, bar_read_addr_dw, bar_read_data_dw);",
        ] {
            assert!(TOP_VHDL.contains(required), "missing BAR read pipeline boundary: {required}");
        }
        assert!(!TOP_VHDL.contains("queue_cpld(req_id, req_tag, addr_dw, read_data_dw);"));
    }

    #[test]
    fn final_image_has_a_separate_bar2_row_stream_transport() {
        for required in [
            "component truega_lfm25_row_streamer is",
            "u_lfm25_row_streamer: truega_lfm25_row_streamer",
            "transaction_bardec <= rx_snapshot_bardec;",
            "if hit_write and (transaction_bardec(2) = '1') then",
            "((tl_rx_bardec(0) = '1') or (tl_rx_bardec(2) = '1'))",
            "read_data_dw := STREAM_CAPABILITY_MAGIC;",
        ] {
            assert!(TOP_VHDL.contains(required), "missing BAR2 transport: {required}");
        }
        for required in [
            "module truega_lfm25_row_streamer (",
            "MODE_GATE_UP_SILU",
            "MODE_DOWN",
            "truega_q8_0_row_block_slot",
            "truega_lfm25_silu_q30_slot",
            "activation_memory [0:143]",
            "weight0_memory [0:143]",
            "weight1_memory [0:143]",
            "activation_read_valid <= activation_valid[read_index];",
            "weight0_read_valid <= weight0_valid[read_index];",
            "weight1_read_valid <= weight1_valid[read_index];",
        ] {
            assert!(ROW_STREAMER_RTL.contains(required), "missing row engine: {required}");
        }
        assert!(GOWIN_PROJECT.contains("truega_lfm25_row_streamer.v"));
    }

    #[test]
    fn pcie_configuration_exposes_a_512k_prefetchable_64_bit_bar2() {
        for ipc in [PCIE_CONTROLLER_IPC, SERDES_IPC] {
            for required in [
                "Bar2_Enable=true",
                "Bit64_2=true",
                "Prefetchable2=true",
                "Size2=512KiloBytes",
                "value2=FFF8000C",
            ] {
                assert!(ipc.contains(required), "missing BAR2 PCIe setting: {required}");
            }
        }
    }
}
