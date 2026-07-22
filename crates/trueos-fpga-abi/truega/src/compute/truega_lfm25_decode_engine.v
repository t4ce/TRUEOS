// Standalone fixed execution join for one honest LFM2.5 decode subgraph.
//
// This first joined circuit executes, without host-side tensor materialization:
//
//   TokenEmbedding(Q8_0 stream) -> resident Q30[1024]
//   RMSNorm(weight stream)      -> resident Q8_0[32]
//   Projection(1024 rows)       -> transactional resident Q30[1024]
//
// Resident handles are circuit metadata, never addresses.  The projection
// output is accepted by the resident store's internal ordered Q30 import port;
// its destination handle is published only after all 1,024 signed-i64 values
// commit.  A poisoned projection aborts that transaction, so partial output is
// unreadable.  This module is deliberately not the full 99-operation decode
// schedule yet.  The remaining fixed joins are explicit and countable:
//   * shortconv: 1,024 x 32 B/C/X triplet blocks plus three BF16 taps, then
//     its 32 Q8 blocks through a 1,024-row output projection and Q30 import;
//   * attention: 3,072 projection rows plus 128 Q/K norm words, then the
//     1,024 Q30 outputs through Q30-to-Q8 and a 1,024-row output projection;
//   * FFN: 4,608 x 32 paired gate/up blocks and 1,024 x 144 down blocks, then
//     the engine's ordered 1,024-value read port through Q30 import;
//   * tied head: 65,536 x 32 row blocks and the token/score result ports.
// The resident residual operation already has the required typed-handle ports.
// Those feed muxes and sequencer states must exist before this circuit can
// truthfully advertise the complete TGD1 capability.
//
// There is no parser, bytecode, processor, DMA, TLB, or runtime shape control.
module truega_lfm25_decode_engine #(
    parameter integer PROJECTION_ROWS = 1024
) (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 clear_i,

    input  wire                 start_i,
    output wire                 start_ready_o,
    input  wire [31:0]          session_epoch_i,

    input  wire                 embedding_valid_i,
    output wire                 embedding_ready_o,
    output wire [4:0]           embedding_block_index_o,
    input  wire [4:0]           embedding_block_index_i,
    input  wire [271:0]         embedding_q8_block_i,

    input  wire                 norm_weight_valid_i,
    output wire                 norm_weight_ready_o,
    output wire [9:0]           norm_weight_index_o,
    input  wire [9:0]           norm_weight_index_i,
    input  wire                 norm_weight_format_bf16_i,
    input  wire [31:0]          norm_weight_bits_i,

    input  wire                 projection_weight_valid_i,
    output wire                 projection_weight_ready_o,
    output wire [12:0]          projection_weight_row_o,
    output wire [4:0]           projection_weight_block_o,
    input  wire [12:0]          projection_weight_row_i,
    input  wire [4:0]           projection_weight_block_i,
    input  wire [271:0]         projection_weight_q8_block_i,

    output wire                 result_valid_o,
    input  wire                 result_ready_i,
    output reg                  result_error_o,
    output reg  [7:0]           result_error_code_o,
    output reg  [36:0]          result_handle_o,

    // Diagnostic resident readback for this joined slice.  It observes only
    // the most recently published projection destination.
    input  wire                 output_read_valid_i,
    output wire                 output_read_ready_o,
    input  wire [9:0]           output_read_index_i,
    output wire                 output_read_rsp_valid_o,
    input  wire                 output_read_rsp_ready_i,
    output wire                 output_read_error_o,
    output wire signed [63:0]   output_read_q30_o,

    output wire [31:0]          active_session_epoch_o,
    output wire [12:0]          projection_rows_retired_o,
    output wire                 busy_o
);
    localparam [4:0] ST_IDLE             = 5'd0;
    localparam [4:0] ST_EMBED_COMMAND    = 5'd1;
    localparam [4:0] ST_EMBED_STREAM     = 5'd2;
    localparam [4:0] ST_NORM_COMMAND     = 5'd3;
    localparam [4:0] ST_NORM_STREAM      = 5'd4;
    localparam [4:0] ST_PROJECTION_RESET = 5'd5;
    localparam [4:0] ST_PROJECTION_START = 5'd6;
    localparam [4:0] ST_ACTIVATE_REQUEST = 5'd7;
    localparam [4:0] ST_ACTIVATE_REPLY   = 5'd8;
    localparam [4:0] ST_IMPORT_COMMAND   = 5'd9;
    localparam [4:0] ST_PROJECT_ROWS     = 5'd10;
    localparam [4:0] ST_RESULT           = 5'd11;

    localparam [1:0] VECTOR_TOKEN_EMBEDDING = 2'd0;
    localparam [1:0] VECTOR_RMSNORM         = 2'd1;
    localparam [1:0] VECTOR_IMPORT_Q30      = 2'd3;

    localparam [7:0] ERROR_EPOCH       = 8'd1;
    localparam [7:0] ERROR_EMBEDDING   = 8'd2;
    localparam [7:0] ERROR_NORM        = 8'd3;
    localparam [7:0] ERROR_PROJECTION  = 8'd4;
    localparam [7:0] ERROR_IMPORT      = 8'd5;
    localparam [7:0] ERROR_HANDLE      = 8'd6;
    localparam [7:0] ERROR_PARAMETER   = 8'd7;

    reg [4:0] state;
    reg [31:0] session_epoch;
    reg [5:0] embedding_count;
    reg [10:0] norm_weight_count;
    reg [5:0] activation_block;
    reg projection_failed;

    wire joined_reset_n = reset_n && !clear_i;
    wire [36:0] stream_handle = {session_epoch, 1'b1, 4'd0};
    wire [36:0] embedding_handle = {session_epoch, 1'b0, 4'd0};
    wire [36:0] norm_handle = {session_epoch, 1'b1, 4'd0};
    wire [36:0] projection_handle = {session_epoch, 1'b0, 4'd1};

    // Do not begin a new session while a diagnostic read response is still
    // owned by the resident engine; otherwise changing state would hide that
    // response and deadlock both clients of the shared inspection port.
    assign start_ready_o = state == ST_IDLE && !output_read_valid_i
        && rv_command_ready && !rv_inspect_rsp_valid;
    assign result_valid_o = state == ST_RESULT;
    assign busy_o = state != ST_IDLE && state != ST_RESULT;
    assign active_session_epoch_o = session_epoch;
    assign embedding_block_index_o = embedding_count[4:0];
    assign norm_weight_index_o = norm_weight_count[9:0];

    // Resident vector engine command/result join.
    wire rv_command_valid = state == ST_EMBED_COMMAND
        || state == ST_NORM_COMMAND || state == ST_IMPORT_COMMAND;
    wire rv_command_ready;
    wire [1:0] rv_command_operation = state == ST_EMBED_COMMAND
        ? VECTOR_TOKEN_EMBEDDING
        : state == ST_NORM_COMMAND ? VECTOR_RMSNORM : VECTOR_IMPORT_Q30;
    wire [36:0] rv_command_source0 = state == ST_EMBED_COMMAND
        ? stream_handle : state == ST_NORM_COMMAND ? embedding_handle : 37'd0;
    wire [36:0] rv_command_destination = state == ST_EMBED_COMMAND
        ? embedding_handle : state == ST_NORM_COMMAND
        ? norm_handle : projection_handle;
    wire rv_result_valid;
    wire rv_result_ready = state == ST_EMBED_STREAM
        || state == ST_NORM_STREAM || state == ST_PROJECT_ROWS;
    wire rv_result_error;
    wire [36:0] rv_result_handle;
    wire rv_busy;
    wire rv_abort = state == ST_PROJECT_ROWS
        && projection_failed && rv_busy;

    assign embedding_ready_o = state == ST_EMBED_STREAM
        && rv_embedding_ready;
    wire rv_embedding_ready;
    wire rv_embedding_valid = state == ST_EMBED_STREAM
        && embedding_valid_i;
    wire rv_embedding_accept = rv_embedding_valid && rv_embedding_ready;

    assign norm_weight_ready_o = state == ST_NORM_STREAM
        && rv_weight_ready;
    wire rv_weight_ready;
    wire rv_weight_valid = state == ST_NORM_STREAM && norm_weight_valid_i;
    wire rv_weight_accept = rv_weight_valid && rv_weight_ready;

    wire projection_result_valid;
    wire projection_result_ready;
    wire [12:0] projection_result_row;
    wire signed [63:0] projection_result_q30;
    wire projection_result_first;
    wire projection_result_last;
    wire rv_import_ready;
    wire rv_import_valid = state == ST_PROJECT_ROWS
        && projection_result_valid && !projection_failed;

    // The projection activation is fetched from the resident Q8 handle through
    // the same typed inspection port used by the verification readback.
    wire external_read_allowed = state == ST_IDLE || state == ST_RESULT;
    wire rv_inspect_valid = state == ST_ACTIVATE_REQUEST
        || (external_read_allowed && output_read_valid_i);
    wire rv_inspect_ready;
    wire [36:0] rv_inspect_handle = state == ST_ACTIVATE_REQUEST
        ? norm_handle : projection_handle;
    wire [9:0] rv_inspect_index = state == ST_ACTIVATE_REQUEST
        ? {4'd0, activation_block} : output_read_index_i;
    wire rv_inspect_rsp_valid;
    wire rv_inspect_rsp_ready = state == ST_ACTIVATE_REPLY
        ? projection_activation_ready
        : external_read_allowed && output_read_rsp_ready_i;
    wire rv_inspect_rsp_error;
    wire [271:0] rv_inspect_rsp_data;

    assign output_read_ready_o = external_read_allowed
        && rv_inspect_ready;
    assign output_read_rsp_valid_o = external_read_allowed
        && rv_inspect_rsp_valid;
    assign output_read_error_o = rv_inspect_rsp_error;
    assign output_read_q30_o = rv_inspect_rsp_data[63:0];

    truega_lfm25_resident_vector_engine resident_vectors (
        .clk(clk), .reset_n(joined_reset_n), .abort_i(rv_abort),
        .command_valid_i(rv_command_valid),
        .command_ready_o(rv_command_ready),
        .command_operation_i(rv_command_operation),
        .command_source0_handle_i(rv_command_source0),
        .command_source1_handle_i(37'd0),
        .command_destination_handle_i(rv_command_destination),
        .embedding_block_valid_i(rv_embedding_valid),
        .embedding_block_ready_o(rv_embedding_ready),
        .embedding_block_index_i(embedding_block_index_i),
        .embedding_q8_block_i(embedding_q8_block_i),
        .weight_valid_i(rv_weight_valid), .weight_ready_o(rv_weight_ready),
        .weight_index_i(norm_weight_index_i),
        .weight_format_bf16_i(norm_weight_format_bf16_i),
        .weight_bits_i(norm_weight_bits_i),
        .import_valid_i(rv_import_valid), .import_ready_o(rv_import_ready),
        .import_index_i(projection_result_row[9:0]),
        .import_q30_i(projection_result_q30),
        .result_valid_o(rv_result_valid), .result_ready_i(rv_result_ready),
        .result_error_o(rv_result_error), .result_handle_o(rv_result_handle),
        .inspect_valid_i(rv_inspect_valid),
        .inspect_ready_o(rv_inspect_ready),
        .inspect_handle_i(rv_inspect_handle),
        .inspect_index_i(rv_inspect_index),
        .inspect_rsp_valid_o(rv_inspect_rsp_valid),
        .inspect_rsp_ready_i(rv_inspect_rsp_ready),
        .inspect_rsp_error_o(rv_inspect_rsp_error),
        .inspect_rsp_data_o(rv_inspect_rsp_data),
        .session_epoch_o(), .busy_o(rv_busy)
    );

    // Fixed 1024-output projection.  Its output is backpressured directly by
    // the resident import transaction, preserving row order across the join.
    wire projection_state_reset_ready;
    wire projection_state_reset_done;
    wire projection_start_ready;
    wire projection_activation_ready;
    wire [4:0] projection_activation_index;
    wire projection_weight_ready;
    wire projection_busy;
    wire projection_done;
    wire projection_error;
    wire projection_poisoned;
    wire [7:0] projection_error_code;
    wire [12:0] projection_rows_retired;

    wire projection_activation_valid = state == ST_ACTIVATE_REPLY
        && rv_inspect_rsp_valid && !rv_inspect_rsp_error;
    wire projection_weight_valid = state == ST_PROJECT_ROWS
        && !projection_failed && rv_import_ready
        && projection_weight_valid_i;
    assign projection_weight_ready_o = state == ST_PROJECT_ROWS
        && !projection_failed && rv_import_ready && projection_weight_ready;
    assign projection_weight_row_o = projection_weight_row;
    assign projection_weight_block_o = projection_weight_block;
    assign projection_result_ready = state == ST_PROJECT_ROWS
        && !projection_failed && rv_import_ready;
    assign projection_rows_retired_o = projection_rows_retired;

    wire [12:0] projection_weight_row;
    wire [4:0] projection_weight_block;
    truega_lfm25_q8_projection_row_engine #(
        .ROW_COUNT(PROJECTION_ROWS)
    ) projection (
        .clk(clk), .reset_n(joined_reset_n),
        .state_reset_i(state == ST_PROJECTION_RESET),
        .state_reset_ready_o(projection_state_reset_ready),
        .state_reset_done_o(projection_state_reset_done),
        .start_i(state == ST_PROJECTION_START),
        .start_ready_o(projection_start_ready),
        .activation_valid_i(projection_activation_valid),
        .activation_ready_o(projection_activation_ready),
        .activation_block_index_o(projection_activation_index),
        .activation_block_index_i(activation_block[4:0]),
        .activation_q8_block_i(rv_inspect_rsp_data),
        .weight_valid_i(projection_weight_valid),
        .weight_ready_o(projection_weight_ready),
        .weight_row_index_o(projection_weight_row),
        .weight_block_index_o(projection_weight_block),
        .weight_row_index_i(projection_weight_row_i),
        .weight_block_index_i(projection_weight_block_i),
        .weight_q8_block_i(projection_weight_q8_block_i),
        .result_valid_o(projection_result_valid),
        .result_ready_i(projection_result_ready),
        .result_row_index_o(projection_result_row),
        .result_q30_o(projection_result_q30),
        .result_first_o(projection_result_first),
        .result_last_o(projection_result_last),
        .busy_o(projection_busy), .done_o(projection_done),
        .error_o(projection_error), .poisoned_o(projection_poisoned),
        .error_code_o(projection_error_code),
        .rows_retired_o(projection_rows_retired)
    );

    always @(posedge clk) begin
        if (!joined_reset_n) begin
            state <= ST_IDLE;
            session_epoch <= 32'd0;
            embedding_count <= 6'd0;
            norm_weight_count <= 11'd0;
            activation_block <= 6'd0;
            projection_failed <= 1'b0;
            result_error_o <= 1'b0;
            result_error_code_o <= 8'd0;
            result_handle_o <= 37'd0;
        end else begin
            case (state)
                ST_IDLE: begin
                    if (start_i && start_ready_o) begin
                        session_epoch <= session_epoch_i;
                        embedding_count <= 6'd0;
                        norm_weight_count <= 11'd0;
                        activation_block <= 6'd0;
                        projection_failed <= 1'b0;
                        result_error_o <= 1'b0;
                        result_error_code_o <= 8'd0;
                        result_handle_o <= 37'd0;
                        if (session_epoch_i == 32'd0) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_EPOCH;
                            state <= ST_RESULT;
                        end else if (PROJECTION_ROWS != 1024) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_PARAMETER;
                            state <= ST_RESULT;
                        end else begin
                            state <= ST_EMBED_COMMAND;
                        end
                    end
                end

                ST_EMBED_COMMAND: begin
                    if (rv_command_valid && rv_command_ready)
                        state <= ST_EMBED_STREAM;
                end

                ST_EMBED_STREAM: begin
                    if (rv_embedding_accept)
                        embedding_count <= embedding_count + 6'd1;
                    if (rv_result_valid && rv_result_ready) begin
                        if (rv_result_error
                                || rv_result_handle != embedding_handle
                                || embedding_count != 6'd32) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= rv_result_error
                                ? ERROR_EMBEDDING : ERROR_HANDLE;
                            state <= ST_RESULT;
                        end else begin
                            norm_weight_count <= 11'd0;
                            state <= ST_NORM_COMMAND;
                        end
                    end
                end

                ST_NORM_COMMAND: begin
                    if (rv_command_valid && rv_command_ready)
                        state <= ST_NORM_STREAM;
                end

                ST_NORM_STREAM: begin
                    if (rv_weight_accept)
                        norm_weight_count <= norm_weight_count + 11'd1;
                    if (rv_result_valid && rv_result_ready) begin
                        if (rv_result_error || rv_result_handle != norm_handle
                                || norm_weight_count != 11'd1024) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= rv_result_error
                                ? ERROR_NORM : ERROR_HANDLE;
                            state <= ST_RESULT;
                        end else begin
                            state <= ST_PROJECTION_RESET;
                        end
                    end
                end

                ST_PROJECTION_RESET: begin
                    if (projection_state_reset_ready) begin
                        activation_block <= 6'd0;
                        state <= ST_PROJECTION_START;
                    end
                end

                ST_PROJECTION_START: begin
                    if (projection_start_ready)
                        state <= ST_ACTIVATE_REQUEST;
                end

                ST_ACTIVATE_REQUEST: begin
                    if (rv_inspect_valid && rv_inspect_ready)
                        state <= ST_ACTIVATE_REPLY;
                end

                ST_ACTIVATE_REPLY: begin
                    if (rv_inspect_rsp_valid && rv_inspect_rsp_error) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_HANDLE;
                        state <= ST_RESULT;
                    end else if (projection_activation_valid
                            && projection_activation_ready) begin
                        if (projection_activation_index
                                != activation_block[4:0]) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_PROJECTION;
                            state <= ST_RESULT;
                        end else if (activation_block == 6'd31) begin
                            state <= ST_IMPORT_COMMAND;
                        end else begin
                            activation_block <= activation_block + 6'd1;
                            state <= ST_ACTIVATE_REQUEST;
                        end
                    end
                end

                ST_IMPORT_COMMAND: begin
                    if (rv_command_valid && rv_command_ready)
                        state <= ST_PROJECT_ROWS;
                end

                ST_PROJECT_ROWS: begin
                    if (projection_done && (projection_error
                            || projection_poisoned))
                        projection_failed <= 1'b1;
                    if (rv_result_valid && rv_result_ready) begin
                        if (projection_failed || projection_error
                                || projection_poisoned) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_PROJECTION;
                            result_handle_o <= 37'd0;
                        end else if (rv_result_error) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_IMPORT;
                            result_handle_o <= 37'd0;
                        end else if (rv_result_handle != projection_handle
                                || projection_rows_retired
                                    != PROJECTION_ROWS) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_HANDLE;
                            result_handle_o <= 37'd0;
                        end else begin
                            result_error_o <= 1'b0;
                            result_error_code_o <= 8'd0;
                            result_handle_o <= projection_handle;
                        end
                        state <= ST_RESULT;
                    end
                end

                ST_RESULT: begin
                    if (result_valid_o && result_ready_i)
                        state <= ST_IDLE;
                end

                default: begin
                    result_error_o <= 1'b1;
                    result_error_code_o <= ERROR_PARAMETER;
                    result_handle_o <= 37'd0;
                    state <= ST_RESULT;
                end
            endcase
        end
    end

    wire unused_observability = projection_result_first
        ^ projection_result_last ^ projection_busy
        ^ projection_state_reset_done ^ projection_error_code[0];
endmodule
