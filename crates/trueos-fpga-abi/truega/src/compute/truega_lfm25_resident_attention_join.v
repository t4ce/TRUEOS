// Fixed resident first-token attention join for the sealed LFM2.5 graph.
//
// The source is one typed resident Q8_0[1024] handle.  The fixed feed order is
// Q/K RMSNorm weights (64 + 64 BF16 words), Q projection (1,024 rows), K
// projection (512 rows), V projection (512 rows), one payload-free first-token
// core commit, and the output projection (1,024 rows).  The asymmetric row
// counts are the model's 16:8 GQA contract; widening K/V to 1,024 would be a
// different model, not a transport optimization.
//
// Q/K/V results are retained until the control-only core item commits.  The
// proven position-zero attention slot then consumes exactly 2,048 Q30 values.
// Its 1,024 values are quantized into 32 native Q8_0 blocks and projected into
// an ordered transactional resident Q30 import.  The destination handle is
// published only after all 1,024 values commit.  Abort after attention begins
// poisons this join until clear_i, because the layer-local KV cache may already
// have advanced.  There is no parser, processor, DMA, TLB, or runtime graph.
module truega_lfm25_resident_attention_join (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 clear_i,
    input  wire                 abort_i,

    input  wire                 start_i,
    output wire                 start_ready_o,
    input  wire [36:0]          source_q8_handle_i,
    input  wire [36:0]          destination_q30_handle_i,
    input  wire [3:0]           layer_i,
    input  wire [16:0]          token_position_i,

    input  wire                 norm_weight_valid_i,
    output wire                 norm_weight_ready_o,
    output wire                 norm_weight_key_o,
    output wire [5:0]           norm_weight_element_o,
    input  wire                 norm_weight_key_i,
    input  wire [5:0]           norm_weight_element_i,
    input  wire [15:0]          norm_weight_bf16_i,

    // kind: 0=Q, 1=K, 2=V, 3=output projection.
    input  wire                 projection_weight_valid_i,
    output wire                 projection_weight_ready_o,
    output wire [1:0]           projection_weight_kind_o,
    output wire [12:0]          projection_weight_row_o,
    output wire [4:0]           projection_weight_block_o,
    input  wire [1:0]           projection_weight_kind_i,
    input  wire [12:0]          projection_weight_row_i,
    input  wire [4:0]           projection_weight_block_i,
    input  wire [271:0]         projection_weight_q8_block_i,

    input  wire                 core_control_valid_i,
    output wire                 core_control_ready_o,
    output reg                  core_control_done_o,

    input  wire                 import_pause_i,
    output wire                 projection_output_valid_o,
    output wire [12:0]          projection_output_row_o,
    output wire signed [63:0]   projection_output_q30_o,

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

    output wire [10:0]          query_rows_retired_o,
    output wire [9:0]           key_rows_retired_o,
    output wire [9:0]           value_rows_retired_o,
    output wire [12:0]          output_rows_retired_o,
    output reg  [10:0]          import_elements_completed_o,
    output reg                  poisoned_o,
    output wire                 busy_o
);
    localparam [5:0] ST_IDLE          = 6'd0;
    localparam [5:0] ST_ACT_REQUEST   = 6'd1;
    localparam [5:0] ST_ACT_REPLY     = 6'd2;
    localparam [5:0] ST_ABORT_INSPECT = 6'd3;
    localparam [5:0] ST_NORM          = 6'd4;
    localparam [5:0] ST_Q_RESET       = 6'd5;
    localparam [5:0] ST_Q_START       = 6'd6;
    localparam [5:0] ST_Q_ACT         = 6'd7;
    localparam [5:0] ST_Q_PROJECT     = 6'd8;
    localparam [5:0] ST_K_RESET       = 6'd9;
    localparam [5:0] ST_K_START       = 6'd10;
    localparam [5:0] ST_K_ACT         = 6'd11;
    localparam [5:0] ST_K_PROJECT     = 6'd12;
    localparam [5:0] ST_V_RESET       = 6'd13;
    localparam [5:0] ST_V_START       = 6'd14;
    localparam [5:0] ST_V_ACT         = 6'd15;
    localparam [5:0] ST_V_PROJECT     = 6'd16;
    localparam [5:0] ST_CORE_WAIT     = 6'd17;
    localparam [5:0] ST_CORE_START    = 6'd18;
    localparam [5:0] ST_CORE_READ     = 6'd19;
    localparam [5:0] ST_CORE_FEED     = 6'd20;
    localparam [5:0] ST_QUANT_START   = 6'd21;
    localparam [5:0] ST_QUANT_FEED    = 6'd22;
    localparam [5:0] ST_QUANT_WAIT    = 6'd23;
    localparam [5:0] ST_OUT_RESET     = 6'd24;
    localparam [5:0] ST_OUT_START     = 6'd25;
    localparam [5:0] ST_OUT_ACT       = 6'd26;
    localparam [5:0] ST_IMPORT_CMD    = 6'd27;
    localparam [5:0] ST_OUT_PROJECT   = 6'd28;
    localparam [5:0] ST_RESULT        = 6'd29;

    localparam [1:0] OP_IMPORT_Q30 = 2'd3;
    localparam [7:0] ERROR_HANDLE     = 8'd1;
    localparam [7:0] ERROR_LAYER      = 8'd2;
    localparam [7:0] ERROR_INSPECT    = 8'd3;
    localparam [7:0] ERROR_NORM       = 8'd4;
    localparam [7:0] ERROR_PROJECTION = 8'd5;
    localparam [7:0] ERROR_CORE       = 8'd6;
    localparam [7:0] ERROR_QUANTIZE   = 8'd7;
    localparam [7:0] ERROR_IMPORT     = 8'd8;
    localparam [7:0] ERROR_ABORT      = 8'd9;

    reg [5:0] state;
    reg [36:0] source_handle;
    reg [36:0] destination_handle;
    reg [3:0] active_layer;
    reg [16:0] active_position;
    reg [5:0] activation_block;
    reg [7:0] norm_count;
    reg [5:0] projection_activation_count;
    reg [271:0] source_activation_memory [0:31];
    reg signed [63:0] projected_memory [0:2047];
    reg [271:0] attention_q8_memory [0:31];
    reg [10:0] core_feed_index;
    reg signed [63:0] core_feed_data;
    reg [10:0] attention_values_accepted;
    reg [5:0] attention_blocks_completed;
    reg core_done_seen;
    reg core_error_seen;
    reg norm_failed;
    reg projection_failed;
    reg output_failed;
    reg abort_seen;

    wire joined_reset_n = reset_n && !clear_i;
    wire source_shape_valid = source_q8_handle_i[36:5] != 32'd0
        && source_q8_handle_i[4] && source_q8_handle_i[3:0] < 4'd4;
    wire destination_shape_valid = destination_q30_handle_i[36:5] != 32'd0
        && !destination_q30_handle_i[4]
        && destination_q30_handle_i[3:0] < 4'd4;
    wire handles_valid = source_shape_valid && destination_shape_valid
        && source_q8_handle_i[36:5] == destination_q30_handle_i[36:5];
    wire layer_valid = layer_i == 4'd2 || layer_i == 4'd5
        || layer_i == 4'd8 || layer_i == 4'd10
        || layer_i == 4'd12 || layer_i == 4'd14;
    wire external_read_allowed = state == ST_IDLE || state == ST_RESULT;

    assign start_ready_o = state == ST_IDLE && !poisoned_o
        && !output_read_valid_i && resident_inspect_ready_i
        && !resident_inspect_rsp_valid_i;
    assign result_valid_o = state == ST_RESULT;
    assign busy_o = state != ST_IDLE && state != ST_RESULT;

    // The source is inspected once.  All four fixed projections share these
    // 32 blocks logically; separate standalone engines keep each proof local.
    assign resident_inspect_valid_o = (state == ST_ACT_REQUEST && !abort_i)
        || (external_read_allowed && output_read_valid_i);
    assign resident_inspect_handle_o = state == ST_ACT_REQUEST
        ? source_handle : destination_handle;
    assign resident_inspect_index_o = state == ST_ACT_REQUEST
        ? {4'd0, activation_block} : output_read_index_i;
    assign resident_inspect_rsp_ready_o = state == ST_ABORT_INSPECT ? 1'b1
        : state == ST_ACT_REPLY ? 1'b1
        : external_read_allowed && output_read_rsp_ready_i;
    assign output_read_ready_o = external_read_allowed
        && resident_inspect_ready_i;
    assign output_read_rsp_valid_o = external_read_allowed
        && resident_inspect_rsp_valid_i;
    assign output_read_error_o = resident_inspect_rsp_error_i;
    assign output_read_q30_o = resident_inspect_rsp_data_i[63:0];

    // Q/K normalization weights are a strict Q[0..63], K[0..63] stream.
    wire attention_norm_ready;
    wire attention_norm_error;
    wire attention_norm_loaded;
    assign norm_weight_key_o = norm_count >= 8'd64;
    assign norm_weight_element_o = norm_count[5:0];
    assign norm_weight_ready_o = state == ST_NORM && attention_norm_ready
        && !abort_i && !norm_failed;
    wire norm_accept = norm_weight_valid_i && norm_weight_ready_o;
    wire norm_sequence_valid = norm_weight_key_i == norm_weight_key_o
        && norm_weight_element_i == norm_weight_element_o;

    // Four exact row engines.  Q/K/V write a single 2,048-word staging RAM;
    // the output engine streams directly into the transactional resident port.
    wire q_reset_ready, q_reset_done, q_start_ready, q_act_ready;
    wire [4:0] q_act_index;
    wire q_weight_ready, q_result_valid, q_result_ready;
    wire [12:0] q_weight_row, q_result_row;
    wire [4:0] q_weight_block;
    wire signed [63:0] q_result;
    wire q_done, q_error, q_poison;
    wire [12:0] q_rows;
    wire q_abort = abort_i || projection_failed;
    wire q_kind_ok = projection_weight_kind_i == 2'd0;

    truega_lfm25_q8_projection_row_engine #(.ROW_COUNT(1024)) q_projection (
        .clk(clk), .reset_n(joined_reset_n), .abort_i(q_abort),
        .state_reset_i(state == ST_Q_RESET),
        .state_reset_ready_o(q_reset_ready), .state_reset_done_o(q_reset_done),
        .start_i(state == ST_Q_START), .start_ready_o(q_start_ready),
        .activation_valid_i(state == ST_Q_ACT),
        .activation_ready_o(q_act_ready),
        .activation_block_index_o(q_act_index),
        .activation_block_index_i(projection_activation_count[4:0]),
        .activation_q8_block_i(source_activation_memory[projection_activation_count[4:0]]),
        .weight_valid_i(state == ST_Q_PROJECT && projection_weight_valid_i
            && q_kind_ok && !abort_i && !projection_failed),
        .weight_ready_o(q_weight_ready),
        .weight_row_index_o(q_weight_row),
        .weight_block_index_o(q_weight_block),
        .weight_row_index_i(projection_weight_row_i),
        .weight_block_index_i(projection_weight_block_i),
        .weight_q8_block_i(projection_weight_q8_block_i),
        .result_valid_o(q_result_valid), .result_ready_i(q_result_ready),
        .result_row_index_o(q_result_row), .result_q30_o(q_result),
        .result_first_o(), .result_last_o(), .busy_o(), .done_o(q_done),
        .error_o(q_error), .poisoned_o(q_poison), .error_code_o(),
        .rows_retired_o(q_rows)
    );
    assign q_result_ready = state == ST_Q_PROJECT && !abort_i
        && !projection_failed;
    assign query_rows_retired_o = q_rows[10:0];

    wire k_reset_ready, k_reset_done, k_start_ready, k_act_ready;
    wire [4:0] k_act_index;
    wire k_weight_ready, k_result_valid, k_result_ready;
    wire [12:0] k_weight_row, k_result_row;
    wire [4:0] k_weight_block;
    wire signed [63:0] k_result;
    wire k_done, k_error, k_poison;
    wire [12:0] k_rows;
    wire k_kind_ok = projection_weight_kind_i == 2'd1;

    truega_lfm25_q8_projection_row_engine #(.ROW_COUNT(512)) k_projection (
        .clk(clk), .reset_n(joined_reset_n), .abort_i(q_abort),
        .state_reset_i(state == ST_K_RESET),
        .state_reset_ready_o(k_reset_ready), .state_reset_done_o(k_reset_done),
        .start_i(state == ST_K_START), .start_ready_o(k_start_ready),
        .activation_valid_i(state == ST_K_ACT),
        .activation_ready_o(k_act_ready), .activation_block_index_o(k_act_index),
        .activation_block_index_i(projection_activation_count[4:0]),
        .activation_q8_block_i(source_activation_memory[projection_activation_count[4:0]]),
        .weight_valid_i(state == ST_K_PROJECT && projection_weight_valid_i
            && k_kind_ok && !abort_i && !projection_failed),
        .weight_ready_o(k_weight_ready), .weight_row_index_o(k_weight_row),
        .weight_block_index_o(k_weight_block),
        .weight_row_index_i(projection_weight_row_i),
        .weight_block_index_i(projection_weight_block_i),
        .weight_q8_block_i(projection_weight_q8_block_i),
        .result_valid_o(k_result_valid), .result_ready_i(k_result_ready),
        .result_row_index_o(k_result_row), .result_q30_o(k_result),
        .result_first_o(), .result_last_o(), .busy_o(), .done_o(k_done),
        .error_o(k_error), .poisoned_o(k_poison), .error_code_o(),
        .rows_retired_o(k_rows)
    );
    assign k_result_ready = state == ST_K_PROJECT && !abort_i
        && !projection_failed;
    assign key_rows_retired_o = k_rows[9:0];

    wire v_reset_ready, v_reset_done, v_start_ready, v_act_ready;
    wire [4:0] v_act_index;
    wire v_weight_ready, v_result_valid, v_result_ready;
    wire [12:0] v_weight_row, v_result_row;
    wire [4:0] v_weight_block;
    wire signed [63:0] v_result;
    wire v_done, v_error, v_poison;
    wire [12:0] v_rows;
    wire v_kind_ok = projection_weight_kind_i == 2'd2;

    truega_lfm25_q8_projection_row_engine #(.ROW_COUNT(512)) v_projection (
        .clk(clk), .reset_n(joined_reset_n), .abort_i(q_abort),
        .state_reset_i(state == ST_V_RESET),
        .state_reset_ready_o(v_reset_ready), .state_reset_done_o(v_reset_done),
        .start_i(state == ST_V_START), .start_ready_o(v_start_ready),
        .activation_valid_i(state == ST_V_ACT),
        .activation_ready_o(v_act_ready), .activation_block_index_o(v_act_index),
        .activation_block_index_i(projection_activation_count[4:0]),
        .activation_q8_block_i(source_activation_memory[projection_activation_count[4:0]]),
        .weight_valid_i(state == ST_V_PROJECT && projection_weight_valid_i
            && v_kind_ok && !abort_i && !projection_failed),
        .weight_ready_o(v_weight_ready), .weight_row_index_o(v_weight_row),
        .weight_block_index_o(v_weight_block),
        .weight_row_index_i(projection_weight_row_i),
        .weight_block_index_i(projection_weight_block_i),
        .weight_q8_block_i(projection_weight_q8_block_i),
        .result_valid_o(v_result_valid), .result_ready_i(v_result_ready),
        .result_row_index_o(v_result_row), .result_q30_o(v_result),
        .result_first_o(), .result_last_o(), .busy_o(), .done_o(v_done),
        .error_o(v_error), .poisoned_o(v_poison), .error_code_o(),
        .rows_retired_o(v_rows)
    );
    assign v_result_ready = state == ST_V_PROJECT && !abort_i
        && !projection_failed;
    assign value_rows_retired_o = v_rows[9:0];

    wire out_reset_ready, out_reset_done, out_start_ready, out_act_ready;
    wire [4:0] out_act_index;
    wire out_weight_ready, out_result_valid, out_result_ready;
    wire [12:0] out_weight_row, out_result_row;
    wire [4:0] out_weight_block;
    wire signed [63:0] out_result;
    wire out_done, out_error, out_poison;
    wire [12:0] out_rows;
    wire out_kind_ok = projection_weight_kind_i == 2'd3;
    wire out_abort = abort_i || output_failed;

    truega_lfm25_q8_projection_row_engine #(.ROW_COUNT(1024)) output_projection (
        .clk(clk), .reset_n(joined_reset_n), .abort_i(out_abort),
        .state_reset_i(state == ST_OUT_RESET),
        .state_reset_ready_o(out_reset_ready), .state_reset_done_o(out_reset_done),
        .start_i(state == ST_OUT_START), .start_ready_o(out_start_ready),
        .activation_valid_i(state == ST_OUT_ACT),
        .activation_ready_o(out_act_ready), .activation_block_index_o(out_act_index),
        .activation_block_index_i(projection_activation_count[4:0]),
        .activation_q8_block_i(attention_q8_memory[projection_activation_count[4:0]]),
        .weight_valid_i(state == ST_OUT_PROJECT && projection_weight_valid_i
            && out_kind_ok && !abort_i && !output_failed
            && resident_import_ready_i && !import_pause_i),
        .weight_ready_o(out_weight_ready), .weight_row_index_o(out_weight_row),
        .weight_block_index_o(out_weight_block),
        .weight_row_index_i(projection_weight_row_i),
        .weight_block_index_i(projection_weight_block_i),
        .weight_q8_block_i(projection_weight_q8_block_i),
        .result_valid_o(out_result_valid), .result_ready_i(out_result_ready),
        .result_row_index_o(out_result_row), .result_q30_o(out_result),
        .result_first_o(), .result_last_o(), .busy_o(), .done_o(out_done),
        .error_o(out_error), .poisoned_o(out_poison), .error_code_o(),
        .rows_retired_o(out_rows)
    );
    assign output_rows_retired_o = out_rows;

    wire q_stage = state == ST_Q_PROJECT;
    wire k_stage = state == ST_K_PROJECT;
    wire v_stage = state == ST_V_PROJECT;
    wire out_stage = state == ST_OUT_PROJECT;
    assign projection_weight_kind_o = q_stage ? 2'd0
        : k_stage ? 2'd1 : v_stage ? 2'd2 : 2'd3;
    assign projection_weight_row_o = q_stage ? q_weight_row
        : k_stage ? k_weight_row : v_stage ? v_weight_row : out_weight_row;
    assign projection_weight_block_o = q_stage ? q_weight_block
        : k_stage ? k_weight_block : v_stage ? v_weight_block : out_weight_block;
    assign projection_weight_ready_o = !abort_i && (
        (q_stage && q_weight_ready) || (k_stage && k_weight_ready)
        || (v_stage && v_weight_ready)
        || (out_stage && out_weight_ready && resident_import_ready_i
            && !import_pause_i));
    wire projection_weight_accept = projection_weight_valid_i
        && projection_weight_ready_o;
    wire projection_kind_expected = projection_weight_kind_i
        == projection_weight_kind_o;

    // Buffer the fixed Q/K/V projected vectors.  The control item starts the
    // core only after all exact GQA row counts retire.
    always @(posedge clk) begin
        if (q_result_valid && q_result_ready)
            projected_memory[q_result_row[9:0]] <= q_result;
        if (k_result_valid && k_result_ready)
            projected_memory[11'd1024 + k_result_row[8:0]] <= k_result;
        if (v_result_valid && v_result_ready)
            projected_memory[11'd1536 + v_result_row[8:0]] <= v_result;
    end

    assign core_control_ready_o = state == ST_CORE_WAIT && !abort_i
        && !poisoned_o;
    wire core_control_accept = core_control_valid_i && core_control_ready_o;
    wire attention_projected_ready;
    wire attention_valid;
    wire attention_ready;
    wire [9:0] attention_index;
    wire signed [63:0] attention_q30;
    wire attention_last;
    wire attention_busy;
    wire attention_done;
    wire attention_error;
    wire [16:0] attention_valid_positions;

    truega_lfm25_attention_first_token_slot attention (
        .clk(clk), .reset_n(joined_reset_n),
        .norm_weight_valid_i(state == ST_NORM && norm_weight_valid_i
            && norm_sequence_valid),
        .norm_weight_ready_o(attention_norm_ready),
        .norm_weight_key_i(norm_weight_key_i),
        .norm_weight_element_i(norm_weight_element_i),
        .norm_weight_format_bf16_i(1'b1),
        .norm_weight_bits_i({16'd0, norm_weight_bf16_i}),
        .norm_weights_loaded_o(attention_norm_loaded),
        .norm_weight_error_o(attention_norm_error),
        .start_i(state == ST_CORE_START), .start_ready_o(),
        .layer_i(active_layer), .position_i(active_position),
        .projected_valid_i(state == ST_CORE_FEED),
        .projected_last_i(core_feed_index == 11'd2047),
        .projected_q30_i(core_feed_data),
        .projected_ready_o(attention_projected_ready),
        .attention_valid_o(attention_valid),
        .attention_ready_i(attention_ready),
        .attention_index_o(attention_index),
        .attention_q30_o(attention_q30),
        .attention_last_o(attention_last),
        .busy_o(attention_busy), .done_o(attention_done),
        .error_o(attention_error),
        .valid_positions_o(attention_valid_positions)
    );

    // Exact 32-value blocks bridge attention Q30 output to the Q8 output GEMV.
    wire quant_sample_ready, quant_busy, quant_done, quant_error;
    wire [5:0] quant_samples;
    wire [271:0] quant_block;
    assign attention_ready = state == ST_QUANT_FEED && quant_sample_ready
        && !abort_i && !poisoned_o;
    truega_q30_to_q8_0_block_slot quantizer (
        .clk(clk), .reset_n(joined_reset_n && !poisoned_o),
        .start_i(state == ST_QUANT_START),
        .sample_valid_i(state == ST_QUANT_FEED && attention_valid),
        .sample_q30_i(attention_q30), .sample_ready_o(quant_sample_ready),
        .busy_o(quant_busy), .done_o(quant_done), .error_o(quant_error),
        .samples_accepted_o(quant_samples), .q8_block_o(quant_block)
    );

    assign resident_command_valid_o = state == ST_IMPORT_CMD && !abort_i;
    assign resident_command_operation_o = OP_IMPORT_Q30;
    assign resident_command_source0_handle_o = 37'd0;
    assign resident_command_source1_handle_o = 37'd0;
    assign resident_command_destination_handle_o = destination_handle;
    assign resident_result_ready_o = state == ST_OUT_PROJECT;
    assign resident_abort_o = state == ST_OUT_PROJECT
        && (abort_i || output_failed || out_error || out_poison);
    assign resident_import_valid_o = state == ST_OUT_PROJECT
        && out_result_valid && !abort_i && !output_failed
        && !import_pause_i;
    assign resident_import_index_o = out_result_row[9:0];
    assign resident_import_q30_o = out_result;
    wire resident_import_accept = resident_import_valid_o
        && resident_import_ready_i;
    assign out_result_ready = state == ST_OUT_PROJECT
        && resident_import_ready_i && !abort_i && !output_failed
        && !import_pause_i;
    assign projection_output_valid_o = state == ST_OUT_PROJECT
        && out_result_valid && !abort_i && !output_failed;
    assign projection_output_row_o = out_result_row;
    assign projection_output_q30_o = out_result;

    task automatic fail_before_import;
        input [7:0] code;
        begin
            result_error_o <= 1'b1;
            result_error_code_o <= code;
            result_handle_o <= 37'd0;
            poisoned_o <= 1'b1;
            state <= ST_RESULT;
        end
    endtask

    always @(posedge clk) begin
        if (!joined_reset_n) begin
            state <= ST_IDLE;
            source_handle <= 37'd0;
            destination_handle <= 37'd0;
            active_layer <= 4'd0;
            active_position <= 17'd0;
            activation_block <= 6'd0;
            norm_count <= 8'd0;
            projection_activation_count <= 6'd0;
            core_feed_index <= 11'd0;
            core_feed_data <= 64'sd0;
            attention_values_accepted <= 11'd0;
            attention_blocks_completed <= 6'd0;
            core_done_seen <= 1'b0;
            core_error_seen <= 1'b0;
            norm_failed <= 1'b0;
            projection_failed <= 1'b0;
            output_failed <= 1'b0;
            abort_seen <= 1'b0;
            import_elements_completed_o <= 11'd0;
            core_control_done_o <= 1'b0;
            poisoned_o <= 1'b0;
            result_error_o <= 1'b0;
            result_error_code_o <= 8'd0;
            result_handle_o <= 37'd0;
        end else begin
            core_control_done_o <= 1'b0;
            if (attention_done)
                core_done_seen <= 1'b1;
            if (attention_error || attention_norm_error) begin
                core_error_seen <= attention_error || core_error_seen;
                norm_failed <= attention_norm_error || norm_failed;
            end
            if (resident_import_accept)
                import_elements_completed_o
                    <= import_elements_completed_o + 11'd1;

            case (state)
                ST_IDLE: begin
                    if (start_i && start_ready_o) begin
                        source_handle <= source_q8_handle_i;
                        destination_handle <= destination_q30_handle_i;
                        active_layer <= layer_i;
                        active_position <= token_position_i;
                        activation_block <= 6'd0;
                        norm_count <= 8'd0;
                        projection_activation_count <= 6'd0;
                        core_feed_index <= 11'd0;
                        attention_values_accepted <= 11'd0;
                        attention_blocks_completed <= 6'd0;
                        core_done_seen <= 1'b0;
                        core_error_seen <= 1'b0;
                        norm_failed <= 1'b0;
                        projection_failed <= 1'b0;
                        output_failed <= 1'b0;
                        abort_seen <= 1'b0;
                        import_elements_completed_o <= 11'd0;
                        result_error_o <= 1'b0;
                        result_error_code_o <= 8'd0;
                        result_handle_o <= 37'd0;
                        if (!handles_valid)
                            fail_before_import(ERROR_HANDLE);
                        else if (!layer_valid || token_position_i != 17'd0)
                            fail_before_import(ERROR_LAYER);
                        else
                            state <= ST_ACT_REQUEST;
                    end
                end

                ST_ACT_REQUEST: begin
                    if (abort_i) begin
                        fail_before_import(ERROR_ABORT);
                    end else if (resident_inspect_valid_o
                            && resident_inspect_ready_i)
                        state <= ST_ACT_REPLY;
                end

                ST_ACT_REPLY: begin
                    if (abort_i) begin
                        poisoned_o <= 1'b1;
                        state <= resident_inspect_rsp_valid_i
                            ? ST_RESULT : ST_ABORT_INSPECT;
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                    end else if (resident_inspect_rsp_valid_i) begin
                        if (resident_inspect_rsp_error_i)
                            fail_before_import(ERROR_INSPECT);
                        else begin
                            source_activation_memory[activation_block[4:0]]
                                <= resident_inspect_rsp_data_i;
                            if (activation_block == 6'd31)
                                state <= ST_NORM;
                            else begin
                                activation_block <= activation_block + 6'd1;
                                state <= ST_ACT_REQUEST;
                            end
                        end
                    end
                end

                ST_ABORT_INSPECT: begin
                    if (resident_inspect_rsp_valid_i) begin
                        result_error_o <= 1'b1;
                        result_error_code_o <= ERROR_ABORT;
                        result_handle_o <= 37'd0;
                        state <= ST_RESULT;
                    end
                end

                ST_NORM: begin
                    if (abort_i)
                        fail_before_import(ERROR_ABORT);
                    else if (norm_failed || attention_norm_error)
                        fail_before_import(ERROR_NORM);
                    else if (norm_accept) begin
                        if (!norm_sequence_valid)
                            fail_before_import(ERROR_NORM);
                        else if (norm_count == 8'd127) begin
                            norm_count <= 8'd128;
                            state <= ST_Q_RESET;
                        end else
                            norm_count <= norm_count + 8'd1;
                    end
                end

                ST_Q_RESET: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (norm_failed || !attention_norm_loaded)
                        fail_before_import(ERROR_NORM);
                    else if (q_reset_ready) begin
                        projection_activation_count <= 6'd0;
                        state <= ST_Q_START;
                    end
                end
                ST_Q_START: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (q_start_ready) state <= ST_Q_ACT;
                end
                ST_Q_ACT: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (q_act_ready) begin
                        if (q_act_index != projection_activation_count[4:0])
                            fail_before_import(ERROR_PROJECTION);
                        else if (projection_activation_count == 6'd31)
                            state <= ST_Q_PROJECT;
                        else projection_activation_count
                            <= projection_activation_count + 6'd1;
                    end
                end
                ST_Q_PROJECT: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (projection_weight_accept && !projection_kind_expected) begin
                        projection_failed <= 1'b1;
                        fail_before_import(ERROR_PROJECTION);
                    end else if (q_done) begin
                        if (q_error || q_poison || q_rows != 13'd1024)
                            fail_before_import(ERROR_PROJECTION);
                        else state <= ST_K_RESET;
                    end
                end

                ST_K_RESET: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (k_reset_ready) begin
                        projection_activation_count <= 6'd0;
                        state <= ST_K_START;
                    end
                end
                ST_K_START: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (k_start_ready) state <= ST_K_ACT;
                end
                ST_K_ACT: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (k_act_ready) begin
                        if (k_act_index != projection_activation_count[4:0])
                            fail_before_import(ERROR_PROJECTION);
                        else if (projection_activation_count == 6'd31)
                            state <= ST_K_PROJECT;
                        else projection_activation_count
                            <= projection_activation_count + 6'd1;
                    end
                end
                ST_K_PROJECT: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (projection_weight_accept && !projection_kind_expected) begin
                        projection_failed <= 1'b1;
                        fail_before_import(ERROR_PROJECTION);
                    end else if (k_done) begin
                        if (k_error || k_poison || k_rows != 13'd512)
                            fail_before_import(ERROR_PROJECTION);
                        else state <= ST_V_RESET;
                    end
                end

                ST_V_RESET: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (v_reset_ready) begin
                        projection_activation_count <= 6'd0;
                        state <= ST_V_START;
                    end
                end
                ST_V_START: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (v_start_ready) state <= ST_V_ACT;
                end
                ST_V_ACT: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (v_act_ready) begin
                        if (v_act_index != projection_activation_count[4:0])
                            fail_before_import(ERROR_PROJECTION);
                        else if (projection_activation_count == 6'd31)
                            state <= ST_V_PROJECT;
                        else projection_activation_count
                            <= projection_activation_count + 6'd1;
                    end
                end
                ST_V_PROJECT: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (projection_weight_accept && !projection_kind_expected) begin
                        projection_failed <= 1'b1;
                        fail_before_import(ERROR_PROJECTION);
                    end else if (v_done) begin
                        if (v_error || v_poison || v_rows != 13'd512)
                            fail_before_import(ERROR_PROJECTION);
                        else state <= ST_CORE_WAIT;
                    end
                end

                ST_CORE_WAIT: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (core_control_accept) begin
                        core_feed_index <= 11'd0;
                        core_done_seen <= 1'b0;
                        core_error_seen <= 1'b0;
                        state <= ST_CORE_START;
                    end
                end
                ST_CORE_START: state <= ST_CORE_READ;
                ST_CORE_READ: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else begin
                        core_feed_data <= projected_memory[core_feed_index];
                        state <= ST_CORE_FEED;
                    end
                end
                ST_CORE_FEED: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (attention_projected_ready) begin
                        if (core_feed_index == 11'd2047) begin
                            attention_values_accepted <= 11'd0;
                            attention_blocks_completed <= 6'd0;
                            state <= ST_QUANT_START;
                        end else begin
                            core_feed_index <= core_feed_index + 11'd1;
                            state <= ST_CORE_READ;
                        end
                    end
                end

                ST_QUANT_START: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (core_error_seen) fail_before_import(ERROR_CORE);
                    else state <= ST_QUANT_FEED;
                end
                ST_QUANT_FEED: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (core_error_seen) fail_before_import(ERROR_CORE);
                    else if (attention_valid && attention_ready) begin
                        if (attention_index != attention_values_accepted[9:0]
                                || attention_last
                                    != (attention_values_accepted == 11'd1023))
                            fail_before_import(ERROR_CORE);
                        else begin
                            attention_values_accepted
                                <= attention_values_accepted + 11'd1;
                            if (attention_values_accepted[4:0] == 5'd31)
                                state <= ST_QUANT_WAIT;
                        end
                    end
                end
                ST_QUANT_WAIT: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (core_error_seen) fail_before_import(ERROR_CORE);
                    else if (quant_done) begin
                        if (quant_error || quant_samples != 6'd32)
                            fail_before_import(ERROR_QUANTIZE);
                        else begin
                            attention_q8_memory[attention_blocks_completed[4:0]]
                                <= quant_block;
                            if (attention_blocks_completed == 6'd31) begin
                                if (!core_done_seen && !attention_done)
                                    fail_before_import(ERROR_CORE);
                                else begin
                                    core_control_done_o <= 1'b1;
                                    state <= ST_OUT_RESET;
                                end
                            end else begin
                                attention_blocks_completed
                                    <= attention_blocks_completed + 6'd1;
                                state <= ST_QUANT_START;
                            end
                        end
                    end
                end

                ST_OUT_RESET: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (out_reset_ready) begin
                        projection_activation_count <= 6'd0;
                        state <= ST_OUT_START;
                    end
                end
                ST_OUT_START: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (out_start_ready) state <= ST_OUT_ACT;
                end
                ST_OUT_ACT: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (out_act_ready) begin
                        if (out_act_index != projection_activation_count[4:0])
                            fail_before_import(ERROR_PROJECTION);
                        else if (projection_activation_count == 6'd31)
                            state <= ST_IMPORT_CMD;
                        else projection_activation_count
                            <= projection_activation_count + 6'd1;
                    end
                end
                ST_IMPORT_CMD: begin
                    if (abort_i) fail_before_import(ERROR_ABORT);
                    else if (resident_command_valid_o
                            && resident_command_ready_i) begin
                        import_elements_completed_o <= 11'd0;
                        state <= ST_OUT_PROJECT;
                    end
                end
                ST_OUT_PROJECT: begin
                    if (abort_i) begin
                        output_failed <= 1'b1;
                        abort_seen <= 1'b1;
                        poisoned_o <= 1'b1;
                    end
                    if (projection_weight_accept && !projection_kind_expected) begin
                        output_failed <= 1'b1;
                        poisoned_o <= 1'b1;
                    end
                    if (out_done && (out_error || out_poison)) begin
                        output_failed <= 1'b1;
                        poisoned_o <= 1'b1;
                    end
                    if (resident_result_valid_i && resident_result_ready_o) begin
                        if (abort_i || output_failed || out_error || out_poison) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= abort_i || abort_seen
                                ? ERROR_ABORT : ERROR_PROJECTION;
                            result_handle_o <= 37'd0;
                            poisoned_o <= 1'b1;
                        end else if (resident_result_error_i
                                || resident_result_handle_i != destination_handle
                                || out_rows != 13'd1024
                                || import_elements_completed_o != 11'd1024) begin
                            result_error_o <= 1'b1;
                            result_error_code_o <= ERROR_IMPORT;
                            result_handle_o <= 37'd0;
                            poisoned_o <= 1'b1;
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
                default: fail_before_import(ERROR_IMPORT);
            endcase
        end
    end

    // These witnesses make accidental replacement of GQA with MHA visible in
    // lint and simulation without making dimensions runtime programmable.
    wire unused_observability = q_reset_done ^ k_reset_done ^ v_reset_done
        ^ out_reset_done ^ attention_busy ^ attention_valid_positions[0]
        ^ quant_busy ^ core_control_accept;
endmodule
