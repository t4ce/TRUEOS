// LFM2.5 shortconv input projection for one output channel.
//
// lfm2.cpp projects the RMS-normalized 1024-vector through one Q8_0 matrix and
// splits the 3072 outputs in the fixed order b, c, x.  This slot proves that
// boundary without a graph interpreter: after start, the feeder presents the
// same 32 activation blocks and the corresponding b/c/x row blocks.  Three
// native Q8_0 GEMV lanes return the signed-Q30 projected scalars.
module truega_lfm25_shortconv_triplet_row_slot (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 start_i,
    output wire                 feeder_ready_o,
    output wire [4:0]           feeder_block_index_o,
    input  wire                 feeder_valid_i,
    input  wire [271:0]         feeder_activation_block_i,
    input  wire [271:0]         feeder_b_weight_block_i,
    input  wire [271:0]         feeder_c_weight_block_i,
    input  wire [271:0]         feeder_x_weight_block_i,
    output reg                  busy_o,
    output reg                  done_o,
    output reg                  error_o,
    output reg [5:0]            blocks_accepted_o,
    output reg signed [63:0]    b_q30_o,
    output reg signed [63:0]    c_q30_o,
    output reg signed [63:0]    x_q30_o
);
    localparam [1:0] ST_IDLE  = 2'd0;
    localparam [1:0] ST_FEED  = 2'd1;
    localparam [1:0] ST_DRAIN = 2'd2;

    reg [1:0] state;
    reg b_seen;
    reg c_seen;
    reg x_seen;
    reg b_error;
    reg c_error;
    reg x_error;
    reg block_in_flight;

    wire feed_accept = feeder_valid_i && feeder_ready_o;
    wire row_first = feed_accept && (blocks_accepted_o == 6'd0);
    wire row_last = feed_accept && (blocks_accepted_o == 6'd31);
    wire gemv_reset_n = reset_n && (state != ST_IDLE);

    wire b_row_valid;
    wire c_row_valid;
    wire x_row_valid;
    wire b_block_valid;
    wire c_block_valid;
    wire x_block_valid;
    wire signed [63:0] b_row;
    wire signed [63:0] c_row;
    wire signed [63:0] x_row;
    wire b_scale_error;
    wire c_scale_error;
    wire x_scale_error;
    wire b_gemv_ready;
    wire c_gemv_ready;
    wire x_gemv_ready;

    // truega_q8_0_gemv is intentionally serialized.  Hold the feeder until all
    // three lanes retire the current block so no accepted native block can be
    // overwritten or silently dropped.
    assign feeder_ready_o = (state == ST_FEED) && !block_in_flight
        && b_gemv_ready && c_gemv_ready && x_gemv_ready;
    assign feeder_block_index_o = blocks_accepted_o[4:0];

    truega_q8_0_gemv b_gemv (
        .clk(clk), .reset_n(gemv_reset_n), .valid_i(feed_accept),
        .ready_o(b_gemv_ready),
        .row_first_i(row_first), .row_last_i(row_last),
        .activation_scale_f16_i(feeder_activation_block_i[15:0]),
        .weight_scale_f16_i(feeder_b_weight_block_i[15:0]),
        .activation_quants_i(feeder_activation_block_i[271:16]),
        .weight_quants_i(feeder_b_weight_block_i[271:16]),
        .block_valid_o(b_block_valid), .block_dot_o(), .block_term_q30_o(),
        .row_valid_o(b_row_valid), .row_q30_o(b_row),
        .scale_error_o(b_scale_error)
    );

    truega_q8_0_gemv c_gemv (
        .clk(clk), .reset_n(gemv_reset_n), .valid_i(feed_accept),
        .ready_o(c_gemv_ready),
        .row_first_i(row_first), .row_last_i(row_last),
        .activation_scale_f16_i(feeder_activation_block_i[15:0]),
        .weight_scale_f16_i(feeder_c_weight_block_i[15:0]),
        .activation_quants_i(feeder_activation_block_i[271:16]),
        .weight_quants_i(feeder_c_weight_block_i[271:16]),
        .block_valid_o(c_block_valid), .block_dot_o(), .block_term_q30_o(),
        .row_valid_o(c_row_valid), .row_q30_o(c_row),
        .scale_error_o(c_scale_error)
    );

    truega_q8_0_gemv x_gemv (
        .clk(clk), .reset_n(gemv_reset_n), .valid_i(feed_accept),
        .ready_o(x_gemv_ready),
        .row_first_i(row_first), .row_last_i(row_last),
        .activation_scale_f16_i(feeder_activation_block_i[15:0]),
        .weight_scale_f16_i(feeder_x_weight_block_i[15:0]),
        .activation_quants_i(feeder_activation_block_i[271:16]),
        .weight_quants_i(feeder_x_weight_block_i[271:16]),
        .block_valid_o(x_block_valid), .block_dot_o(), .block_term_q30_o(),
        .row_valid_o(x_row_valid), .row_q30_o(x_row),
        .scale_error_o(x_scale_error)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            blocks_accepted_o <= 6'd0;
            b_q30_o <= 64'sd0;
            c_q30_o <= 64'sd0;
            x_q30_o <= 64'sd0;
            b_seen <= 1'b0;
            c_seen <= 1'b0;
            x_seen <= 1'b0;
            b_error <= 1'b0;
            c_error <= 1'b0;
            x_error <= 1'b0;
            block_in_flight <= 1'b0;
        end else begin
            done_o <= 1'b0;
            if (b_block_valid && c_block_valid && x_block_valid)
                block_in_flight <= 1'b0;
            case (state)
                ST_IDLE: begin
                    busy_o <= 1'b0;
                    if (start_i) begin
                        state <= ST_FEED;
                        busy_o <= 1'b1;
                        error_o <= 1'b0;
                        blocks_accepted_o <= 6'd0;
                        b_q30_o <= 64'sd0;
                        c_q30_o <= 64'sd0;
                        x_q30_o <= 64'sd0;
                        b_seen <= 1'b0;
                        c_seen <= 1'b0;
                        x_seen <= 1'b0;
                        b_error <= 1'b0;
                        c_error <= 1'b0;
                        x_error <= 1'b0;
                        block_in_flight <= 1'b0;
                    end
                end

                ST_FEED: begin
                    if (feed_accept) begin
                        block_in_flight <= 1'b1;
                        blocks_accepted_o <= blocks_accepted_o + 6'd1;
                        if (blocks_accepted_o == 6'd31)
                            state <= ST_DRAIN;
                    end
                end

                ST_DRAIN: begin
                    if (b_row_valid) begin
                        b_q30_o <= b_row;
                        b_seen <= 1'b1;
                        b_error <= b_scale_error;
                    end
                    if (c_row_valid) begin
                        c_q30_o <= c_row;
                        c_seen <= 1'b1;
                        c_error <= c_scale_error;
                    end
                    if (x_row_valid) begin
                        x_q30_o <= x_row;
                        x_seen <= 1'b1;
                        x_error <= x_scale_error;
                    end

                    if ((b_seen || b_row_valid)
                        && (c_seen || c_row_valid)
                        && (x_seen || x_row_valid)) begin
                        state <= ST_IDLE;
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= (b_error || b_scale_error)
                            || (c_error || c_scale_error)
                            || (x_error || x_scale_error);
                    end
                end

                default: begin
                    state <= ST_IDLE;
                    busy_o <= 1'b0;
                    done_o <= 1'b1;
                    error_o <= 1'b1;
                end
            endcase
        end
    end
endmodule
