// Honest no-DDR baseline for the first token of each LFM2.5 attention layer.
//
// This wrapper deliberately implements position 0 only.  It keeps the six
// layer-local state slots in truega_lfm25_attention_token_slot, but gives each
// slot exactly one cache position and uses inferred FPGA-local RAM.  It is not
// a 16K-context implementation.
//
// Before start, software/model-loading logic supplies all 64 Q RMSNorm and all
// 64 K RMSNorm weights.  Each accepted raw F32 or BF16 model word is converted
// by truega_float_to_q30 and retained in FPGA registers.  A failed conversion
// clears that element's loaded bit, pulses norm_weight_error_o, and therefore
// prevents a transaction from starting until the element is replaced.
module truega_lfm25_attention_first_token_slot (
    input  wire                clk,
    input  wire                reset_n,

    input  wire                norm_weight_valid_i,
    output wire                norm_weight_ready_o,
    input  wire                norm_weight_key_i,
    input  wire [5:0]          norm_weight_element_i,
    input  wire                norm_weight_format_bf16_i,
    input  wire [31:0]         norm_weight_bits_i,
    output wire                norm_weights_loaded_o,
    output reg                 norm_weight_error_o,

    input  wire                start_i,
    output wire                start_ready_o,
    input  wire [3:0]          layer_i,
    input  wire [16:0]         position_i,
    input  wire                projected_valid_i,
    input  wire                projected_last_i,
    input  wire signed [63:0]  projected_q30_i,
    output wire                projected_ready_o,

    output wire                attention_valid_o,
    input  wire                attention_ready_i,
    output wire [9:0]          attention_index_o,
    output wire signed [63:0]  attention_q30_o,
    output wire                attention_last_o,
    output wire                busy_o,
    output wire                done_o,
    output wire                error_o,
    output wire [16:0]         valid_positions_o
);
    localparam signed [63:0] Q30_ONE = 64'sd1073741824;

    reg signed [63:0] q_norm_weight_q30 [0:63];
    reg signed [63:0] k_norm_weight_q30 [0:63];
    reg [63:0] q_norm_weight_loaded;
    reg [63:0] k_norm_weight_loaded;
    integer reset_index;

    wire signed [63:0] decoded_norm_weight_q30;
    wire decoded_norm_weight_error;
    truega_float_to_q30 norm_weight_decode (
        .format_bf16_i(norm_weight_format_bf16_i),
        .bits_i(norm_weight_bits_i),
        .q30_o(decoded_norm_weight_q30),
        .error_o(decoded_norm_weight_error)
    );

    wire core_busy;
    wire core_done;
    wire core_error;
    wire core_norm_req_valid;
    wire core_norm_req_key;
    wire [5:0] core_norm_req_element;
    wire core_rope_req_valid;
    wire [16:0] core_rope_req_position;
    wire [4:0] core_rope_req_pair;
    reg rejected_start_done;
    reg rejected_start_error;

    assign norm_weights_loaded_o = &q_norm_weight_loaded
        && &k_norm_weight_loaded;
    // A start has priority over a simultaneous model-weight transfer.  Weight
    // upload is explicitly backpressured for the whole active transaction.
    assign norm_weight_ready_o = !core_busy && !start_i;
    assign start_ready_o = !core_busy;
    wire accepted_weight = norm_weight_valid_i && norm_weight_ready_o;
    wire accepted_start = start_i && start_ready_o;
    wire valid_first_token_start = accepted_start
        && position_i == 17'd0 && norm_weights_loaded_o;

    assign busy_o = core_busy;
    assign done_o = core_done || rejected_start_done;
    assign error_o = core_error || rejected_start_error;

    always @(posedge clk) begin
        if (!reset_n) begin
            q_norm_weight_loaded <= 64'd0;
            k_norm_weight_loaded <= 64'd0;
            norm_weight_error_o <= 1'b0;
            rejected_start_done <= 1'b0;
            rejected_start_error <= 1'b0;
            for (reset_index = 0; reset_index < 64;
                 reset_index = reset_index + 1) begin
                q_norm_weight_q30[reset_index] <= 64'sd0;
                k_norm_weight_q30[reset_index] <= 64'sd0;
            end
        end else begin
            norm_weight_error_o <= 1'b0;
            rejected_start_done <= 1'b0;
            rejected_start_error <= 1'b0;

            if (accepted_weight) begin
                if (decoded_norm_weight_error) begin
                    norm_weight_error_o <= 1'b1;
                    if (norm_weight_key_i)
                        k_norm_weight_loaded[norm_weight_element_i] <= 1'b0;
                    else
                        q_norm_weight_loaded[norm_weight_element_i] <= 1'b0;
                end else if (norm_weight_key_i) begin
                    k_norm_weight_q30[norm_weight_element_i]
                        <= decoded_norm_weight_q30;
                    k_norm_weight_loaded[norm_weight_element_i] <= 1'b1;
                end else begin
                    q_norm_weight_q30[norm_weight_element_i]
                        <= decoded_norm_weight_q30;
                    q_norm_weight_loaded[norm_weight_element_i] <= 1'b1;
                end
            end

            if (accepted_start && !valid_first_token_start) begin
                rejected_start_done <= 1'b1;
                rejected_start_error <= 1'b1;
            end
        end
    end

    wire signed [63:0] core_norm_weight_q30 = core_norm_req_key
        ? k_norm_weight_q30[core_norm_req_element]
        : q_norm_weight_q30[core_norm_req_element];

    truega_lfm25_attention_token_slot #(
        .CACHE_POSITIONS(1),
        .EXTERNAL_CACHE(0)
    ) core (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(valid_first_token_start),
        .layer_i(layer_i),
        .position_i(position_i),
        .projected_valid_i(projected_valid_i),
        .projected_last_i(projected_last_i),
        .projected_q30_i(projected_q30_i),
        .projected_ready_o(projected_ready_o),

        .norm_req_valid_o(core_norm_req_valid),
        .norm_req_key_o(core_norm_req_key),
        .norm_req_element_o(core_norm_req_element),
        .norm_rsp_valid_i(core_norm_req_valid),
        .norm_rsp_weight_q30_i(core_norm_weight_q30),
        .rope_req_valid_o(core_rope_req_valid),
        .rope_req_position_o(core_rope_req_position),
        .rope_req_pair_o(core_rope_req_pair),
        .rope_rsp_valid_i(core_rope_req_valid),
        .rope_rsp_cos_q30_i(Q30_ONE),
        .rope_rsp_sin_q30_i(64'sd0),

        .cache_req_valid_o(),
        .cache_req_write_o(),
        .cache_req_word_address_o(),
        .cache_req_write_q30_o(),
        .cache_req_ready_i(1'b0),
        .cache_rsp_valid_i(1'b0),
        .cache_rsp_read_q30_i(64'sd0),

        .attention_valid_o(attention_valid_o),
        .attention_ready_i(attention_ready_i),
        .attention_index_o(attention_index_o),
        .attention_q30_o(attention_q30_o),
        .attention_last_o(attention_last_o),
        .busy_o(core_busy),
        .done_o(core_done),
        .error_o(core_error),
        .valid_positions_o(valid_positions_o)
    );

    // Nonzero RoPE requests are unreachable because the wrapper rejects every
    // nonzero position before starting the composite.
    wire unused_rope_request = core_rope_req_position[0]
        | core_rope_req_pair[0];
endmodule
