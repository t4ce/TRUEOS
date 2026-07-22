// Fixed layer-0 gate-row diagnostic slot.
//
// This is the boundary between the already-proven Q8_0 GEMV datapath and a
// future native-image/DDR reader.  After start_i is accepted, the feeder
// presents exactly 32 unchanged Q8_0 activation/weight block pairs.  The slot
// supplies the requested block index and applies ordinary ready/valid
// backpressure; it never interprets model metadata or generates addresses.
//
// DIAGNOSTIC_ENABLE defaults to zero so adding this source to the project does
// not create a callable function or alter the heartbeat bitstream.  The sealed
// golden-vector testbench overrides it to one.
module truega_lfm25_gate_row_slot #(
    parameter DIAGNOSTIC_ENABLE = 0
) (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 start_i,

    output wire                 feeder_ready_o,
    output wire [4:0]           feeder_block_index_o,
    input  wire                 feeder_valid_i,
    input  wire [271:0]         feeder_activation_block_i,
    input  wire [271:0]         feeder_weight_block_i,

    output reg                  busy_o,
    output reg                  done_o,
    output reg                  error_o,
    output reg  [5:0]           blocks_accepted_o,
    output reg  signed [63:0]   row_q30_o
);
    localparam STATE_IDLE  = 2'd0;
    localparam STATE_FEED  = 2'd1;
    localparam STATE_DRAIN = 2'd2;

    reg [1:0] state;
    reg block_in_flight;
    wire start_accept = DIAGNOSTIC_ENABLE && start_i && (state == STATE_IDLE);
    wire feed_accept = feeder_valid_i && feeder_ready_o;
    wire gemv_reset_n = reset_n && (state != STATE_IDLE);
    wire gemv_block_valid;
    wire signed [20:0] gemv_block_dot;
    wire signed [63:0] gemv_block_term_q30;
    wire gemv_row_valid;
    wire signed [63:0] gemv_row_q30;
    wire gemv_scale_error;

    // The dot product is intentionally serialized: a new native Q8_0 block
    // may be accepted only after the previous block has retired.  Keep that
    // backpressure at this feeder boundary so an upstream BAR/ROM reader
    // cannot advance and silently drop blocks while the GEMV is busy.
    assign feeder_ready_o = DIAGNOSTIC_ENABLE
                          && (state == STATE_FEED)
                          && !block_in_flight;
    assign feeder_block_index_o = blocks_accepted_o[4:0];

    truega_q8_0_gemv gemv (
        .clk(clk),
        .reset_n(gemv_reset_n),
        .valid_i(feed_accept),
        .row_first_i(feed_accept && (blocks_accepted_o == 6'd0)),
        .row_last_i(feed_accept && (blocks_accepted_o == 6'd31)),
        .activation_scale_f16_i(feeder_activation_block_i[15:0]),
        .weight_scale_f16_i(feeder_weight_block_i[15:0]),
        .activation_quants_i(feeder_activation_block_i[271:16]),
        .weight_quants_i(feeder_weight_block_i[271:16]),
        .block_valid_o(gemv_block_valid),
        .block_dot_o(gemv_block_dot),
        .block_term_q30_o(gemv_block_term_q30),
        .row_valid_o(gemv_row_valid),
        .row_q30_o(gemv_row_q30),
        .scale_error_o(gemv_scale_error)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= STATE_IDLE;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            blocks_accepted_o <= 6'd0;
            row_q30_o <= 64'sd0;
            block_in_flight <= 1'b0;
        end else begin
            done_o <= 1'b0;

            case (state)
                STATE_IDLE: begin
                    busy_o <= 1'b0;
                    if (start_accept) begin
                        state <= STATE_FEED;
                        busy_o <= 1'b1;
                        error_o <= 1'b0;
                        blocks_accepted_o <= 6'd0;
                        row_q30_o <= 64'sd0;
                        block_in_flight <= 1'b0;
                    end
                end

                STATE_FEED: begin
                    if (gemv_block_valid)
                        block_in_flight <= 1'b0;
                    if (feed_accept) begin
                        block_in_flight <= 1'b1;
                        blocks_accepted_o <= blocks_accepted_o + 6'd1;
                        if (blocks_accepted_o == 6'd31)
                            state <= STATE_DRAIN;
                    end
                end

                STATE_DRAIN: begin
                    if (gemv_row_valid) begin
                        state <= STATE_IDLE;
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= gemv_scale_error;
                        row_q30_o <= gemv_row_q30;
                        block_in_flight <= 1'b0;
                    end
                end

                default: begin
                    state <= STATE_IDLE;
                    busy_o <= 1'b0;
                    done_o <= 1'b1;
                    error_o <= 1'b1;
                    block_in_flight <= 1'b0;
                end
            endcase
        end
    end

    // These per-block observability signals deliberately terminate here.  They
    // keep the GEMV implementation intact while the fixed diagnostic contract
    // returns only the complete signed-Q30 row result.
    wire unused_gemv_outputs = gemv_block_valid
                             ^ gemv_block_dot[0]
                             ^ gemv_block_term_q30[0];
endmodule
