// Fixed resident RMSNorm and residual joins for the LFM2.5 decode circuit.
//
// This controller is deliberately only a typed connection to one shared
// truega_lfm25_resident_vector_engine.  It never exposes resident payloads to
// the host:
//
//   RMSNorm:     resident Q30[1024] + 1024 ordered BF16 weights
//                -> resident Q8_0[32]
//   ResidualAdd: resident Q30[1024] + resident Q30[1024]
//                -> resident Q30[1024]
//
// The resident engine owns the transactional destination store.  Consequently
// an abort, malformed weight stream, stale source, or worker error can never
// publish a partially replaced destination.  Handles are circuit metadata:
// [36:5] session epoch, [4] type (0=Q30, 1=Q8_0), [3:0] slot.  Token position
// is latched alongside the command and returned unchanged with its result.
// There is no parser, processor, DMA, TLB, or host tensor arithmetic here.
module truega_lfm25_resident_norm_residual_join (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 clear_i,
    input  wire                 abort_i,

    input  wire                 start_i,
    output wire                 start_ready_o,
    // 0 = RMSNorm, 1 = ResidualAdd.
    input  wire                 operation_i,
    input  wire [36:0]          source0_q30_handle_i,
    input  wire [36:0]          source1_q30_handle_i,
    input  wire [36:0]          destination_handle_i,
    input  wire [31:0]          token_position_i,

    input  wire                 weight_valid_i,
    output wire                 weight_ready_o,
    output wire [9:0]           expected_weight_index_o,
    input  wire [9:0]           weight_index_i,
    input  wire                 weight_format_bf16_i,
    input  wire [31:0]          weight_bits_i,

    output wire                 result_valid_o,
    input  wire                 result_ready_i,
    output reg                  result_error_o,
    output reg  [7:0]           result_error_code_o,
    output reg                  result_operation_o,
    output reg  [31:0]          result_token_position_o,
    output reg  [36:0]          result_handle_o,

    // Diagnostic typed readback of the current destination.  A Q30 handle
    // uses indices 0..1023; a Q8_0 handle uses block indices 0..31.
    input  wire                 output_read_valid_i,
    output wire                 output_read_ready_o,
    input  wire [9:0]           output_read_index_i,
    output wire                 output_read_rsp_valid_o,
    input  wire                 output_read_rsp_ready_i,
    output wire                 output_read_error_o,
    output wire [271:0]         output_read_data_o,

    // Shared resident-vector typed command interface.
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

    // RMSNorm weight input of the shared resident-vector engine.
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
    input  wire [271:0]         resident_inspect_rsp_data_i,

    output reg  [10:0]          weights_accepted_o,
    output reg                  poisoned_o,
    output wire                 busy_o
);
    localparam [2:0] ST_IDLE        = 3'd0;
    localparam [2:0] ST_COMMAND     = 3'd1;
    localparam [2:0] ST_NORM_STREAM = 3'd2;
    localparam [2:0] ST_RESIDUAL    = 3'd3;
    localparam [2:0] ST_RESULT      = 3'd4;

    localparam [1:0] OP_RMSNORM      = 2'd1;
    localparam [1:0] OP_RESIDUAL_ADD = 2'd2;

    localparam [7:0] ERROR_NONE          = 8'd0;
    localparam [7:0] ERROR_HANDLE        = 8'd1;
    localparam [7:0] ERROR_WEIGHT_ORDER  = 8'd2;
    localparam [7:0] ERROR_WEIGHT_FORMAT = 8'd3;
    localparam [7:0] ERROR_RESIDENT      = 8'd4;
    localparam [7:0] ERROR_RESULT        = 8'd5;
    localparam [7:0] ERROR_ABORT         = 8'd6;

    reg [2:0] state;
    reg active_operation;
    reg [36:0] source0_handle;
    reg [36:0] source1_handle;
    reg [36:0] destination_handle;
    reg abort_seen;
    reg protocol_failed;
    reg [7:0] protocol_error_code;

    wire joined_reset_n = reset_n && !clear_i;
    wire source0_shape_valid = source0_q30_handle_i[36:5] != 32'd0
        && source0_q30_handle_i[4] == 1'b0
        && source0_q30_handle_i[3:0] < 4'd4;
    wire source1_shape_valid = source1_q30_handle_i[36:5] != 32'd0
        && source1_q30_handle_i[4] == 1'b0
        && source1_q30_handle_i[3:0] < 4'd4;
    wire norm_destination_valid = destination_handle_i[36:5] != 32'd0
        && destination_handle_i[4] == 1'b1
        && destination_handle_i[3:0] < 4'd4;
    wire residual_destination_valid = destination_handle_i[36:5] != 32'd0
        && destination_handle_i[4] == 1'b0
        && destination_handle_i[3:0] < 4'd4;
    wire norm_handles_valid = source0_shape_valid
        && source1_q30_handle_i == 37'd0
        && norm_destination_valid
        && source0_q30_handle_i[36:5] == destination_handle_i[36:5];
    wire residual_handles_valid = source0_shape_valid
        && source1_shape_valid && residual_destination_valid
        && source0_q30_handle_i[36:5] == source1_q30_handle_i[36:5]
        && source0_q30_handle_i[36:5] == destination_handle_i[36:5]
        && destination_handle_i[3:0] != source0_q30_handle_i[3:0]
        && destination_handle_i[3:0] != source1_q30_handle_i[3:0];
    wire start_handles_valid = operation_i
        ? residual_handles_valid : norm_handles_valid;

    wire external_read_allowed = state == ST_IDLE || state == ST_RESULT;
    assign start_ready_o = state == ST_IDLE && !poisoned_o
        && !output_read_valid_i && resident_inspect_ready_i
        && !resident_inspect_rsp_valid_i;
    assign result_valid_o = state == ST_RESULT;
    assign busy_o = state != ST_IDLE && state != ST_RESULT;
    assign expected_weight_index_o = weights_accepted_o[9:0];

    assign resident_command_valid_o = state == ST_COMMAND
        && !abort_i && !poisoned_o;
    assign resident_command_operation_o = active_operation
        ? OP_RESIDUAL_ADD : OP_RMSNORM;
    assign resident_command_source0_handle_o = source0_handle;
    assign resident_command_source1_handle_o = active_operation
        ? source1_handle : 37'd0;
    assign resident_command_destination_handle_o = destination_handle;
    assign resident_result_ready_o = state == ST_NORM_STREAM
        || state == ST_RESIDUAL;

    // Once a resident command has been accepted, any local stream fault or
    // caller abort terminates the resident transaction.  The store cleared the
    // destination-valid bit at transaction begin, so no partial replacement
    // can become observable.
    assign resident_abort_o = (state == ST_NORM_STREAM
            || state == ST_RESIDUAL)
        && (abort_i || abort_seen || protocol_failed);

    assign weight_ready_o = state == ST_NORM_STREAM
        && !abort_i && !abort_seen && !protocol_failed
        && resident_weight_ready_i;
    assign resident_weight_valid_o = state == ST_NORM_STREAM
        && !abort_i && !abort_seen && !protocol_failed && weight_valid_i;
    assign resident_weight_index_o = weight_index_i;
    assign resident_weight_format_bf16_o = weight_format_bf16_i;
    assign resident_weight_bits_o = weight_bits_i;
    wire weight_accept = weight_valid_i && weight_ready_o;

    assign resident_inspect_valid_o = external_read_allowed
        && output_read_valid_i;
    assign resident_inspect_handle_o = destination_handle;
    assign resident_inspect_index_o = output_read_index_i;
    assign resident_inspect_rsp_ready_o = external_read_allowed
        && output_read_rsp_ready_i;
    assign output_read_ready_o = external_read_allowed
        && resident_inspect_ready_i;
    assign output_read_rsp_valid_o = external_read_allowed
        && resident_inspect_rsp_valid_i;
    assign output_read_error_o = resident_inspect_rsp_error_i;
    assign output_read_data_o = resident_inspect_rsp_data_i;

    always @(posedge clk) begin
        if (!joined_reset_n) begin
            state <= ST_IDLE;
            active_operation <= 1'b0;
            source0_handle <= 37'd0;
            source1_handle <= 37'd0;
            destination_handle <= 37'd0;
            abort_seen <= 1'b0;
            protocol_failed <= 1'b0;
            protocol_error_code <= ERROR_NONE;
            weights_accepted_o <= 11'd0;
            poisoned_o <= 1'b0;
            result_error_o <= 1'b0;
            result_error_code_o <= ERROR_NONE;
            result_operation_o <= 1'b0;
            result_token_position_o <= 32'd0;
            result_handle_o <= 37'd0;
        end else begin
            case (state)
                ST_IDLE: begin
                    if (start_i && start_ready_o) begin
                        active_operation <= operation_i;
                        source0_handle <= source0_q30_handle_i;
                        source1_handle <= source1_q30_handle_i;
                        destination_handle <= destination_handle_i;
                        abort_seen <= 1'b0;
                        protocol_failed <= 1'b0;
                        protocol_error_code <= ERROR_NONE;
                        weights_accepted_o <= 11'd0;
                        result_error_o <= 1'b0;
                        result_error_code_o <= ERROR_NONE;
                        result_operation_o <= operation_i;
                        result_token_position_o <= token_position_i;
                        result_handle_o <= 37'd0;
                        if (!start_handles_valid) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_HANDLE;
                            state <= ST_RESULT;
                        end else begin
                            state <= ST_COMMAND;
                        end
                    end
                end

                ST_COMMAND: begin
                    if (abort_i) begin
                        poisoned_o <= 1'b1;
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        result_handle_o <= 37'd0;
                        state <= ST_RESULT;
                    end else if (resident_command_valid_o
                            && resident_command_ready_i) begin
                        state <= active_operation
                            ? ST_RESIDUAL : ST_NORM_STREAM;
                    end
                end

                ST_NORM_STREAM: begin
                    if (abort_i) begin
                        abort_seen <= 1'b1;
                        poisoned_o <= 1'b1;
                    end

                    if (weight_accept) begin
                        weights_accepted_o <= weights_accepted_o + 11'd1;
                        if (weight_index_i != weights_accepted_o[9:0]) begin
                            protocol_failed <= 1'b1;
                            protocol_error_code <= ERROR_WEIGHT_ORDER;
                            poisoned_o <= 1'b1;
                        end else if (!weight_format_bf16_i) begin
                            protocol_failed <= 1'b1;
                            protocol_error_code <= ERROR_WEIGHT_FORMAT;
                            poisoned_o <= 1'b1;
                        end
                    end

                    if (resident_result_valid_i
                            && resident_result_ready_o) begin
                        result_handle_o <= 37'd0;
                        if (protocol_failed) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= protocol_error_code;
                        end else if (abort_seen || abort_i) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_ABORT;
                        end else if (resident_result_error_i) begin
                            poisoned_o <= 1'b1;
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_RESIDENT;
                        end else if (resident_result_handle_i
                                != destination_handle
                                || weights_accepted_o != 11'd1024) begin
                            poisoned_o <= 1'b1;
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_RESULT;
                        end else begin
                            result_error_o <= 1'b0;
                            result_error_code_o <= ERROR_NONE;
                            result_handle_o <= destination_handle;
                        end
                        state <= ST_RESULT;
                    end
                end

                ST_RESIDUAL: begin
                    if (abort_i) begin
                        abort_seen <= 1'b1;
                        poisoned_o <= 1'b1;
                    end

                    if (resident_result_valid_i
                            && resident_result_ready_o) begin
                        result_handle_o <= 37'd0;
                        if (abort_seen || abort_i) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_ABORT;
                        end else if (resident_result_error_i) begin
                            poisoned_o <= 1'b1;
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_RESIDENT;
                        end else if (resident_result_handle_i
                                != destination_handle) begin
                            poisoned_o <= 1'b1;
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_RESULT;
                        end else begin
                            result_error_o <= 1'b0;
                            result_error_code_o <= ERROR_NONE;
                            result_handle_o <= destination_handle;
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
                    result_error_code_o <= ERROR_RESULT;
                    result_handle_o <= 37'd0;
                    state <= ST_RESULT;
                end
            endcase
        end
    end
endmodule
