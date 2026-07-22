// Exact one-token, one-channel LFM2.5 causal shortconv circuit.
//
// Inputs b/c/x are the Q30 outputs of the fixed Q8_0 triplet projection slot.
// For the pinned l_cache=3 model, lfm2.cpp prepends two causal state values and
// computes the three-tap depthwise convolution in oldest/newest/current order:
//
//   bx      = RNE_Q30(b * x)
//   conv    = RNE_Q30(k[0] * old0) + RNE_Q30(k[1] * old1)
//           + RNE_Q30(k[2] * bx)
//   y       = RNE_Q30(c * conv)
//   newstate = {old1, bx}
//
// k is the sealed model's BF16 shortconv kernel.  y_q30_o is deliberately the
// Q30 block-quantizer input boundary for the following Q8_0 out projection.
module truega_lfm25_shortconv_channel_slot (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire signed [63:0]  b_q30_i,
    input  wire signed [63:0]  c_q30_i,
    input  wire signed [63:0]  x_q30_i,
    input  wire signed [63:0]  state_oldest_q30_i,
    input  wire signed [63:0]  state_newest_q30_i,
    input  wire [15:0]         kernel_oldest_bf16_i,
    input  wire [15:0]         kernel_newest_bf16_i,
    input  wire [15:0]         kernel_current_bf16_i,
    output reg                 busy_o,
    output reg                 done_o,
    output reg                 error_o,
    output reg signed [63:0]   bx_q30_o,
    output reg signed [63:0]   conv_q30_o,
    output reg signed [63:0]   y_q30_o,
    output reg signed [63:0]   state_oldest_q30_o,
    output reg signed [63:0]   state_newest_q30_o
);
    localparam [2:0] ST_IDLE = 3'd0;
    localparam [2:0] ST_BX   = 3'd1;
    localparam [2:0] ST_K0   = 3'd2;
    localparam [2:0] ST_K1   = 3'd3;
    localparam [2:0] ST_K2   = 3'd4;
    localparam [2:0] ST_Y    = 3'd5;

    wire signed [63:0] kernel_oldest_q30;
    wire signed [63:0] kernel_newest_q30;
    wire signed [63:0] kernel_current_q30;
    wire kernel_oldest_error;
    wire kernel_newest_error;
    wire kernel_current_error;

    truega_float_to_q30 decode_kernel_oldest (
        .format_bf16_i(1'b1), .bits_i({16'd0, kernel_oldest_bf16_i}),
        .q30_o(kernel_oldest_q30), .error_o(kernel_oldest_error)
    );
    truega_float_to_q30 decode_kernel_newest (
        .format_bf16_i(1'b1), .bits_i({16'd0, kernel_newest_bf16_i}),
        .q30_o(kernel_newest_q30), .error_o(kernel_newest_error)
    );
    truega_float_to_q30 decode_kernel_current (
        .format_bf16_i(1'b1), .bits_i({16'd0, kernel_current_bf16_i}),
        .q30_o(kernel_current_q30), .error_o(kernel_current_error)
    );

    reg [2:0] state;
    reg multiply_start;
    reg multiply_waiting;
    reg signed [63:0] b_q30;
    reg signed [63:0] c_q30;
    reg signed [63:0] x_q30;
    reg signed [63:0] state_oldest_q30;
    reg signed [63:0] state_newest_q30;
    reg signed [63:0] kernel_oldest;
    reg signed [63:0] kernel_newest;
    reg signed [63:0] kernel_current;
    reg signed [63:0] bx_q30;
    reg signed [63:0] kernel_term0;
    reg signed [63:0] kernel_term1;
    reg signed [63:0] conv_q30;
    reg signed [63:0] multiply_left;
    reg signed [63:0] multiply_right;
    wire multiply_busy;
    wire multiply_done;
    wire multiply_overflow;
    wire signed [63:0] multiply_result;

    wire signed [65:0] conv_sum_ext =
          $signed({{2{kernel_term0[63]}}, kernel_term0})
        + $signed({{2{kernel_term1[63]}}, kernel_term1})
        + $signed({{2{multiply_result[63]}}, multiply_result});
    wire conv_overflow = conv_sum_ext[65:63] != {3{conv_sum_ext[63]}};

    always @* begin
        multiply_left = 64'sd0;
        multiply_right = 64'sd0;
        case (state)
            ST_BX: begin multiply_left = b_q30; multiply_right = x_q30; end
            ST_K0: begin multiply_left = kernel_oldest; multiply_right = state_oldest_q30; end
            ST_K1: begin multiply_left = kernel_newest; multiply_right = state_newest_q30; end
            ST_K2: begin multiply_left = kernel_current; multiply_right = bx_q30; end
            ST_Y:  begin multiply_left = c_q30; multiply_right = conv_q30; end
            default: begin end
        endcase
    end

    truega_q30_mul_seq multiply (
        .clk(clk), .reset_n(reset_n), .start_i(multiply_start),
        .left_q30_i(multiply_left), .right_q30_i(multiply_right),
        .busy_o(multiply_busy), .done_o(multiply_done),
        .overflow_o(multiply_overflow), .result_q30_o(multiply_result)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            multiply_start <= 1'b0;
            multiply_waiting <= 1'b0;
            b_q30 <= 64'sd0;
            c_q30 <= 64'sd0;
            x_q30 <= 64'sd0;
            state_oldest_q30 <= 64'sd0;
            state_newest_q30 <= 64'sd0;
            kernel_oldest <= 64'sd0;
            kernel_newest <= 64'sd0;
            kernel_current <= 64'sd0;
            bx_q30 <= 64'sd0;
            kernel_term0 <= 64'sd0;
            kernel_term1 <= 64'sd0;
            conv_q30 <= 64'sd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            bx_q30_o <= 64'sd0;
            conv_q30_o <= 64'sd0;
            y_q30_o <= 64'sd0;
            state_oldest_q30_o <= 64'sd0;
            state_newest_q30_o <= 64'sd0;
        end else begin
            done_o <= 1'b0;
            multiply_start <= 1'b0;

            if (state == ST_IDLE) begin
                busy_o <= 1'b0;
                if (start_i) begin
                    bx_q30_o <= 64'sd0;
                    conv_q30_o <= 64'sd0;
                    y_q30_o <= 64'sd0;
                    state_oldest_q30_o <= 64'sd0;
                    state_newest_q30_o <= 64'sd0;
                    multiply_waiting <= 1'b0;
                    if (kernel_oldest_error || kernel_newest_error
                        || kernel_current_error) begin
                        error_o <= 1'b1;
                        done_o <= 1'b1;
                    end else begin
                        b_q30 <= b_q30_i;
                        c_q30 <= c_q30_i;
                        x_q30 <= x_q30_i;
                        state_oldest_q30 <= state_oldest_q30_i;
                        state_newest_q30 <= state_newest_q30_i;
                        kernel_oldest <= kernel_oldest_q30;
                        kernel_newest <= kernel_newest_q30;
                        kernel_current <= kernel_current_q30;
                        kernel_term0 <= 64'sd0;
                        kernel_term1 <= 64'sd0;
                        error_o <= 1'b0;
                        busy_o <= 1'b1;
                        state <= ST_BX;
                    end
                end
            end else if (busy_o) begin
                if (!multiply_waiting) begin
                    multiply_start <= 1'b1;
                    multiply_waiting <= 1'b1;
                end else if (multiply_done) begin
                    multiply_waiting <= 1'b0;
                    if (multiply_overflow) begin
                        state <= ST_IDLE;
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= 1'b1;
                    end else begin
                        case (state)
                            ST_BX: begin bx_q30 <= multiply_result; state <= ST_K0; end
                            ST_K0: begin kernel_term0 <= multiply_result; state <= ST_K1; end
                            ST_K1: begin kernel_term1 <= multiply_result; state <= ST_K2; end
                            ST_K2: begin
                                if (conv_overflow) begin
                                    state <= ST_IDLE;
                                    busy_o <= 1'b0;
                                    done_o <= 1'b1;
                                    error_o <= 1'b1;
                                end else begin
                                    conv_q30 <= conv_sum_ext[63:0];
                                    state <= ST_Y;
                                end
                            end
                            ST_Y: begin
                                bx_q30_o <= bx_q30;
                                conv_q30_o <= conv_q30;
                                y_q30_o <= multiply_result;
                                state_oldest_q30_o <= state_newest_q30;
                                state_newest_q30_o <= bx_q30;
                                state <= ST_IDLE;
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                error_o <= 1'b0;
                            end
                            default: begin
                                state <= ST_IDLE;
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                error_o <= 1'b1;
                            end
                        endcase
                    end
                end
            end
        end
    end

    wire unused_multiply_busy = multiply_busy;
endmodule
