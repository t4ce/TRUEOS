"""Add the vendor DDR PHY and a full-frame self-test to a staged USB project."""

import argparse
from pathlib import Path
import shutil

from prepare_usb_rgba8 import replace


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("demo", type=Path, help="Sipeed pro_ddr_test directory")
    parser.add_argument("staging", type=Path)
    parser.add_argument("--capture", action="store_true",
                        help="Connect a staged DVI receiver to DDR instead of running the DDR self-test")
    test_modes = parser.add_mutually_exclusive_group()
    test_modes.add_argument("--readout-test", action="store_true",
                        help="Seed the capture slot with known pixels for a DDR-to-USB hardware test")
    test_modes.add_argument("--raster-test", action="store_true",
                            help="Feed two-dimensional test rasters through the full capture path")
    args = parser.parse_args()
    if (args.readout_test or args.raster_test) and not args.capture:
        parser.error("Both test modes require --capture")
    vendor = args.demo / "src"
    src = args.staging / "src"
    vendor_top = (vendor / "top.v").read_text()
    ddr_ports = vendor_top.split("    output [15-1:0] ddr_addr,", 1)[1].split("\n);", 1)[0]
    ddr_instances = vendor_top.split("wire pll_lock;", 1)[1].split("\nwire wdone;", 1)[0]
    ddr_instances = ("wire pll_lock;" + ddr_instances).replace(".clkin(sys_clk)", ".clkin(CLK_IN)")
    ddr_instances = replace(ddr_instances, "wire app_burst;", "wire app_burst = 0;")

    top = src / "Cypress_TOP.v"
    rtl = top.read_text()
    rtl = replace(rtl, "    input      CLK_IN,",
                  "    input      CLK_IN,\n    output [15-1:0] ddr_addr," + ddr_ports + ",")
    common = """
    reg [21:0] ddr_startup = 0;
    always @(posedge CLK_IN)
        if (!ddr_startup[21]) ddr_startup <= ddr_startup + 1'b1;
    wire rst_n = ddr_startup[21];
""" + ddr_instances + """
    assign app_wdf_mask = 0;
"""
    memory_rtl = common + """
    wire ddr_done, ddr_failed;
    reg [2:0] ddr_status_meta, ddr_status_sync;
    always @(posedge PHY_CLKOUT) begin
        ddr_status_meta <= {ddr_failed, ddr_done, init_calib_complete};
        ddr_status_sync <= ddr_status_meta;
    end
    wire [23:0] camera_status = {16'b0, 5'h12, ddr_status_sync};
    rgba8_ddr_selftest memory_test (
        .clk(clk_x1), .reset(!rst_n), .calibrated(init_calib_complete),
        .cmd_ready(app_rdy), .wr_data_rdy(app_wdf_rdy), .cmd_en(app_en),
        .cmd(app_cmd), .addr(app_addr), .wr_data_en(app_wdf_wren),
        .wr_data_end(app_wdf_end), .wr_data(app_wdf_data),
        .rd_data_valid(app_rd_data_valid), .rd_data(app_rd_data),
        .done(ddr_done), .failed(ddr_failed)
    );
"""
    if args.capture:
        rtl = replace(rtl, "    usb_rgba8_source source (", """    wire pattern_valid, camera_valid, camera_request, release_request;
    wire [7:0] pattern_byte, camera_byte;
    wire [14:0] camera_packet;
    wire [1:0] camera_batch;
    assign frame_valid = pattern_valid || camera_valid;
    assign frame_byte = camera_valid ? camera_byte : pattern_byte;
    usb_rgba8_source #(.CAMERA_READOUT(1)) source (""")
        rtl = replace(rtl, "        .valid(frame_valid), .data(frame_byte)",
                      """        .camera_request(camera_request), .camera_packet(camera_packet),
        .camera_batch(camera_batch),
        .release_request(release_request),
        .valid(pattern_valid), .data(pattern_byte)""")
        rtl = replace(rtl, ".frame_sequence(15'd0)", ".frame_sequence(usb_sequence)")
        rtl = replace(rtl, ".capture_trace(8'd0)", ".capture_trace(trace_sync)")
        start = rtl.index("    wire complete, dropped;")
        end = rtl.index("    usb_fifo usb_fifo", start)
        rtl = rtl[:start] + common + """
    // A slot is armed once per startup. A damaged raster can be retried
    // while the store is empty. The first valid raster stays frozen.
    wire frozen, shot_ready, token_in_valid, token_in_ready;
    wire token_out_valid, token_out_ready;
    wire [1:0] token_in_kind, token_out_kind;
    wire [255:0] token_in_data, token_out_data;
    reg [2:0] pixel_trace;
    always @(posedge rgb_clk or negedge rst_n)
        if (!rst_n) pixel_trace <= 0;
        else pixel_trace <= pixel_trace | {token_in_valid && !token_in_ready,
                                           arm_capture, rgb_vsync};
    reg [3:0] ddr_trace;
    always @(posedge clk_x1)
        if (!ddr_reset_sync[1]) ddr_trace <= 0;
        else if (token_out_valid && token_out_ready)
            ddr_trace[token_out_kind] <= 1;
    (* async_reg = "true" *) reg [7:0] trace_meta, trace_sync;
    always @(posedge PHY_CLKOUT) begin
        trace_meta <= {1'b1, ddr_trace, pixel_trace};
        trace_sync <= trace_meta;
    end
    (* async_reg = "true" *) reg [1:0] frozen_sync, pixels_reset_sync, calibrated_sync;
    (* async_reg = "true" *) reg [1:0] pll_sync, phase_sync, ddr_reset_sync;
    always @(posedge rgb_clk or negedge rst_n) begin
        if (!rst_n) begin
            pixels_reset_sync <= 0;
            frozen_sync <= 0;
            calibrated_sync <= 0;
            pll_sync <= 0;
            phase_sync <= 0;
        end else begin
            pixels_reset_sync <= {pixels_reset_sync[0], 1'b1};
            frozen_sync <= {frozen_sync[0], frozen};
            calibrated_sync <= {calibrated_sync[0], init_calib_complete};
            pll_sync <= {pll_sync[0], pll_ready};
            phase_sync <= {phase_sync[0], phase_ready};
        end
    end
    always @(posedge clk_x1 or negedge rst_n)
        if (!rst_n) ddr_reset_sync <= 0;
        else ddr_reset_sync <= {ddr_reset_sync[0], 1'b1};

    // Retry an abort after DDR consumes it. A committed slot stops capture.
    reg wait_for_result;
    reg result_toggle, frozen_previous, frozen_visible;
    reg [14:0] frame_sequence, frame_sequence_gray;
    wire [14:0] next_sequence = frame_sequence + 1'b1;
    (* async_reg = "true" *) reg [1:0] result_sync;
    always @(posedge clk_x1) begin
        if (!ddr_reset_sync[1]) begin
            result_toggle <= 0;
            frozen_previous <= 0;
            frozen_visible <= 0;
            frame_sequence <= 0;
            frame_sequence_gray <= 0;
        end else begin
            frozen_previous <= frozen;
            frozen_visible <= frozen && frozen_previous;
            if ((token_out_valid && token_out_ready && token_out_kind == 2'd3) ||
                (frozen_previous && !frozen))
                result_toggle <= !result_toggle;
            if (frozen && !frozen_previous) begin
                frame_sequence <= next_sequence;
                frame_sequence_gray <= next_sequence ^ (next_sequence >> 1);
            end
        end
    end
    reg result_at_arm;
    wire arm_capture = shot_ready && !wait_for_result && calibrated_sync[1] &&
                       pll_sync[1] && phase_sync[1] && !frozen_sync[1];
    always @(posedge rgb_clk) begin
        if (!pixels_reset_sync[1]) begin
            result_sync <= 0;
            result_at_arm <= 0;
            wait_for_result <= 0;
        end else begin
            result_sync <= {result_sync[0], result_toggle};
            if (arm_capture) begin
                result_at_arm <= result_sync[1];
                wait_for_result <= 1;
            end else if (result_sync[1] != result_at_arm) begin
                wait_for_result <= 0;
            end
        end
    end
    hdmi_frame_ingress ingress (
        .pixel_clk(rgb_clk), .reset(!pixels_reset_sync[1]),
        .shot_valid(arm_capture), .shot_ready(shot_ready),
        .vsync(rgb_vsync), .de(rgb_de), .red(red), .green(green), .blue(blue),
        .token_valid(token_in_valid), .token_ready(token_in_ready),
        .token_kind(token_in_kind), .token_data(token_in_data)
    );
    async_frame_tokens tokens (
        .write_clk(rgb_clk), .write_reset(!pixels_reset_sync[1]),
        .write_valid(token_in_valid), .write_ready(token_in_ready),
        .write_data({token_in_kind, token_in_data}),
        .read_clk(clk_x1), .read_reset(!ddr_reset_sync[1]),
        .read_valid(token_out_valid), .read_ready(token_out_ready),
        .read_data({token_out_kind, token_out_data})
    );
    rgba8_frame_store frame_store (
        .clk(clk_x1), .reset(!ddr_reset_sync[1]), .calibrated(init_calib_complete),
        .capture_valid(token_out_valid), .capture_ready(token_out_ready),
        .capture_kind(token_out_kind), .capture_data(token_out_data),
        .frame_ready(frozen), .release_frame(ddr_release),
        .read_valid(ddr_read_valid), .read_ready(ddr_read_ready),
        .read_index(ddr_read_index), .reply_valid(ddr_reply_valid),
        .reply_ready(ddr_reply_ready), .reply_data(ddr_reply_data),
        .cmd_ready(app_rdy), .wr_data_rdy(app_wdf_rdy), .cmd_en(app_en),
        .cmd(app_cmd), .addr(app_addr), .wr_data_en(app_wdf_wren),
        .wr_data_end(app_wdf_end), .wr_data(app_wdf_data),
        .rd_data_valid(app_rd_data_valid), .rd_data(app_rd_data)
    );
    (* async_reg = "true" *) reg [9:0] capture_status_meta, capture_status_sync;
    always @(posedge PHY_CLKOUT) begin
        capture_status_meta <= {hdmi_hpd, ddc_sda_activity, ddc_scl_activity,
                                ddc_sda_level, ddc_scl_level, edid_seen,
                                frozen_visible, init_calib_complete, phase_ready, pll_ready};
        capture_status_sync <= capture_status_meta;
    end
    assign camera_status = {8'hdd, 2'b0, capture_status_sync[9:4], 4'ha, capture_status_sync[3:0]};

    (* async_reg = "true" *) reg [1:0] usb_ddr_reset_sync;
    always @(posedge PHY_CLKOUT or negedge rst_n)
        if (!rst_n) usb_ddr_reset_sync <= 0;
        else usb_ddr_reset_sync <= {usb_ddr_reset_sync[0], 1'b1};
    localparam ALLOW_RELEASE = 1;
    wire [14:0] ddr_release_index;
    wire ddr_release_valid;
    wire ddr_release = ALLOW_RELEASE && ddr_release_valid && frozen_previous &&
                       frozen && ddr_release_index == frame_sequence;
    (* async_reg = "true" *) reg [14:0] sequence_meta, sequence_sync;
    wire [14:0] usb_sequence;
    genvar sequence_bit;
    generate for (sequence_bit = 0; sequence_bit < 15; sequence_bit = sequence_bit + 1)
        assign usb_sequence[sequence_bit] = ^sequence_sync[14:sequence_bit];
    endgenerate
    always @(posedge PHY_CLKOUT) begin
        sequence_meta <= frame_sequence_gray;
        sequence_sync <= sequence_meta;
    end
    async_frame_tokens #(.ADDR_BITS(2), .DATA_BITS(15)) releases (
        .write_clk(PHY_CLKOUT), .write_reset(!usb_ddr_reset_sync[1]),
        .write_valid(release_request), .write_data(camera_packet),
        .read_clk(clk_x1), .read_reset(!ddr_reset_sync[1]),
        .read_valid(ddr_release_valid), .read_ready(1'b1), .read_data(ddr_release_index)
    );
    wire usb_read_valid, usb_read_ready, ddr_read_valid, ddr_read_ready;
    wire usb_reply_valid, usb_reply_ready, ddr_reply_valid, ddr_reply_ready;
    wire [18:0] usb_read_index, ddr_read_index, usb_reply_index;
    wire [255:0] usb_reply_data, ddr_reply_data;
    reg [18:0] active_read_index;
    always @(posedge clk_x1)
        if (ddr_read_valid && ddr_read_ready) active_read_index <= ddr_read_index;
    async_frame_tokens #(.ADDR_BITS(2), .DATA_BITS(19)) read_requests (
        .write_clk(PHY_CLKOUT), .write_reset(!usb_ddr_reset_sync[1]),
        .write_valid(usb_read_valid), .write_ready(usb_read_ready), .write_data(usb_read_index),
        .read_clk(clk_x1), .read_reset(!ddr_reset_sync[1]),
        .read_valid(ddr_read_valid), .read_ready(ddr_read_ready), .read_data(ddr_read_index)
    );
    async_frame_tokens #(.ADDR_BITS(2), .DATA_BITS(275)) read_replies (
        .write_clk(clk_x1), .write_reset(!ddr_reset_sync[1]),
        .write_valid(ddr_reply_valid), .write_ready(ddr_reply_ready),
        .write_data({active_read_index, ddr_reply_data}),
        .read_clk(PHY_CLKOUT), .read_reset(!usb_ddr_reset_sync[1]),
        .read_valid(usb_reply_valid), .read_ready(usb_reply_ready),
        .read_data({usb_reply_index, usb_reply_data})
    );
    usb_ddr_frame_reply readout (
        .clk(PHY_CLKOUT), .reset(RESET | usb_busreset | !usb_ddr_reset_sync[1]),
        .frozen(capture_status_sync[3]), .request(camera_request), .packet(camera_packet),
        .batch(camera_batch),
        .read_valid(usb_read_valid), .read_ready(usb_read_ready), .read_index(usb_read_index),
        .reply_valid(usb_reply_valid), .reply_ready(usb_reply_ready),
        .reply_index(usb_reply_index), .reply_data(usb_reply_data),
        .valid(camera_valid), .data(camera_byte)
    );

""" + rtl[end:]
        # Gowin can bind an implicit scalar at a module port before it
        # encounters the later declaration. Keep the consumers after the
        # explicit command, sequence and data nets.
        store_start = rtl.index("    rgba8_frame_store frame_store (")
        store_end = rtl.index("    (* async_reg", store_start)
        store = rtl[store_start:store_end]
        rtl = rtl[:store_start] + rtl[store_end:]
        source_start = rtl.index("    usb_rgba8_source #(")
        source_end = rtl.index("    );", source_start) + len("    );\n")
        source = rtl[source_start:source_end]
        rtl = rtl[:source_start] + rtl[source_end:]
        rtl = replace(rtl, "    usb_fifo usb_fifo", store + source + "    usb_fifo usb_fifo")
        if args.readout_test:
            rtl = replace(rtl, "ALLOW_RELEASE = 1", "ALLOW_RELEASE = 0")
            rtl = replace(rtl, """        .capture_valid(token_out_valid), .capture_ready(token_out_ready),
        .capture_kind(token_out_kind), .capture_data(token_out_data),""", """        .capture_valid(seed_valid), .capture_ready(seed_ready),
        .capture_kind(seed_kind), .capture_data(seed_data),""")
            rtl = replace(rtl, "    rgba8_frame_store frame_store (", """    assign token_out_ready = 1'b1;
    wire seed_valid, seed_ready;
    wire [1:0] seed_kind;
    wire [255:0] seed_data;
    rgba8_ddr_test_pattern seed (
        .clk(clk_x1), .reset(!ddr_reset_sync[1] || !init_calib_complete),
        .valid(seed_valid), .ready(seed_ready), .kind(seed_kind), .data(seed_data)
    );
    rgba8_frame_store frame_store (""")
            rtl = replace(rtl, "4'ha, capture_status_sync[3:0]",
                          "4'hb, capture_status_sync[3:0]")
        if args.raster_test:
            # Keep the physical receiver diagnostics, while the capture
            # pipeline consumes a distinctly tagged internal raster source.
            start = rtl.index("    // A slot is armed once per startup.")
            end = rtl.index("    rgba8_frame_store frame_store", start)
            capture = rtl[start:end].replace("rgb_clk", "CLK_IN")
            capture = replace(capture, "arm_capture, rgb_vsync", "arm_capture, test_vsync")
            capture = replace(capture, "pll_sync[1] && phase_sync[1] && !frozen_sync[1]",
                              "!frozen_sync[1]")
            capture = replace(capture, ".vsync(rgb_vsync), .de(rgb_de), .red(red), .green(green), .blue(blue)",
                              ".vsync(test_vsync), .de(test_de), .red(test_red), .green(test_green), .blue(test_blue)")
            rtl = rtl[:start] + """    wire test_vsync, test_de;
    wire [7:0] test_red, test_green, test_blue;
    hdmi_test_raster test_raster (
        .clk(CLK_IN), .reset(!rst_n), .vsync(test_vsync), .de(test_de),
        .red(test_red), .green(test_green), .blue(test_blue)
    );
""" + capture + rtl[end:]
            rtl = replace(rtl, "4'ha, capture_status_sync[3:0]",
                          "4'hc, capture_status_sync[3:0]")
    else:
        rtl = replace(rtl, "    wire [23:0] camera_status = 0;", memory_rtl)
    top.write_text(rtl)

    paths = {
        "truega_ddr_pll.v": vendor / "gowin_pll/gowin_pll.v",
        "truega_ddr_core.v": vendor / "ddr3_memory_interface/ddr3_memory_interface.v",
    }
    local_sources = (("hdmi_frame_ingress.v", "rgba8_ddr_burst.v", "async_frame_tokens.v",
                      "usb_ddr_frame_reply.v")
                     if args.capture else ("rgba8_ddr_selftest.v",))
    for name in ("rgba8_frame_store.v", *local_sources):
        paths[name] = Path(__file__).with_name(name)
    if args.readout_test:
        paths["rgba8_ddr_test_pattern.v"] = Path(__file__).with_name("rgba8_ddr_test_pattern.v")
    if args.raster_test:
        paths["hdmi_test_raster.v"] = Path(__file__).with_name("hdmi_test_raster.v")
    for name, path in paths.items():
        shutil.copyfile(path, src / name)

    with (src / "usb_ref.cst").open("a") as constraints:
        constraints.write("\n")
        for line in (vendor / "pro.cst").read_text().splitlines():
            if '"ddr_' in line:
                constraints.write(line + "\n")

    with (src / "usb_ref.sdc").open("a") as constraints:
        constraints.write("\n")
        for line in (vendor / "pro.sdc").read_text().splitlines():
            if line.startswith("create_clock"):
                constraints.write(line + "\n")
        # The vendor PHY treats its three clock domains as asynchronous.
        constraints.write("set_clock_groups -asynchronous "
                          "-group [get_clocks {clk100}] -group [get_clocks {clk_x1}] "
                          "-group [get_clocks {mem400}]\n")
        constraints.write("set_clock_groups -asynchronous "
                          "-group [get_clocks {PHY_CLKOUT}] -group [get_clocks {clk_x1}]\n")
        if args.raster_test:
            constraints.write("set_clock_groups -asynchronous "
                              "-group [get_clocks {clk_50M}] -group [get_clocks {clk_x1}]\n")
        if args.capture and not (args.readout_test or args.raster_test):
            constraints.write("create_clock -name rx_pixel -period 13.468 "
                              "[get_pins {receiver/dvi2rgb_inst/u_clkdiv5/CLKOUT}]\n"
                              "set_clock_groups -asynchronous -group [get_clocks {rx_pixel}] "
                              "-group [get_clocks {clk_50M}] -group [get_clocks {clk_x1}] "
                              "-group [get_clocks {PHY_CLKOUT}]\n")

    project, = args.staging.glob("*.gprj")
    entries = "".join(f'<File path="src/{name}" type="file.verilog" enable="1"/>\n    '
                      for name in paths)
    project.write_text(replace(project.read_text(), "</FileList>", entries + "</FileList>"))


if __name__ == "__main__":
    main()
