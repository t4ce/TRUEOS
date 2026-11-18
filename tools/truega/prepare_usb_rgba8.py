"""Adapt the Sipeed USB bulk loopback project to originate one RGBA8 frame."""

import argparse
from pathlib import Path
import shutil
from make_z7_edid import full_hd_edid


def replace(text, old, new):
    if text.count(old) != 1:
        raise SystemExit(f"Unexpected vendor source around {old!r}")
    return text.replace(old, new)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("demo", type=Path, help="Sipeed usb_cy_bulkloop directory")
    parser.add_argument("staging", type=Path)
    parser.add_argument("--edid", action="store_true",
                        help="Expose the Z7 Full HD EDID on the Pro Dock HDMI input")
    receiver_mode = parser.add_mutually_exclusive_group()
    receiver_mode.add_argument("--dvi-ip-data", type=Path,
                        help="Gowin IDE ipcore/DVI_RX/data directory for the HDMI receiver probe")
    receiver_mode.add_argument("--clock-probe", action="store_true",
                               help="Measure TMDS clock frequency before the receiver PLL")
    args = parser.parse_args()
    shutil.copytree(args.demo, args.staging)
    src = args.staging / "src"
    shutil.copyfile(Path(__file__).with_name("usb_rgba8_source.v"),
                    src / "usb_rgba8_source.v")

    top = src / "Cypress_TOP.v"
    rtl = top.read_text()
    rtl = replace(rtl, "    assign RESET = ~pll_locked;", """    // Allow the PHY and host to settle before attaching after reprogramming.
    reg [22:0] usb_startup = 0;
    always @(posedge PHY_CLKOUT or negedge pll_locked) begin
        if (!pll_locked)
            usb_startup <= 0;
        else if (!usb_startup[22])
            usb_startup <= usb_startup + 1'b1;
    end
    assign RESET = !usb_startup[22] || !pll_locked;""")
    rtl = replace(rtl, ".i_reset           (usb_busreset )",
                  ".i_reset           (RESET | usb_busreset )")
    rtl = replace(rtl, "assign usb_txcork    = (endpt_sel ==CONTROL_EP) ? 1'b0 : ep_usb_txcork;",
                  """// Replies are fixed-size. Hold EP2 IN until one full USB packet is ready.
    assign usb_txcork = endpt_sel == CONTROL_EP ? 1'b0 :
                       ep_usb_txcork || (endpt_sel == 4'd2 &&
                       ep_usb_txlen < (usb_highspeed ? 12'd512 : 12'd64));""")
    rtl = replace(rtl, "    usb_fifo usb_fifo", """    wire [7:0] frame_byte;
    wire frame_valid;
    wire hpd_request;
    wire [23:0] camera_status;
    usb_rgba8_source source (
        .clk(PHY_CLKOUT), .reset(RESET | usb_busreset),
        .request_valid(ep2_data_valid), .request_data(ep2_data),
        .camera_status(camera_status),
        .frame_sequence(15'd0),
        .capture_trace(8'd0),
        .hpd_request(hpd_request),
        .valid(frame_valid), .data(frame_byte)
    );

    usb_fifo usb_fifo""")
    rtl = replace(rtl, ".i_ep2_tx_dval (ep2_data_valid ),",
                  ".i_ep2_tx_dval (frame_valid),")
    rtl = replace(rtl, ".i_ep2_tx_data (ep2_data),",
                  ".i_ep2_tx_data (frame_byte),")
    sources = ["usb_rgba8_source.v"]
    if args.edid or args.dvi_ip_data or args.clock_probe:
        sources.append("hdmi_edid_slave.v")
        shutil.copyfile(Path(__file__).with_name(sources[-1]), src / sources[-1])
        (args.staging / "z7-edid.hex").write_text(
            "".join(f"{byte:02x}\n" for byte in full_hd_edid()))
        rtl = replace(rtl, "    input      CLK_IN,", """    input      CLK_IN,
    input      ddc_scl,
    inout      ddc_sda,
    output     hdmi_hpd,
    output [2:0] ddc_select,""")
        rtl = replace(rtl, "    localparam CONTROL_EP", """    // IC3 routes HDMI SCL on Y4, IC4 routes HDMI SDA on Y1.
    // IC4 swaps its S0/S2 inputs, so P26 must be high for both.
    assign ddc_select = 3'b100;
    // A USB command cycles HPD without reprogramming the board. Keep its
    // toggle across USB bus reset so the request cannot turn into two pulses.
    reg hpd_toggle = 0;
    always @(posedge PHY_CLKOUT)
        if (hpd_request) hpd_toggle <= !hpd_toggle;
    (* async_reg = "true" *) reg [1:0] hpd_command = 0;
    reg hpd_observed = 0;
    reg [24:0] ddc_startup = 0;
    always @(posedge CLK_IN) begin
        hpd_command <= {hpd_command[0], hpd_toggle};
        if (hpd_command[1] != hpd_observed) begin
            hpd_observed <= hpd_command[1];
            ddc_startup <= 0;
        end else if (!ddc_startup[24]) begin
            ddc_startup <= ddc_startup + 1'b1;
        end
    end
    assign hdmi_hpd = ddc_startup[24];
    wire edid_seen;
    wire ddc_scl_activity, ddc_sda_activity, ddc_scl_level, ddc_sda_level;
    hdmi_edid_slave ddc (
        .clk(CLK_IN), .reset(!hdmi_hpd), .scl(ddc_scl), .sda(ddc_sda),
        .address_seen(edid_seen), .scl_activity(ddc_scl_activity),
        .sda_activity(ddc_sda_activity), .scl_level(ddc_scl_level), .sda_level(ddc_sda_level)
    );

    localparam CONTROL_EP""")
        with (src / "usb_ref.cst").open("a") as constraints:
            for name, pin in (("ddc_scl", "K25"), ("ddc_sda", "K26"),
                              ("hdmi_hpd", "G26"), ("ddc_select[0]", "N19"),
                              ("ddc_select[1]", "P19"), ("ddc_select[2]", "P26")):
                constraints.write(f'\nIO_LOC "{name}" {pin};\n'
                                  f'IO_PORT "{name}" IO_TYPE=LVCMOS33 PULL_MODE=NONE;\n')
    if args.dvi_ip_data:
        for name in ("dvi_rx_top.v", "dvi2rgb.v", "static_macro_define.v"):
            shutil.copyfile(args.dvi_ip_data / name, src / name)
        (src / "top_define.v").write_text("`define module_name DVI_RX_Top\n")
        (src / "dvi_rx_defines.v").write_text(
            '`define RX_PLL_CLKIN_FRE "74.25"\n'
            '`define RX_USE_5AT_PLL\n'
            '`define RX_PLL_IDIV_SEL 1\n'
            '`define RX_PLL_MDIV_SEL 10\n'
            '`define RX_PLL_FBDIV_SEL 1\n'
            '`define RX_PLL_ODIV_SEL 2\n'
            '`define RX_PLL_ODIV0_SEL 2\n'
            '`define RX_CLKOUT0_PE_COARSE 0\n'
            '`define RX_CLKOUT0_PE_FINE 0\n'
            '`define RX_DYN_DPA_EN "FALSE"\n'
            '`define RX_DYN_PE0_SEL "FALSE"\n'
            '`define AUTO_PHASE\n')
        sources += ["dvi_rx_top.v", "dvi2rgb.v", "hdmi_rgba8_capture.v"]
        shutil.copyfile(Path(__file__).with_name(sources[-1]), src / sources[-1])
        rtl = replace(rtl, "    input      CLK_IN,", """    input      CLK_IN,
    input      tmds_clk_p, tmds_clk_n,
    input [2:0] tmds_data_p, tmds_data_n,""")
        rtl = replace(rtl, "    usb_fifo usb_fifo", """    wire rgb_clk, rgb_vsync, rgb_de, pll_ready, phase_ready;
    wire [7:0] red, green, blue;
    DVI_RX_Top receiver (
        .I_init_clk(CLK_IN), .I_rst_n(hdmi_hpd),
        .I_tmds_clk_p(tmds_clk_p), .I_tmds_clk_n(tmds_clk_n),
        .I_tmds_data_p(tmds_data_p), .I_tmds_data_n(tmds_data_n),
        .O_pll_lock(pll_ready), .O_pll_phase_lock(phase_ready),
        .O_rgb_clk(rgb_clk), .O_rgb_vs(rgb_vsync), .O_rgb_de(rgb_de),
        .O_rgb_r(red), .O_rgb_g(green), .O_rgb_b(blue)
    );
    wire complete, dropped;
    hdmi_rgba8_capture raster (
        .pixel_clk(rgb_clk), .reset(!pll_ready), .arm(1'b1),
        .vsync(rgb_vsync), .de(rgb_de), .red(red), .green(green), .blue(blue),
        .pixel_ready(1'b1), .frame_complete(complete), .frame_dropped(dropped)
    );
    reg saw_complete = 0;
    reg saw_dropped = 0;
    always @(posedge rgb_clk) begin
        if (!pll_ready) begin
            saw_complete <= 0;
            saw_dropped <= 0;
        end else begin
            saw_complete <= saw_complete || complete;
            saw_dropped <= saw_dropped || dropped;
        end
    end
    reg [3:0] status_async, status_sync;
    always @(posedge PHY_CLKOUT) begin
        status_async <= {saw_dropped, saw_complete, phase_ready, pll_ready};
        status_sync <= status_async;
    end
    assign camera_status = {16'b0, 4'h7, status_sync};

    usb_fifo usb_fifo""")
        with (src / "usb_ref.cst").open("a") as constraints:
            for name, positive, negative in (
                ("tmds_clk_p", "K20", "J20"), ("tmds_data_p[0]", "D19", "C19"),
                ("tmds_data_p[1]", "M15", "L15"), ("tmds_data_p[2]", "J25", "J26"),
            ):
                constraints.write(f'\nIO_LOC "{name}" {positive}, {negative};\n'
                                  f'IO_PORT "{name}" IO_TYPE=LVDS25;\n')
    elif args.clock_probe:
        rtl = replace(rtl, "    input      CLK_IN,", """    input      CLK_IN,
    input      tmds_clk_p, tmds_clk_n,""")
        rtl = replace(rtl, "    usb_fifo usb_fifo", """    wire input_clock;
    TLVDS_IBUF clock_input (.I(tmds_clk_p), .IB(tmds_clk_n), .O(input_clock));
    reg [3:0] prescaler = 0;
    always @(posedge input_clock)
        prescaler <= prescaler + 1'b1;

    // A 1 ms window at the USB PHY's 60 MHz clock. Each counted edge
    // represents 16 input cycles, for a 16 kHz measurement resolution.
    reg [2:0] divided_sync;
    reg [1:0] edid_sync;
    reg [15:0] window_ticks, edges, measured_edges;
    wire new_edge = divided_sync[2:1] == 2'b01;
    always @(posedge PHY_CLKOUT) begin
        divided_sync <= {divided_sync[1:0], prescaler[3]};
        edid_sync <= {edid_sync[0], edid_seen};
        if (RESET) begin
            window_ticks <= 0;
            edges <= 0;
            measured_edges <= 0;
        end else if (window_ticks == 59999) begin
            window_ticks <= 0;
            edges <= 0;
            measured_edges <= edges + new_edge;
        end else begin
            window_ticks <= window_ticks + 1'b1;
            edges <= edges + new_edge;
        end
    end
    assign camera_status = {measured_edges, 7'h40, edid_sync[1]};

    usb_fifo usb_fifo""")
        with (src / "usb_ref.cst").open("a") as constraints:
            constraints.write('\nIO_LOC "tmds_clk_p" K20, J20;\n'
                              'IO_PORT "tmds_clk_p" IO_TYPE=LVDS25;\n')
    else:
        rtl = replace(rtl, "    wire [23:0] camera_status;",
                      "    wire [23:0] camera_status = 0;")
    top.write_text(rtl)

    project, = args.staging.glob("*.gprj")
    xml = project.read_text()
    entries = "".join(f'<File path="src/{name}" type="file.verilog" enable="1"/>\n    '
                      for name in sources)
    project.write_text(replace(xml, "</FileList>", entries + "</FileList>"))


if __name__ == "__main__":
    main()
