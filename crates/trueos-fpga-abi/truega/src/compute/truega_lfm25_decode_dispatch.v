// Fixed BAR-register dispatcher for the AOT LFM2.5 decode circuits.
//
// This block is a one-of-ten circuit mux, not a processor or command queue.  A
// request is accepted only while idle, its complete register envelope is
// latched once, and exactly one fused circuit receives execute_start_o.  That
// circuit must retire before another doorbell can be accepted.
//
// ENABLE defaults to zero.  Capability words remain zero until the complete
// matching execution engine is deliberately enabled in the board top level.
module truega_lfm25_decode_dispatch #(
    parameter integer ENABLE = 0
) (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire [31:0]          command_i,
    input  wire [31:0]          position_i,
    input  wire [31:0]          session_epoch_i,
    input  wire                 doorbell_i,
    input  wire [31:0]          doorbell_value_i,

    output wire [31:0]          capability_magic_o,
    output wire [31:0]          capability_bits_o,
    output reg  [31:0]          state_o,
    output reg  [31:0]          result0_o,
    output reg  [31:0]          result1_o,
    output reg  signed [63:0]   argmax_score_q30_o,

    output reg                  execute_start_o,
    output reg  [3:0]           execute_operation_o,
    output reg  [7:0]           execute_layer_o,
    output reg  [31:0]          execute_position_o,
    output reg  [7:0]           execute_input_slot_o,
    output reg  [7:0]           execute_residual_slot_o,
    output reg  [31:0]          execute_session_epoch_o,
    output reg                  execute_session_begin_o,

    input  wire                 engine_done_i,
    input  wire                 engine_error_i,
    input  wire [31:0]          engine_error_code_i,
    input  wire [7:0]           engine_result_slot_i,
    input  wire [31:0]          engine_result_position_i,
    input  wire [31:0]          engine_argmax_token_i,
    input  wire [31:0]          engine_argmax_rows_i,
    input  wire signed [63:0]   engine_argmax_score_q30_i,
    output reg                  retire_o
);
    localparam [31:0] CAPABILITY_MAGIC = 32'h31444754; // "TGD1"
    localparam [31:0] CAPABILITY_BITS = 32'h000003ff;
    localparam [31:0] DOORBELL_MAGIC = 32'h4f434544; // "DECO"
    localparam [31:0] STATE_IDLE = 32'd0;
    localparam [31:0] STATE_BUSY = 32'd1;
    localparam [31:0] STATE_COMPLETE = 32'd2;
    localparam [31:0] STATE_FAILED = 32'd3;
    localparam [31:0] ERROR_DISABLED = 32'hbad30001;
    localparam [31:0] ERROR_DOORBELL = 32'hbad30002;
    localparam [31:0] ERROR_COMMAND = 32'hbad30003;
    localparam [31:0] ERROR_SESSION = 32'hbad30004;
    localparam [31:0] ERROR_COMPLETION = 32'hbad30005;

    localparam [3:0] OP_TOKEN_EMBEDDING = 4'd0;
    localparam [3:0] OP_OPERATOR_RMSNORM = 4'd1;
    localparam [3:0] OP_SHORTCONV = 4'd2;
    localparam [3:0] OP_ATTENTION = 4'd3;
    localparam [3:0] OP_OPERATOR_RESIDUAL = 4'd4;
    localparam [3:0] OP_FFN_RMSNORM = 4'd5;
    localparam [3:0] OP_FFN = 4'd6;
    localparam [3:0] OP_FFN_RESIDUAL = 4'd7;
    localparam [3:0] OP_FINAL_RMSNORM = 4'd8;
    localparam [3:0] OP_LM_HEAD_ARGMAX = 4'd9;

    wire [7:0] request_operation_byte = command_i[7:0];
    wire [3:0] request_operation = request_operation_byte[3:0];
    wire [7:0] request_layer = command_i[15:8];
    wire [7:0] request_input_slot = command_i[23:16];
    wire [7:0] request_residual_slot = command_i[31:24];
    wire operation_valid = request_operation_byte <= 8'd9;
    wire layer_present = request_layer != 8'hff;
    wire input_present = request_input_slot != 8'hff;
    wire residual_present = request_residual_slot != 8'hff;
    wire layer_valid = !layer_present || request_layer < 8'd16;

    reg shape_valid;
    always @* begin
        shape_valid = 1'b0;
        case (request_operation)
            OP_TOKEN_EMBEDDING:
                shape_valid = !layer_present && !input_present && !residual_present;
            OP_OPERATOR_RMSNORM,
            OP_SHORTCONV,
            OP_ATTENTION,
            OP_FFN_RMSNORM,
            OP_FFN:
                shape_valid = layer_present && input_present && !residual_present;
            OP_OPERATOR_RESIDUAL,
            OP_FFN_RESIDUAL:
                shape_valid = layer_present && input_present && residual_present;
            OP_FINAL_RMSNORM,
            OP_LM_HEAD_ARGMAX:
                shape_valid = !layer_present && input_present && !residual_present;
            default:
                shape_valid = 1'b0;
        endcase
    end

    reg session_valid;
    reg [31:0] active_session_epoch;
    reg active_session_begin;
    wire request_begins_session = request_operation == OP_TOKEN_EMBEDDING
        && position_i == 32'd0
        && session_epoch_i != 32'd0
        && (!session_valid || session_epoch_i != active_session_epoch);
    wire request_matches_session = session_valid
        && session_epoch_i == active_session_epoch;
    wire request_session_valid = request_begins_session || request_matches_session;

    assign capability_magic_o = ENABLE != 0 ? CAPABILITY_MAGIC : 32'd0;
    assign capability_bits_o = ENABLE != 0 ? CAPABILITY_BITS : 32'd0;

    always @(posedge clk) begin
        if (!reset_n) begin
            state_o <= STATE_IDLE;
            result0_o <= 32'd0;
            result1_o <= 32'd0;
            argmax_score_q30_o <= 64'sd0;
            execute_start_o <= 1'b0;
            execute_operation_o <= 4'd0;
            execute_layer_o <= 8'hff;
            execute_position_o <= 32'd0;
            execute_input_slot_o <= 8'hff;
            execute_residual_slot_o <= 8'hff;
            execute_session_epoch_o <= 32'd0;
            execute_session_begin_o <= 1'b0;
            retire_o <= 1'b0;
            session_valid <= 1'b0;
            active_session_epoch <= 32'd0;
            active_session_begin <= 1'b0;
        end else begin
            execute_start_o <= 1'b0;
            execute_session_begin_o <= 1'b0;
            retire_o <= 1'b0;

            if (engine_done_i && state_o == STATE_BUSY) begin
                if (engine_error_i) begin
                    state_o <= STATE_FAILED;
                    result0_o <= engine_error_code_i;
                    result1_o <= execute_position_o;
                    if (active_session_begin)
                        session_valid <= 1'b0;
                end else if (execute_operation_o == OP_LM_HEAD_ARGMAX) begin
                    if (engine_argmax_token_i < 32'd65536
                        && engine_argmax_rows_i == 32'd65536) begin
                        state_o <= STATE_COMPLETE;
                        result0_o <= engine_argmax_token_i;
                        result1_o <= engine_argmax_rows_i;
                        argmax_score_q30_o <= engine_argmax_score_q30_i;
                    end else begin
                        state_o <= STATE_FAILED;
                        result0_o <= ERROR_COMPLETION;
                        result1_o <= execute_position_o;
                    end
                end else if (engine_result_slot_i != 8'hff
                    && engine_result_position_i == execute_position_o) begin
                    state_o <= STATE_COMPLETE;
                    result0_o <= {24'd0, engine_result_slot_i};
                    result1_o <= engine_result_position_i;
                    if (active_session_begin)
                        session_valid <= 1'b1;
                end else begin
                    state_o <= STATE_FAILED;
                    result0_o <= ERROR_COMPLETION;
                    result1_o <= execute_position_o;
                    if (active_session_begin)
                        session_valid <= 1'b0;
                end
                active_session_begin <= 1'b0;
                retire_o <= 1'b1;
            end else if (doorbell_i) begin
                result0_o <= 32'd0;
                result1_o <= 32'd0;
                argmax_score_q30_o <= 64'sd0;
                if (ENABLE == 0) begin
                    state_o <= STATE_FAILED;
                    result0_o <= ERROR_DISABLED;
                    retire_o <= 1'b1;
                end else if (state_o == STATE_BUSY || doorbell_value_i != DOORBELL_MAGIC) begin
                    state_o <= STATE_FAILED;
                    result0_o <= ERROR_DOORBELL;
                    retire_o <= 1'b1;
                end else if (!operation_valid || !layer_valid || !shape_valid
                    || session_epoch_i == 32'd0) begin
                    state_o <= STATE_FAILED;
                    result0_o <= ERROR_COMMAND;
                    retire_o <= 1'b1;
                end else if (!request_session_valid) begin
                    state_o <= STATE_FAILED;
                    result0_o <= ERROR_SESSION;
                    retire_o <= 1'b1;
                end else begin
                    state_o <= STATE_BUSY;
                    execute_operation_o <= request_operation;
                    execute_layer_o <= request_layer;
                    execute_position_o <= position_i;
                    execute_input_slot_o <= request_input_slot;
                    execute_residual_slot_o <= request_residual_slot;
                    execute_session_epoch_o <= session_epoch_i;
                    execute_start_o <= 1'b1;
                    active_session_begin <= request_begins_session;
                    if (request_begins_session) begin
                        // Immediately invalidate the old session.  It cannot be
                        // resurrected if the replacement embedding later fails.
                        session_valid <= 1'b0;
                        active_session_epoch <= session_epoch_i;
                        execute_session_begin_o <= 1'b1;
                    end
                end
            end
        end
    end
endmodule
