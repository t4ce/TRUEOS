// Complete fixed 1024-element LFM2.5 RMSNorm -> native Q8_0 vector slot.
//
// One start accepts exactly 1024 Q30 elements and their immutable F32/BF16
// RMSNorm weights.  Values are buffered in FPGA-local memories while the
// reduction slot computes mean-square + the pinned 1e-5 epsilon and its own
// reciprocal square root.  The buffered vector is then normalized element by
// element and quantized into exactly 32 native 272-bit Q8_0 blocks.
//
// There is no input for inv_rms and no host arithmetic boundary.  Both input
// and block output use ready/valid backpressure.  done_o pulses only when the
// consumer accepts block 31.
module truega_lfm25_rmsnorm_vector_slot (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,

    input  wire                input_valid_i,
    output wire                input_ready_o,
    input  wire signed [63:0]  x_q30_i,
    input  wire                weight_format_bf16_i,
    input  wire [31:0]         weight_bits_i,

    output wire                output_valid_o,
    input  wire                output_ready_i,
    output wire [4:0]          output_block_index_o,
    output wire                output_last_o,
    output wire [271:0]        output_q8_block_o,

    output reg                 busy_o,
    output reg                 done_o,
    output reg                 error_o,
    output reg [10:0]          inputs_accepted_o,
    output reg [5:0]           blocks_retired_o,
    output wire signed [63:0]  mean_square_q30_o,
    output wire signed [63:0]  inv_rms_q30_o
);
    localparam [3:0] ST_IDLE         = 4'd0;
    localparam [3:0] ST_COLLECT      = 4'd1;
    localparam [3:0] ST_WAIT_REDUCE  = 4'd2;
    localparam [3:0] ST_BLOCK_START  = 4'd3;
    localparam [3:0] ST_NORM_START   = 4'd4;
    localparam [3:0] ST_NORM_WAIT    = 4'd5;
    localparam [3:0] ST_QUANT_FEED   = 4'd6;
    localparam [3:0] ST_WAIT_QUANT   = 4'd7;
    localparam [3:0] ST_OUTPUT       = 4'd8;

    reg [3:0] state;
    reg signed [63:0] x_memory [0:1023];
    reg weight_format_memory [0:1023];
    reg [31:0] weight_bits_memory [0:1023];
    reg [9:0] element_index;
    reg [4:0] output_block_index;

    wire input_accept = input_valid_i && input_ready_o;
    wire output_accept = output_valid_o && output_ready_i;

    wire reduce_start = start_i && (state == ST_IDLE);
    wire reduce_sample_ready;
    wire reduce_busy;
    wire reduce_done;
    wire reduce_error;
    wire [10:0] reduce_samples;
    wire signed [63:0] reduce_mean_square;
    wire signed [63:0] reduce_inv_rms;
    wire reduce_sample_valid = input_accept;

    reg norm_start;
    reg signed [63:0] norm_x;
    reg norm_weight_format;
    reg [31:0] norm_weight_bits;
    wire norm_busy;
    wire norm_done;
    wire norm_error;
    wire signed [63:0] normalized_q30;
    wire signed [63:0] unused_residual_sum;

    reg quant_start;
    reg quant_sample_valid;
    reg signed [63:0] quant_sample;
    wire quant_sample_ready;
    wire quant_busy;
    wire quant_done;
    wire quant_error;
    wire [5:0] quant_samples;
    wire [271:0] quant_block;
    reg [271:0] output_block;

    assign input_ready_o = (state == ST_COLLECT) && reduce_sample_ready;
    assign output_valid_o = state == ST_OUTPUT;
    assign output_block_index_o = output_block_index;
    assign output_last_o = output_block_index == 5'd31;
    assign output_q8_block_o = output_block;
    assign mean_square_q30_o = reduce_mean_square;
    assign inv_rms_q30_o = reduce_inv_rms;

    truega_lfm25_rmsnorm_reduce_slot reduction (
        .clk(clk), .reset_n(reset_n), .start_i(reduce_start),
        .sample_valid_i(reduce_sample_valid), .sample_q30_i(x_q30_i),
        .sample_ready_o(reduce_sample_ready), .busy_o(reduce_busy),
        .done_o(reduce_done), .error_o(reduce_error),
        .samples_accepted_o(reduce_samples),
        .mean_square_q30_o(reduce_mean_square),
        .inv_rms_q30_o(reduce_inv_rms)
    );

    truega_lfm25_rmsnorm_residual_slot normalize_element (
        .clk(clk), .reset_n(reset_n), .start_i(norm_start),
        .x_q30_i(norm_x), .inv_rms_q30_i(reduce_inv_rms),
        .weight_format_bf16_i(norm_weight_format),
        .weight_bits_i(norm_weight_bits),
        .residual_q30_i(64'sd0), .branch_q30_i(64'sd0),
        .busy_o(norm_busy), .done_o(norm_done), .error_o(norm_error),
        .normalized_q30_o(normalized_q30),
        .residual_sum_q30_o(unused_residual_sum)
    );

    truega_q30_to_q8_0_block_slot quantize_block (
        .clk(clk), .reset_n(reset_n), .start_i(quant_start),
        .sample_valid_i(quant_sample_valid), .sample_q30_i(quant_sample),
        .sample_ready_o(quant_sample_ready), .busy_o(quant_busy),
        .done_o(quant_done), .error_o(quant_error),
        .samples_accepted_o(quant_samples), .q8_block_o(quant_block)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            element_index <= 10'd0;
            output_block_index <= 5'd0;
            norm_start <= 1'b0;
            norm_x <= 64'sd0;
            norm_weight_format <= 1'b0;
            norm_weight_bits <= 32'd0;
            quant_start <= 1'b0;
            quant_sample_valid <= 1'b0;
            quant_sample <= 64'sd0;
            output_block <= 272'd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            inputs_accepted_o <= 11'd0;
            blocks_retired_o <= 6'd0;
        end else begin
            done_o <= 1'b0;
            norm_start <= 1'b0;
            quant_start <= 1'b0;

            case (state)
                ST_IDLE: begin
                    busy_o <= 1'b0;
                    quant_sample_valid <= 1'b0;
                    if (start_i) begin
                        state <= ST_COLLECT;
                        busy_o <= 1'b1;
                        error_o <= 1'b0;
                        inputs_accepted_o <= 11'd0;
                        blocks_retired_o <= 6'd0;
                        output_block_index <= 5'd0;
                        element_index <= 10'd0;
                        output_block <= 272'd0;
                    end
                end

                ST_COLLECT: begin
                    if (input_accept) begin
                        x_memory[inputs_accepted_o[9:0]] <= x_q30_i;
                        weight_format_memory[inputs_accepted_o[9:0]]
                            <= weight_format_bf16_i;
                        weight_bits_memory[inputs_accepted_o[9:0]] <= weight_bits_i;
                        inputs_accepted_o <= inputs_accepted_o + 11'd1;
                        if (inputs_accepted_o == 11'd1023)
                            state <= ST_WAIT_REDUCE;
                    end
                end

                ST_WAIT_REDUCE: begin
                    if (reduce_done) begin
                        if (reduce_error || reduce_samples != 11'd1024) begin
                            state <= ST_IDLE;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                        end else begin
                            element_index <= 10'd0;
                            output_block_index <= 5'd0;
                            state <= ST_BLOCK_START;
                        end
                    end
                end

                ST_BLOCK_START: begin
                    quant_start <= 1'b1;
                    quant_sample_valid <= 1'b0;
                    state <= ST_NORM_START;
                end

                ST_NORM_START: begin
                    norm_x <= x_memory[element_index];
                    norm_weight_format <= weight_format_memory[element_index];
                    norm_weight_bits <= weight_bits_memory[element_index];
                    norm_start <= 1'b1;
                    state <= ST_NORM_WAIT;
                end

                ST_NORM_WAIT: begin
                    if (norm_done) begin
                        if (norm_error) begin
                            state <= ST_IDLE;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                        end else begin
                            quant_sample <= normalized_q30;
                            quant_sample_valid <= 1'b1;
                            state <= ST_QUANT_FEED;
                        end
                    end
                end

                ST_QUANT_FEED: begin
                    if (quant_sample_valid && quant_sample_ready) begin
                        quant_sample_valid <= 1'b0;
                        if (element_index[4:0] == 5'd31) begin
                            state <= ST_WAIT_QUANT;
                        end else begin
                            element_index <= element_index + 10'd1;
                            state <= ST_NORM_START;
                        end
                    end
                end

                ST_WAIT_QUANT: begin
                    if (quant_done) begin
                        if (quant_error || quant_samples != 6'd32) begin
                            state <= ST_IDLE;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                        end else begin
                            output_block <= quant_block;
                            state <= ST_OUTPUT;
                        end
                    end
                end

                ST_OUTPUT: begin
                    if (output_accept) begin
                        blocks_retired_o <= blocks_retired_o + 6'd1;
                        if (output_block_index == 5'd31) begin
                            state <= ST_IDLE;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b0;
                        end else begin
                            output_block_index <= output_block_index + 5'd1;
                            element_index <= element_index + 10'd1;
                            state <= ST_BLOCK_START;
                        end
                    end
                end

                default: begin
                    state <= ST_IDLE;
                    busy_o <= 1'b0;
                    done_o <= 1'b1;
                    error_o <= 1'b1;
                    quant_sample_valid <= 1'b0;
                end
            endcase
        end
    end

    wire unused_internal_busy = reduce_busy ^ norm_busy ^ quant_busy;
endmodule
