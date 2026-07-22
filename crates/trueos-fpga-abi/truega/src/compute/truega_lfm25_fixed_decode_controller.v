// Fixed first-token LFM2.5 decode orchestrator.
//
// This is a hard-wired 99-operation circuit schedule. TGF2 feeds prepare the
// next operation and its later TGD1 command only validates/renders the already
// resident result (the two residual operations execute at their TGD1 edge).
// There is no instruction memory, graph parser, soft processor, DMA, TLB, or
// runtime shape selection.
//
// The controller sits between truega_lfm25_feed_frontend/completion_slot and
// truega_lfm25_decode_dispatch. It owns one resident-vector datapath through
// truega_lfm25_fixed_decode_datapath below.
module truega_lfm25_fixed_decode_controller #(
    // Test-only scheduler acceleration. Production must leave this zero. The
    // exact 194,616-item/99-command protocol is still checked when enabled;
    // only payload reads and numerical latency are elided.
    parameter integer FAST_SCHEDULE_SIM = 0
) (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 clear_i,
    input  wire                 abort_i,

    input  wire                 feed_item_valid_i,
    output reg                  feed_item_ready_o,
    input  wire [7:0]           feed_item_mode_i,
    input  wire [7:0]           feed_item_layer_i,
    input  wire [7:0]           feed_item_lane_mask_i,
    input  wire [7:0]           feed_item_payload_format_i,
    input  wire [31:0]          feed_item_session_epoch_i,
    input  wire [31:0]          feed_item_sequence_i,
    input  wire [31:0]          feed_item_position_i,
    input  wire [31:0]          feed_item_token_i,
    input  wire [31:0]          feed_item_index_i,
    input  wire [15:0]          feed_item_stages_per_lane_i,
    input  wire [15:0]          feed_item_payload_bytes_per_stage_i,
    input  wire                 feed_frontend_poisoned_i,
    output reg                  feed_item_error_o,
    output reg  [31:0]          feed_item_error_code_o,

    output wire                 payload_read_valid_o,
    output wire [1:0]           payload_read_bank_o,
    output wire [7:0]           payload_read_slot_o,
    output wire [3:0]           payload_read_word_o,
    input  wire                 payload_read_ready_i,
    input  wire                 payload_read_rsp_valid_i,
    input  wire [31:0]          payload_read_data_i,
    input  wire                 payload_read_error_i,

    input  wire                 execute_start_i,
    input  wire [3:0]           execute_operation_i,
    input  wire [7:0]           execute_layer_i,
    input  wire [31:0]          execute_position_i,
    input  wire [7:0]           execute_input_slot_i,
    input  wire [7:0]           execute_residual_slot_i,
    input  wire [31:0]          execute_session_epoch_i,
    input  wire                 execute_session_begin_i,
    output reg                  engine_done_o,
    output reg                  engine_error_o,
    output reg  [31:0]          engine_error_code_o,
    output reg  [7:0]           engine_result_slot_o,
    output reg  [31:0]          engine_result_position_o,
    output reg  [31:0]          engine_argmax_token_o,
    output reg  [31:0]          engine_argmax_rows_o,
    output reg signed [63:0]    engine_argmax_score_q30_o,

    output reg  [6:0]           operation_ordinal_o,
    output reg  [31:0]          feed_items_retired_o,
    output reg                  poisoned_o,
    output wire                 busy_o
);
    localparam [5:0] ST_WAIT_ITEM      = 6'd0;
    localparam [5:0] ST_DP_START       = 6'd1;
    localparam [5:0] ST_READ_REQ       = 6'd2;
    localparam [5:0] ST_READ_RSP       = 6'd3;
    localparam [5:0] ST_SEND_STAGE     = 6'd4;
    localparam [5:0] ST_SEND_CONTROL   = 6'd5;
    localparam [5:0] ST_ITEM_FINISH    = 6'd6;
    localparam [5:0] ST_ITEM_EFFECT    = 6'd7;
    localparam [5:0] ST_ITEM_RETIRE    = 6'd8;
    localparam [5:0] ST_WAIT_DROP      = 6'd9;
    localparam [5:0] ST_WAIT_EXEC      = 6'd10;
    localparam [5:0] ST_EXEC_DP_START  = 6'd11;
    localparam [5:0] ST_EXEC_DP_RESULT = 6'd12;
    localparam [5:0] ST_FAST_EXEC      = 6'd13;
    localparam [5:0] ST_POISONED       = 6'd14;

    localparam [31:0] ERROR_FEED_ORDER = 32'hbad4_3001;
    localparam [31:0] ERROR_FEED_DOMAIN = 32'hbad4_3002;
    localparam [31:0] ERROR_FEED_SHAPE = 32'hbad4_3003;
    localparam [31:0] ERROR_PAYLOAD = 32'hbad4_3004;
    localparam [31:0] ERROR_EXEC_ORDER = 32'hbad4_3101;
    localparam [31:0] ERROR_EXEC_SLOTS = 32'hbad4_3102;
    localparam [31:0] ERROR_DATAPATH = 32'hbad4_3200;

    reg [5:0] state;
    reg [31:0] active_epoch;
    reg [2:0] feed_part;
    reg [31:0] expected_item;
    reg operation_active;
    reg [7:0] read_stage;
    reg [1:0] read_bank;
    reg [4:0] read_word;
    reg [511:0] payload_buffer;
    reg [7:0] current_mode;
    reg [31:0] current_item;
    reg [7:0] current_layer;
    reg [15:0] current_stages;
    reg [7:0] current_lanes;
    reg [15:0] current_payload_bytes;
    reg retired_operation_complete;

    function [3:0] operation_for_ordinal;
        input [6:0] ordinal;
        reg [6:0] phase;
        begin
            if (ordinal == 0)
                operation_for_ordinal = 4'd0;
            else if (ordinal <= 7'd96) begin
                phase = (ordinal - 1'b1) % 6;
                case (phase)
                    0: operation_for_ordinal = 4'd1;
                    1: operation_for_ordinal = 4'd2; // schedule bit may replace with attention
                    2: operation_for_ordinal = 4'd4;
                    3: operation_for_ordinal = 4'd5;
                    4: operation_for_ordinal = 4'd6;
                    default: operation_for_ordinal = 4'd7;
                endcase
            end else if (ordinal == 7'd97)
                operation_for_ordinal = 4'd8;
            else
                operation_for_ordinal = 4'd9;
        end
    endfunction

    function [7:0] layer_for_ordinal;
        input [6:0] ordinal;
        begin
            if (ordinal >= 1 && ordinal <= 96)
                layer_for_ordinal = (ordinal - 1'b1) / 6;
            else
                layer_for_ordinal = 8'hff;
        end
    endfunction

    function is_attention_layer;
        input [7:0] layer;
        begin
            // LFM2.5 schedule bits 2,5,8,10,12,14.
            is_attention_layer = layer < 16 && (16'h5524 & (16'h1 << layer)) != 0;
        end
    endfunction

    wire [7:0] expected_layer = layer_for_ordinal(operation_ordinal_o);
    wire [3:0] ordinal_base_operation = operation_for_ordinal(operation_ordinal_o);
    wire [3:0] expected_operation = ordinal_base_operation == 4'd2
        && is_attention_layer(expected_layer) ? 4'd3 : ordinal_base_operation;

    function [7:0] mode_for_part;
        input [3:0] operation;
        input [2:0] part;
        begin
            case (operation)
                4'd0: mode_for_part = 8'd0;
                4'd1: mode_for_part = 8'd1;
                4'd2: mode_for_part = part == 0 ? 8'd4
                    : part == 1 ? 8'd5 : 8'd6;
                4'd3: mode_for_part = 8'd7 + part;
                4'd5: mode_for_part = 8'd2;
                4'd6: mode_for_part = part == 0 ? 8'd13 : 8'd14;
                4'd8: mode_for_part = 8'd3;
                4'd9: mode_for_part = 8'd15;
                default: mode_for_part = 8'hff;
            endcase
        end
    endfunction

    function [2:0] parts_for_operation;
        input [3:0] operation;
        begin
            case (operation)
                4'd2: parts_for_operation = 3;
                4'd3: parts_for_operation = 6;
                4'd6: parts_for_operation = 2;
                4'd4, 4'd7: parts_for_operation = 0;
                default: parts_for_operation = 1;
            endcase
        end
    endfunction

    function [31:0] items_for_mode;
        input [7:0] mode;
        begin
            case (mode)
                8'd5, 8'd6, 8'd8, 8'd12, 8'd14: items_for_mode = 1024;
                8'd9, 8'd10: items_for_mode = 512;
                8'd13: items_for_mode = 4608;
                8'd15: items_for_mode = 65536;
                default: items_for_mode = 1;
            endcase
        end
    endfunction

    function [7:0] output_slot_for_ordinal;
        input [6:0] ordinal;
        input [3:0] operation;
        input [7:0] layer;
        reg [7:0] h;
        begin
            h = layer % 3;
            case (operation)
                4'd0: output_slot_for_ordinal = 0;
                4'd1, 4'd5, 4'd8: output_slot_for_ordinal = 0;
                4'd2, 4'd3: output_slot_for_ordinal = (h + 1) % 3;
                4'd4: output_slot_for_ordinal = (h + 2) % 3;
                4'd6: output_slot_for_ordinal = h;
                4'd7: output_slot_for_ordinal = (h + 1) % 3;
                default: output_slot_for_ordinal = 8'hff;
            endcase
        end
    endfunction

    wire [7:0] expected_mode = mode_for_part(expected_operation, feed_part);
    wire [31:0] expected_mode_items = items_for_mode(expected_mode);
    wire operation_has_feed = parts_for_operation(expected_operation) != 0;
    wire last_item_of_part = expected_item + 1'b1 == expected_mode_items;
    wire last_part_of_operation = feed_part + 1'b1
        == parts_for_operation(expected_operation);

    reg item_order_valid;
    reg item_domain_valid;
    reg item_shape_valid;
    always @* begin
        item_order_valid = feed_item_mode_i == expected_mode
            && feed_item_layer_i == expected_layer
            && feed_item_sequence_i == expected_item
            && feed_item_index_i == expected_item;
        item_domain_valid = feed_item_session_epoch_i != 0
            && feed_item_position_i == 0
            && (operation_ordinal_o == 0
                ? (active_epoch == 0 || feed_item_session_epoch_i == active_epoch)
                : feed_item_session_epoch_i == active_epoch);
        item_shape_valid = !feed_frontend_poisoned_i
            && feed_item_lane_mask_i == ((1 << current_lanes_for_mode(expected_mode)) - 1)
            && feed_item_stages_per_lane_i == stages_for_mode(expected_mode)
            && feed_item_payload_bytes_per_stage_i == bytes_for_mode(expected_mode)
            && feed_item_payload_format_i == format_for_mode(expected_mode)
            && ((expected_mode == 0) ? feed_item_token_i < 65536
                : feed_item_token_i == 32'hffff_ffff);
    end

    function [7:0] current_lanes_for_mode;
        input [7:0] mode;
        begin
            case (mode)
                5, 7: current_lanes_for_mode = mode == 5 ? 3 : 2;
                11: current_lanes_for_mode = 0;
                13: current_lanes_for_mode = 2;
                default: current_lanes_for_mode = 1;
            endcase
        end
    endfunction
    function [15:0] stages_for_mode;
        input [7:0] mode;
        begin
            case (mode)
                4: stages_for_mode = 96;
                7: stages_for_mode = 2;
                11: stages_for_mode = 0;
                14: stages_for_mode = 144;
                default: stages_for_mode = 32;
            endcase
        end
    endfunction
    function [15:0] bytes_for_mode;
        input [7:0] mode;
        begin
            if (mode == 11) bytes_for_mode = 0;
            else if (mode == 1 || mode == 2 || mode == 3
                    || mode == 4 || mode == 7) bytes_for_mode = 64;
            else bytes_for_mode = 34;
        end
    endfunction
    function [7:0] format_for_mode;
        input [7:0] mode;
        begin
            if (mode == 11) format_for_mode = 0;
            else if (mode == 1 || mode == 2 || mode == 3 || mode == 7)
                format_for_mode = 1;
            else if (mode == 4) format_for_mode = 2;
            else format_for_mode = 3;
        end
    endfunction

    reg execute_shape_valid;
    reg [7:0] expected_input_slot;
    reg [7:0] expected_residual_slot;
    always @* begin
        expected_input_slot = 8'hff;
        expected_residual_slot = 8'hff;
        case (expected_operation)
            4'd1: expected_input_slot = expected_layer % 3;
            4'd2, 4'd3, 4'd5, 4'd6, 4'd8, 4'd9:
                expected_input_slot = expected_operation == 4'd5
                    ? (expected_layer + 2) % 3
                    : expected_operation == 4'd8 ? 1
                    : (expected_operation == 4'd2 || expected_operation == 4'd3
                        ? 0 : expected_operation == 4'd6
                            ? 0 : 0);
            4'd4: begin
                expected_input_slot = (expected_layer + 1) % 3;
                expected_residual_slot = expected_layer % 3;
            end
            4'd7: begin
                expected_input_slot = expected_layer % 3;
                expected_residual_slot = (expected_layer + 2) % 3;
            end
            default: begin end
        endcase
        execute_shape_valid = execute_operation_i == expected_operation
            && execute_layer_i == expected_layer
            && execute_position_i == 0
            && execute_session_epoch_i == active_epoch
            && execute_input_slot_i == expected_input_slot
            && execute_residual_slot_i == expected_residual_slot
            && execute_session_begin_i == (operation_ordinal_o == 0);
    end

    wire datapath_start_ready;
    wire datapath_start = state == ST_DP_START
        || state == ST_EXEC_DP_START;
    wire datapath_stage_ready;
    wire datapath_stage_valid = state == ST_SEND_STAGE
        || state == ST_SEND_CONTROL;
    wire datapath_item_finish = state == ST_ITEM_FINISH;
    wire datapath_item_effect_done;
    wire datapath_result_valid;
    reg datapath_result_ready;
    wire datapath_result_error;
    wire [7:0] datapath_result_error_code;
    wire [7:0] datapath_result_slot;
    wire [31:0] datapath_result_token;
    wire [31:0] datapath_result_rows;
    wire signed [63:0] datapath_result_score;

    generate if (FAST_SCHEDULE_SIM == 0) begin : gen_datapath
        truega_lfm25_fixed_decode_datapath datapath (
            .clk(clk), .reset_n(reset_n), .clear_i(clear_i), .abort_i(abort_i),
            .start_i(datapath_start), .start_ready_o(datapath_start_ready),
            .operation_i(expected_operation), .layer_i(expected_layer),
            .position_i(32'd0), .session_epoch_i(active_epoch),
            .input_slot_i(expected_input_slot),
            .residual_slot_i(expected_residual_slot),
            .destination_slot_i(output_slot_for_ordinal(operation_ordinal_o,
                expected_operation, expected_layer)),
            .feed_stage_valid_i(datapath_stage_valid),
            .feed_stage_ready_o(datapath_stage_ready),
            .feed_mode_i(current_mode), .feed_item_i(current_item),
            .feed_bank_i(read_bank), .feed_stage_i(read_stage),
            .feed_payload_i(payload_buffer),
            .feed_item_finish_i(datapath_item_finish),
            .feed_item_effect_done_o(datapath_item_effect_done),
            .result_valid_o(datapath_result_valid),
            .result_ready_i(datapath_result_ready),
            .result_error_o(datapath_result_error),
            .result_error_code_o(datapath_result_error_code),
            .result_slot_o(datapath_result_slot),
            .result_token_o(datapath_result_token),
            .result_rows_o(datapath_result_rows),
            .result_score_q30_o(datapath_result_score)
        );
    end else begin : gen_fast_datapath
        assign datapath_start_ready = 1'b1;
        assign datapath_stage_ready = 1'b1;
        assign datapath_item_effect_done = 1'b1;
        assign datapath_result_valid = 1'b1;
        assign datapath_result_error = 1'b0;
        assign datapath_result_error_code = 8'd0;
        assign datapath_result_slot = output_slot_for_ordinal(
            operation_ordinal_o, expected_operation, expected_layer);
        assign datapath_result_token = 32'd1;
        assign datapath_result_rows = 32'd65536;
        assign datapath_result_score = 64'sd0;
    end endgenerate

    assign payload_read_valid_o = FAST_SCHEDULE_SIM == 0 && state == ST_READ_REQ;
    assign payload_read_bank_o = read_bank;
    assign payload_read_slot_o = read_stage;
    assign payload_read_word_o = read_word[3:0];
    assign busy_o = state != ST_WAIT_ITEM || operation_ordinal_o != 0
        || operation_active || poisoned_o;

    task automatic poison;
        input [31:0] code;
        begin
            poisoned_o <= 1'b1;
            feed_item_error_o <= 1'b1;
            feed_item_error_code_o <= code;
            engine_error_o <= 1'b1;
            engine_error_code_o <= code;
            state <= ST_POISONED;
        end
    endtask

    task automatic begin_current_item;
        begin
            current_mode <= feed_item_mode_i;
            current_item <= feed_item_index_i;
            current_layer <= feed_item_layer_i;
            current_stages <= feed_item_stages_per_lane_i;
            current_lanes <= current_lanes_for_mode(feed_item_mode_i);
            current_payload_bytes <= feed_item_payload_bytes_per_stage_i;
            read_stage <= 0;
            read_bank <= 0;
            read_word <= 0;
            payload_buffer <= 0;
        end
    endtask

    always @(posedge clk) begin
        if (!reset_n || clear_i) begin
            state <= ST_WAIT_ITEM;
            active_epoch <= 0;
            feed_part <= 0;
            expected_item <= 0;
            operation_active <= 0;
            read_stage <= 0;
            read_bank <= 0;
            read_word <= 0;
            payload_buffer <= 0;
            current_mode <= 0;
            current_item <= 0;
            current_layer <= 8'hff;
            current_stages <= 0;
            current_lanes <= 0;
            current_payload_bytes <= 0;
            retired_operation_complete <= 0;
            feed_item_ready_o <= 0;
            feed_item_error_o <= 0;
            feed_item_error_code_o <= 0;
            engine_done_o <= 0;
            engine_error_o <= 0;
            engine_error_code_o <= 0;
            engine_result_slot_o <= 8'hff;
            engine_result_position_o <= 0;
            engine_argmax_token_o <= 0;
            engine_argmax_rows_o <= 0;
            engine_argmax_score_q30_o <= 0;
            operation_ordinal_o <= 0;
            feed_items_retired_o <= 0;
            poisoned_o <= 0;
            datapath_result_ready <= 0;
        end else begin
            feed_item_ready_o <= 0;
            feed_item_error_o <= 0;
            engine_done_o <= 0;
            engine_error_o <= 0;
            datapath_result_ready <= 0;

            if (abort_i && !poisoned_o)
                poison(ERROR_DATAPATH | 32'hff);
            else case (state)
                ST_WAIT_ITEM: begin
                    if (!operation_has_feed) begin
                        state <= ST_WAIT_EXEC;
                    end else if (feed_item_valid_i) begin
                        if (!item_order_valid)
                            poison(ERROR_FEED_ORDER);
                        else if (!item_domain_valid)
                            poison(ERROR_FEED_DOMAIN);
                        else if (!item_shape_valid)
                            poison(ERROR_FEED_SHAPE);
                        else begin
                            if (operation_ordinal_o == 0 && active_epoch == 0)
                                active_epoch <= feed_item_session_epoch_i;
                            begin_current_item();
                            if (!operation_active) begin
                                state <= ST_DP_START;
                            end else if (FAST_SCHEDULE_SIM != 0)
                                state <= ST_ITEM_RETIRE;
                            else if (feed_item_stages_per_lane_i == 0)
                                state <= ST_SEND_CONTROL;
                            else
                                state <= ST_READ_REQ;
                        end
                    end
                end

                ST_DP_START: begin
                    if (datapath_start_ready) begin
                        operation_active <= 1;
                        if (FAST_SCHEDULE_SIM != 0)
                            state <= ST_ITEM_RETIRE;
                        else if (current_stages == 0)
                            state <= ST_SEND_CONTROL;
                        else
                            state <= ST_READ_REQ;
                    end
                end

                ST_READ_REQ: begin
                    if (payload_read_ready_i)
                        state <= ST_READ_RSP;
                end
                ST_READ_RSP: begin
                    if (payload_read_rsp_valid_i) begin
                        if (payload_read_error_i)
                            poison(ERROR_PAYLOAD);
                        else begin
                            payload_buffer[read_word * 32 +: 32]
                                <= payload_read_data_i;
                            if (read_word + 1'b1
                                    == (current_payload_bytes + 3) / 4)
                                state <= ST_SEND_STAGE;
                            else begin
                                read_word <= read_word + 1'b1;
                                state <= ST_READ_REQ;
                            end
                        end
                    end
                end
                ST_SEND_STAGE: begin
                    if (datapath_stage_ready) begin
                        payload_buffer <= 0;
                        read_word <= 0;
                        // Q/K norms must reach the attention join as all 64 Q
                        // values followed by all 64 K values. Other paired or
                        // triplet modes are consumed stage-major/lane-minor.
                        if (current_mode == 7) begin
                            if (read_stage + 1'b1 < current_stages) begin
                                read_stage <= read_stage + 1'b1;
                                state <= ST_READ_REQ;
                            end else if (read_bank + 1'b1 < current_lanes) begin
                                read_stage <= 0;
                                read_bank <= read_bank + 1'b1;
                                state <= ST_READ_REQ;
                            end else
                                state <= ST_ITEM_FINISH;
                        end else if (read_bank + 1'b1 < current_lanes) begin
                            read_bank <= read_bank + 1'b1;
                            state <= ST_READ_REQ;
                        end else begin
                            read_bank <= 0;
                            if (read_stage + 1'b1 < current_stages) begin
                                read_stage <= read_stage + 1'b1;
                                state <= ST_READ_REQ;
                            end else
                                state <= ST_ITEM_FINISH;
                        end
                    end
                end
                ST_SEND_CONTROL: begin
                    if (datapath_stage_ready)
                        state <= ST_ITEM_FINISH;
                end
                ST_ITEM_FINISH: begin
                    state <= ST_ITEM_EFFECT;
                end
                ST_ITEM_EFFECT: begin
                    if (datapath_item_effect_done)
                        state <= ST_ITEM_RETIRE;
                end
                ST_ITEM_RETIRE: begin
                    feed_item_ready_o <= 1;
                    feed_items_retired_o <= feed_items_retired_o + 1'b1;
                    retired_operation_complete <= last_item_of_part
                        && last_part_of_operation;
                    if (last_item_of_part) begin
                        expected_item <= 0;
                        if (last_part_of_operation) begin
                            feed_part <= 0;
                            state <= ST_WAIT_DROP;
                        end else begin
                            feed_part <= feed_part + 1'b1;
                            state <= ST_WAIT_DROP;
                        end
                    end else begin
                        expected_item <= expected_item + 1'b1;
                        state <= ST_WAIT_DROP;
                    end
                end
                ST_WAIT_DROP: begin
                    if (!feed_item_valid_i) begin
                        if (retired_operation_complete)
                            state <= ST_WAIT_EXEC;
                        else
                            state <= ST_WAIT_ITEM;
                    end
                end
                ST_WAIT_EXEC: begin
                    if (execute_start_i) begin
                        if (!execute_shape_valid)
                            begin
                                poison(execute_operation_i != expected_operation
                                    || execute_layer_i != expected_layer
                                    || execute_session_epoch_i != active_epoch
                                    ? ERROR_EXEC_ORDER : ERROR_EXEC_SLOTS);
                                engine_done_o <= 1;
                            end
                        else if (expected_operation == 4'd4
                                || expected_operation == 4'd7) begin
                            state <= ST_EXEC_DP_START;
                        end else if (FAST_SCHEDULE_SIM != 0)
                            state <= ST_FAST_EXEC;
                        else if (!datapath_result_valid)
                            poison(ERROR_DATAPATH | 32'h01);
                        else begin
                            datapath_result_ready <= 1;
                            engine_done_o <= 1;
                            engine_error_o <= datapath_result_error;
                            engine_error_code_o <= ERROR_DATAPATH
                                | datapath_result_error_code;
                            engine_result_slot_o <= datapath_result_slot;
                            engine_result_position_o <= 0;
                            engine_argmax_token_o <= datapath_result_token;
                            engine_argmax_rows_o <= datapath_result_rows;
                            engine_argmax_score_q30_o <= datapath_result_score;
                            operation_active <= expected_operation == 4'd8;
                            if (operation_ordinal_o == 98)
                                operation_ordinal_o <= 0;
                            else
                                operation_ordinal_o <= operation_ordinal_o + 1'b1;
                            state <= ST_WAIT_ITEM;
                        end
                    end
                end
                ST_EXEC_DP_START: begin
                    if (datapath_start_ready) begin
                        operation_active <= 1;
                        state <= ST_EXEC_DP_RESULT;
                    end
                end
                ST_EXEC_DP_RESULT: begin
                    if (datapath_result_valid) begin
                        datapath_result_ready <= 1;
                        engine_done_o <= 1;
                        engine_error_o <= datapath_result_error;
                        engine_error_code_o <= ERROR_DATAPATH
                            | datapath_result_error_code;
                        engine_result_slot_o <= datapath_result_slot;
                        engine_result_position_o <= 0;
                        operation_active <= 0;
                        operation_ordinal_o <= operation_ordinal_o + 1'b1;
                        state <= ST_WAIT_ITEM;
                    end
                end
                ST_FAST_EXEC: begin
                    engine_done_o <= 1;
                    engine_result_slot_o <= output_slot_for_ordinal(
                        operation_ordinal_o, expected_operation, expected_layer);
                    engine_result_position_o <= 0;
                    if (expected_operation == 9) begin
                        engine_argmax_token_o <= 1;
                        engine_argmax_rows_o <= 65536;
                        engine_argmax_score_q30_o <= 0;
                    end
                    operation_active <= expected_operation == 8;
                    if (operation_ordinal_o == 98)
                        operation_ordinal_o <= 0;
                    else
                        operation_ordinal_o <= operation_ordinal_o + 1'b1;
                    state <= ST_WAIT_ITEM;
                end
                ST_POISONED: begin
                    if (feed_item_valid_i) begin
                        feed_item_ready_o <= 1;
                        feed_item_error_o <= 1;
                        feed_item_error_code_o <= engine_error_code_o;
                    end
                    if (execute_start_i) begin
                        engine_done_o <= 1;
                        engine_error_o <= 1;
                    end
                end
                default: poison(ERROR_DATAPATH | 32'hfe);
            endcase
        end
    end
endmodule
