// Numerical half of the fixed first-token decode controller.
//
// Exactly one resident-vector engine owns all typed Q30/Q8 slots. The fixed
// joins below are mutually exclusive clients selected by `owner`; this is a
// circuit mux, not a run-time graph or command processor.
module truega_lfm25_fixed_decode_datapath (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 clear_i,
    input  wire                 abort_i,

    input  wire                 start_i,
    output wire                 start_ready_o,
    input  wire [3:0]           operation_i,
    input  wire [7:0]           layer_i,
    input  wire [31:0]          position_i,
    input  wire [31:0]          session_epoch_i,
    input  wire [7:0]           input_slot_i,
    input  wire [7:0]           residual_slot_i,
    input  wire [7:0]           destination_slot_i,

    input  wire                 feed_stage_valid_i,
    output reg                  feed_stage_ready_o,
    input  wire [7:0]           feed_mode_i,
    input  wire [31:0]          feed_item_i,
    input  wire [1:0]           feed_bank_i,
    input  wire [7:0]           feed_stage_i,
    input  wire [511:0]         feed_payload_i,
    input  wire                 feed_item_finish_i,
    output reg                  feed_item_effect_done_o,

    output wire                 result_valid_o,
    input  wire                 result_ready_i,
    output wire                 result_error_o,
    output wire [7:0]           result_error_code_o,
    output wire [7:0]           result_slot_o,
    output wire [31:0]          result_token_o,
    output wire [31:0]          result_rows_o,
    output wire signed [63:0]   result_score_q30_o
);
    localparam [2:0] OWNER_NONE = 3'd0;
    localparam [2:0] OWNER_DIRECT = 3'd1;
    localparam [2:0] OWNER_NORM = 3'd2;
    localparam [2:0] OWNER_SHORT = 3'd3;
    localparam [2:0] OWNER_ATTN = 3'd4;
    localparam [2:0] OWNER_FFN = 3'd5;
    localparam [2:0] OWNER_TAIL = 3'd6;

    reg [2:0] owner;
    reg [3:0] active_operation;
    reg [7:0] active_layer;
    reg [31:0] active_position;
    reg [31:0] active_epoch;
    reg [7:0] active_input_slot;
    reg [7:0] active_residual_slot;
    reg [7:0] active_destination_slot;
    reg launch_pending;

    function [36:0] typed_handle;
        input [31:0] epoch;
        input type_q8;
        input [7:0] slot;
        begin
            typed_handle = {epoch, type_q8, slot[3:0]};
        end
    endfunction
    wire [36:0] input_q30 = typed_handle(active_epoch, 1'b0,
        active_input_slot);
    wire [36:0] input_q8 = typed_handle(active_epoch, 1'b1,
        active_input_slot);
    wire [36:0] residual_q30 = typed_handle(active_epoch, 1'b0,
        active_residual_slot);
    wire destination_is_q8 = active_operation == 1 || active_operation == 5
        || active_operation == 8;
    wire [36:0] destination_handle = typed_handle(active_epoch,
        destination_is_q8, active_destination_slot);

    // ---- Shared resident-vector client wires -----------------------------
    wire resident_command_ready;
    wire resident_result_valid;
    wire resident_result_error;
    wire [36:0] resident_result_handle;
    wire resident_weight_ready;
    wire resident_import_ready;
    wire resident_inspect_ready;
    wire resident_inspect_rsp_valid;
    wire resident_inspect_rsp_error;
    wire [271:0] resident_inspect_rsp_data;

    wire norm_cmd_valid, short_cmd_valid, attn_cmd_valid, ffn_cmd_valid,
        tail_cmd_valid;
    wire [1:0] norm_cmd_op, short_cmd_op, attn_cmd_op, ffn_cmd_op, tail_cmd_op;
    wire [36:0] norm_cmd_s0, norm_cmd_s1, norm_cmd_dst;
    wire [36:0] short_cmd_s0, short_cmd_s1, short_cmd_dst;
    wire [36:0] attn_cmd_s0, attn_cmd_s1, attn_cmd_dst;
    wire [36:0] ffn_cmd_s0, ffn_cmd_s1, ffn_cmd_dst;
    wire [36:0] tail_cmd_s0, tail_cmd_s1, tail_cmd_dst;
    wire norm_result_ready, short_result_ready, attn_result_ready,
        ffn_result_ready, tail_result_ready;
    wire norm_abort, short_abort, attn_abort, ffn_abort, tail_abort;
    wire norm_weight_valid, tail_weight_valid;
    wire [9:0] norm_weight_index, tail_weight_index;
    wire norm_weight_format, tail_weight_format;
    wire [31:0] norm_weight_bits, tail_weight_bits;
    wire short_import_valid, attn_import_valid, ffn_import_valid;
    wire [9:0] short_import_index, attn_import_index, ffn_import_index;
    wire signed [63:0] short_import_q30, attn_import_q30, ffn_import_q30;
    wire norm_inspect_valid, short_inspect_valid, attn_inspect_valid,
        ffn_inspect_valid, tail_inspect_valid;
    wire [36:0] norm_inspect_handle, short_inspect_handle,
        attn_inspect_handle, ffn_inspect_handle, tail_inspect_handle;
    wire [9:0] norm_inspect_index, short_inspect_index, attn_inspect_index,
        ffn_inspect_index, tail_inspect_index;
    wire norm_inspect_rsp_ready, short_inspect_rsp_ready,
        attn_inspect_rsp_ready, ffn_inspect_rsp_ready, tail_inspect_rsp_ready;

    wire direct_cmd_valid = owner == OWNER_DIRECT && launch_pending
        && active_operation == 0;
    wire [1:0] resident_command_operation = owner == OWNER_DIRECT ? 2'd0
        : owner == OWNER_NORM ? norm_cmd_op
        : owner == OWNER_SHORT ? short_cmd_op
        : owner == OWNER_ATTN ? attn_cmd_op
        : owner == OWNER_FFN ? ffn_cmd_op : tail_cmd_op;
    wire resident_command_valid = owner == OWNER_DIRECT ? direct_cmd_valid
        : owner == OWNER_NORM ? norm_cmd_valid
        : owner == OWNER_SHORT ? short_cmd_valid
        : owner == OWNER_ATTN ? attn_cmd_valid
        : owner == OWNER_FFN ? ffn_cmd_valid
        : owner == OWNER_TAIL ? tail_cmd_valid : 1'b0;
    wire [36:0] resident_command_s0 = owner == OWNER_DIRECT
        ? typed_handle(active_epoch, 1'b1, 0)
        : owner == OWNER_NORM ? norm_cmd_s0
        : owner == OWNER_SHORT ? short_cmd_s0
        : owner == OWNER_ATTN ? attn_cmd_s0
        : owner == OWNER_FFN ? ffn_cmd_s0 : tail_cmd_s0;
    wire [36:0] resident_command_s1 = owner == OWNER_DIRECT ? 37'd0
        : owner == OWNER_NORM ? norm_cmd_s1
        : owner == OWNER_SHORT ? short_cmd_s1
        : owner == OWNER_ATTN ? attn_cmd_s1
        : owner == OWNER_FFN ? ffn_cmd_s1 : tail_cmd_s1;
    wire [36:0] resident_command_dst = owner == OWNER_DIRECT
        ? destination_handle
        : owner == OWNER_NORM ? norm_cmd_dst
        : owner == OWNER_SHORT ? short_cmd_dst
        : owner == OWNER_ATTN ? attn_cmd_dst
        : owner == OWNER_FFN ? ffn_cmd_dst : tail_cmd_dst;
    wire resident_result_ready = owner == OWNER_DIRECT ? result_ready_i
        : owner == OWNER_NORM ? norm_result_ready
        : owner == OWNER_SHORT ? short_result_ready
        : owner == OWNER_ATTN ? attn_result_ready
        : owner == OWNER_FFN ? ffn_result_ready
        : owner == OWNER_TAIL ? tail_result_ready : 1'b0;
    wire resident_abort = abort_i || (owner == OWNER_NORM ? norm_abort
        : owner == OWNER_SHORT ? short_abort
        : owner == OWNER_ATTN ? attn_abort
        : owner == OWNER_FFN ? ffn_abort
        : owner == OWNER_TAIL ? tail_abort : 1'b0);
    wire resident_weight_valid = owner == OWNER_NORM ? norm_weight_valid
        : owner == OWNER_TAIL ? tail_weight_valid : 1'b0;
    wire [9:0] resident_weight_index = owner == OWNER_NORM
        ? norm_weight_index : tail_weight_index;
    wire resident_weight_format = owner == OWNER_NORM
        ? norm_weight_format : tail_weight_format;
    wire [31:0] resident_weight_bits = owner == OWNER_NORM
        ? norm_weight_bits : tail_weight_bits;
    wire resident_import_valid = owner == OWNER_SHORT ? short_import_valid
        : owner == OWNER_ATTN ? attn_import_valid
        : owner == OWNER_FFN ? ffn_import_valid : 1'b0;
    wire [9:0] resident_import_index = owner == OWNER_SHORT
        ? short_import_index : owner == OWNER_ATTN ? attn_import_index
        : ffn_import_index;
    wire signed [63:0] resident_import_q30 = owner == OWNER_SHORT
        ? short_import_q30 : owner == OWNER_ATTN ? attn_import_q30
        : ffn_import_q30;
    wire resident_inspect_valid = owner == OWNER_NORM ? norm_inspect_valid
        : owner == OWNER_SHORT ? short_inspect_valid
        : owner == OWNER_ATTN ? attn_inspect_valid
        : owner == OWNER_FFN ? ffn_inspect_valid
        : owner == OWNER_TAIL ? tail_inspect_valid : 1'b0;
    wire [36:0] resident_inspect_handle = owner == OWNER_NORM
        ? norm_inspect_handle : owner == OWNER_SHORT ? short_inspect_handle
        : owner == OWNER_ATTN ? attn_inspect_handle
        : owner == OWNER_FFN ? ffn_inspect_handle : tail_inspect_handle;
    wire [9:0] resident_inspect_index = owner == OWNER_NORM
        ? norm_inspect_index : owner == OWNER_SHORT ? short_inspect_index
        : owner == OWNER_ATTN ? attn_inspect_index
        : owner == OWNER_FFN ? ffn_inspect_index : tail_inspect_index;
    wire resident_inspect_rsp_ready = owner == OWNER_NORM
        ? norm_inspect_rsp_ready : owner == OWNER_SHORT ? short_inspect_rsp_ready
        : owner == OWNER_ATTN ? attn_inspect_rsp_ready
        : owner == OWNER_FFN ? ffn_inspect_rsp_ready
        : owner == OWNER_TAIL ? tail_inspect_rsp_ready : 1'b0;

    wire direct_embedding_ready;
    truega_lfm25_resident_vector_engine resident (
        .clk(clk), .reset_n(reset_n && !clear_i), .abort_i(resident_abort),
        .command_valid_i(resident_command_valid),
        .command_ready_o(resident_command_ready),
        .command_operation_i(resident_command_operation),
        .command_source0_handle_i(resident_command_s0),
        .command_source1_handle_i(resident_command_s1),
        .command_destination_handle_i(resident_command_dst),
        .embedding_block_valid_i(owner == OWNER_DIRECT
            && feed_stage_valid_i && feed_mode_i == 0),
        .embedding_block_ready_o(direct_embedding_ready),
        .embedding_block_index_i(feed_stage_i[4:0]),
        .embedding_q8_block_i(feed_payload_i[271:0]),
        .weight_valid_i(resident_weight_valid),
        .weight_ready_o(resident_weight_ready),
        .weight_index_i(resident_weight_index),
        .weight_format_bf16_i(resident_weight_format),
        .weight_bits_i(resident_weight_bits),
        .import_valid_i(resident_import_valid),
        .import_ready_o(resident_import_ready),
        .import_index_i(resident_import_index),
        .import_q30_i(resident_import_q30),
        .result_valid_o(resident_result_valid),
        .result_ready_i(resident_result_ready),
        .result_error_o(resident_result_error),
        .result_handle_o(resident_result_handle),
        .inspect_valid_i(resident_inspect_valid),
        .inspect_ready_o(resident_inspect_ready),
        .inspect_handle_i(resident_inspect_handle),
        .inspect_index_i(resident_inspect_index),
        .inspect_rsp_valid_o(resident_inspect_rsp_valid),
        .inspect_rsp_ready_i(resident_inspect_rsp_ready),
        .inspect_rsp_error_o(resident_inspect_rsp_error),
        .inspect_rsp_data_o(resident_inspect_rsp_data),
        .session_epoch_o(), .busy_o()
    );

    // ---- Payload scalarizer and coefficient store ------------------------
    reg scalar_busy;
    reg [5:0] scalar_index;
    reg [7:0] scalar_mode;
    reg [1:0] scalar_bank;
    reg [7:0] scalar_stage;
    reg [511:0] scalar_payload;
    wire [15:0] scalar_bf16 = scalar_payload[scalar_index * 16 +: 16];
    wire scalar_to_norm = scalar_busy
        && (scalar_mode == 1 || scalar_mode == 2);
    wire scalar_to_tail = scalar_busy && scalar_mode == 3;
    wire scalar_to_attn = scalar_busy && scalar_mode == 7;
    wire scalar_ready = scalar_to_norm ? norm_feed_ready
        : scalar_to_tail ? tail_norm_feed_ready
        : scalar_to_attn ? attn_norm_feed_ready : 1'b0;
    wire scalar_accept = scalar_busy && scalar_ready;

    reg [15:0] shortconv_coeff [0:3071];
    integer coeff_i;
    reg [271:0] triplet_b;
    reg [271:0] triplet_c;
    reg [271:0] ffn_weight0;
    reg ffn_row_started;

    // ---- Join-facing feed wires ------------------------------------------
    wire norm_feed_ready;
    wire tail_norm_feed_ready;
    wire attn_norm_feed_ready;
    wire short_triplet_ready;
    wire short_projection_ready;
    wire attn_projection_ready;
    wire attn_core_ready;
    wire ffn_row_ready;
    wire ffn_weight_ready;
    wire tail_lm_ready;

    always @* begin
        feed_stage_ready_o = 1'b0;
        case (feed_mode_i)
            0: feed_stage_ready_o = owner == OWNER_DIRECT
                && direct_embedding_ready;
            1, 2, 3, 7: feed_stage_ready_o = !scalar_busy;
            4: feed_stage_ready_o = owner == OWNER_SHORT;
            5: feed_stage_ready_o = owner == OWNER_SHORT
                && (feed_bank_i < 2 || short_triplet_ready);
            6: feed_stage_ready_o = owner == OWNER_SHORT
                && short_projection_ready;
            8, 9, 10, 12: feed_stage_ready_o = owner == OWNER_ATTN
                && attn_projection_ready;
            11: feed_stage_ready_o = owner == OWNER_ATTN && attn_core_ready;
            13: feed_stage_ready_o = owner == OWNER_FFN
                && (feed_bank_i == 0
                    ? (feed_stage_i != 0 || ffn_row_started || ffn_row_ready)
                    : (ffn_row_started && ffn_weight_ready));
            14: feed_stage_ready_o = owner == OWNER_FFN
                && ffn_row_started && ffn_weight_ready;
            15: feed_stage_ready_o = owner == OWNER_TAIL && tail_lm_ready;
            default: feed_stage_ready_o = 1'b0;
        endcase
    end
    wire feed_stage_accept = feed_stage_valid_i && feed_stage_ready_o;

    // ---- Norm/residual join ----------------------------------------------
    wire norm_start_ready;
    wire norm_join_result_valid;
    wire norm_join_result_error;
    wire [7:0] norm_join_error_code;
    wire [36:0] norm_join_result_handle;
    truega_lfm25_resident_norm_residual_join norm_residual (
        .clk(clk), .reset_n(reset_n), .clear_i(clear_i), .abort_i(abort_i),
        .start_i(launch_pending && owner == OWNER_NORM),
        .start_ready_o(norm_start_ready),
        .operation_i(active_operation == 4 || active_operation == 7),
        .source0_q30_handle_i(input_q30),
        .source1_q30_handle_i(residual_q30),
        .destination_handle_i(destination_handle),
        .token_position_i(active_position),
        .weight_valid_i(scalar_to_norm), .weight_ready_o(norm_feed_ready),
        .expected_weight_index_o(),
        .weight_index_i({scalar_stage[4:0], scalar_index[4:0]}),
        .weight_format_bf16_i(1'b1),
        .weight_bits_i({16'd0, scalar_bf16}),
        .result_valid_o(norm_join_result_valid),
        .result_ready_i(result_ready_i && owner == OWNER_NORM),
        .result_error_o(norm_join_result_error),
        .result_error_code_o(norm_join_error_code), .result_operation_o(),
        .result_token_position_o(), .result_handle_o(norm_join_result_handle),
        .output_read_valid_i(1'b0), .output_read_ready_o(),
        .output_read_index_i(10'd0), .output_read_rsp_valid_o(),
        .output_read_rsp_ready_i(1'b1), .output_read_error_o(),
        .output_read_data_o(),
        .resident_command_valid_o(norm_cmd_valid),
        .resident_command_ready_i(resident_command_ready),
        .resident_command_operation_o(norm_cmd_op),
        .resident_command_source0_handle_o(norm_cmd_s0),
        .resident_command_source1_handle_o(norm_cmd_s1),
        .resident_command_destination_handle_o(norm_cmd_dst),
        .resident_result_valid_i(resident_result_valid),
        .resident_result_ready_o(norm_result_ready),
        .resident_result_error_i(resident_result_error),
        .resident_result_handle_i(resident_result_handle),
        .resident_abort_o(norm_abort),
        .resident_weight_valid_o(norm_weight_valid),
        .resident_weight_ready_i(resident_weight_ready),
        .resident_weight_index_o(norm_weight_index),
        .resident_weight_format_bf16_o(norm_weight_format),
        .resident_weight_bits_o(norm_weight_bits),
        .resident_inspect_valid_o(norm_inspect_valid),
        .resident_inspect_ready_i(resident_inspect_ready),
        .resident_inspect_handle_o(norm_inspect_handle),
        .resident_inspect_index_o(norm_inspect_index),
        .resident_inspect_rsp_valid_i(resident_inspect_rsp_valid),
        .resident_inspect_rsp_ready_o(norm_inspect_rsp_ready),
        .resident_inspect_rsp_error_i(resident_inspect_rsp_error),
        .resident_inspect_rsp_data_i(resident_inspect_rsp_data),
        .weights_accepted_o(), .poisoned_o(), .busy_o()
    );

    // ---- Shortconv join ---------------------------------------------------
    wire short_start_ready, short_join_result_valid, short_join_result_error;
    wire [7:0] short_join_error_code;
    wire [36:0] short_join_result_handle;
    wire [10:0] short_channels;
    wire [12:0] short_rows;
    truega_lfm25_resident_shortconv_join shortconv (
        .clk(clk), .reset_n(reset_n), .clear_i(clear_i), .abort_i(abort_i),
        .layer_reset_i(1'b0), .layer_reset_slot_i(4'd0),
        .layer_reset_ready_o(), .layer_reset_done_o(),
        .start_i(launch_pending && owner == OWNER_SHORT),
        .start_ready_o(short_start_ready), .source_q8_handle_i(input_q8),
        .destination_q30_handle_i(destination_handle),
        .layer_slot_i(shortconv_slot(active_layer)),
        .token_position_i(active_position),
        .triplet_valid_i(feed_stage_valid_i && feed_mode_i == 5
            && feed_bank_i == 2), .triplet_ready_o(short_triplet_ready),
        .triplet_channel_o(), .triplet_block_o(),
        .triplet_b_q8_block_i(triplet_b), .triplet_c_q8_block_i(triplet_c),
        .triplet_x_q8_block_i(feed_payload_i[271:0]),
        .kernel_oldest_bf16_i(shortconv_coeff[feed_item_i * 3]),
        .kernel_newest_bf16_i(shortconv_coeff[feed_item_i * 3 + 1]),
        .kernel_current_bf16_i(shortconv_coeff[feed_item_i * 3 + 2]),
        .projection_weight_valid_i(feed_stage_valid_i && feed_mode_i == 6),
        .projection_weight_ready_o(short_projection_ready),
        .projection_weight_row_o(), .projection_weight_block_o(),
        .projection_weight_row_i(feed_item_i[12:0]),
        .projection_weight_block_i(feed_stage_i[4:0]),
        .projection_weight_q8_block_i(feed_payload_i[271:0]),
        .import_pause_i(1'b0), .projection_output_valid_o(),
        .projection_output_row_o(), .projection_output_q30_o(),
        .shortconv_output_accept_o(), .shortconv_output_block_index_o(),
        .shortconv_output_q8_block_o(),
        .result_valid_o(short_join_result_valid),
        .result_ready_i(result_ready_i && owner == OWNER_SHORT),
        .result_error_o(short_join_result_error),
        .result_error_code_o(short_join_error_code),
        .result_handle_o(short_join_result_handle),
        .output_read_valid_i(1'b0), .output_read_ready_o(),
        .output_read_index_i(10'd0), .output_read_rsp_valid_o(),
        .output_read_rsp_ready_i(1'b1), .output_read_error_o(),
        .output_read_q30_o(),
        .resident_command_valid_o(short_cmd_valid),
        .resident_command_ready_i(resident_command_ready),
        .resident_command_operation_o(short_cmd_op),
        .resident_command_source0_handle_o(short_cmd_s0),
        .resident_command_source1_handle_o(short_cmd_s1),
        .resident_command_destination_handle_o(short_cmd_dst),
        .resident_result_valid_i(resident_result_valid),
        .resident_result_ready_o(short_result_ready),
        .resident_result_error_i(resident_result_error),
        .resident_result_handle_i(resident_result_handle),
        .resident_abort_o(short_abort),
        .resident_inspect_valid_o(short_inspect_valid),
        .resident_inspect_ready_i(resident_inspect_ready),
        .resident_inspect_handle_o(short_inspect_handle),
        .resident_inspect_index_o(short_inspect_index),
        .resident_inspect_rsp_valid_i(resident_inspect_rsp_valid),
        .resident_inspect_rsp_ready_o(short_inspect_rsp_ready),
        .resident_inspect_rsp_error_i(resident_inspect_rsp_error),
        .resident_inspect_rsp_data_i(resident_inspect_rsp_data),
        .resident_import_valid_o(short_import_valid),
        .resident_import_ready_i(resident_import_ready),
        .resident_import_index_o(short_import_index),
        .resident_import_q30_o(short_import_q30),
        .shortconv_channels_retired_o(short_channels),
        .projection_rows_retired_o(short_rows),
        .import_elements_completed_o(), .busy_o()
    );

    function [3:0] shortconv_slot;
        input [7:0] layer;
        integer n;
        integer k;
        begin
            n = 0;
            for (k = 0; k < layer; k = k + 1)
                if ((16'h5524 & (16'h1 << k)) == 0) n = n + 1;
            shortconv_slot = n[3:0];
        end
    endfunction

    // ---- Attention join ---------------------------------------------------
    wire attn_start_ready, attn_join_result_valid, attn_join_result_error;
    wire [7:0] attn_join_error_code;
    wire [36:0] attn_join_result_handle;
    wire [10:0] attn_q_rows;
    wire [9:0] attn_k_rows, attn_v_rows;
    wire [12:0] attn_out_rows;
    wire attn_core_done;
    reg attn_core_seen;
    truega_lfm25_resident_attention_join attention (
        .clk(clk), .reset_n(reset_n), .clear_i(clear_i), .abort_i(abort_i),
        .start_i(launch_pending && owner == OWNER_ATTN),
        .start_ready_o(attn_start_ready), .source_q8_handle_i(input_q8),
        .destination_q30_handle_i(destination_handle),
        .layer_i(active_layer[3:0]), .token_position_i(active_position[16:0]),
        .norm_weight_valid_i(scalar_to_attn),
        .norm_weight_ready_o(attn_norm_feed_ready), .norm_weight_key_o(),
        .norm_weight_element_o(), .norm_weight_key_i(scalar_bank[0]),
        .norm_weight_element_i({scalar_stage[0], scalar_index[4:0]}),
        .norm_weight_bf16_i(scalar_bf16),
        .projection_weight_valid_i(feed_stage_valid_i
            && (feed_mode_i == 8 || feed_mode_i == 9 || feed_mode_i == 10
                || feed_mode_i == 12)),
        .projection_weight_ready_o(attn_projection_ready),
        .projection_weight_kind_o(), .projection_weight_row_o(),
        .projection_weight_block_o(),
        .projection_weight_kind_i(feed_mode_i == 8 ? 2'd0
            : feed_mode_i == 9 ? 2'd1
            : feed_mode_i == 10 ? 2'd2 : 2'd3),
        .projection_weight_row_i(feed_item_i[12:0]),
        .projection_weight_block_i(feed_stage_i[4:0]),
        .projection_weight_q8_block_i(feed_payload_i[271:0]),
        .core_control_valid_i(feed_stage_valid_i && feed_mode_i == 11),
        .core_control_ready_o(attn_core_ready),
        .core_control_done_o(attn_core_done), .import_pause_i(1'b0),
        .projection_output_valid_o(), .projection_output_row_o(),
        .projection_output_q30_o(),
        .result_valid_o(attn_join_result_valid),
        .result_ready_i(result_ready_i && owner == OWNER_ATTN),
        .result_error_o(attn_join_result_error),
        .result_error_code_o(attn_join_error_code),
        .result_handle_o(attn_join_result_handle),
        .output_read_valid_i(1'b0), .output_read_ready_o(),
        .output_read_index_i(10'd0), .output_read_rsp_valid_o(),
        .output_read_rsp_ready_i(1'b1), .output_read_error_o(),
        .output_read_q30_o(),
        .resident_command_valid_o(attn_cmd_valid),
        .resident_command_ready_i(resident_command_ready),
        .resident_command_operation_o(attn_cmd_op),
        .resident_command_source0_handle_o(attn_cmd_s0),
        .resident_command_source1_handle_o(attn_cmd_s1),
        .resident_command_destination_handle_o(attn_cmd_dst),
        .resident_result_valid_i(resident_result_valid),
        .resident_result_ready_o(attn_result_ready),
        .resident_result_error_i(resident_result_error),
        .resident_result_handle_i(resident_result_handle),
        .resident_abort_o(attn_abort),
        .resident_inspect_valid_o(attn_inspect_valid),
        .resident_inspect_ready_i(resident_inspect_ready),
        .resident_inspect_handle_o(attn_inspect_handle),
        .resident_inspect_index_o(attn_inspect_index),
        .resident_inspect_rsp_valid_i(resident_inspect_rsp_valid),
        .resident_inspect_rsp_ready_o(attn_inspect_rsp_ready),
        .resident_inspect_rsp_error_i(resident_inspect_rsp_error),
        .resident_inspect_rsp_data_i(resident_inspect_rsp_data),
        .resident_import_valid_o(attn_import_valid),
        .resident_import_ready_i(resident_import_ready),
        .resident_import_index_o(attn_import_index),
        .resident_import_q30_o(attn_import_q30),
        .query_rows_retired_o(attn_q_rows), .key_rows_retired_o(attn_k_rows),
        .value_rows_retired_o(attn_v_rows),
        .output_rows_retired_o(attn_out_rows),
        .import_elements_completed_o(), .poisoned_o(), .busy_o()
    );

    // ---- FFN join ---------------------------------------------------------
    wire ffn_start_ready, ffn_join_result_valid, ffn_join_result_error;
    wire [7:0] ffn_join_error_code;
    wire [36:0] ffn_join_result_handle;
    wire ffn_row_done, ffn_row_error;
    wire [12:0] ffn_gate_rows;
    wire [10:0] ffn_down_rows;
    wire ffn_row_start = owner == OWNER_FFN && feed_stage_valid_i
        && (feed_mode_i == 13 || feed_mode_i == 14)
        && feed_stage_i == 0 && !ffn_row_started;
    truega_lfm25_resident_ffn_join ffn (
        .clk(clk), .reset_n(reset_n), .clear_i(clear_i), .abort_i(abort_i),
        .start_i(launch_pending && owner == OWNER_FFN),
        .start_ready_o(ffn_start_ready), .source_q8_handle_i(input_q8),
        .destination_q30_handle_i(destination_handle),
        .row_start_i(ffn_row_start), .row_down_i(feed_mode_i == 14),
        .row_index_i(feed_item_i[12:0]), .row_ready_o(ffn_row_ready),
        .expected_row_down_o(), .expected_row_index_o(),
        .weight_valid_i(feed_stage_valid_i && ffn_row_started
            && (feed_mode_i == 14 || (feed_mode_i == 13 && feed_bank_i == 1))),
        .weight_block_index_i(feed_stage_i), .weight0_q8_block_i(
            feed_mode_i == 13 ? ffn_weight0 : feed_payload_i[271:0]),
        .weight1_q8_block_i(feed_payload_i[271:0]),
        .weight_ready_o(ffn_weight_ready), .expected_weight_block_o(),
        .row_done_o(ffn_row_done), .row_error_o(ffn_row_error),
        .row_done_down_o(), .row_done_index_o(), .import_pause_i(1'b0),
        .import_adapter_valid_o(), .import_adapter_index_o(),
        .import_adapter_q30_o(), .result_valid_o(ffn_join_result_valid),
        .result_ready_i(result_ready_i && owner == OWNER_FFN),
        .result_error_o(ffn_join_result_error),
        .result_error_code_o(ffn_join_error_code),
        .result_handle_o(ffn_join_result_handle),
        .output_read_valid_i(1'b0), .output_read_ready_o(),
        .output_read_index_i(10'd0), .output_read_rsp_valid_o(),
        .output_read_rsp_ready_i(1'b1), .output_read_error_o(),
        .output_read_q30_o(),
        .resident_command_valid_o(ffn_cmd_valid),
        .resident_command_ready_i(resident_command_ready),
        .resident_command_operation_o(ffn_cmd_op),
        .resident_command_source0_handle_o(ffn_cmd_s0),
        .resident_command_source1_handle_o(ffn_cmd_s1),
        .resident_command_destination_handle_o(ffn_cmd_dst),
        .resident_result_valid_i(resident_result_valid),
        .resident_result_ready_o(ffn_result_ready),
        .resident_result_error_i(resident_result_error),
        .resident_result_handle_i(resident_result_handle),
        .resident_abort_o(ffn_abort),
        .resident_inspect_valid_o(ffn_inspect_valid),
        .resident_inspect_ready_i(resident_inspect_ready),
        .resident_inspect_handle_o(ffn_inspect_handle),
        .resident_inspect_index_o(ffn_inspect_index),
        .resident_inspect_rsp_valid_i(resident_inspect_rsp_valid),
        .resident_inspect_rsp_ready_o(ffn_inspect_rsp_ready),
        .resident_inspect_rsp_error_i(resident_inspect_rsp_error),
        .resident_inspect_rsp_data_i(resident_inspect_rsp_data),
        .resident_import_valid_o(ffn_import_valid),
        .resident_import_ready_i(resident_import_ready),
        .resident_import_index_o(ffn_import_index),
        .resident_import_q30_o(ffn_import_q30),
        .gate_up_rows_completed_o(ffn_gate_rows),
        .down_rows_completed_o(ffn_down_rows),
        .import_elements_completed_o(), .busy_o()
    );

    // ---- Split final norm / tied head join -------------------------------
    wire tail_start_ready, tail_norm_result_valid;
    wire [36:0] tail_norm_result_handle;
    wire tail_join_result_valid, tail_join_result_error;
    wire [7:0] tail_join_error_code;
    wire [31:0] tail_token;
    wire signed [63:0] tail_score;
    wire [16:0] tail_rows;
    wire tail_lm_row_done, tail_lm_row_error;
    truega_lfm25_resident_decode_tail tail (
        .clk(clk), .reset_n(reset_n), .clear_i(clear_i), .abort_i(abort_i),
        .start_i(launch_pending && owner == OWNER_TAIL
            && active_operation == 8),
        .start_ready_o(tail_start_ready), .source_q30_handle_i(input_q30),
        .normalized_q8_handle_i(destination_handle),
        .norm_weight_valid_i(scalar_to_tail),
        .norm_weight_ready_o(tail_norm_feed_ready),
        .expected_norm_weight_index_o(),
        .norm_weight_index_i({scalar_stage[4:0], scalar_index[4:0]}),
        .norm_weight_format_bf16_i(1'b1),
        .norm_weight_bits_i({16'd0, scalar_bf16}),
        .norm_result_valid_o(tail_norm_result_valid),
        .norm_result_ready_i(result_ready_i && owner == OWNER_TAIL
            && active_operation == 8),
        .norm_result_handle_o(tail_norm_result_handle),
        .lm_weight_valid_i(feed_stage_valid_i && feed_mode_i == 15),
        .lm_weight_ready_o(tail_lm_ready), .expected_lm_row_o(),
        .expected_lm_block_o(), .lm_weight_row_i(feed_item_i),
        .lm_weight_block_i(feed_stage_i[4:0]),
        .lm_weight_q8_block_i(feed_payload_i[271:0]),
        .lm_row_done_o(tail_lm_row_done), .lm_row_error_o(tail_lm_row_error),
        .lm_row_retired_index_o(), .lm_row_score_q30_o(),
        .activation_pause_i(1'b0), .result_valid_o(tail_join_result_valid),
        .result_ready_i(result_ready_i && owner == OWNER_TAIL
            && active_operation == 9),
        .result_error_o(tail_join_result_error),
        .result_error_code_o(tail_join_error_code),
        .result_token_o(tail_token), .result_score_q30_o(tail_score),
        .result_rows_retired_o(tail_rows), .poisoned_o(), .busy_o(),
        .resident_command_valid_o(tail_cmd_valid),
        .resident_command_ready_i(resident_command_ready),
        .resident_command_operation_o(tail_cmd_op),
        .resident_command_source0_handle_o(tail_cmd_s0),
        .resident_command_source1_handle_o(tail_cmd_s1),
        .resident_command_destination_handle_o(tail_cmd_dst),
        .resident_result_valid_i(resident_result_valid),
        .resident_result_ready_o(tail_result_ready),
        .resident_result_error_i(resident_result_error),
        .resident_result_handle_i(resident_result_handle),
        .resident_abort_o(tail_abort),
        .resident_weight_valid_o(tail_weight_valid),
        .resident_weight_ready_i(resident_weight_ready),
        .resident_weight_index_o(tail_weight_index),
        .resident_weight_format_bf16_o(tail_weight_format),
        .resident_weight_bits_o(tail_weight_bits),
        .resident_inspect_valid_o(tail_inspect_valid),
        .resident_inspect_ready_i(resident_inspect_ready),
        .resident_inspect_handle_o(tail_inspect_handle),
        .resident_inspect_index_o(tail_inspect_index),
        .resident_inspect_rsp_valid_i(resident_inspect_rsp_valid),
        .resident_inspect_rsp_ready_o(tail_inspect_rsp_ready),
        .resident_inspect_rsp_error_i(resident_inspect_rsp_error),
        .resident_inspect_rsp_data_i(resident_inspect_rsp_data)
    );

    // The controller keeps owner=TAIL across the explicit FinalRmsNorm result.
    // The next head feed is therefore accepted only after that result ready
    // released the tail's norm barrier.
    wire direct_result_valid = owner == OWNER_DIRECT && resident_result_valid;
    assign result_valid_o = owner == OWNER_DIRECT ? direct_result_valid
        : owner == OWNER_NORM ? norm_join_result_valid
        : owner == OWNER_SHORT ? short_join_result_valid
        : owner == OWNER_ATTN ? attn_join_result_valid
        : owner == OWNER_FFN ? ffn_join_result_valid
        : owner == OWNER_TAIL && active_operation == 8 ? tail_norm_result_valid
        : owner == OWNER_TAIL ? tail_join_result_valid : 1'b0;
    assign result_error_o = owner == OWNER_DIRECT ? resident_result_error
        : owner == OWNER_NORM ? norm_join_result_error
        : owner == OWNER_SHORT ? short_join_result_error
        : owner == OWNER_ATTN ? attn_join_result_error
        : owner == OWNER_FFN ? ffn_join_result_error
        : owner == OWNER_TAIL && active_operation == 8 ? 1'b0
        : tail_join_result_error;
    assign result_error_code_o = owner == OWNER_NORM ? norm_join_error_code
        : owner == OWNER_SHORT ? short_join_error_code
        : owner == OWNER_ATTN ? attn_join_error_code
        : owner == OWNER_FFN ? ffn_join_error_code
        : owner == OWNER_TAIL && active_operation == 9 ? tail_join_error_code
        : 8'd0;
    wire [36:0] selected_handle = owner == OWNER_DIRECT
        ? resident_result_handle : owner == OWNER_NORM ? norm_join_result_handle
        : owner == OWNER_SHORT ? short_join_result_handle
        : owner == OWNER_ATTN ? attn_join_result_handle
        : owner == OWNER_FFN ? ffn_join_result_handle
        : tail_norm_result_handle;
    assign result_slot_o = active_operation == 9 ? 8'hff
        : {4'd0, selected_handle[3:0]};
    assign result_token_o = active_operation == 9 ? tail_token : 0;
    assign result_rows_o = active_operation == 9 ? {15'd0, tail_rows} : 0;
    assign result_score_q30_o = active_operation == 9 ? tail_score : 0;

    assign start_ready_o = owner == OWNER_NONE && operation_i <= 8
        && operation_i != 9;

    reg [7:0] finished_mode;
    reg [31:0] finished_item;
    always @* begin
        feed_item_effect_done_o = 1'b0;
        case (finished_mode)
            0: feed_item_effect_done_o = direct_result_valid;
            1, 2: feed_item_effect_done_o = norm_join_result_valid;
            3: feed_item_effect_done_o = tail_norm_result_valid;
            4: feed_item_effect_done_o = 1'b1;
            5: feed_item_effect_done_o = short_channels > finished_item;
            6: feed_item_effect_done_o = finished_item == 1023
                ? short_join_result_valid : short_rows > finished_item;
            7: feed_item_effect_done_o = scalar_busy == 0;
            8: feed_item_effect_done_o = attn_q_rows > finished_item;
            9: feed_item_effect_done_o = attn_k_rows > finished_item;
            10: feed_item_effect_done_o = attn_v_rows > finished_item;
            11: feed_item_effect_done_o = attn_core_seen;
            12: feed_item_effect_done_o = finished_item == 1023
                ? attn_join_result_valid : attn_out_rows > finished_item;
            13: feed_item_effect_done_o = ffn_gate_rows > finished_item;
            14: feed_item_effect_done_o = finished_item == 1023
                ? ffn_join_result_valid : ffn_down_rows > finished_item;
            15: feed_item_effect_done_o = finished_item == 65535
                ? tail_join_result_valid : tail_rows > finished_item;
            default: feed_item_effect_done_o = 1'b0;
        endcase
    end

    always @(posedge clk) begin
        if (!reset_n || clear_i) begin
            owner <= OWNER_NONE;
            active_operation <= 0;
            active_layer <= 8'hff;
            active_position <= 0;
            active_epoch <= 0;
            active_input_slot <= 8'hff;
            active_residual_slot <= 8'hff;
            active_destination_slot <= 8'hff;
            launch_pending <= 0;
            scalar_busy <= 0;
            scalar_index <= 0;
            scalar_mode <= 0;
            scalar_bank <= 0;
            scalar_stage <= 0;
            scalar_payload <= 0;
            triplet_b <= 0;
            triplet_c <= 0;
            ffn_weight0 <= 0;
            ffn_row_started <= 0;
            finished_mode <= 8'hff;
            finished_item <= 0;
            attn_core_seen <= 0;
        end else begin
            if (start_i && start_ready_o) begin
                active_operation <= operation_i;
                active_layer <= layer_i;
                active_position <= position_i;
                active_epoch <= session_epoch_i;
                active_input_slot <= input_slot_i;
                active_residual_slot <= residual_slot_i;
                active_destination_slot <= destination_slot_i;
                launch_pending <= 1;
                case (operation_i)
                    0: owner <= OWNER_DIRECT;
                    1, 4, 5, 7: owner <= OWNER_NORM;
                    2: owner <= OWNER_SHORT;
                    3: owner <= OWNER_ATTN;
                    6: owner <= OWNER_FFN;
                    8: owner <= OWNER_TAIL;
                    default: owner <= OWNER_NONE;
                endcase
            end
            if (launch_pending && (
                    (owner == OWNER_DIRECT && resident_command_ready)
                    || (owner == OWNER_NORM && norm_start_ready)
                    || (owner == OWNER_SHORT && short_start_ready)
                    || (owner == OWNER_ATTN && attn_start_ready)
                    || (owner == OWNER_FFN && ffn_start_ready)
                    || (owner == OWNER_TAIL && tail_start_ready)))
                launch_pending <= 0;
            // Tail remains owned after its norm boundary; selecting operation
            // nine only changes which held result is exposed.
            if (owner == OWNER_TAIL && active_operation == 8
                    && tail_norm_result_valid && result_ready_i)
                active_operation <= 9;
            else if (result_valid_o && result_ready_i
                    && !(owner == OWNER_TAIL && active_operation == 8))
                owner <= OWNER_NONE;

            if (feed_stage_accept
                    && (feed_mode_i == 1 || feed_mode_i == 2
                        || feed_mode_i == 3 || feed_mode_i == 7)) begin
                scalar_busy <= 1;
                scalar_index <= 0;
                scalar_mode <= feed_mode_i;
                scalar_bank <= feed_bank_i;
                scalar_stage <= feed_stage_i;
                scalar_payload <= feed_payload_i;
            end else if (scalar_accept) begin
                if (scalar_index == 31)
                    scalar_busy <= 0;
                else
                    scalar_index <= scalar_index + 1'b1;
            end

            if (feed_stage_accept && feed_mode_i == 4)
                for (coeff_i = 0; coeff_i < 32; coeff_i = coeff_i + 1)
                    shortconv_coeff[feed_stage_i * 32 + coeff_i]
                        <= feed_payload_i[coeff_i * 16 +: 16];
            if (feed_stage_accept && feed_mode_i == 5 && feed_bank_i == 0)
                triplet_b <= feed_payload_i[271:0];
            if (feed_stage_accept && feed_mode_i == 5 && feed_bank_i == 1)
                triplet_c <= feed_payload_i[271:0];
            if (feed_stage_accept && feed_mode_i == 13 && feed_bank_i == 0)
                ffn_weight0 <= feed_payload_i[271:0];
            if (ffn_row_start && ffn_row_ready)
                ffn_row_started <= 1;
            if (feed_item_finish_i) begin
                finished_mode <= feed_mode_i;
                finished_item <= feed_item_i;
                ffn_row_started <= 0;
            end
            if (attn_core_done)
                attn_core_seen <= 1;
            if (feed_stage_accept && feed_mode_i == 11)
                attn_core_seen <= 0;
        end
    end
endmodule
