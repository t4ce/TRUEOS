// Fixed TGF2 feed-to-decode slice.
//
// Accepted graph, fused at build time:
//   EmbeddingQ8Row[0] -> OperatorRmsNormWeights[0]
//   -> AttentionQueryRows[0..1023] -> resident Q30 handle.
//
// This joins the strict BAR2 frontend, async completion slot, and resident
// decode engine. It contains no graph parser, host math, TGD1 interpreter,
// DMA, TLB, or runtime shape selection.
module truega_lfm25_feed_decode_slice (
    input  wire                 clk,
    input  wire                 reset_n,

    input  wire                 bar2_write_valid_i,
    input  wire [18:0]          bar2_write_address_i,
    input  wire [31:0]          bar2_write_data_i,
    input  wire [3:0]           bar2_write_strobe_i,
    output wire                 bar2_write_ready_o,

    input  wire                 irq_ack_i,
    input  wire                 feed_control_write_i,
    input  wire [31:0]          feed_control_value_i,
    output wire                 irq_retire_o,

    output wire [31:0]          capability_magic_o,
    output wire [31:0]          capability_version_record_bytes_o,
    output wire [31:0]          capability_bits_o,
    output wire [31:0]          capability_model_generation_o,
    output wire [31:0]          capability_shape_set_tag_o,
    output wire [31:0]          feed_state_o,
    output wire [31:0]          retired_mode_layer_o,
    output wire [31:0]          retired_session_epoch_o,
    output wire [31:0]          retired_sequence_o,
    output wire [31:0]          retired_item_o,
    output wire [31:0]          feed_error_code_o,
    output wire [31:0]          completion_count_o,

    output wire                 final_result_valid_o,
    input  wire                 final_result_ready_i,
    output wire                 final_result_error_o,
    output wire [7:0]           final_result_error_code_o,
    output wire [36:0]          final_result_handle_o,
    output wire [12:0]          projection_rows_retired_o,

    input  wire                 output_read_valid_i,
    output wire                 output_read_ready_o,
    input  wire [9:0]           output_read_index_i,
    output wire                 output_read_rsp_valid_o,
    input  wire                 output_read_rsp_ready_i,
    output wire                 output_read_error_o,
    output wire signed [63:0]   output_read_q30_o
);
    localparam [2:0] PH_EMBED = 3'd0;
    localparam [2:0] PH_NORM = 3'd1;
    localparam [2:0] PH_PROJECTION = 3'd2;
    localparam [2:0] PH_DONE = 3'd3;
    localparam [2:0] PH_FAILED = 3'd4;

    localparam [4:0] ST_WAIT_ITEM = 5'd0;
    localparam [4:0] ST_ENGINE_START = 5'd1;
    localparam [4:0] ST_EMBED_READ_REQ = 5'd2;
    localparam [4:0] ST_EMBED_READ_RSP = 5'd3;
    localparam [4:0] ST_EMBED_SEND = 5'd4;
    localparam [4:0] ST_EMBED_WAIT_DONE = 5'd5;
    localparam [4:0] ST_NORM_READ_REQ = 5'd6;
    localparam [4:0] ST_NORM_READ_RSP = 5'd7;
    localparam [4:0] ST_NORM_SEND = 5'd8;
    localparam [4:0] ST_NORM_WAIT_DONE = 5'd9;
    localparam [4:0] ST_PROJ_READ_REQ = 5'd10;
    localparam [4:0] ST_PROJ_READ_RSP = 5'd11;
    localparam [4:0] ST_PROJ_SEND = 5'd12;
    localparam [4:0] ST_PROJ_WAIT_ROW = 5'd13;
    localparam [4:0] ST_FAIL_RETIRE = 5'd14;
    localparam [4:0] ST_WAIT_ACK = 5'd15;

    localparam [31:0] STATE_IDLE = 32'd0;
    localparam [31:0] ERROR_ORDER = 32'hbad4_1001;
    localparam [31:0] ERROR_EPOCH = 32'hbad4_1002;
    localparam [31:0] ERROR_SHAPE = 32'hbad4_1003;
    localparam [31:0] ERROR_ENGINE_BASE = 32'hbad4_2000;

    reg [2:0] phase;
    reg [4:0] state;
    reg [31:0] session_epoch;
    reg [10:0] projection_row;
    reg [7:0] stage_index;
    reg [4:0] read_word;
    reg [5:0] norm_element;
    reg [511:0] payload_buffer;
    reg [31:0] failure_code;
    reg sequence_failed;

    wire frontend_reset;
    wire frontend_item_valid;
    wire frontend_item_ready;
    wire frontend_poisoned;
    wire [7:0] item_mode;
    wire [7:0] item_layer;
    wire [7:0] item_lane_mask;
    wire [7:0] item_payload_format;
    wire [31:0] item_session_epoch;
    wire [31:0] item_sequence;
    wire [31:0] item_position;
    wire [31:0] item_token;
    wire [31:0] item_index;
    wire [15:0] item_stages_per_lane;
    wire [15:0] item_last_stage_slot;
    wire [15:0] item_payload_bytes_per_stage;
    wire [31:0] item_stage_generation;
    wire [31:0] item_shape_tag;

    wire payload_read_ready;
    wire payload_read_rsp_valid;
    wire [31:0] payload_read_data;
    wire payload_read_error;
    wire payload_read_valid = state == ST_EMBED_READ_REQ
        || state == ST_NORM_READ_REQ || state == ST_PROJ_READ_REQ;
    wire [3:0] payload_read_word = read_word[3:0];

    wire frontend_bar_valid = bar2_write_valid_i
        && feed_state_o == STATE_IDLE && state == ST_WAIT_ITEM
        && !sequence_failed && phase != PH_DONE;
    wire frontend_bar_ready;
    assign bar2_write_ready_o = frontend_bar_ready
        && feed_state_o == STATE_IDLE && state == ST_WAIT_ITEM
        && !sequence_failed && phase != PH_DONE;

    truega_lfm25_feed_frontend frontend (
        .clk(clk), .reset_n(reset_n), .state_reset_i(frontend_reset),
        .bar2_write_valid_i(frontend_bar_valid),
        .bar2_write_address_i(bar2_write_address_i),
        .bar2_write_data_i(bar2_write_data_i),
        .bar2_write_strobe_i(bar2_write_strobe_i),
        .bar2_write_ready_o(frontend_bar_ready),
        .capability_magic_o(capability_magic_o),
        .capability_version_record_bytes_o(capability_version_record_bytes_o),
        .capability_bits_o(capability_bits_o),
        .capability_model_generation_o(capability_model_generation_o),
        .capability_shape_set_tag_o(capability_shape_set_tag_o),
        .item_valid_o(frontend_item_valid), .item_ready_i(frontend_item_ready),
        .item_mode_o(item_mode), .item_layer_o(item_layer),
        .item_lane_mask_o(item_lane_mask),
        .item_payload_format_o(item_payload_format),
        .item_session_epoch_o(item_session_epoch),
        .item_sequence_o(item_sequence), .item_position_o(item_position),
        .item_token_o(item_token), .item_index_o(item_index),
        .item_stages_per_lane_o(item_stages_per_lane),
        .item_last_stage_slot_o(item_last_stage_slot),
        .item_payload_bytes_per_stage_o(item_payload_bytes_per_stage),
        .item_stage_generation_o(item_stage_generation),
        .item_shape_tag_o(item_shape_tag),
        .payload_read_valid_i(payload_read_valid),
        .payload_read_bank_i(2'd0), .payload_read_slot_i(stage_index),
        .payload_read_word_i(payload_read_word),
        .payload_read_ready_o(payload_read_ready),
        .payload_read_rsp_valid_o(payload_read_rsp_valid),
        .payload_read_data_o(payload_read_data),
        .payload_read_error_o(payload_read_error),
        .poisoned_o(frontend_poisoned)
    );

    reg item_shape_valid;
    reg item_order_valid;
    reg item_epoch_valid;
    always @* begin
        item_shape_valid = 1'b0;
        item_order_valid = 1'b0;
        item_epoch_valid = item_session_epoch != 32'd0;
        case (phase)
            PH_EMBED: begin
                item_order_valid = item_mode == 8'd0 && item_index == 32'd0
                    && item_sequence == 32'd0;
                item_shape_valid = item_layer == 8'hff
                    && item_lane_mask == 8'd1 && item_payload_format == 8'd3
                    && item_position == 32'd0 && item_token < 32'd65536
                    && item_stages_per_lane == 16'd32
                    && item_last_stage_slot == 16'd31
                    && item_payload_bytes_per_stage == 16'd34
                    && item_stage_generation == 32'd32
                    && item_shape_tag == 32'h46ea_2684;
            end
            PH_NORM: begin
                item_order_valid = item_mode == 8'd1 && item_index == 32'd0
                    && item_sequence == 32'd0;
                item_epoch_valid = item_session_epoch == session_epoch;
                item_shape_valid = item_layer == 8'd2
                    && item_lane_mask == 8'd1 && item_payload_format == 8'd1
                    && item_position == 32'd0
                    && item_token == 32'hffff_ffff
                    && item_stages_per_lane == 16'd32
                    && item_last_stage_slot == 16'd31
                    && item_payload_bytes_per_stage == 16'd64
                    && item_stage_generation == 32'd32
                    && item_shape_tag == 32'hf27a_4365;
            end
            PH_PROJECTION: begin
                item_order_valid = item_mode == 8'd8
                    && item_index == projection_row
                    && item_sequence == projection_row;
                item_epoch_valid = item_session_epoch == session_epoch;
                item_shape_valid = item_layer == 8'd2
                    && item_lane_mask == 8'd1 && item_payload_format == 8'd3
                    && item_position == 32'd0
                    && item_token == 32'hffff_ffff
                    && item_stages_per_lane == 16'd32
                    && item_last_stage_slot == 16'd31
                    && item_payload_bytes_per_stage == 16'd34
                    && item_stage_generation == (projection_row + 1'b1) * 32
                    && item_shape_tag == 32'h15d6_8491;
            end
            default: begin end
        endcase
    end

    wire engine_start = state == ST_ENGINE_START;
    wire engine_start_ready;
    wire engine_embedding_ready;
    wire [4:0] engine_embedding_block;
    wire engine_norm_ready;
    wire [9:0] engine_norm_index;
    wire engine_projection_ready;
    wire [12:0] engine_projection_row;
    wire [4:0] engine_projection_block;
    wire engine_result_valid;
    wire engine_result_error;
    wire [7:0] engine_result_error_code;
    wire [36:0] engine_result_handle;
    wire [12:0] engine_rows_retired;

    wire engine_embedding_valid = state == ST_EMBED_SEND;
    wire engine_norm_valid = state == ST_NORM_SEND;
    wire engine_projection_valid = state == ST_PROJ_SEND;
    wire [15:0] selected_bf16 = payload_buffer[norm_element * 16 +: 16];

    truega_lfm25_decode_engine engine (
        .clk(clk), .reset_n(reset_n), .clear_i(frontend_reset),
        .start_i(engine_start), .start_ready_o(engine_start_ready),
        .session_epoch_i(session_epoch),
        .embedding_valid_i(engine_embedding_valid),
        .embedding_ready_o(engine_embedding_ready),
        .embedding_block_index_o(engine_embedding_block),
        .embedding_block_index_i(stage_index[4:0]),
        .embedding_q8_block_i(payload_buffer[271:0]),
        .norm_weight_valid_i(engine_norm_valid),
        .norm_weight_ready_o(engine_norm_ready),
        .norm_weight_index_o(engine_norm_index),
        .norm_weight_index_i({stage_index[4:0], norm_element[4:0]}),
        .norm_weight_format_bf16_i(1'b1),
        .norm_weight_bits_i({16'd0, selected_bf16}),
        .projection_weight_valid_i(engine_projection_valid),
        .projection_weight_ready_o(engine_projection_ready),
        .projection_weight_row_o(engine_projection_row),
        .projection_weight_block_o(engine_projection_block),
        .projection_weight_row_i({2'd0, projection_row}),
        .projection_weight_block_i(stage_index[4:0]),
        .projection_weight_q8_block_i(payload_buffer[271:0]),
        .result_valid_o(engine_result_valid),
        .result_ready_i(final_result_ready_i),
        .result_error_o(engine_result_error),
        .result_error_code_o(engine_result_error_code),
        .result_handle_o(engine_result_handle),
        .output_read_valid_i(output_read_valid_i),
        .output_read_ready_o(output_read_ready_o),
        .output_read_index_i(output_read_index_i),
        .output_read_rsp_valid_o(output_read_rsp_valid_o),
        .output_read_rsp_ready_i(output_read_rsp_ready_i),
        .output_read_error_o(output_read_error_o),
        .output_read_q30_o(output_read_q30_o),
        .active_session_epoch_o(),
        .projection_rows_retired_o(engine_rows_retired),
        .busy_o()
    );
    assign final_result_valid_o = engine_result_valid;
    assign final_result_error_o = engine_result_error;
    assign final_result_error_code_o = engine_result_error_code;
    assign final_result_handle_o = engine_result_handle;
    assign projection_rows_retired_o = engine_rows_retired;

    wire engine_failed = engine_result_valid && engine_result_error;
    wire embedding_consumed = state == ST_EMBED_WAIT_DONE
        && (engine_norm_ready || engine_failed);
    wire norm_consumed = state == ST_NORM_WAIT_DONE
        && (engine_projection_ready || engine_failed);
    wire projection_consumed = state == ST_PROJ_WAIT_ROW
        && (engine_rows_retired > projection_row || engine_failed);
    wire malformed_retires = state == ST_FAIL_RETIRE;
    wire item_retires = embedding_consumed || norm_consumed
        || projection_consumed || malformed_retires;
    wire retirement_error = malformed_retires || engine_failed;
    wire [31:0] retirement_error_code = malformed_retires ? failure_code
        : engine_failed ? ERROR_ENGINE_BASE | engine_result_error_code : 32'd0;
    assign frontend_item_ready = item_retires;

    truega_lfm25_feed_completion_slot completion (
        .clk(clk), .reset_n(reset_n),
        .item_valid_i(frontend_item_valid), .item_ready_i(item_retires),
        .item_mode_i(item_mode), .item_layer_i(item_layer),
        .item_session_epoch_i(item_session_epoch),
        .item_sequence_i(item_sequence), .item_index_i(item_index),
        .item_error_i(retirement_error),
        .item_error_code_i(retirement_error_code),
        .frontend_poisoned_i(frontend_poisoned),
        .irq_ack_i(irq_ack_i), .control_write_i(feed_control_write_i),
        .control_value_i(feed_control_value_i),
        .frontend_state_reset_o(frontend_reset),
        .state_o(feed_state_o),
        .retired_mode_layer_o(retired_mode_layer_o),
        .retired_session_epoch_o(retired_session_epoch_o),
        .retired_sequence_o(retired_sequence_o),
        .retired_item_o(retired_item_o),
        .error_code_o(feed_error_code_o),
        .completion_count_o(completion_count_o),
        .irq_retire_o(irq_retire_o)
    );

    always @(posedge clk) begin
        if (!reset_n || frontend_reset) begin
            phase <= PH_EMBED;
            state <= ST_WAIT_ITEM;
            session_epoch <= 32'd0;
            projection_row <= 11'd0;
            stage_index <= 8'd0;
            read_word <= 5'd0;
            norm_element <= 6'd0;
            payload_buffer <= 512'd0;
            failure_code <= 32'd0;
            sequence_failed <= 1'b0;
        end else begin
            case (state)
                ST_WAIT_ITEM: begin
                    if (frontend_item_valid) begin
                        if (!item_order_valid) begin
                            failure_code <= ERROR_ORDER;
                            state <= ST_FAIL_RETIRE;
                        end else if (!item_epoch_valid) begin
                            failure_code <= ERROR_EPOCH;
                            state <= ST_FAIL_RETIRE;
                        end else if (!item_shape_valid) begin
                            failure_code <= ERROR_SHAPE;
                            state <= ST_FAIL_RETIRE;
                        end else begin
                            stage_index <= 8'd0;
                            read_word <= 5'd0;
                            norm_element <= 6'd0;
                            if (phase == PH_EMBED) begin
                                session_epoch <= item_session_epoch;
                                state <= ST_ENGINE_START;
                            end else if (phase == PH_NORM) begin
                                state <= ST_NORM_READ_REQ;
                            end else begin
                                state <= ST_PROJ_READ_REQ;
                            end
                        end
                    end
                end

                ST_ENGINE_START: begin
                    if (engine_start_ready)
                        state <= ST_EMBED_READ_REQ;
                end

                ST_EMBED_READ_REQ: begin
                    if (payload_read_valid && payload_read_ready)
                        state <= ST_EMBED_READ_RSP;
                end
                ST_EMBED_READ_RSP: begin
                    if (payload_read_rsp_valid) begin
                        payload_buffer[read_word * 32 +: 32]
                            <= payload_read_data;
                        if (payload_read_error) begin
                            failure_code <= ERROR_SHAPE;
                            state <= ST_FAIL_RETIRE;
                        end else if (read_word == 5'd8) begin
                            state <= ST_EMBED_SEND;
                        end else begin
                            read_word <= read_word + 1'b1;
                            state <= ST_EMBED_READ_REQ;
                        end
                    end
                end
                ST_EMBED_SEND: begin
                    if (engine_embedding_valid && engine_embedding_ready) begin
                        if (engine_embedding_block != stage_index[4:0]) begin
                            failure_code <= ERROR_ENGINE_BASE | 32'hff;
                            state <= ST_FAIL_RETIRE;
                        end else if (stage_index == 8'd31) begin
                            state <= ST_EMBED_WAIT_DONE;
                        end else begin
                            stage_index <= stage_index + 1'b1;
                            read_word <= 5'd0;
                            state <= ST_EMBED_READ_REQ;
                        end
                    end
                end

                ST_NORM_READ_REQ: begin
                    if (payload_read_valid && payload_read_ready)
                        state <= ST_NORM_READ_RSP;
                end
                ST_NORM_READ_RSP: begin
                    if (payload_read_rsp_valid) begin
                        payload_buffer[read_word * 32 +: 32]
                            <= payload_read_data;
                        if (payload_read_error) begin
                            failure_code <= ERROR_SHAPE;
                            state <= ST_FAIL_RETIRE;
                        end else if (read_word == 5'd15) begin
                            norm_element <= 6'd0;
                            state <= ST_NORM_SEND;
                        end else begin
                            read_word <= read_word + 1'b1;
                            state <= ST_NORM_READ_REQ;
                        end
                    end
                end
                ST_NORM_SEND: begin
                    if (engine_norm_valid && engine_norm_ready) begin
                        if (engine_norm_index
                                != {stage_index[4:0], norm_element[4:0]}) begin
                            failure_code <= ERROR_ENGINE_BASE | 32'hff;
                            state <= ST_FAIL_RETIRE;
                        end else if (norm_element == 6'd31) begin
                            if (stage_index == 8'd31) begin
                                state <= ST_NORM_WAIT_DONE;
                            end else begin
                                stage_index <= stage_index + 1'b1;
                                read_word <= 5'd0;
                                norm_element <= 6'd0;
                                state <= ST_NORM_READ_REQ;
                            end
                        end else begin
                            norm_element <= norm_element + 1'b1;
                        end
                    end
                end

                ST_PROJ_READ_REQ: begin
                    if (payload_read_valid && payload_read_ready)
                        state <= ST_PROJ_READ_RSP;
                end
                ST_PROJ_READ_RSP: begin
                    if (payload_read_rsp_valid) begin
                        payload_buffer[read_word * 32 +: 32]
                            <= payload_read_data;
                        if (payload_read_error) begin
                            failure_code <= ERROR_SHAPE;
                            state <= ST_FAIL_RETIRE;
                        end else if (read_word == 5'd8) begin
                            state <= ST_PROJ_SEND;
                        end else begin
                            read_word <= read_word + 1'b1;
                            state <= ST_PROJ_READ_REQ;
                        end
                    end
                end
                ST_PROJ_SEND: begin
                    if (engine_projection_valid && engine_projection_ready) begin
                        if (engine_projection_row != {2'd0, projection_row}
                                || engine_projection_block
                                    != stage_index[4:0]) begin
                            failure_code <= ERROR_ENGINE_BASE | 32'hff;
                            state <= ST_FAIL_RETIRE;
                        end else if (stage_index == 8'd31) begin
                            state <= ST_PROJ_WAIT_ROW;
                        end else begin
                            stage_index <= stage_index + 1'b1;
                            read_word <= 5'd0;
                            state <= ST_PROJ_READ_REQ;
                        end
                    end
                end

                ST_EMBED_WAIT_DONE,
                ST_NORM_WAIT_DONE,
                ST_PROJ_WAIT_ROW: begin
                    if (item_retires) begin
                        if (retirement_error) begin
                            phase <= PH_FAILED;
                            sequence_failed <= 1'b1;
                        end else if (state == ST_EMBED_WAIT_DONE) begin
                            phase <= PH_NORM;
                        end else if (state == ST_NORM_WAIT_DONE) begin
                            phase <= PH_PROJECTION;
                            projection_row <= 11'd0;
                        end else if (projection_row == 11'd1023) begin
                            phase <= PH_DONE;
                        end else begin
                            projection_row <= projection_row + 1'b1;
                        end
                        state <= ST_WAIT_ACK;
                    end
                end

                ST_FAIL_RETIRE: begin
                    if (item_retires) begin
                        phase <= PH_FAILED;
                        sequence_failed <= 1'b1;
                        state <= ST_WAIT_ACK;
                    end
                end

                ST_WAIT_ACK: begin
                    if (feed_state_o == STATE_IDLE && !frontend_item_valid)
                        state <= ST_WAIT_ITEM;
                end

                default: begin
                    failure_code <= ERROR_SHAPE;
                    sequence_failed <= 1'b1;
                    phase <= PH_FAILED;
                    state <= ST_WAIT_ITEM;
                end
            endcase
        end
    end
endmodule
