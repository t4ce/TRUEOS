// Fixed native-model ingress for the Gowin DDR3 controller application port.
//
// This is deliberately not a command processor.  A producer in the DDR user
// clock domain presents the sealed image as little-endian 32-bit words.  Eight
// words are packed into one native 256-bit DDR beat.  The image size is fixed at
// synthesis time and the writer stops after exactly that many bytes.
//
// The PCIe/TLP clock and the DDR user clock are separate clock domains in the
// integrated design.  Put an asynchronous FIFO before this module; do not drive
// data_valid_i directly from the BAR decoder.

module truega_ddr3_model_writer #(
    parameter [31:0] EXPECTED_BYTES = 32'd376701952,
    // Native Gowin application addresses count 32-bit words.  A 256-bit beat
    // therefore advances the address by eight.
    parameter [28:0] BASE_ADDR_DWORDS = 29'd0
) (
    input  wire         clk,
    input  wire         reset_n,
    input  wire         init_calib_complete_i,

    input  wire         start_i,
    input  wire [31:0]  data_i,
    input  wire         data_valid_i,
    output wire         data_ready_o,

    output reg          busy_o,
    output reg          done_o,
    output reg          error_o,
    output reg  [31:0]  bytes_received_o,
    output reg  [31:0]  beats_written_o,

    input  wire         app_cmd_ready_i,
    input  wire         app_wr_data_ready_i,
    output wire [2:0]   app_cmd_o,
    output wire         app_cmd_en_o,
    output wire [28:0]  app_addr_o,
    output wire [255:0] app_wr_data_o,
    output wire         app_wr_data_en_o,
    output wire         app_wr_data_end_o,
    output wire [31:0]  app_wr_data_mask_o,
    output wire         app_burst_o
);

localparam [31:0] EXPECTED_BEATS = EXPECTED_BYTES >> 5;

reg [255:0] beat_data;
reg [2:0]   word_index;
reg         beat_pending;
reg [28:0]  write_addr;

wire accept_word = data_valid_i && data_ready_o;
wire accept_beat = beat_pending && app_cmd_ready_i && app_wr_data_ready_i;

assign data_ready_o = busy_o && init_calib_complete_i && !beat_pending
                    && (bytes_received_o < EXPECTED_BYTES);

assign app_cmd_o          = 3'b000; // Gowin native-port write
assign app_cmd_en_o       = accept_beat;
assign app_addr_o         = write_addr;
assign app_wr_data_o      = beat_data;
assign app_wr_data_en_o   = accept_beat;
assign app_wr_data_end_o  = accept_beat;
assign app_wr_data_mask_o = 32'b0;
assign app_burst_o        = 1'b0;

always @(posedge clk or negedge reset_n) begin
    if (!reset_n) begin
        busy_o          <= 1'b0;
        done_o          <= 1'b0;
        error_o         <= 1'b0;
        bytes_received_o <= 32'b0;
        beats_written_o <= 32'b0;
        beat_data       <= 256'b0;
        word_index      <= 3'b0;
        beat_pending    <= 1'b0;
        write_addr      <= BASE_ADDR_DWORDS;
    end else begin
        done_o <= 1'b0;

        if (start_i) begin
            if (busy_o || !init_calib_complete_i || (EXPECTED_BYTES[4:0] != 5'b0)) begin
                busy_o  <= 1'b0;
                error_o <= 1'b1;
            end else begin
                busy_o           <= 1'b1;
                error_o          <= 1'b0;
                bytes_received_o <= 32'b0;
                beats_written_o  <= 32'b0;
                beat_data        <= 256'b0;
                word_index       <= 3'b0;
                beat_pending     <= 1'b0;
                write_addr       <= BASE_ADDR_DWORDS;
            end
        end else if (busy_o) begin
            // Losing calibration invalidates the in-flight residency proof.
            if (!init_calib_complete_i) begin
                busy_o  <= 1'b0;
                error_o <= 1'b1;
            end else begin
                if (accept_word) begin
                    beat_data[word_index * 32 +: 32] <= data_i;
                    bytes_received_o <= bytes_received_o + 32'd4;
                    if (word_index == 3'd7) begin
                        word_index   <= 3'b0;
                        beat_pending <= 1'b1;
                    end else begin
                        word_index <= word_index + 3'd1;
                    end
                end

                if (accept_beat) begin
                    beat_pending   <= 1'b0;
                    write_addr     <= write_addr + 29'd8;
                    beats_written_o <= beats_written_o + 32'd1;
                    if (beats_written_o == (EXPECTED_BEATS - 32'd1)) begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                    end
                end
            end
        end
    end
end

endmodule
