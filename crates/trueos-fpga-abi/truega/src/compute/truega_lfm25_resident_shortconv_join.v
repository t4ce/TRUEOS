// Fixed resident short-convolution operation join for LFM2.5.
//
// A typed resident Q8_0[1024] handle is preloaded through the shared resident
// inspection port.  The ten-layer causal shortconv slot consumes exactly
// 1,024 x 32 ordered B/C/X triplet blocks and fixed BF16 kernel taps.  Its 32
// native Q8_0 outputs feed a fixed 1,024-row output projection, whose signed
// Q30 results are transactionally imported into a resident destination.
//
// Layer position/cache validation remains owned by the shortconv circuit.
// Abort while its cache is advancing poisons the layer.  A later projection or
// import failure also poisons that already-advanced layer.  No runtime graph,
// parser, processor, DMA, TLB, or host tensor math exists here.
module truega_lfm25_resident_shortconv_join (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 clear_i,
    input  wire                 abort_i,

    input  wire                 layer_reset_i,
    input  wire [3:0]           layer_reset_slot_i,
    output wire                 layer_reset_ready_o,
    output wire                 layer_reset_done_o,

    input  wire                 start_i,
    output wire                 start_ready_o,
    input  wire [36:0]          source_q8_handle_i,
    input  wire [36:0]          destination_q30_handle_i,
    input  wire [3:0]           layer_slot_i,
    input  wire [31:0]          token_position_i,

    input  wire                 triplet_valid_i,
    output wire                 triplet_ready_o,
    output wire [9:0]           triplet_channel_o,
    output wire [4:0]           triplet_block_o,
    input  wire [271:0]         triplet_b_q8_block_i,
    input  wire [271:0]         triplet_c_q8_block_i,
    input  wire [271:0]         triplet_x_q8_block_i,
    input  wire [15:0]          kernel_oldest_bf16_i,
    input  wire [15:0]          kernel_newest_bf16_i,
    input  wire [15:0]          kernel_current_bf16_i,

    input  wire                 projection_weight_valid_i,
    output wire                 projection_weight_ready_o,
    output wire [12:0]          projection_weight_row_o,
    output wire [4:0]           projection_weight_block_o,
    input  wire [12:0]          projection_weight_row_i,
    input  wire [4:0]           projection_weight_block_i,
    input  wire [271:0]         projection_weight_q8_block_i,

    input  wire                 import_pause_i,
    output wire                 projection_output_valid_o,
    output wire [12:0]          projection_output_row_o,
    output wire signed [63:0]   projection_output_q30_o,
    output wire                 shortconv_output_accept_o,
    output wire [4:0]           shortconv_output_block_index_o,
    output wire [271:0]         shortconv_output_q8_block_o,

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

    output wire                 resident_inspect_valid_o,
    input  wire                 resident_inspect_ready_i,
    output wire [36:0]          resident_inspect_handle_o,
    output wire [9:0]           resident_inspect_index_o,
    input  wire                 resident_inspect_rsp_valid_i,
    output wire                 resident_inspect_rsp_ready_o,
    input  wire                 resident_inspect_rsp_error_i,
    input  wire [271:0]         resident_inspect_rsp_data_i,

    output wire                 resident_import_valid_o,
    input  wire                 resident_import_ready_i,
    output wire [9:0]           resident_import_index_o,
    output wire signed [63:0]   resident_import_q30_o,

    output wire [10:0]          shortconv_channels_retired_o,
    output wire [12:0]          projection_rows_retired_o,
    output reg  [10:0]          import_elements_completed_o,
    output wire                 busy_o
);
    localparam [3:0] ST_IDLE          = 4'd0;
    localparam [3:0] ST_SC_START      = 4'd1;
    localparam [3:0] ST_ACT_REQUEST   = 4'd2;
    localparam [3:0] ST_ACT_REPLY     = 4'd3;
    localparam [3:0] ST_SC_FEED       = 4'd4;
    localparam [3:0] ST_PROJ_RESET    = 4'd5;
    localparam [3:0] ST_PROJ_START    = 4'd6;
    localparam [3:0] ST_PROJ_ACT      = 4'd7;
    localparam [3:0] ST_IMPORT_CMD    = 4'd8;
    localparam [3:0] ST_PROJECT       = 4'd9;
    localparam [3:0] ST_RESULT        = 4'd10;
    localparam [3:0] ST_ABORT_INSPECT = 4'd11;

    localparam [1:0] OP_IMPORT_Q30 = 2'd3;
    localparam [7:0] ERROR_HANDLE     = 8'd1;
    localparam [7:0] ERROR_INSPECT    = 8'd2;
    localparam [7:0] ERROR_SHORTCONV  = 8'd3;
    localparam [7:0] ERROR_PROJECTION = 8'd4;
    localparam [7:0] ERROR_IMPORT     = 8'd5;
    localparam [7:0] ERROR_ABORT      = 8'd6;

    reg [3:0] state;
    reg [36:0] source_handle;
    reg [36:0] destination_handle;
    reg [3:0] active_layer;
    reg [31:0] active_position;
    reg [5:0] activation_block;
    reg [5:0] shortconv_output_count;
    reg [5:0] projection_activation_count;
    reg [271:0] shortconv_output_memory [0:31];
    reg abort_seen;
    reg shortconv_failed_latched;
    reg projection_failed;
    reg poison_layer_latched;

    wire joined_reset_n = reset_n && !clear_i;
    wire source_shape_valid = source_q8_handle_i[36:5] != 32'd0
        && source_q8_handle_i[4] == 1'b1
        && source_q8_handle_i[3:0] < 4'd4;
    wire destination_shape_valid = destination_q30_handle_i[36:5] != 32'd0
        && destination_q30_handle_i[4] == 1'b0
        && destination_q30_handle_i[3:0] < 4'd4;
    wire handles_valid = source_shape_valid && destination_shape_valid
        && source_q8_handle_i[36:5] == destination_q30_handle_i[36:5]
        && layer_slot_i < 4'd10;

    assign start_ready_o = state == ST_IDLE && !layer_reset_i
        && !output_read_valid_i && resident_inspect_ready_i
        && !resident_inspect_rsp_valid_i;
    assign result_valid_o = state == ST_RESULT;
    assign busy_o = state != ST_IDLE && state != ST_RESULT;

    // Ten-layer causal shortconv.  Its output blocks are retained locally so
    // an invalid position or aborted token never strands the projection in a
    // partially loaded transaction.
    wire sc_state_reset_ready;
    wire sc_state_reset_done;
    wire sc_activation_ready;
    wire [4:0] sc_activation_index;
    wire sc_row_ready;
    wire [9:0] sc_row_channel;
    wire [4:0] sc_row_block;
    wire sc_output_valid;
    wire sc_output_ready = state == ST_SC_FEED
        && shortconv_output_count < 6'd32 && !abort_seen && !abort_i;
    wire [4:0] sc_output_index;
    wire sc_output_last;
    wire [271:0] sc_output_block;
    wire sc_busy;
    wire sc_done;
    wire sc_error;
    wire [10:0] sc_channels;
    wire [5:0] sc_blocks;
    wire sc_abort = abort_i && (state == ST_SC_START
        || state == ST_ACT_REQUEST || state == ST_ACT_REPLY
        || state == ST_SC_FEED);
    wire sc_poison_layer = poison_layer_latched;
    wire sc_activation_valid = state == ST_ACT_REPLY
        && resident_inspect_rsp_valid_i && !resident_inspect_rsp_error_i
        && !abort_seen && !abort_i;
    wire sc_output_accept = sc_output_valid && sc_output_ready;

    assign layer_reset_ready_o = state == ST_IDLE && sc_state_reset_ready;
    assign layer_reset_done_o = sc_state_reset_done;
    assign triplet_ready_o = state == ST_SC_FEED && sc_row_ready
        && !abort_seen && !abort_i;
    assign triplet_channel_o = sc_row_channel;
    assign triplet_block_o = sc_row_block;
    assign shortconv_output_accept_o = sc_output_accept;
    assign shortconv_output_block_index_o = sc_output_index;
    assign shortconv_output_q8_block_o = sc_output_block;
    assign shortconv_channels_retired_o = sc_channels;

    truega_lfm25_shortconv_token_slot shortconv (
        .clk(clk), .reset_n(joined_reset_n),
        .abort_i(sc_abort), .poison_layer_i(sc_poison_layer),
        .poison_layer_slot_i(active_layer),
        .state_reset_i(state == ST_IDLE && layer_reset_i),
        .state_reset_layer_i(layer_reset_slot_i),
        .state_reset_ready_o(sc_state_reset_ready),
        .state_reset_done_o(sc_state_reset_done),
        .start_i(state == ST_SC_START), .layer_slot_i(active_layer),
        .token_position_i(active_position),
        .activation_valid_i(sc_activation_valid),
        .activation_ready_o(sc_activation_ready),
        .activation_block_index_o(sc_activation_index),
        .activation_q8_block_i(resident_inspect_rsp_data_i),
        .row_valid_i(state == ST_SC_FEED && triplet_valid_i
            && !abort_seen && !abort_i),
        .row_ready_o(sc_row_ready), .row_channel_index_o(sc_row_channel),
        .row_block_index_o(sc_row_block),
        .row_b_weight_q8_block_i(triplet_b_q8_block_i),
        .row_c_weight_q8_block_i(triplet_c_q8_block_i),
        .row_x_weight_q8_block_i(triplet_x_q8_block_i),
        .kernel_oldest_bf16_i(kernel_oldest_bf16_i),
        .kernel_newest_bf16_i(kernel_newest_bf16_i),
        .kernel_current_bf16_i(kernel_current_bf16_i),
        .output_valid_o(sc_output_valid), .output_ready_i(sc_output_ready),
        .output_block_index_o(sc_output_index),
        .output_last_o(sc_output_last),
        .output_y_q8_block_o(sc_output_block),
        .busy_o(sc_busy), .done_o(sc_done), .error_o(sc_error),
        .channels_retired_o(sc_channels), .blocks_retired_o(sc_blocks)
    );

    // Fixed 1,024-row output projection.
    wire projection_state_reset_ready;
    wire projection_state_reset_done;
    wire projection_start_ready;
    wire projection_activation_ready;
    wire [4:0] projection_activation_index;
    wire projection_weight_ready;
    wire [12:0] projection_weight_row;
    wire [4:0] projection_weight_block;
    wire projection_result_valid;
    wire projection_result_ready;
    wire [12:0] projection_result_row;
    wire signed [63:0] projection_result_q30;
    wire projection_result_first;
    wire projection_result_last;
    wire projection_busy;
    wire projection_done;
    wire projection_error;
    wire projection_poisoned;
    wire [7:0] projection_error_code;
    wire [12:0] projection_rows;
    wire projection_abort = (state == ST_PROJ_START
            || state == ST_PROJ_ACT || state == ST_IMPORT_CMD
            || state == ST_PROJECT)
        && (abort_i || abort_seen);
    wire projection_activation_valid = state == ST_PROJ_ACT;
    wire projection_weight_valid = state == ST_PROJECT
        && projection_weight_valid_i && !abort_seen && !abort_i
        && resident_import_ready_i;

    assign projection_weight_ready_o = state == ST_PROJECT
        && projection_weight_ready && resident_import_ready_i
        && !abort_seen && !abort_i;
    assign projection_weight_row_o = projection_weight_row;
    assign projection_weight_block_o = projection_weight_block;
    assign projection_result_ready = state == ST_PROJECT
        && resident_import_ready_i && !import_pause_i
        && !abort_seen && !abort_i;
    assign projection_output_valid_o = state == ST_PROJECT
        && projection_result_valid && !abort_seen && !abort_i;
    assign projection_output_row_o = projection_result_row;
    assign projection_output_q30_o = projection_result_q30;
    assign projection_rows_retired_o = projection_rows;

    truega_lfm25_q8_projection_row_engine #(
        .ROW_COUNT(1024)
    ) output_projection (
        .clk(clk), .reset_n(joined_reset_n), .abort_i(projection_abort),
        .state_reset_i(state == ST_PROJ_RESET),
        .state_reset_ready_o(projection_state_reset_ready),
        .state_reset_done_o(projection_state_reset_done),
        .start_i(state == ST_PROJ_START),
        .start_ready_o(projection_start_ready),
        .activation_valid_i(projection_activation_valid),
        .activation_ready_o(projection_activation_ready),
        .activation_block_index_o(projection_activation_index),
        .activation_block_index_i(projection_activation_count[4:0]),
        .activation_q8_block_i(
            shortconv_output_memory[projection_activation_count[4:0]]),
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
        .rows_retired_o(projection_rows)
    );

    assign resident_command_valid_o = state == ST_IMPORT_CMD && !abort_i;
    assign resident_command_operation_o = OP_IMPORT_Q30;
    assign resident_command_source0_handle_o = 37'd0;
    assign resident_command_source1_handle_o = 37'd0;
    assign resident_command_destination_handle_o = destination_handle;
    assign resident_result_ready_o = state == ST_PROJECT;
    assign resident_abort_o = state == ST_PROJECT
        && (abort_i || abort_seen || projection_failed);

    wire external_read_allowed = state == ST_IDLE || state == ST_RESULT;
    assign resident_inspect_valid_o = (state == ST_ACT_REQUEST
            && !abort_i && !sc_done)
        || (external_read_allowed && output_read_valid_i);
    assign resident_inspect_handle_o = state == ST_ACT_REQUEST
        ? source_handle : destination_handle;
    assign resident_inspect_index_o = state == ST_ACT_REQUEST
        ? {4'd0, activation_block} : output_read_index_i;
    assign resident_inspect_rsp_ready_o = state == ST_ABORT_INSPECT
        ? 1'b1 : state == ST_ACT_REPLY ? sc_activation_ready
        : external_read_allowed && output_read_rsp_ready_i;
    assign output_read_ready_o = external_read_allowed
        && resident_inspect_ready_i;
    assign output_read_rsp_valid_o = external_read_allowed
        && resident_inspect_rsp_valid_i;
    assign output_read_error_o = resident_inspect_rsp_error_i;
    assign output_read_q30_o = resident_inspect_rsp_data_i[63:0];

    assign resident_import_valid_o = state == ST_PROJECT
        && projection_result_valid && !import_pause_i
        && !abort_seen && !abort_i && !projection_failed;
    assign resident_import_index_o = projection_result_row[9:0];
    assign resident_import_q30_o = projection_result_q30;
    wire resident_import_accept = resident_import_valid_o
        && resident_import_ready_i;

    always @(posedge clk) begin
        if (!joined_reset_n) begin
            state <= ST_IDLE;
            source_handle <= 37'd0;
            destination_handle <= 37'd0;
            active_layer <= 4'd0;
            active_position <= 32'd0;
            activation_block <= 6'd0;
            shortconv_output_count <= 6'd0;
            projection_activation_count <= 6'd0;
            abort_seen <= 1'b0;
            shortconv_failed_latched <= 1'b0;
            projection_failed <= 1'b0;
            poison_layer_latched <= 1'b0;
            import_elements_completed_o <= 11'd0;
            result_error_o <= 1'b0;
            result_error_code_o <= 8'd0;
            result_handle_o <= 37'd0;
        end else begin
            if (sc_output_accept) begin
                shortconv_output_memory[shortconv_output_count[4:0]]
                    <= sc_output_block;
                shortconv_output_count <= shortconv_output_count + 6'd1;
            end
            if (sc_done && sc_error)
                shortconv_failed_latched <= 1'b1;
            if (resident_import_accept)
                import_elements_completed_o
                    <= import_elements_completed_o + 11'd1;
            if (state == ST_PROJECT && projection_done
                    && (projection_error || projection_poisoned)) begin
                projection_failed <= 1'b1;
                poison_layer_latched <= 1'b1;
            end

            case (state)
                ST_IDLE: begin
                    if (poison_layer_latched)
                        poison_layer_latched <= 1'b0;
                    if (start_i && start_ready_o) begin
                        source_handle <= source_q8_handle_i;
                        destination_handle <= destination_q30_handle_i;
                        active_layer <= layer_slot_i;
                        active_position <= token_position_i;
                        activation_block <= 6'd0;
                        shortconv_output_count <= 6'd0;
                        projection_activation_count <= 6'd0;
                        abort_seen <= 1'b0;
                        shortconv_failed_latched <= 1'b0;
                        projection_failed <= 1'b0;
                        poison_layer_latched <= 1'b0;
                        import_elements_completed_o <= 11'd0;
                        result_error_o <= 1'b0;
                        result_error_code_o <= 8'd0;
                        result_handle_o <= 37'd0;
                        if (!handles_valid) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_HANDLE;
                            state <= ST_RESULT;
                        end else begin
                            state <= ST_SC_START;
                        end
                    end
                end

                ST_SC_START: begin
                    if (abort_i)
                        abort_seen <= 1'b1;
                    state <= ST_ACT_REQUEST;
                end

                ST_ACT_REQUEST: begin
                    if (abort_i)
                        abort_seen <= 1'b1;
                    if ((sc_done && sc_error)
                            || shortconv_failed_latched) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= abort_seen || abort_i
                            ? ERROR_ABORT : ERROR_SHORTCONV;
                        state <= ST_RESULT;
                    end else if (resident_inspect_valid_o
                            && resident_inspect_ready_i) begin
                        state <= ST_ACT_REPLY;
                    end
                end

                ST_ACT_REPLY: begin
                    if (abort_i) begin
                        abort_seen <= 1'b1;
                        state <= resident_inspect_rsp_valid_i
                            && resident_inspect_rsp_ready_o
                            ? ST_SC_FEED : ST_ABORT_INSPECT;
                    end else if (resident_inspect_rsp_valid_i
                            && resident_inspect_rsp_error_i) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_INSPECT;
                        state <= ST_RESULT;
                    end else if (sc_activation_valid
                            && sc_activation_ready) begin
                        if (sc_activation_index != activation_block[4:0]) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_SHORTCONV;
                            state <= ST_RESULT;
                        end else if (activation_block == 6'd31) begin
                            state <= ST_SC_FEED;
                        end else begin
                            activation_block <= activation_block + 6'd1;
                            state <= ST_ACT_REQUEST;
                        end
                    end
                end

                ST_ABORT_INSPECT: begin
                    if (resident_inspect_rsp_valid_i
                            && resident_inspect_rsp_ready_o)
                        state <= ST_SC_FEED;
                end

                ST_SC_FEED: begin
                    if (abort_i)
                        abort_seen <= 1'b1;
                    if (sc_done || shortconv_failed_latched) begin
                        if (sc_error || shortconv_failed_latched
                                || abort_seen || abort_i) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= abort_seen || abort_i
                                ? ERROR_ABORT : ERROR_SHORTCONV;
                            state <= ST_RESULT;
                        end else if (sc_channels != 11'd1024
                                || sc_blocks != 6'd32
                                || shortconv_output_count != 6'd32) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_SHORTCONV;
                            state <= ST_RESULT;
                        end else begin
                            state <= ST_PROJ_RESET;
                        end
                    end
                end

                ST_PROJ_RESET: begin
                    if (abort_i) begin
                        poison_layer_latched <= 1'b1;
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        state <= ST_RESULT;
                    end else if (projection_state_reset_ready) begin
                        projection_activation_count <= 6'd0;
                        state <= ST_PROJ_START;
                    end
                end

                ST_PROJ_START: begin
                    if (abort_i) begin
                        poison_layer_latched <= 1'b1;
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        state <= ST_RESULT;
                    end else if (projection_start_ready) begin
                        state <= ST_PROJ_ACT;
                    end
                end

                ST_PROJ_ACT: begin
                    if (abort_i) begin
                        projection_failed <= 1'b1;
                        poison_layer_latched <= 1'b1;
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        state <= ST_RESULT;
                    end else if (projection_activation_valid
                            && projection_activation_ready) begin
                        if (projection_activation_index
                                != projection_activation_count[4:0]) begin
                            projection_failed <= 1'b1;
                            poison_layer_latched <= 1'b1;
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_PROJECTION;
                            state <= ST_RESULT;
                        end else if (projection_activation_count == 6'd31) begin
                            state <= ST_IMPORT_CMD;
                        end else begin
                            projection_activation_count
                                <= projection_activation_count + 6'd1;
                        end
                    end
                end

                ST_IMPORT_CMD: begin
                    if (abort_i) begin
                        projection_failed <= 1'b1;
                        poison_layer_latched <= 1'b1;
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        state <= ST_RESULT;
                    end else if (resident_command_valid_o
                            && resident_command_ready_i) begin
                        import_elements_completed_o <= 11'd0;
                        state <= ST_PROJECT;
                    end
                end

                ST_PROJECT: begin
                    if (abort_i) begin
                        abort_seen <= 1'b1;
                        projection_failed <= 1'b1;
                        poison_layer_latched <= 1'b1;
                    end
                    if (resident_result_valid_i
                            && resident_result_ready_o) begin
                        if (abort_seen || abort_i) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_ABORT;
                            result_handle_o <= 37'd0;
                            poison_layer_latched <= 1'b1;
                        end else if (projection_failed || projection_error
                                || projection_poisoned) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_PROJECTION;
                            result_handle_o <= 37'd0;
                            poison_layer_latched <= 1'b1;
                        end else if (resident_result_error_i) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_IMPORT;
                            result_handle_o <= 37'd0;
                            poison_layer_latched <= 1'b1;
                        end else if (resident_result_handle_i
                                != destination_handle
                                || projection_rows != 13'd1024
                                || import_elements_completed_o != 11'd1024) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_IMPORT;
                            result_handle_o <= 37'd0;
                            poison_layer_latched <= 1'b1;
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
                    result_error_code_o <= ERROR_IMPORT;
                    result_handle_o <= 37'd0;
                    state <= ST_RESULT;
                end
            endcase
        end
    end

    wire unused_projection = projection_result_first
        ^ projection_result_last ^ projection_busy
        ^ projection_state_reset_done ^ projection_error_code[0]
        ^ sc_busy ^ sc_output_last;
endmodule
