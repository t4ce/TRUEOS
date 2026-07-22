// Fixed final decode tail for one LFM2.5 token.
//
// A typed resident Q30[1024] handle is consumed by the shared resident-vector
// engine's fixed RMSNorm operation.  Exactly 1,024 canonical BF16 weights are
// accepted in index order and the resulting resident Q8_0[32] handle is read
// directly into the tied 65,536-row LM-head argmax circuit.  Only the winning
// token and its signed i64 Q30 score leave this join; the normalized activation
// is never materialized as a host tensor.
//
// The resident ports below attach to the one shared resident-vector engine.
// Handles are typed circuit metadata, not addresses.  There is no parser,
// processor, DMA, TLB, runtime graph, or host-side tensor arithmetic.
module truega_lfm25_resident_decode_tail #(
    parameter integer LM_HEAD_ROWS = 65536
) (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 clear_i,
    input  wire                 abort_i,

    input  wire                 start_i,
    output wire                 start_ready_o,
    input  wire [36:0]          source_q30_handle_i,
    input  wire [36:0]          normalized_q8_handle_i,

    input  wire                 norm_weight_valid_i,
    output wire                 norm_weight_ready_o,
    output wire [9:0]           expected_norm_weight_index_o,
    input  wire [9:0]           norm_weight_index_i,
    input  wire                 norm_weight_format_bf16_i,
    input  wire [31:0]          norm_weight_bits_i,

    // Explicit operation boundary.  This publication is held after the
    // FinalRmsNorm feed/command completes and before any tied-head state is
    // released.  The host worker can therefore retire and publish the
    // normalized resident handle as its own TGD1 result before beginning the
    // separate TiedLmHeadRows feed/argmax operation.
    output wire                 norm_result_valid_o,
    input  wire                 norm_result_ready_i,
    output wire [36:0]          norm_result_handle_o,

    input  wire                 lm_weight_valid_i,
    output wire                 lm_weight_ready_o,
    output wire [31:0]          expected_lm_row_o,
    output wire [4:0]           expected_lm_block_o,
    input  wire [31:0]          lm_weight_row_i,
    input  wire [4:0]           lm_weight_block_i,
    input  wire [271:0]         lm_weight_q8_block_i,

    output wire                 lm_row_done_o,
    output wire                 lm_row_error_o,
    output wire [31:0]          lm_row_retired_index_o,
    output wire signed [63:0]   lm_row_score_q30_o,

    // Verification throttle.  Production ties this low.  It proves that a
    // resident Q8 reply remains owned and stable while the head is paused.
    input  wire                 activation_pause_i,

    output wire                 result_valid_o,
    input  wire                 result_ready_i,
    output reg                  result_error_o,
    output reg  [7:0]           result_error_code_o,
    output reg  [31:0]          result_token_o,
    output reg signed [63:0]    result_score_q30_o,
    output reg  [16:0]          result_rows_retired_o,
    output reg                  poisoned_o,
    output wire                 busy_o,

    // Shared resident-vector typed command/result interface.
    output wire                 resident_command_valid_o,
    input  wire                 resident_command_ready_i,
    output wire [1:0]           resident_command_operation_o,
    output wire [36:0]          resident_command_source0_handle_o,
    output wire [36:0]          resident_command_source1_handle_o,
    output wire [36:0]          resident_command_destination_handle_o,
    input  wire                 resident_result_valid_i,
    output wire                 resident_result_ready_o,
    input  wire                 resident_result_error_i,
    input  wire [36:0]          resident_result_handle_i,
    output wire                 resident_abort_o,

    // Shared resident-vector RMSNorm weight stream.
    output wire                 resident_weight_valid_o,
    input  wire                 resident_weight_ready_i,
    output wire [9:0]           resident_weight_index_o,
    output wire                 resident_weight_format_bf16_o,
    output wire [31:0]          resident_weight_bits_o,

    // Shared resident-vector typed inspection interface.
    output wire                 resident_inspect_valid_o,
    input  wire                 resident_inspect_ready_i,
    output wire [36:0]          resident_inspect_handle_o,
    output wire [9:0]           resident_inspect_index_o,
    input  wire                 resident_inspect_rsp_valid_i,
    output wire                 resident_inspect_rsp_ready_o,
    input  wire                 resident_inspect_rsp_error_i,
    input  wire [271:0]         resident_inspect_rsp_data_i
);
    localparam [3:0] ST_IDLE          = 4'd0;
    localparam [3:0] ST_NORM_COMMAND  = 4'd1;
    localparam [3:0] ST_NORM_STREAM   = 4'd2;
    localparam [3:0] ST_NORM_ABORT    = 4'd3;
    localparam [3:0] ST_HEAD_RESET    = 4'd4;
    localparam [3:0] ST_HEAD_START    = 4'd5;
    localparam [3:0] ST_ACT_REQUEST   = 4'd6;
    localparam [3:0] ST_ACT_REPLY     = 4'd7;
    localparam [3:0] ST_HEAD_ROWS     = 4'd8;
    localparam [3:0] ST_ABORT_INSPECT = 4'd9;
    localparam [3:0] ST_RESULT        = 4'd10;
    localparam [3:0] ST_NORM_RESULT   = 4'd11;

    localparam [1:0] OP_RMSNORM = 2'd1;

    localparam [7:0] ERROR_NONE        = 8'd0;
    localparam [7:0] ERROR_HANDLE      = 8'd1;
    localparam [7:0] ERROR_NORM_DOMAIN = 8'd2;
    localparam [7:0] ERROR_NORM        = 8'd3;
    localparam [7:0] ERROR_INSPECT     = 8'd4;
    localparam [7:0] ERROR_HEAD        = 8'd5;
    localparam [7:0] ERROR_ABORT       = 8'd6;

    reg [3:0] state;
    reg [36:0] source_handle;
    reg [36:0] normalized_handle;
    reg [10:0] norm_weight_count;
    reg [5:0] activation_block;
    reg norm_fault;

    wire joined_reset_n = reset_n && !clear_i;
    wire source_handle_valid = source_q30_handle_i[36:5] != 32'd0
        && source_q30_handle_i[4] == 1'b0
        && source_q30_handle_i[3:0] < 4'd4;
    wire normalized_handle_valid = normalized_q8_handle_i[36:5] != 32'd0
        && normalized_q8_handle_i[4] == 1'b1
        && normalized_q8_handle_i[3:0] < 4'd4;
    wire handles_valid = source_handle_valid && normalized_handle_valid
        && source_q30_handle_i[36:5] == normalized_q8_handle_i[36:5];

    assign start_ready_o = state == ST_IDLE && !poisoned_o
        && resident_command_ready_i && !resident_inspect_rsp_valid_i;
    assign result_valid_o = state == ST_RESULT;
    assign norm_result_valid_o = state == ST_NORM_RESULT;
    assign norm_result_handle_o = normalized_handle;
    assign busy_o = state != ST_IDLE && state != ST_RESULT;
    assign expected_norm_weight_index_o = norm_weight_count[9:0];

    assign resident_command_valid_o = state == ST_NORM_COMMAND && !abort_i;
    assign resident_command_operation_o = OP_RMSNORM;
    assign resident_command_source0_handle_o = source_handle;
    assign resident_command_source1_handle_o = 37'd0;
    assign resident_command_destination_handle_o = normalized_handle;

    wire canonical_bf16 = norm_weight_format_bf16_i
        && norm_weight_bits_i[31:16] == 16'd0;
    wire norm_tag_valid = norm_weight_index_i
        == norm_weight_count[9:0];
    wire norm_attempt = state == ST_NORM_STREAM
        && norm_weight_valid_i && resident_weight_ready_i
        && !abort_i && !norm_fault;
    wire norm_attempt_valid = norm_attempt && canonical_bf16
        && norm_tag_valid;
    wire norm_attempt_invalid = norm_attempt && (!canonical_bf16
        || !norm_tag_valid);

    assign norm_weight_ready_o = state == ST_NORM_STREAM
        && resident_weight_ready_i && !abort_i && !norm_fault;
    assign resident_weight_valid_o = norm_attempt_valid;
    assign resident_weight_index_o = norm_weight_index_i;
    assign resident_weight_format_bf16_o = 1'b1;
    assign resident_weight_bits_o = norm_weight_bits_i;
    assign resident_result_ready_o = state == ST_NORM_STREAM
        || state == ST_NORM_ABORT;
    assign resident_abort_o = (state == ST_NORM_STREAM
        && (abort_i || norm_fault || norm_attempt_invalid))
        || state == ST_NORM_ABORT;

    // The normalized resident vector is synchronously inspected one Q8 block
    // at a time.  A response is not consumed while the verification throttle
    // is asserted, so both the shared store and payload remain stable.
    assign resident_inspect_valid_o = state == ST_ACT_REQUEST && !abort_i;
    assign resident_inspect_handle_o = normalized_handle;
    assign resident_inspect_index_o = {4'd0, activation_block[4:0]};
    assign resident_inspect_rsp_ready_o = state == ST_ABORT_INSPECT
        ? 1'b1 : state == ST_ACT_REPLY
        && head_activation_ready && !activation_pause_i && !abort_i;

    wire head_reset_n = joined_reset_n
        && !(abort_i && (state == ST_HEAD_RESET
            || state == ST_HEAD_START || state == ST_ACT_REQUEST
            || state == ST_ACT_REPLY || state == ST_HEAD_ROWS));
    wire head_state_reset = state == ST_HEAD_RESET;
    wire head_state_reset_ready;
    wire head_state_reset_done;
    wire head_start = state == ST_HEAD_START;
    wire head_start_ready;
    wire head_activation_valid = state == ST_ACT_REPLY
        && resident_inspect_rsp_valid_i && !resident_inspect_rsp_error_i
        && !activation_pause_i && !abort_i;
    wire head_activation_ready;
    wire head_row_ready;
    wire [31:0] head_expected_row;
    wire [4:0] head_expected_block;
    wire head_row_done;
    wire head_row_error;
    wire [31:0] head_retired_row;
    wire signed [63:0] head_row_score;
    wire head_busy;
    wire head_done;
    wire head_error;
    wire head_poisoned;
    wire [16:0] head_rows_retired;
    wire [31:0] head_token;
    wire signed [63:0] head_score;

    assign lm_weight_ready_o = state == ST_HEAD_ROWS && head_row_ready
        && !abort_i;
    assign expected_lm_row_o = head_expected_row;
    assign expected_lm_block_o = head_expected_block;
    assign lm_row_done_o = state == ST_HEAD_ROWS && head_row_done;
    assign lm_row_error_o = state == ST_HEAD_ROWS && head_row_error;
    assign lm_row_retired_index_o = head_retired_row;
    assign lm_row_score_q30_o = head_row_score;

    truega_lfm25_tied_lm_head_argmax_slot #(
        .ROW_COUNT(LM_HEAD_ROWS)
    ) tied_lm_head (
        .clk(clk), .reset_n(head_reset_n),
        .state_reset_i(head_state_reset),
        .state_reset_ready_o(head_state_reset_ready),
        .state_reset_done_o(head_state_reset_done),
        .start_i(head_start), .start_ready_o(head_start_ready),
        .activation_valid_i(head_activation_valid),
        .activation_ready_o(head_activation_ready),
        .activation_block_index_o(),
        .activation_block_index_i(activation_block[4:0]),
        .activation_q8_block_i(resident_inspect_rsp_data_i),
        .row_valid_i(state == ST_HEAD_ROWS && lm_weight_valid_i
            && !abort_i),
        .row_ready_o(head_row_ready), .row_index_o(head_expected_row),
        .row_block_index_o(head_expected_block),
        .row_index_i(lm_weight_row_i),
        .row_block_index_i(lm_weight_block_i),
        .row_weight_q8_block_i(lm_weight_q8_block_i),
        .row_done_o(head_row_done), .row_error_o(head_row_error),
        .row_retired_index_o(head_retired_row),
        .row_score_q30_o(head_row_score), .busy_o(head_busy),
        .done_o(head_done), .error_o(head_error),
        .poisoned_o(head_poisoned), .rows_retired_o(head_rows_retired),
        .token_o(head_token), .score_q30_o(head_score)
    );

    always @(posedge clk) begin
        if (!joined_reset_n) begin
            state <= ST_IDLE;
            source_handle <= 37'd0;
            normalized_handle <= 37'd0;
            norm_weight_count <= 11'd0;
            activation_block <= 6'd0;
            norm_fault <= 1'b0;
            result_error_o <= 1'b0;
            result_error_code_o <= ERROR_NONE;
            result_token_o <= 32'd0;
            result_score_q30_o <= 64'sd0;
            result_rows_retired_o <= 17'd0;
            poisoned_o <= 1'b0;
        end else begin
            case (state)
                ST_IDLE: begin
                    if (start_i && start_ready_o) begin
                        source_handle <= source_q30_handle_i;
                        normalized_handle <= normalized_q8_handle_i;
                        norm_weight_count <= 11'd0;
                        activation_block <= 6'd0;
                        norm_fault <= 1'b0;
                        result_error_o <= 1'b0;
                        result_error_code_o <= ERROR_NONE;
                        result_token_o <= 32'd0;
                        result_score_q30_o <= 64'sd0;
                        result_rows_retired_o <= 17'd0;
                        if (!handles_valid) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_HANDLE;
                            state <= ST_RESULT;
                        end else begin
                            state <= ST_NORM_COMMAND;
                        end
                    end
                end

                ST_NORM_COMMAND: begin
                    if (abort_i) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        state <= ST_RESULT;
                    end else if (resident_command_valid_o
                            && resident_command_ready_i) begin
                        norm_weight_count <= 11'd0;
                        state <= ST_NORM_STREAM;
                    end
                end

                ST_NORM_STREAM: begin
                    if (norm_attempt_invalid) begin
                        norm_fault <= 1'b1;
                        poisoned_o <= 1'b1;
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_NORM_DOMAIN;
                        state <= ST_NORM_ABORT;
                    end else if (abort_i) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        state <= ST_NORM_ABORT;
                    end else begin
                        if (norm_attempt_valid)
                            norm_weight_count <= norm_weight_count + 11'd1;
                        if (resident_result_valid_i) begin
                            if (resident_result_error_i
                                    || resident_result_handle_i
                                        != normalized_handle
                                    || norm_weight_count != 11'd1024) begin
                                result_error_o <= 1'b1;
                                result_error_code_o <= ERROR_NORM;
                                state <= ST_RESULT;
                            end else begin
                                activation_block <= 6'd0;
                                state <= ST_NORM_RESULT;
                            end
                        end
                    end
                end

                ST_NORM_ABORT: begin
                    if (resident_result_valid_i) begin
                        // The aborted resident command is consumed before the
                        // public result is exposed, keeping the shared lane
                        // ownership exact for the next operation.
                        state <= ST_RESULT;
                    end
                end

                ST_NORM_RESULT: begin
                    // Nothing belonging to the head is asserted in this
                    // state.  The typed handle and valid bit remain stable
                    // until the distinct FinalRmsNorm result is retired.
                    if (abort_i) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        state <= ST_RESULT;
                    end else if (norm_result_valid_o
                            && norm_result_ready_i) begin
                        state <= ST_HEAD_RESET;
                    end
                end

                ST_HEAD_RESET: begin
                    if (abort_i) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        state <= ST_RESULT;
                    end else if (head_state_reset_done) begin
                        state <= ST_HEAD_START;
                    end
                end

                ST_HEAD_START: begin
                    if (abort_i) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        state <= ST_RESULT;
                    end else if (head_start_ready) begin
                        activation_block <= 6'd0;
                        state <= ST_ACT_REQUEST;
                    end
                end

                ST_ACT_REQUEST: begin
                    if (abort_i) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        state <= ST_RESULT;
                    end else if (resident_inspect_valid_o
                            && resident_inspect_ready_i) begin
                        state <= ST_ACT_REPLY;
                    end
                end

                ST_ACT_REPLY: begin
                    if (abort_i) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        state <= resident_inspect_rsp_valid_i
                            && resident_inspect_rsp_ready_o
                            ? ST_RESULT : ST_ABORT_INSPECT;
                    end else if (resident_inspect_rsp_valid_i
                            && resident_inspect_rsp_error_i) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_INSPECT;
                        // Consume this reply on the same edge.
                        state <= ST_RESULT;
                    end else if (head_activation_valid
                            && head_activation_ready) begin
                        if (activation_block == 6'd31) begin
                            state <= ST_HEAD_ROWS;
                        end else begin
                            activation_block <= activation_block + 6'd1;
                            state <= ST_ACT_REQUEST;
                        end
                    end
                end

                ST_ABORT_INSPECT: begin
                    if (resident_inspect_rsp_valid_i
                            && resident_inspect_rsp_ready_o)
                        state <= ST_RESULT;
                end

                ST_HEAD_ROWS: begin
                    if (abort_i) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        result_rows_retired_o <= head_rows_retired;
                        state <= ST_RESULT;
                    end else if (head_done) begin
                        result_rows_retired_o <= head_rows_retired;
                        if (head_error || head_poisoned
                                || head_rows_retired != LM_HEAD_ROWS) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_HEAD;
                            if (head_poisoned)
                                poisoned_o <= 1'b1;
                        end else begin
                            result_token_o <= head_token;
                            result_score_q30_o <= head_score;
                        end
                        state <= ST_RESULT;
                    end
                end

                ST_RESULT: begin
                    if (result_valid_o && result_ready_i)
                        state <= ST_IDLE;
                end

                default: begin
                    poisoned_o <= 1'b1;
                    result_error_o <= 1'b1;
                    result_error_code_o <= ERROR_HEAD;
                    result_token_o <= 32'd0;
                    result_score_q30_o <= 64'sd0;
                    result_rows_retired_o <= 17'd0;
                    state <= ST_RESULT;
                end
            endcase
        end
    end

    wire unused_head = head_busy ^ head_state_reset_ready;
endmodule
