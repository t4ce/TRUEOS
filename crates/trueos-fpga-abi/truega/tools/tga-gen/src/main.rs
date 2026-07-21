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

use firmware::{FunctionSpec, FUNCTIONS};

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

    let function_rtl = rename_rust_hdl_top(firmware::generate());
    let firmware_hash: [u8; 32] = Sha256::digest(function_rtl.as_bytes()).into();
    let manifest = manifest_bytes(firmware_hash);
    let rtl = format!("{function_rtl}\n{}", emit_manifest_verilog(&manifest));
    let rust_interface = emit_rust_interface(firmware_hash);

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
        if spec.output_bytes != 4 || !matches!(spec.input_bytes, 0 | 8) {
            return Err(format!(
                "slot {} uses input/output shape {}/{}; v1 hardware supports only () -> u32 or (u32, u32) -> u32",
                spec.id, spec.input_bytes, spec.output_bytes
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

fn rename_rust_hdl_top(verilog: String) -> String {
    verilog.replace("top$", "truega_functions$").replacen(
        "module top(",
        "module truega_functions(",
        1,
    )
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

fn emit_rust_interface(firmware_hash: [u8; 32]) -> String {
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
        rust.push_str("    use super::{FunctionId, result_u32};\n\n");
        writeln!(rust, "    pub const ID: FunctionId = super::{};", spec.rust_name).unwrap();
        writeln!(rust, "    pub const INPUT_BYTES: usize = {};", spec.input_bytes).unwrap();
        writeln!(rust, "    pub const OUTPUT_BYTES: usize = {};", spec.output_bytes).unwrap();
        if spec.input_bytes == 0 {
            rust.push_str("    pub const fn encode() -> [u8; 0] { [] }\n");
        } else {
            rust.push_str("    pub fn encode(a: u32, b: u32) -> [u8; 8] {\n");
            rust.push_str("        let mut bytes = [0; 8];\n");
            rust.push_str("        bytes[..4].copy_from_slice(&a.to_le_bytes());\n");
            rust.push_str("        bytes[4..].copy_from_slice(&b.to_le_bytes());\n");
            rust.push_str("        bytes\n");
            rust.push_str("    }\n");
        }
        rust.push_str("    pub fn decode(bytes: &[u8]) -> Option<u32> { result_u32(bytes) }\n");
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
        let interface = emit_rust_interface([0; 32]);
        assert!(interface.contains("HEARTBEAT"));
        assert!(interface.contains("ADD_U32"));
        assert!(interface.contains("XOR_U32"));
        assert!(interface.contains("pub const FIRMWARE_MANIFEST: FirmwareManifest"));
        assert!(interface.contains("pub mod led_step_heartbeat"));
        assert!(interface.contains("pub mod add_u32"));
        assert!(interface.contains("pub mod xor_u32"));
        assert_eq!(FUNCTIONS.len(), 3);
    }

    #[test]
    fn v1_catalogue_is_contiguous_and_physically_supported() {
        validate_catalogue().unwrap();
        assert_eq!(FUNCTIONS.map(|function| function.id), [0, 1, 2]);
        assert_eq!(FUNCTIONS.map(|function| function.output_bytes), [4, 4, 4]);
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
}
