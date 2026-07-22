// Fixed resident FFN execution join for the LFM2.5 decode circuit.
//
// A typed resident Q8_0 handle is inspected block-by-block into the proven
// full-shape FFN row engine.  After exactly 4,608 paired gate/up rows and 1,024
// down rows complete, a one-entry elastic adapter drains the engine's
// synchronous Q30 output-read port into an OP_IMPORT_Q30 transaction.  The
// destination resident Q30 handle is published only after all 1,024 ordered
// values commit.  Abort during import leaves the destination unpublished.
//
// This controller connects to one shared truega_lfm25_resident_vector_engine;
// it does not duplicate tensor storage.  There is no runtime graph, parser,
// processor, DMA, TLB, or host-side tensor math.
module truega_lfm25_resident_ffn_join (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 clear_i,
    input  wire                 abort_i,

    input  wire                 start_i,
    output wire                 start_ready_o,
    input  wire [36:0]          source_q8_handle_i,
    input  wire [36:0]          destination_q30_handle_i,

    input  wire                 row_start_i,
    input  wire                 row_down_i,
    input  wire [12:0]          row_index_i,
    output wire                 row_ready_o,
    output wire                 expected_row_down_o,
    output wire [12:0]          expected_row_index_o,

    input  wire                 weight_valid_i,
    input  wire [7:0]           weight_block_index_i,
    input  wire [271:0]         weight0_q8_block_i,
    input  wire [271:0]         weight1_q8_block_i,
    output wire                 weight_ready_o,
    output wire [7:0]           expected_weight_block_o,

    output wire                 row_done_o,
    output wire                 row_error_o,
    output wire                 row_done_down_o,
    output wire [12:0]          row_done_index_o,

    // Verification throttle.  Production ties this low; it proves the
    // one-entry adapter obeys stable ready/valid semantics under backpressure.
    input  wire                 import_pause_i,
    output wire                 import_adapter_valid_o,
    output wire [9:0]           import_adapter_index_o,
    output wire signed [63:0]   import_adapter_q30_o,

    output wire                 result_valid_o,
    input  wire                 result_ready_i,
    output reg                  result_error_o,
    output reg  [7:0]           result_error_code_o,
    output reg  [36:0]          result_handle_o,

    input  wire                 output_read_valid_i,
    output wire                 output_read_ready_o,
    input  wire [9:0]           output_read_index_i,
    output wire                 output_read_rsp_valid_o,
    input  wire                 output_read_rsp_ready_i,
    output wire                 output_read_error_o,
    output wire signed [63:0]   output_read_q30_o,

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

    // Shared resident-vector typed inspection interface.
    output wire                 resident_inspect_valid_o,
    input  wire                 resident_inspect_ready_i,
    output wire [36:0]          resident_inspect_handle_o,
    output wire [9:0]           resident_inspect_index_o,
    input  wire                 resident_inspect_rsp_valid_i,
    output wire                 resident_inspect_rsp_ready_o,
    input  wire                 resident_inspect_rsp_error_i,
    input  wire [271:0]         resident_inspect_rsp_data_i,

    // Shared resident-vector Q30 import stream.
    output wire                 resident_import_valid_o,
    input  wire                 resident_import_ready_i,
    output wire [9:0]           resident_import_index_o,
    output wire signed [63:0]   resident_import_q30_o,

    output wire [12:0]          gate_up_rows_completed_o,
    output wire [10:0]          down_rows_completed_o,
    output reg  [10:0]          import_elements_completed_o,
    output wire                 busy_o
);
    localparam [3:0] ST_IDLE          = 4'd0;
    localparam [3:0] ST_CLEAR_FFN     = 4'd1;
    localparam [3:0] ST_ACT_REQUEST   = 4'd2;
    localparam [3:0] ST_ACT_REPLY     = 4'd3;
    localparam [3:0] ST_FEED_ROWS     = 4'd4;
    localparam [3:0] ST_IMPORT_CMD    = 4'd5;
    localparam [3:0] ST_IMPORT_OUTPUT = 4'd6;
    localparam [3:0] ST_RESULT        = 4'd7;

    localparam [1:0] OP_IMPORT_Q30 = 2'd3;
    localparam [7:0] ERROR_HANDLE   = 8'd1;
    localparam [7:0] ERROR_INSPECT  = 8'd2;
    localparam [7:0] ERROR_FFN      = 8'd3;
    localparam [7:0] ERROR_IMPORT   = 8'd4;
    localparam [7:0] ERROR_ABORT    = 8'd5;
    localparam [7:0] ERROR_ADAPTER  = 8'd6;

    reg [3:0] state;
    reg [36:0] source_handle;
    reg [36:0] destination_handle;
    reg [5:0] activation_block;
    reg abort_seen;

    wire source_shape_valid = source_q8_handle_i[36:5] != 32'd0
        && source_q8_handle_i[4] == 1'b1
        && source_q8_handle_i[3:0] < 4'd4;
    wire destination_shape_valid = destination_q30_handle_i[36:5] != 32'd0
        && destination_q30_handle_i[4] == 1'b0
        && destination_q30_handle_i[3:0] < 4'd4;
    wire handles_valid = source_shape_valid && destination_shape_valid
        && source_q8_handle_i[36:5] == destination_q30_handle_i[36:5];

    wire joined_reset_n = reset_n && !clear_i;
    assign start_ready_o = state == ST_IDLE && !output_read_valid_i
        && !resident_inspect_rsp_valid_i;
    assign result_valid_o = state == ST_RESULT;
    assign busy_o = state != ST_IDLE && state != ST_RESULT;

    // Fixed FFN engine and its resident activation preload.
    wire ffn_activation_ready;
    wire [4:0] ffn_activation_index;
    wire ffn_activation_valid = state == ST_ACT_REPLY
        && resident_inspect_rsp_valid_i && !resident_inspect_rsp_error_i;
    wire ffn_row_ready;
    wire ffn_expected_row_down;
    wire [12:0] ffn_expected_row_index;
    wire ffn_weight_ready;
    wire [7:0] ffn_expected_weight;
    wire ffn_row_done;
    wire ffn_row_error;
    wire ffn_row_done_down;
    wire [12:0] ffn_row_done_index;
    wire ffn_poison;
    wire [7:0] ffn_error_code;
    wire ffn_complete;
    wire ffn_output_read_valid;
    wire ffn_output_read_error;
    wire signed [63:0] ffn_output_read_q30;
    wire [5:0] ffn_activation_blocks;
    wire [12:0] ffn_gate_up_rows;
    wire [7:0] ffn_down_blocks;
    wire [10:0] ffn_down_rows;

    assign row_ready_o = state == ST_FEED_ROWS && ffn_row_ready
        && !abort_i;
    assign expected_row_down_o = ffn_expected_row_down;
    assign expected_row_index_o = ffn_expected_row_index;
    assign weight_ready_o = state == ST_FEED_ROWS && ffn_weight_ready
        && !abort_i;
    assign expected_weight_block_o = ffn_expected_weight;
    assign row_done_o = state == ST_FEED_ROWS && ffn_row_done;
    assign row_error_o = state == ST_FEED_ROWS && ffn_row_error;
    assign row_done_down_o = ffn_row_done_down;
    assign row_done_index_o = ffn_row_done_index;
    assign gate_up_rows_completed_o = ffn_gate_up_rows;
    assign down_rows_completed_o = ffn_down_rows;

    // One outstanding synchronous read and one elastic output entry.  Payload
    // registers are not cleared for correctness; their validity bits are.
    reg [10:0] read_issue_count;
    reg read_pending;
    reg [9:0] pending_read_index;
    reg elastic_valid;
    reg [9:0] elastic_index;
    reg signed [63:0] elastic_q30;
    reg adapter_error;

    wire ffn_output_read = state == ST_IMPORT_OUTPUT && !abort_seen
        && !abort_i && !read_pending && !elastic_valid
        && read_issue_count < 11'd1024;
    wire resident_import_accept = resident_import_valid_o
        && resident_import_ready_i;

    truega_lfm25_resident_ffn_row_engine ffn (
        .clk(clk), .reset_n(joined_reset_n),
        .clear_i(state == ST_CLEAR_FFN),
        .activation_valid_i(ffn_activation_valid),
        .activation_block_index_i(activation_block[4:0]),
        .activation_block_i(resident_inspect_rsp_data_i),
        .activation_ready_o(ffn_activation_ready),
        .activation_block_index_o(ffn_activation_index),
        .row_start_i(state == ST_FEED_ROWS && row_start_i && !abort_i),
        .row_down_i(row_down_i), .row_index_i(row_index_i),
        .row_ready_o(ffn_row_ready), .row_down_o(ffn_expected_row_down),
        .row_index_o(ffn_expected_row_index),
        .weight_valid_i(state == ST_FEED_ROWS && weight_valid_i
            && !abort_i),
        .weight_block_index_i(weight_block_index_i),
        .weight0_block_i(weight0_q8_block_i),
        .weight1_block_i(weight1_q8_block_i),
        .weight_ready_o(ffn_weight_ready),
        .weight_block_index_o(ffn_expected_weight),
        .row_done_o(ffn_row_done), .row_error_o(ffn_row_error),
        .row_done_down_o(ffn_row_done_down),
        .row_done_index_o(ffn_row_done_index),
        .poison_o(ffn_poison), .error_code_o(ffn_error_code),
        .busy_o(), .complete_o(ffn_complete),
        .output_read_i(ffn_output_read),
        .output_read_index_i(read_issue_count[9:0]),
        .output_read_valid_o(ffn_output_read_valid),
        .output_read_error_o(ffn_output_read_error),
        .output_read_q30_o(ffn_output_read_q30),
        .activation_blocks_loaded_o(ffn_activation_blocks),
        .gate_up_rows_completed_o(ffn_gate_up_rows),
        .down_activation_blocks_o(ffn_down_blocks),
        .down_rows_completed_o(ffn_down_rows)
    );

    // Only the fixed internal import command is emitted.  Source handles are
    // zero because OP_IMPORT_Q30 is a destination-only resident operation.
    assign resident_command_valid_o = state == ST_IMPORT_CMD && !abort_i;
    assign resident_command_operation_o = OP_IMPORT_Q30;
    assign resident_command_source0_handle_o = 37'd0;
    assign resident_command_source1_handle_o = 37'd0;
    assign resident_command_destination_handle_o = destination_handle;
    assign resident_result_ready_o = state == ST_IMPORT_OUTPUT;
    assign resident_abort_o = state == ST_IMPORT_OUTPUT
        && (abort_i || abort_seen);

    // Activation preload and post-result diagnostic reads share the typed
    // resident inspection port.  A new start is held off until a diagnostic
    // response has been consumed.
    wire external_read_allowed = state == ST_IDLE || state == ST_RESULT;
    assign resident_inspect_valid_o = state == ST_ACT_REQUEST
        || (external_read_allowed && output_read_valid_i);
    assign resident_inspect_handle_o = state == ST_ACT_REQUEST
        ? source_handle : destination_handle;
    assign resident_inspect_index_o = state == ST_ACT_REQUEST
        ? {4'd0, activation_block} : output_read_index_i;
    assign resident_inspect_rsp_ready_o = state == ST_ACT_REPLY
        ? ffn_activation_ready
        : external_read_allowed && output_read_rsp_ready_i;
    assign output_read_ready_o = external_read_allowed
        && resident_inspect_ready_i;
    assign output_read_rsp_valid_o = external_read_allowed
        && resident_inspect_rsp_valid_i;
    assign output_read_error_o = resident_inspect_rsp_error_i;
    assign output_read_q30_o = resident_inspect_rsp_data_i[63:0];

    // Elastic entry remains bit-stable while either the resident import port
    // or the explicit verification throttle applies backpressure.
    assign resident_import_valid_o = state == ST_IMPORT_OUTPUT
        && elastic_valid && !abort_seen && !abort_i && !import_pause_i;
    assign resident_import_index_o = elastic_index;
    assign resident_import_q30_o = elastic_q30;
    assign import_adapter_valid_o = state == ST_IMPORT_OUTPUT
        && elastic_valid && !abort_seen && !abort_i;
    assign import_adapter_index_o = elastic_index;
    assign import_adapter_q30_o = elastic_q30;

    always @(posedge clk) begin
        if (!joined_reset_n) begin
            state <= ST_IDLE;
            source_handle <= 37'd0;
            destination_handle <= 37'd0;
            activation_block <= 6'd0;
            abort_seen <= 1'b0;
            read_issue_count <= 11'd0;
            read_pending <= 1'b0;
            pending_read_index <= 10'd0;
            elastic_valid <= 1'b0;
            elastic_index <= 10'd0;
            elastic_q30 <= 64'sd0;
            adapter_error <= 1'b0;
            import_elements_completed_o <= 11'd0;
            result_error_o <= 1'b0;
            result_error_code_o <= 8'd0;
            result_handle_o <= 37'd0;
        end else begin
            case (state)
                ST_IDLE: begin
                    if (start_i && start_ready_o) begin
                        source_handle <= source_q8_handle_i;
                        destination_handle <= destination_q30_handle_i;
                        activation_block <= 6'd0;
                        abort_seen <= 1'b0;
                        read_issue_count <= 11'd0;
                        read_pending <= 1'b0;
                        elastic_valid <= 1'b0;
                        adapter_error <= 1'b0;
                        import_elements_completed_o <= 11'd0;
                        result_error_o <= 1'b0;
                        result_error_code_o <= 8'd0;
                        result_handle_o <= 37'd0;
                        if (!handles_valid) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_HANDLE;
                            state <= ST_RESULT;
                        end else begin
                            state <= ST_CLEAR_FFN;
                        end
                    end
                end

                ST_CLEAR_FFN: begin
                    if (abort_i) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        state <= ST_RESULT;
                    end else begin
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
                        state <= ST_RESULT;
                    end else if (resident_inspect_rsp_valid_i
                            && resident_inspect_rsp_error_i) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_INSPECT;
                        state <= ST_RESULT;
                    end else if (ffn_activation_valid
                            && ffn_activation_ready) begin
                        if (ffn_activation_index
                                != activation_block[4:0]) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_FFN;
                            state <= ST_RESULT;
                        end else if (activation_block == 6'd31) begin
                            state <= ST_FEED_ROWS;
                        end else begin
                            activation_block <= activation_block + 6'd1;
                            state <= ST_ACT_REQUEST;
                        end
                    end
                end

                ST_FEED_ROWS: begin
                    if (abort_i) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        state <= ST_RESULT;
                    end else if (ffn_poison || ffn_row_error) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_FFN;
                        state <= ST_RESULT;
                    end else if (ffn_complete) begin
                        if (ffn_activation_blocks != 6'd32
                                || ffn_gate_up_rows != 13'd4608
                                || ffn_down_blocks != 8'd144
                                || ffn_down_rows != 11'd1024) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_FFN;
                            state <= ST_RESULT;
                        end else begin
                            state <= ST_IMPORT_CMD;
                        end
                    end
                end

                ST_IMPORT_CMD: begin
                    if (abort_i) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        state <= ST_RESULT;
                    end else if (resident_command_valid_o
                            && resident_command_ready_i) begin
                        read_issue_count <= 11'd0;
                        read_pending <= 1'b0;
                        elastic_valid <= 1'b0;
                        adapter_error <= 1'b0;
                        import_elements_completed_o <= 11'd0;
                        state <= ST_IMPORT_OUTPUT;
                    end
                end

                ST_IMPORT_OUTPUT: begin
                    if (abort_i)
                        abort_seen <= 1'b1;

                    if (ffn_output_read) begin
                        read_pending <= 1'b1;
                        pending_read_index <= read_issue_count[9:0];
                        read_issue_count <= read_issue_count + 11'd1;
                    end
                    if (ffn_output_read_valid) begin
                        read_pending <= 1'b0;
                        if (ffn_output_read_error || elastic_valid) begin
                            adapter_error <= 1'b1;
                            abort_seen <= 1'b1;
                        end else begin
                            elastic_valid <= 1'b1;
                            elastic_index <= pending_read_index;
                            elastic_q30 <= ffn_output_read_q30;
                        end
                    end
                    if (resident_import_accept) begin
                        elastic_valid <= 1'b0;
                        import_elements_completed_o
                            <= import_elements_completed_o + 11'd1;
                    end

                    if (resident_result_valid_i
                            && resident_result_ready_o) begin
                        if (abort_seen || abort_i) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_ABORT;
                            result_handle_o <= 37'd0;
                        end else if (adapter_error) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_ADAPTER;
                            result_handle_o <= 37'd0;
                        end else if (resident_result_error_i) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_IMPORT;
                            result_handle_o <= 37'd0;
                        end else if (resident_result_handle_i
                                != destination_handle
                                || import_elements_completed_o != 11'd1024
                                || read_issue_count != 11'd1024
                                || read_pending || elastic_valid) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_ADAPTER;
                            result_handle_o <= 37'd0;
                        end else begin
                            result_error_o <= 1'b0;
                            result_error_code_o <= 8'd0;
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
                    result_error_o <= 1'b1;
                    result_error_code_o <= ERROR_ADAPTER;
                    result_handle_o <= 37'd0;
                    state <= ST_RESULT;
                end
            endcase
        end
    end

    wire unused_ffn_error_code = ffn_error_code[0];
endmodule
