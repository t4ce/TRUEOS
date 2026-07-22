// LFM2.5 RMSNorm per-element and residual-add fixed slot.
//
// This is the second pass of RMSNorm:
//   normalized[i] = RNE_Q30(RNE_Q30(x[i] * inv_rms) * weight[i])
// where inv_rms = 1/sqrt(mean(x*x) + epsilon). The vector reduction and
// reciprocal-square-root are FPGA circuit inputs to this element slot, never
// host-computed values. Weight accepts source F32 or sealed-model BF16 bits.
// The independent residual output implements the two exact additions around
// the operator and FFN blocks in lfm2.cpp.
module truega_lfm25_rmsnorm_residual_slot (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire signed [63:0]  x_q30_i,
    input  wire signed [63:0]  inv_rms_q30_i,
    input  wire                weight_format_bf16_i,
    input  wire [31:0]         weight_bits_i,
    input  wire signed [63:0]  residual_q30_i,
    input  wire signed [63:0]  branch_q30_i,
    output reg                 busy_o,
    output reg                 done_o,
    output reg                 error_o,
    output reg signed [63:0]   normalized_q30_o,
    output reg signed [63:0]   residual_sum_q30_o
);
    localparam [1:0] ST_IDLE = 2'd0;
    localparam [1:0] ST_X_INV = 2'd1;
    localparam [1:0] ST_WEIGHT = 2'd2;

    wire signed [63:0] decoded_weight_q30;
    wire decoded_weight_error;
    truega_float_to_q30 weight_decode (
        .format_bf16_i(weight_format_bf16_i),
        .bits_i(weight_bits_i),
        .q30_o(decoded_weight_q30),
        .error_o(decoded_weight_error)
    );

    reg [1:0] state;
    reg multiply_start;
    reg multiply_waiting;
    reg signed [63:0] x_q30;
    reg signed [63:0] inv_rms_q30;
    reg signed [63:0] weight_q30;
    reg signed [63:0] x_scaled_q30;
    reg signed [63:0] residual_q30;
    reg signed [63:0] branch_q30;
    reg signed [63:0] multiply_left;
    reg signed [63:0] multiply_right;
    wire multiply_busy;
    wire multiply_done;
    wire multiply_overflow;
    wire signed [63:0] multiply_result;
    wire signed [64:0] residual_sum_ext =
        $signed({residual_q30[63], residual_q30})
        + $signed({branch_q30[63], branch_q30});
    wire residual_overflow = residual_sum_ext[64] != residual_sum_ext[63];

    always @* begin
        multiply_left = 64'sd0;
        multiply_right = 64'sd0;
        case (state)
            ST_X_INV: begin
                multiply_left = x_q30;
                multiply_right = inv_rms_q30;
            end
            ST_WEIGHT: begin
                multiply_left = x_scaled_q30;
                multiply_right = weight_q30;
            end
            default: begin end
        endcase
    end

    truega_q30_mul_seq multiply (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(multiply_start),
        .left_q30_i(multiply_left),
        .right_q30_i(multiply_right),
        .busy_o(multiply_busy),
        .done_o(multiply_done),
        .overflow_o(multiply_overflow),
        .result_q30_o(multiply_result)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            multiply_start <= 1'b0;
            multiply_waiting <= 1'b0;
            x_q30 <= 64'sd0;
            inv_rms_q30 <= 64'sd0;
            weight_q30 <= 64'sd0;
            x_scaled_q30 <= 64'sd0;
            residual_q30 <= 64'sd0;
            branch_q30 <= 64'sd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            normalized_q30_o <= 64'sd0;
            residual_sum_q30_o <= 64'sd0;
        end else begin
            done_o <= 1'b0;
            multiply_start <= 1'b0;
            if (start_i && !busy_o) begin
                normalized_q30_o <= 64'sd0;
                residual_sum_q30_o <= 64'sd0;
                multiply_waiting <= 1'b0;
                if (decoded_weight_error) begin
                    error_o <= 1'b1;
                    done_o <= 1'b1;
                    state <= ST_IDLE;
                end else begin
                    x_q30 <= x_q30_i;
                    inv_rms_q30 <= inv_rms_q30_i;
                    weight_q30 <= decoded_weight_q30;
                    residual_q30 <= residual_q30_i;
                    branch_q30 <= branch_q30_i;
                    error_o <= 1'b0;
                    busy_o <= 1'b1;
                    state <= ST_X_INV;
                end
            end else if (busy_o) begin
                if (!multiply_waiting) begin
                    multiply_start <= 1'b1;
                    multiply_waiting <= 1'b1;
                end else if (multiply_done) begin
                    multiply_waiting <= 1'b0;
                    if (multiply_overflow) begin
                        error_o <= 1'b1;
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        state <= ST_IDLE;
                    end else if (state == ST_X_INV) begin
                        x_scaled_q30 <= multiply_result;
                        state <= ST_WEIGHT;
                    end else begin
                        normalized_q30_o <= multiply_result;
                        residual_sum_q30_o <= residual_sum_ext[63:0];
                        error_o <= residual_overflow;
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        state <= ST_IDLE;
                    end
                end
            end
        end
    end

    wire unused_multiply_busy = multiply_busy;
endmodule
