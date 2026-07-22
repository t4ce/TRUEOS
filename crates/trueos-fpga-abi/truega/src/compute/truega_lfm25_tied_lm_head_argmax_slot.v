// Resident one-token tied LM-head row engine.
//
// The activation is loaded once as exactly 32 native Q8_0 blocks.  The engine
// then requests exactly ROW_COUNT ordered weight rows, 32 native Q8_0 blocks
// per row, and reuses truega_q8_0_gemv for every signed-Q30 score.  Production
// uses the default ROW_COUNT=65536, matching the fixed LFM2.5 vocabulary.  The
// parameter exists only so the complete control/argmax contract can be tested
// without simulating 2,097,152 blocks.
//
// Payload RAM is intentionally unreset and synchronously read.  It is never
// observable before a complete activation load.  Sequence/validity metadata
// is reset.  A mismatched response tag or row arithmetic error poisons the
// lane; state_reset_i is the only recovery path.
module truega_lfm25_tied_lm_head_argmax_slot #(
    parameter integer ROW_COUNT = 65536
) (
    input  wire                 clk,
    input  wire                 reset_n,

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

    input  wire                 row_valid_i,
    output wire                 row_ready_o,
    output wire [31:0]          row_index_o,
    output wire [4:0]           row_block_index_o,
    input  wire [31:0]          row_index_i,
    input  wire [4:0]           row_block_index_i,
    input  wire [271:0]         row_weight_q8_block_i,

    output reg                  row_done_o,
    output reg                  row_error_o,
    output reg [31:0]           row_retired_index_o,
    output reg signed [63:0]    row_score_q30_o,

    output reg                  busy_o,
    output reg                  done_o,
    output reg                  error_o,
    output reg                  poisoned_o,
    output reg [16:0]           rows_retired_o,
    output reg [31:0]           token_o,
    output reg signed [63:0]    score_q30_o
);
    localparam [2:0] ST_IDLE       = 3'd0;
    localparam [2:0] ST_LOAD_ACT   = 3'd1;
    localparam [2:0] ST_ACT_READ   = 3'd2;
    localparam [2:0] ST_ROW_FEED   = 3'd3;
    localparam [2:0] ST_BLOCK_WAIT = 3'd4;
    localparam [2:0] ST_ROW_DRAIN  = 3'd5;
    localparam integer BLOCKS_PER_ROW = 32;

    reg [2:0] state;
    reg [5:0] activation_count;
    reg [31:0] current_row;
    reg [4:0] current_block;
    reg [271:0] activation_memory [0:31];
    reg [271:0] activation_block;
    reg max_valid;
    reg [31:0] max_token;
    reg signed [63:0] max_score_q30;

    wire parameter_contract_valid = ROW_COUNT > 0 && ROW_COUNT <= 65536;
    assign state_reset_ready_o = state == ST_IDLE && !start_i;
    assign start_ready_o = state == ST_IDLE && !state_reset_i;
    assign activation_ready_o = state == ST_LOAD_ACT;
    assign activation_block_index_o = activation_count[4:0];
    assign row_ready_o = state == ST_ROW_FEED;
    assign row_index_o = current_row;
    assign row_block_index_o = current_block;

    wire activation_accept = activation_valid_i && activation_ready_o;
    wire activation_sequence_valid = activation_block_index_i
        == activation_count[4:0];
    wire row_accept = row_valid_i && row_ready_o;
    wire row_sequence_valid = row_index_i == current_row
        && row_block_index_i == current_block;

    wire gemv_reset_n = reset_n && state != ST_IDLE;
    wire gemv_block_valid;
    wire signed [20:0] gemv_block_dot;
    wire signed [63:0] gemv_block_term_q30;
    wire gemv_row_valid;
    wire signed [63:0] gemv_row_q30;
    wire gemv_scale_error;
    truega_q8_0_gemv row_gemv (
        .clk(clk),
        .reset_n(gemv_reset_n),
        .valid_i(row_accept && row_sequence_valid),
        .row_first_i(row_accept && row_sequence_valid
            && current_block == 5'd0),
        .row_last_i(row_accept && row_sequence_valid
            && current_block == 5'd31),
        .activation_scale_f16_i(activation_block[15:0]),
        .weight_scale_f16_i(row_weight_q8_block_i[15:0]),
        .activation_quants_i(activation_block[271:16]),
        .weight_quants_i(row_weight_q8_block_i[271:16]),
        .block_valid_o(gemv_block_valid),
        .block_dot_o(gemv_block_dot),
        .block_term_q30_o(gemv_block_term_q30),
        .row_valid_o(gemv_row_valid),
        .row_q30_o(gemv_row_q30),
        .scale_error_o(gemv_scale_error)
    );

    wire row_beats_max = !max_valid
        || $signed(gemv_row_q30) > $signed(max_score_q30);
    wire [31:0] candidate_token = row_beats_max
        ? current_row : max_token;
    wire signed [63:0] candidate_score_q30 = row_beats_max
        ? gemv_row_q30 : max_score_q30;

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            activation_count <= 6'd0;
            current_row <= 32'd0;
            current_block <= 5'd0;
            activation_block <= 272'd0;
            max_valid <= 1'b0;
            max_token <= 32'd0;
            max_score_q30 <= 64'sd0;
            state_reset_done_o <= 1'b0;
            row_done_o <= 1'b0;
            row_error_o <= 1'b0;
            row_retired_index_o <= 32'd0;
            row_score_q30_o <= 64'sd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            poisoned_o <= 1'b0;
            rows_retired_o <= 17'd0;
            token_o <= 32'd0;
            score_q30_o <= 64'sd0;
        end else begin
            state_reset_done_o <= 1'b0;
            row_done_o <= 1'b0;
            row_error_o <= 1'b0;
            done_o <= 1'b0;

            case (state)
                ST_IDLE: begin
                    busy_o <= 1'b0;
                    if (state_reset_i && state_reset_ready_o) begin
                        poisoned_o <= 1'b0;
                        error_o <= 1'b0;
                        activation_count <= 6'd0;
                        current_row <= 32'd0;
                        current_block <= 5'd0;
                        max_valid <= 1'b0;
                        rows_retired_o <= 17'd0;
                        token_o <= 32'd0;
                        score_q30_o <= 64'sd0;
                        state_reset_done_o <= 1'b1;
                    end else if (start_i && start_ready_o) begin
                        if (poisoned_o || !parameter_contract_valid) begin
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                        end else begin
                            activation_count <= 6'd0;
                            current_row <= 32'd0;
                            current_block <= 5'd0;
                            max_valid <= 1'b0;
                            max_token <= 32'd0;
                            max_score_q30 <= 64'sd0;
                            rows_retired_o <= 17'd0;
                            token_o <= 32'd0;
                            score_q30_o <= 64'sd0;
                            row_retired_index_o <= 32'd0;
                            row_score_q30_o <= 64'sd0;
                            busy_o <= 1'b1;
                            error_o <= 1'b0;
                            state <= ST_LOAD_ACT;
                        end
                    end
                end

                ST_LOAD_ACT: begin
                    if (activation_accept) begin
                        if (!activation_sequence_valid) begin
                            poisoned_o <= 1'b1;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                            state <= ST_IDLE;
                        end else begin
                            activation_memory[activation_count[4:0]]
                                <= activation_q8_block_i;
                            activation_count <= activation_count + 6'd1;
                            if (activation_count == 6'd31) begin
                                current_row <= 32'd0;
                                current_block <= 5'd0;
                                state <= ST_ACT_READ;
                            end
                        end
                    end
                end

                ST_ACT_READ: begin
                    // Registered read keeps the 32x272 payload as an unreset
                    // synchronous RAM instead of a resettable register bank.
                    activation_block <= activation_memory[current_block];
                    state <= ST_ROW_FEED;
                end

                ST_ROW_FEED: begin
                    if (row_accept) begin
                        if (!row_sequence_valid) begin
                            poisoned_o <= 1'b1;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                            row_done_o <= 1'b1;
                            row_error_o <= 1'b1;
                            row_retired_index_o <= current_row;
                            row_score_q30_o <= 64'sd0;
                            state <= ST_IDLE;
                        end else begin
                            if (current_block == 5'd31)
                                state <= ST_ROW_DRAIN;
                            else
                                state <= ST_BLOCK_WAIT;
                        end
                    end
                end

                ST_BLOCK_WAIT: begin
                    if (gemv_block_valid) begin
                        current_block <= current_block + 5'd1;
                        state <= ST_ACT_READ;
                    end
                end

                ST_ROW_DRAIN: begin
                    if (gemv_row_valid) begin
                        row_done_o <= 1'b1;
                        row_retired_index_o <= current_row;
                        row_score_q30_o <= gemv_row_q30;
                        if (gemv_scale_error) begin
                            row_error_o <= 1'b1;
                            poisoned_o <= 1'b1;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                            state <= ST_IDLE;
                        end else begin
                            rows_retired_o <= rows_retired_o + 17'd1;
                            max_valid <= 1'b1;
                            max_token <= candidate_token;
                            max_score_q30 <= candidate_score_q30;
                            if (current_row == ROW_COUNT - 1) begin
                                token_o <= candidate_token;
                                score_q30_o <= candidate_score_q30;
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                error_o <= 1'b0;
                                state <= ST_IDLE;
                            end else begin
                                current_row <= current_row + 32'd1;
                                current_block <= 5'd0;
                                state <= ST_ACT_READ;
                            end
                        end
                    end
                end

                default: begin
                    poisoned_o <= 1'b1;
                    busy_o <= 1'b0;
                    done_o <= 1'b1;
                    error_o <= 1'b1;
                    state <= ST_IDLE;
                end
            endcase
        end
    end

    wire unused_gemv_observability = gemv_block_dot[0]
        ^ gemv_block_term_q30[0];
endmodule
