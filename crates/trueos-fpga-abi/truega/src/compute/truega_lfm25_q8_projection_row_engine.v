// Fixed Q8_0 projection-row engine for the LFM2.5 decode join.
//
// One transaction loads exactly 32 ordered native activation blocks, then
// consumes exactly 32 ordered native weight blocks for each fixed output row.
// Each accepted row is evaluated by the proven truega_q8_0_gemv circuit and
// published as a full signed-i64 Q30 value. Output row/data/first/last remain
// stable under backpressure. Tag or scale errors poison the engine until the
// explicit state-reset handshake recovers it.
//
// This is a standalone fixed circuit: no command parser, BAR, DMA, host, or
// runtime shape machinery exists here. Activation payload RAM is intentionally
// unreset and synchronously read; reset only clears sequencing metadata.
module truega_lfm25_q8_projection_row_engine #(
    parameter integer ROW_COUNT = 1024
) (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 abort_i,

    input  wire                 state_reset_i,
    output wire                 state_reset_ready_o,
    output reg                  state_reset_done_o,

    input  wire                 start_i,
    output wire                 start_ready_o,

    input  wire                 activation_valid_i,
    output wire                 activation_ready_o,
    output wire [4:0]           activation_block_index_o,
    input  wire [4:0]           activation_block_index_i,
    input  wire [271:0]         activation_q8_block_i,

    input  wire                 weight_valid_i,
    output wire                 weight_ready_o,
    output wire [12:0]          weight_row_index_o,
    output wire [4:0]           weight_block_index_o,
    input  wire [12:0]          weight_row_index_i,
    input  wire [4:0]           weight_block_index_i,
    input  wire [271:0]         weight_q8_block_i,

    output wire                 result_valid_o,
    input  wire                 result_ready_i,
    output reg  [12:0]          result_row_index_o,
    output reg  signed [63:0]   result_q30_o,
    output reg                  result_first_o,
    output reg                  result_last_o,

    output reg                  busy_o,
    output reg                  done_o,
    output reg                  error_o,
    output reg                  poisoned_o,
    output reg  [7:0]           error_code_o,
    output reg  [12:0]          rows_retired_o
);
    localparam [2:0] ST_IDLE       = 3'd0;
    localparam [2:0] ST_LOAD_ACT   = 3'd1;
    localparam [2:0] ST_ACT_READ   = 3'd2;
    localparam [2:0] ST_ROW_FEED   = 3'd3;
    localparam [2:0] ST_BLOCK_WAIT = 3'd4;
    localparam [2:0] ST_ROW_DRAIN  = 3'd5;
    localparam [2:0] ST_ROW_OUTPUT = 3'd6;

    localparam [7:0] ERROR_PARAMETER        = 8'd1;
    localparam [7:0] ERROR_ACTIVATION_ORDER = 8'd2;
    localparam [7:0] ERROR_WEIGHT_ORDER     = 8'd3;
    localparam [7:0] ERROR_SCALE            = 8'd4;
    localparam [7:0] ERROR_INTERNAL         = 8'd5;
    localparam [7:0] ERROR_ABORT            = 8'd6;

    reg [2:0] state;
    reg [5:0] activation_count;
    reg [12:0] current_row;
    reg [4:0] current_block;
    reg [4:0] activation_read_index;
    reg [271:0] activation_memory [0:31];
    reg [271:0] activation_read_data;
    wire gemv_ready;

    wire parameter_contract_valid = ROW_COUNT == 512
        || ROW_COUNT == 1024 || ROW_COUNT == 2048
        || ROW_COUNT == 3072 || ROW_COUNT == 4608;
    assign state_reset_ready_o = state == ST_IDLE && !start_i;
    assign start_ready_o = state == ST_IDLE && !state_reset_i;
    assign activation_ready_o = state == ST_LOAD_ACT;
    assign activation_block_index_o = activation_count[4:0];
    assign weight_ready_o = state == ST_ROW_FEED && gemv_ready;
    assign weight_row_index_o = current_row;
    assign weight_block_index_o = current_block;
    assign result_valid_o = state == ST_ROW_OUTPUT;

    wire activation_accept = activation_valid_i && activation_ready_o;
    wire activation_sequence_valid = activation_block_index_i
        == activation_count[4:0];
    wire weight_accept = weight_valid_i && weight_ready_o;
    wire weight_sequence_valid = weight_row_index_i == current_row
        && weight_block_index_i == current_block;
    wire result_accept = result_valid_o && result_ready_i;

    // Unreset synchronous payload read. No activation is consumed before the
    // complete-load counter reaches 32.
    always @(posedge clk) begin
        activation_read_data <= activation_memory[activation_read_index];
    end

    wire gemv_reset_n = reset_n && !state_reset_i && state != ST_IDLE;
    wire gemv_block_valid;
    wire signed [20:0] gemv_block_dot;
    wire signed [63:0] gemv_block_term_q30;
    wire gemv_row_valid;
    wire signed [63:0] gemv_row_q30;
    wire gemv_scale_error;
    truega_q8_0_gemv row_gemv (
        .clk(clk),
        .reset_n(gemv_reset_n),
        .valid_i(weight_accept && weight_sequence_valid),
        .ready_o(gemv_ready),
        .row_first_i(weight_accept && weight_sequence_valid
            && current_block == 5'd0),
        .row_last_i(weight_accept && weight_sequence_valid
            && current_block == 5'd31),
        .activation_scale_f16_i(activation_read_data[15:0]),
        .weight_scale_f16_i(weight_q8_block_i[15:0]),
        .activation_quants_i(activation_read_data[271:16]),
        .weight_quants_i(weight_q8_block_i[271:16]),
        .block_valid_o(gemv_block_valid),
        .block_dot_o(gemv_block_dot),
        .block_term_q30_o(gemv_block_term_q30),
        .row_valid_o(gemv_row_valid),
        .row_q30_o(gemv_row_q30),
        .scale_error_o(gemv_scale_error)
    );

    task automatic poison;
        input [7:0] code;
        begin
            poisoned_o <= 1'b1;
            error_o <= 1'b1;
            error_code_o <= code;
            busy_o <= 1'b0;
            done_o <= 1'b1;
            state <= ST_IDLE;
        end
    endtask

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            activation_count <= 6'd0;
            current_row <= 13'd0;
            current_block <= 5'd0;
            activation_read_index <= 5'd0;
            result_row_index_o <= 13'd0;
            result_q30_o <= 64'sd0;
            result_first_o <= 1'b0;
            result_last_o <= 1'b0;
            state_reset_done_o <= 1'b0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            poisoned_o <= 1'b0;
            error_code_o <= 8'd0;
            rows_retired_o <= 13'd0;
        end else begin
            state_reset_done_o <= 1'b0;
            done_o <= 1'b0;
            if (abort_i && state != ST_IDLE) begin
                poison(ERROR_ABORT);
            end else case (state)
                ST_IDLE: begin
                    busy_o <= 1'b0;
                    if (state_reset_i && state_reset_ready_o) begin
                        activation_count <= 6'd0;
                        current_row <= 13'd0;
                        current_block <= 5'd0;
                        activation_read_index <= 5'd0;
                        result_row_index_o <= 13'd0;
                        result_q30_o <= 64'sd0;
                        result_first_o <= 1'b0;
                        result_last_o <= 1'b0;
                        rows_retired_o <= 13'd0;
                        poisoned_o <= 1'b0;
                        error_o <= 1'b0;
                        error_code_o <= 8'd0;
                        state_reset_done_o <= 1'b1;
                    end else if (start_i && start_ready_o) begin
                        if (poisoned_o) begin
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                        end else if (!parameter_contract_valid) begin
                            poison(ERROR_PARAMETER);
                        end else begin
                            activation_count <= 6'd0;
                            current_row <= 13'd0;
                            current_block <= 5'd0;
                            activation_read_index <= 5'd0;
                            result_row_index_o <= 13'd0;
                            result_q30_o <= 64'sd0;
                            result_first_o <= 1'b0;
                            result_last_o <= 1'b0;
                            rows_retired_o <= 13'd0;
                            error_o <= 1'b0;
                            error_code_o <= 8'd0;
                            busy_o <= 1'b1;
                            state <= ST_LOAD_ACT;
                        end
                    end
                end

                ST_LOAD_ACT: begin
                    if (activation_accept) begin
                        if (!activation_sequence_valid) begin
                            poison(ERROR_ACTIVATION_ORDER);
                        end else begin
                            activation_memory[activation_count[4:0]]
                                <= activation_q8_block_i;
                            activation_count <= activation_count + 6'd1;
                            if (activation_count == 6'd31) begin
                                current_row <= 13'd0;
                                current_block <= 5'd0;
                                activation_read_index <= 5'd0;
                                state <= ST_ACT_READ;
                            end
                        end
                    end
                end

                ST_ACT_READ: begin
                    state <= ST_ROW_FEED;
                end

                ST_ROW_FEED: begin
                    if (weight_accept) begin
                        if (!weight_sequence_valid) begin
                            poison(ERROR_WEIGHT_ORDER);
                        end else if (current_block == 5'd31) begin
                            state <= ST_ROW_DRAIN;
                        end else begin
                            state <= ST_BLOCK_WAIT;
                        end
                    end
                end

                ST_BLOCK_WAIT: begin
                    if (gemv_block_valid) begin
                        current_block <= current_block + 5'd1;
                        activation_read_index <= current_block + 5'd1;
                        state <= ST_ACT_READ;
                    end
                end

                ST_ROW_DRAIN: begin
                    if (gemv_row_valid) begin
                        if (gemv_scale_error) begin
                            poison(ERROR_SCALE);
                        end else begin
                            result_row_index_o <= current_row;
                            result_q30_o <= gemv_row_q30;
                            result_first_o <= current_row == 13'd0;
                            result_last_o <= current_row == ROW_COUNT - 1;
                            state <= ST_ROW_OUTPUT;
                        end
                    end
                end

                ST_ROW_OUTPUT: begin
                    if (result_accept) begin
                        rows_retired_o <= rows_retired_o + 13'd1;
                        if (result_last_o) begin
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b0;
                            state <= ST_IDLE;
                        end else begin
                            current_row <= current_row + 13'd1;
                            current_block <= 5'd0;
                            activation_read_index <= 5'd0;
                            state <= ST_ACT_READ;
                        end
                    end
                end

                default: begin
                    poison(ERROR_INTERNAL);
                end
            endcase
        end
    end

    wire unused_gemv_observability = gemv_block_dot[0]
        ^ gemv_block_term_q30[0];
endmodule
