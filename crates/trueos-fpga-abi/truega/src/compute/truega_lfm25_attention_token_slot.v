// Fixed LFM2.5 one-token attention path after Q/K/V projection.
//
// Input order is exactly Q[16][64], K[8][64], V[8][64], all signed Q30.
// The slot performs per-head Q/K RMSNorm, NEOX RoPE, per-layer KV append,
// 16:8 GQA scaled-dot attention, stable online softmax/value accumulation,
// and streams 1024 Q30 values for the existing Q8 output-GEMV boundary.
//
// norm_* and rope_* are fixed FPGA-local model-ROM lookup ports.  They are
// not host/runtime operation inputs.  Production cache storage is selected by
// EXTERNAL_CACHE=1 and binds cache_* directly to the board-local DDR
// controller.  EXTERNAL_CACHE=0 infers parameterized RAM for focused tests.
module truega_lfm25_attention_token_slot #(
    parameter integer CACHE_POSITIONS = 16384,
    parameter integer EXTERNAL_CACHE = 1,
    // The general/external-cache circuit retains an independent KV history
    // for each of the six attention layers.  A deliberately position-zero-only
    // wrapper may set this to one: no accepted transaction can revisit cached
    // payload from an earlier layer, while valid_positions[] still enforces
    // each layer's independent one-shot state.
    parameter integer CACHE_LAYER_SLOTS = 6,
    parameter integer CACHE_TOTAL_WORDS =
        CACHE_LAYER_SLOTS * CACHE_POSITIONS * 8 * 64 * 2
) (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire [3:0]          layer_i,
    input  wire [16:0]         position_i,
    input  wire                projected_valid_i,
    input  wire                projected_last_i,
    input  wire signed [63:0]  projected_q30_i,
    output wire                projected_ready_o,

    output wire                norm_req_valid_o,
    output wire                norm_req_key_o,
    output wire [5:0]          norm_req_element_o,
    input  wire                norm_rsp_valid_i,
    input  wire signed [63:0]  norm_rsp_weight_q30_i,
    output wire                rope_req_valid_o,
    output wire [16:0]         rope_req_position_o,
    output wire [4:0]          rope_req_pair_o,
    input  wire                rope_rsp_valid_i,
    input  wire signed [63:0]  rope_rsp_cos_q30_i,
    input  wire signed [63:0]  rope_rsp_sin_q30_i,

    output wire                cache_req_valid_o,
    output wire                cache_req_write_o,
    output wire [29:0]         cache_req_word_address_o,
    output wire signed [63:0]  cache_req_write_q30_o,
    input  wire                cache_req_ready_i,
    input  wire                cache_rsp_valid_i,
    input  wire signed [63:0]  cache_rsp_read_q30_i,

    output wire                attention_valid_o,
    input  wire                attention_ready_i,
    output wire [9:0]          attention_index_o,
    output wire signed [63:0]  attention_q30_o,
    output wire                attention_last_o,
    output reg                 busy_o,
    output reg                 done_o,
    output reg                 error_o,
    output reg [16:0]          valid_positions_o
);
    localparam [4:0] ST_IDLE          = 5'd0;
    localparam [4:0] ST_INGEST        = 5'd1;
    localparam [4:0] ST_Q_WEIGHT      = 5'd2;
    localparam [4:0] ST_K_WEIGHT      = 5'd3;
    localparam [4:0] ST_ROPE          = 5'd4;
    localparam [4:0] ST_RMS_BEGIN     = 5'd5;
    localparam [4:0] ST_RMS_FEED      = 5'd6;
    localparam [4:0] ST_RMS_WAIT      = 5'd7;
    localparam [4:0] ST_QK_BEGIN      = 5'd8;
    localparam [4:0] ST_QK_WAIT       = 5'd9;
    localparam [4:0] ST_CACHE_WRITE   = 5'd10;
    localparam [4:0] ST_DOT_BEGIN     = 5'd11;
    localparam [4:0] ST_K_READ_REQ    = 5'd12;
    localparam [4:0] ST_K_READ_WAIT   = 5'd13;
    localparam [4:0] ST_DOT_FEED      = 5'd14;
    localparam [4:0] ST_DOT_WAIT      = 5'd15;
    localparam [4:0] ST_V_READ_REQ    = 5'd16;
    localparam [4:0] ST_V_READ_WAIT   = 5'd17;
    localparam [4:0] ST_SOFT_BEGIN    = 5'd18;
    localparam [4:0] ST_SOFT_WAIT     = 5'd19;
    localparam [4:0] ST_OUTPUT        = 5'd20;
    localparam [4:0] ST_RMS_READ      = 5'd21;
    localparam [4:0] ST_QK_LOAD       = 5'd22;
    localparam [4:0] ST_CACHE_LOAD    = 5'd23;
    localparam [4:0] ST_OUTPUT_LOAD   = 5'd24;

    reg [4:0] state;
    reg [2:0] layer_ordinal;
    reg [16:0] transaction_position;
    reg [16:0] valid_positions [0:5];
    integer reset_index;

    reg signed [63:0] q_raw [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] k_raw [0:511]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] v_raw [0:511]
        /* synthesis syn_ramstyle="block_ram" */;
    // RoPE emits the low and high half of a head in the same cycle.  Separate
    // physical banks retain the exact logical vectors while giving each RAM a
    // single write address; a flat array would require two writes and Gowin
    // would implement roughly 98K bits as flip-flops.
    reg signed [63:0] q_rotated_lo [0:511]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] q_rotated_hi [0:511]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] k_rotated_lo [0:255]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] k_rotated_hi [0:255]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] attention [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] q_weight [0:63];
    reg signed [63:0] k_weight [0:63];
    reg signed [63:0] rope_cos [0:31];
    reg signed [63:0] rope_sin [0:31];
    reg signed [63:0] head_scores [0:CACHE_POSITIONS-1];

    reg [11:0] ingest_index;
    reg [6:0] constant_index;
    reg current_key;
    reg [4:0] current_head;
    reg [5:0] current_element;
    reg [4:0] current_pair;
    reg signed [63:0] current_inv_rms;
    reg signed [63:0] rms_sample_q30;
    reg signed [63:0] pair_x_lo_q30;
    reg signed [63:0] pair_x_hi_q30;
    reg signed [63:0] pair_weight_lo_q30;
    reg signed [63:0] pair_weight_hi_q30;
    reg signed [63:0] pair_cos_q30;
    reg signed [63:0] pair_sin_q30;
    reg [10:0] cache_write_index;
    reg signed [63:0] cache_write_q30;
    reg [3:0] attention_head;
    reg [16:0] attention_position;
    reg [5:0] attention_element;
    reg signed [63:0] cache_read_q30;
    reg signed [63:0] dot_query_q30;
    reg signed [63:0] soft_score_q30;
    reg [9:0] output_index;
    reg signed [63:0] output_q30;

    wire layer_valid = layer_i == 4'd2 || layer_i == 4'd5
        || layer_i == 4'd8 || layer_i == 4'd10
        || layer_i == 4'd12 || layer_i == 4'd14;
    wire [2:0] decoded_ordinal = layer_i == 4'd2 ? 3'd0
        : layer_i == 4'd5 ? 3'd1
        : layer_i == 4'd8 ? 3'd2
        : layer_i == 4'd10 ? 3'd3
        : layer_i == 4'd12 ? 3'd4 : 3'd5;

    wire [2:0] mapped_kv_head = attention_head[3:1];
    wire cache_write_value_kind = cache_write_index >= 11'd512;
    wire [9:0] cache_write_vector_index = cache_write_value_kind
        ? cache_write_index - 11'd512 : cache_write_index[9:0];
    wire [2:0] cache_write_head = cache_write_vector_index[8:6];
    wire [5:0] cache_write_element = cache_write_vector_index[5:0];
    wire [7:0] k_rotated_read_address = {
        cache_write_vector_index[8:6], cache_write_vector_index[4:0]};
    wire signed [63:0] k_rotated_read = cache_write_vector_index[5]
        ? k_rotated_hi[k_rotated_read_address]
        : k_rotated_lo[k_rotated_read_address];
    wire [8:0] q_rotated_read_address = {
        attention_head, current_element[4:0]};
    wire signed [63:0] q_rotated_read = current_element[5]
        ? q_rotated_hi[q_rotated_read_address]
        : q_rotated_lo[q_rotated_read_address];

    reg [16:0] cache_address_position;
    reg [2:0] cache_address_head;
    reg [5:0] cache_address_element;
    reg cache_address_value;
    reg [63:0] cache_address_wide;
    wire [2:0] cache_layer_ordinal = CACHE_LAYER_SLOTS == 1
        ? 3'd0 : layer_ordinal;
    always @* begin
        cache_address_position = transaction_position;
        cache_address_head = cache_write_head;
        cache_address_element = cache_write_element;
        cache_address_value = cache_write_value_kind;
        if (state == ST_K_READ_REQ || state == ST_K_READ_WAIT) begin
            cache_address_position = attention_position;
            cache_address_head = mapped_kv_head;
            cache_address_element = current_element;
            cache_address_value = 1'b0;
        end else if (state == ST_V_READ_REQ || state == ST_V_READ_WAIT) begin
            cache_address_position = attention_position;
            cache_address_head = mapped_kv_head;
            cache_address_element = attention_element;
            cache_address_value = 1'b1;
        end
        cache_address_wide = (((((cache_layer_ordinal * CACHE_POSITIONS)
            + cache_address_position) * 8 + cache_address_head) * 64
            + cache_address_element) * 2 + cache_address_value);
    end

    assign projected_ready_o = busy_o && state == ST_INGEST;
    assign norm_req_valid_o = state == ST_Q_WEIGHT || state == ST_K_WEIGHT;
    assign norm_req_key_o = state == ST_K_WEIGHT;
    assign norm_req_element_o = constant_index[5:0];
    assign rope_req_valid_o = state == ST_ROPE;
    assign rope_req_position_o = transaction_position;
    assign rope_req_pair_o = constant_index[4:0];
    wire cache_transaction_valid = state == ST_CACHE_WRITE
        || state == ST_K_READ_REQ || state == ST_V_READ_REQ;
    assign cache_req_valid_o = EXTERNAL_CACHE != 0 && cache_transaction_valid;
    assign cache_req_write_o = state == ST_CACHE_WRITE;
    assign cache_req_word_address_o = cache_address_wide[29:0];
    assign cache_req_write_q30_o = cache_write_q30;
    assign attention_valid_o = state == ST_OUTPUT;
    assign attention_index_o = output_index;
    assign attention_q30_o = output_q30;
    assign attention_last_o = state == ST_OUTPUT && output_index == 10'd1023;

    wire local_cache_ready;
    wire local_cache_rsp_valid;
    wire signed [63:0] local_cache_read_q30;
    generate
        if (EXTERNAL_CACHE == 0) begin : g_local_cache
            truega_lfm25_attention_local_cache #(
                .CACHE_WORDS(CACHE_TOTAL_WORDS)
            ) local_cache (
                .clk(clk), .reset_n(reset_n),
                .req_valid_i(cache_transaction_valid),
                .req_write_i(state == ST_CACHE_WRITE),
                .req_word_address_i(cache_address_wide[29:0]),
                .req_write_q30_i(cache_write_q30),
                .req_ready_o(local_cache_ready),
                .rsp_valid_o(local_cache_rsp_valid),
                .rsp_read_q30_o(local_cache_read_q30)
            );
        end else begin : g_no_local_cache
            assign local_cache_ready = 1'b0;
            assign local_cache_rsp_valid = 1'b0;
            assign local_cache_read_q30 = 64'sd0;
        end
    endgenerate
    wire selected_cache_ready = EXTERNAL_CACHE != 0
        ? cache_req_ready_i : local_cache_ready;
    wire selected_cache_rsp_valid = EXTERNAL_CACHE != 0
        ? cache_rsp_valid_i : local_cache_rsp_valid;
    wire signed [63:0] selected_cache_read_q30 = EXTERNAL_CACHE != 0
        ? cache_rsp_read_q30_i : local_cache_read_q30;

    wire rms_start = state == ST_RMS_BEGIN;
    wire rms_sample_valid = state == ST_RMS_FEED;
    wire rms_sample_ready;
    wire rms_busy;
    wire rms_done;
    wire rms_error;
    wire signed [63:0] rms_inverse;
    truega_lfm25_head_rms_inverse_slot rms (
        .clk(clk), .reset_n(reset_n), .start_i(rms_start),
        .sample_valid_i(rms_sample_valid), .sample_q30_i(rms_sample_q30),
        .sample_ready_o(rms_sample_ready), .busy_o(rms_busy),
        .done_o(rms_done), .error_o(rms_error), .inv_rms_q30_o(rms_inverse)
    );

    wire qk_start = state == ST_QK_BEGIN;
    wire qk_busy;
    wire qk_done;
    wire qk_error;
    wire signed [63:0] pair_y_lo;
    wire signed [63:0] pair_y_hi;
    truega_lfm25_qk_norm_rope_slot qk_rope (
        .clk(clk), .reset_n(reset_n), .start_i(qk_start),
        .x_lo_q30_i(pair_x_lo_q30), .x_hi_q30_i(pair_x_hi_q30),
        .inv_rms_q30_i(current_inv_rms),
        .weight_lo_q30_i(pair_weight_lo_q30),
        .weight_hi_q30_i(pair_weight_hi_q30),
        .cos_q30_i(pair_cos_q30), .sin_q30_i(pair_sin_q30),
        .busy_o(qk_busy), .done_o(qk_done), .error_o(qk_error),
        .y_lo_q30_o(pair_y_lo), .y_hi_q30_o(pair_y_hi)
    );

    wire dot_start = state == ST_DOT_BEGIN;
    wire dot_sample_valid = state == ST_DOT_FEED;
    wire dot_sample_ready;
    wire dot_busy;
    wire dot_done;
    wire dot_error;
    wire [2:0] dot_mapped_head;
    wire signed [63:0] dot_score;
    truega_lfm25_gqa_dot_slot dot (
        .clk(clk), .reset_n(reset_n), .start_i(dot_start),
        .query_head_i(attention_head), .kv_head_i(mapped_kv_head),
        .sample_valid_i(dot_sample_valid),
        .sample_last_i(current_element == 6'd63),
        .query_q30_i(dot_query_q30),
        .key_q30_i(cache_read_q30), .sample_ready_o(dot_sample_ready),
        .busy_o(dot_busy), .done_o(dot_done), .error_o(dot_error),
        .mapped_kv_head_o(dot_mapped_head), .score_q30_o(dot_score)
    );

    wire soft_start = state == ST_SOFT_BEGIN;
    wire soft_busy;
    wire soft_done;
    wire soft_error;
    wire soft_result_valid;
    wire signed [63:0] soft_result;
    truega_lfm25_online_softmax_value_slot softmax (
        .clk(clk), .reset_n(reset_n), .start_i(soft_start),
        .begin_i(attention_position == 0),
        .last_i(attention_position == transaction_position),
        .score_q30_i(soft_score_q30),
        .value_q30_i(cache_read_q30), .busy_o(soft_busy),
        .done_o(soft_done), .error_o(soft_error),
        .result_valid_o(soft_result_valid), .result_q30_o(soft_result)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            layer_ordinal <= 3'd0;
            transaction_position <= 17'd0;
            ingest_index <= 12'd0;
            constant_index <= 7'd0;
            current_key <= 1'b0;
            current_head <= 5'd0;
            current_element <= 6'd0;
            current_pair <= 5'd0;
            current_inv_rms <= 64'sd0;
            rms_sample_q30 <= 64'sd0;
            pair_x_lo_q30 <= 64'sd0;
            pair_x_hi_q30 <= 64'sd0;
            pair_weight_lo_q30 <= 64'sd0;
            pair_weight_hi_q30 <= 64'sd0;
            pair_cos_q30 <= 64'sd0;
            pair_sin_q30 <= 64'sd0;
            cache_write_index <= 11'd0;
            cache_write_q30 <= 64'sd0;
            attention_head <= 4'd0;
            attention_position <= 17'd0;
            attention_element <= 6'd0;
            cache_read_q30 <= 64'sd0;
            dot_query_q30 <= 64'sd0;
            soft_score_q30 <= 64'sd0;
            output_index <= 10'd0;
            output_q30 <= 64'sd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            valid_positions_o <= 17'd0;
            for (reset_index = 0; reset_index < 6; reset_index = reset_index + 1)
                valid_positions[reset_index] <= 17'd0;
        end else begin
            done_o <= 1'b0;
            if (start_i && !busy_o) begin
                if (!layer_valid || position_i >= CACHE_POSITIONS
                    || position_i != valid_positions[decoded_ordinal]) begin
                    done_o <= 1'b1;
                    error_o <= 1'b1;
                end else begin
                    layer_ordinal <= decoded_ordinal;
                    transaction_position <= position_i;
                    ingest_index <= 12'd0;
                    busy_o <= 1'b1;
                    error_o <= 1'b0;
                    valid_positions_o <= valid_positions[decoded_ordinal];
                    state <= ST_INGEST;
                end
            end else if (busy_o) begin
                case (state)
                    ST_INGEST: begin
                        if (projected_valid_i && projected_ready_o) begin
                            if (ingest_index < 12'd1024)
                                q_raw[ingest_index] <= projected_q30_i;
                            else if (ingest_index < 12'd1536)
                                k_raw[ingest_index - 12'd1024] <= projected_q30_i;
                            else
                                v_raw[ingest_index - 12'd1536] <= projected_q30_i;
                            if ((ingest_index == 12'd2047) != projected_last_i) begin
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                error_o <= 1'b1;
                                state <= ST_IDLE;
                            end else if (ingest_index == 12'd2047) begin
                                constant_index <= 7'd0;
                                state <= ST_Q_WEIGHT;
                            end else begin
                                ingest_index <= ingest_index + 12'd1;
                            end
                        end
                    end
                    ST_Q_WEIGHT: begin
                        if (norm_rsp_valid_i) begin
                            q_weight[constant_index[5:0]] <= norm_rsp_weight_q30_i;
                            if (constant_index == 7'd63) begin
                                constant_index <= 7'd0;
                                state <= ST_K_WEIGHT;
                            end else constant_index <= constant_index + 7'd1;
                        end
                    end
                    ST_K_WEIGHT: begin
                        if (norm_rsp_valid_i) begin
                            k_weight[constant_index[5:0]] <= norm_rsp_weight_q30_i;
                            if (constant_index == 7'd63) begin
                                constant_index <= 7'd0;
                                state <= ST_ROPE;
                            end else constant_index <= constant_index + 7'd1;
                        end
                    end
                    ST_ROPE: begin
                        if (rope_rsp_valid_i) begin
                            rope_cos[constant_index[4:0]] <= rope_rsp_cos_q30_i;
                            rope_sin[constant_index[4:0]] <= rope_rsp_sin_q30_i;
                            if (constant_index == 7'd31) begin
                                current_key <= 1'b0;
                                current_head <= 5'd0;
                                current_element <= 6'd0;
                                state <= ST_RMS_BEGIN;
                            end else constant_index <= constant_index + 7'd1;
                        end
                    end
                    ST_RMS_BEGIN: begin
                        current_element <= 6'd0;
                        state <= ST_RMS_READ;
                    end
                    ST_RMS_READ: begin
                        rms_sample_q30 <= current_key
                            ? k_raw[current_head * 64 + current_element]
                            : q_raw[current_head * 64 + current_element];
                        state <= ST_RMS_FEED;
                    end
                    ST_RMS_FEED: begin
                        if (rms_sample_valid && rms_sample_ready) begin
                            if (current_element == 6'd63)
                                state <= ST_RMS_WAIT;
                            else begin
                                current_element <= current_element + 6'd1;
                                state <= ST_RMS_READ;
                            end
                        end
                    end
                    ST_RMS_WAIT: begin
                        if (rms_done) begin
                            if (rms_error) begin
                                busy_o <= 1'b0; done_o <= 1'b1;
                                error_o <= 1'b1; state <= ST_IDLE;
                            end else begin
                                current_inv_rms <= rms_inverse;
                                current_pair <= 5'd0;
                                state <= ST_QK_LOAD;
                            end
                        end
                    end
                    ST_QK_LOAD: begin
                        if (current_key) begin
                            pair_x_lo_q30 <= k_raw[current_head * 64 + current_pair];
                            pair_x_hi_q30 <= k_raw[current_head * 64 + current_pair + 32];
                            pair_weight_lo_q30 <= k_weight[current_pair];
                            pair_weight_hi_q30 <= k_weight[current_pair + 32];
                        end else begin
                            pair_x_lo_q30 <= q_raw[current_head * 64 + current_pair];
                            pair_x_hi_q30 <= q_raw[current_head * 64 + current_pair + 32];
                            pair_weight_lo_q30 <= q_weight[current_pair];
                            pair_weight_hi_q30 <= q_weight[current_pair + 32];
                        end
                        pair_cos_q30 <= rope_cos[current_pair];
                        pair_sin_q30 <= rope_sin[current_pair];
                        state <= ST_QK_BEGIN;
                    end
                    ST_QK_BEGIN: state <= ST_QK_WAIT;
                    ST_QK_WAIT: begin
                        if (qk_done) begin
                            if (qk_error) begin
                                busy_o <= 1'b0; done_o <= 1'b1;
                                error_o <= 1'b1; state <= ST_IDLE;
                            end else begin
                                if (current_key) begin
                                    k_rotated_lo[{current_head[2:0], current_pair}]
                                        <= pair_y_lo;
                                    k_rotated_hi[{current_head[2:0], current_pair}]
                                        <= pair_y_hi;
                                end else begin
                                    q_rotated_lo[{current_head[3:0], current_pair}]
                                        <= pair_y_lo;
                                    q_rotated_hi[{current_head[3:0], current_pair}]
                                        <= pair_y_hi;
                                end
                                if (current_pair == 5'd31) begin
                                    if ((!current_key && current_head == 5'd15)
                                        || (current_key && current_head == 5'd7)) begin
                                        if (!current_key) begin
                                            current_key <= 1'b1;
                                            current_head <= 5'd0;
                                            state <= ST_RMS_BEGIN;
                                        end else begin
                                            cache_write_index <= 11'd0;
                                            state <= ST_CACHE_LOAD;
                                        end
                                    end else begin
                                        current_head <= current_head + 5'd1;
                                        state <= ST_RMS_BEGIN;
                                    end
                                end else begin
                                    current_pair <= current_pair + 5'd1;
                                    state <= ST_QK_LOAD;
                                end
                            end
                        end
                    end
                    ST_CACHE_LOAD: begin
                        cache_write_q30 <= cache_write_value_kind
                            ? v_raw[cache_write_vector_index]
                            : k_rotated_read;
                        state <= ST_CACHE_WRITE;
                    end
                    ST_CACHE_WRITE: begin
                        if (selected_cache_ready) begin
                            if (cache_write_index == 11'd1023) begin
                                valid_positions[layer_ordinal]
                                    <= valid_positions[layer_ordinal] + 17'd1;
                                valid_positions_o
                                    <= valid_positions[layer_ordinal] + 17'd1;
                                attention_head <= 4'd0;
                                attention_position <= 17'd0;
                                current_element <= 6'd0;
                                state <= ST_DOT_BEGIN;
                            end else begin
                                cache_write_index <= cache_write_index + 11'd1;
                                state <= ST_CACHE_LOAD;
                            end
                        end
                    end
                    ST_DOT_BEGIN: begin
                        current_element <= 6'd0;
                        state <= ST_K_READ_REQ;
                    end
                    ST_K_READ_REQ: begin
                        if (selected_cache_ready) begin
                            dot_query_q30 <= q_rotated_read;
                            state <= ST_K_READ_WAIT;
                        end
                    end
                    ST_K_READ_WAIT: begin
                        if (selected_cache_rsp_valid) begin
                            cache_read_q30 <= selected_cache_read_q30;
                            state <= ST_DOT_FEED;
                        end
                    end
                    ST_DOT_FEED: begin
                        if (dot_sample_valid && dot_sample_ready) begin
                            if (current_element == 6'd63)
                                state <= ST_DOT_WAIT;
                            else begin
                                current_element <= current_element + 6'd1;
                                state <= ST_K_READ_REQ;
                            end
                        end
                    end
                    ST_DOT_WAIT: begin
                        if (dot_done) begin
                            if (dot_error) begin
                                busy_o <= 1'b0; done_o <= 1'b1;
                                error_o <= 1'b1; state <= ST_IDLE;
                            end else begin
                                head_scores[attention_position] <= dot_score;
                                if (attention_position == transaction_position) begin
                                    attention_position <= 17'd0;
                                    attention_element <= 6'd0;
                                    state <= ST_V_READ_REQ;
                                end else begin
                                    attention_position <= attention_position + 17'd1;
                                    state <= ST_DOT_BEGIN;
                                end
                            end
                        end
                    end
                    ST_V_READ_REQ: begin
                        if (selected_cache_ready) begin
                            soft_score_q30 <= head_scores[attention_position];
                            state <= ST_V_READ_WAIT;
                        end
                    end
                    ST_V_READ_WAIT: begin
                        if (selected_cache_rsp_valid) begin
                            cache_read_q30 <= selected_cache_read_q30;
                            state <= ST_SOFT_BEGIN;
                        end
                    end
                    ST_SOFT_BEGIN: state <= ST_SOFT_WAIT;
                    ST_SOFT_WAIT: begin
                        if (soft_done) begin
                            if (soft_error) begin
                                busy_o <= 1'b0; done_o <= 1'b1;
                                error_o <= 1'b1; state <= ST_IDLE;
                            end else if (attention_position == transaction_position) begin
                                if (!soft_result_valid) begin
                                    busy_o <= 1'b0; done_o <= 1'b1;
                                    error_o <= 1'b1; state <= ST_IDLE;
                                end else begin
                                    attention[attention_head * 64 + attention_element]
                                        <= soft_result;
                                    if (attention_element == 6'd63) begin
                                        if (attention_head == 4'd15) begin
                                            output_index <= 10'd0;
                                            state <= ST_OUTPUT_LOAD;
                                        end else begin
                                            attention_head <= attention_head + 4'd1;
                                            attention_position <= 17'd0;
                                            current_element <= 6'd0;
                                            state <= ST_DOT_BEGIN;
                                        end
                                    end else begin
                                        attention_element <= attention_element + 6'd1;
                                        attention_position <= 17'd0;
                                        state <= ST_V_READ_REQ;
                                    end
                                end
                            end else begin
                                attention_position <= attention_position + 17'd1;
                                state <= ST_V_READ_REQ;
                            end
                        end
                    end
                    ST_OUTPUT_LOAD: begin
                        output_q30 <= attention[output_index];
                        state <= ST_OUTPUT;
                    end
                    ST_OUTPUT: begin
                        if (attention_valid_o && attention_ready_i) begin
                            if (output_index == 10'd1023) begin
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                state <= ST_IDLE;
                            end else begin
                                output_index <= output_index + 10'd1;
                                state <= ST_OUTPUT_LOAD;
                            end
                        end
                    end
                    default: begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= 1'b1;
                        state <= ST_IDLE;
                    end
                endcase
            end
        end
    end

    wire unused_children = rms_busy | qk_busy | dot_busy | soft_busy
        | dot_mapped_head[0];
endmodule

// Test/small-context cache.  This module is instantiated only by the
// EXTERNAL_CACHE=0 generate branch, so the production top cannot infer it.
// Reads are registered to permit block-RAM inference.
module truega_lfm25_attention_local_cache #(
    parameter integer CACHE_WORDS = 12288
) (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                req_valid_i,
    input  wire                req_write_i,
    input  wire [29:0]         req_word_address_i,
    input  wire signed [63:0]  req_write_q30_i,
    output wire                req_ready_o,
    output reg                 rsp_valid_o,
    output reg signed [63:0]   rsp_read_q30_o
);
    reg signed [63:0] memory [0:CACHE_WORDS-1];
    assign req_ready_o = 1'b1;
    always @(posedge clk) begin
        if (!reset_n) begin
            rsp_valid_o <= 1'b0;
            rsp_read_q30_o <= 64'sd0;
        end else begin
            rsp_valid_o <= 1'b0;
            if (req_valid_i) begin
                if (req_write_i)
                    memory[req_word_address_i] <= req_write_q30_i;
                else begin
                    rsp_read_q30_o <= memory[req_word_address_i];
                    rsp_valid_o <= 1'b1;
                end
            end
        end
    end
endmodule
