// Stable online softmax and value accumulation for one LFM2.5 head element.
//
// One instance is reused for each of the 64 value dimensions.  Scores and
// values are Q30.  The state follows the online recurrence (m, denominator,
// numerator), so no score buffer and no host-side softmax are needed.
//
// exp(x), x <= 0, uses iterative ln(2) range reduction and a fifth-order
// Horner polynomial on [-ln(2), 0].  Its worst scalar absolute error is below
// 1.6e-4.  All products use the shared 64-cycle shift/add Q30 multiplier.  The
// final normalization uses a 94-cycle restoring divider.  There are no wide
// combinational multipliers, dividers, or remainder operators in this slot.
module truega_lfm25_online_softmax_value_slot (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire                begin_i,
    input  wire                last_i,
    input  wire signed [63:0]  score_q30_i,
    input  wire signed [63:0]  value_q30_i,
    output reg                 busy_o,
    output reg                 done_o,
    output reg                 error_o,
    output reg                 result_valid_o,
    output reg signed [63:0]   result_q30_o
);
    localparam signed [63:0] Q30_ONE = 64'sd1073741824;
    localparam signed [63:0] LN2_Q30 = 64'sd744261118;
    localparam signed [63:0] C2_Q30 = 64'sd536870912;
    localparam signed [63:0] C3_Q30 = 64'sd178956971;
    localparam signed [63:0] C4_Q30 = 64'sd44739243;
    localparam signed [63:0] C5_Q30 = 64'sd8947849;
    localparam signed [63:0] NEG_LN2_Q30 = -64'sd744261118;
    localparam signed [63:0] NEG_SIXTEEN_Q30 = -64'sd17179869184;

    localparam [3:0] ST_IDLE       = 4'd0;
    localparam [3:0] ST_EXP_RANGE  = 4'd1;
    localparam [3:0] ST_EXP_POLY   = 4'd2;
    localparam [3:0] ST_DEN_ALPHA  = 4'd3;
    localparam [3:0] ST_NUM_ALPHA  = 4'd4;
    localparam [3:0] ST_VALUE_BETA = 4'd5;
    localparam [3:0] ST_DIVIDE     = 4'd6;

    function [63:0] rounded_pow2_shift;
        input [63:0] value;
        input [4:0] shift;
        reg [63:0] shifted;
        reg [63:0] mask;
        reg [63:0] discarded;
        reg [63:0] half;
        begin
            if (shift == 0) begin
                rounded_pow2_shift = value;
            end else begin
                shifted = value >> shift;
                mask = (64'd1 << shift) - 64'd1;
                discarded = value & mask;
                half = 64'd1 << (shift - 1'b1);
                if ((discarded > half)
                    || ((discarded == half) && shifted[0]))
                    shifted = shifted + 64'd1;
                rounded_pow2_shift = shifted;
            end
        end
    endfunction

    reg [3:0] state;
    reg sequence_active;
    reg last_latched;
    reg score_is_new_max;
    reg signed [63:0] score;
    reg signed [63:0] value;
    reg signed [63:0] maximum_q30;
    reg signed [63:0] denominator_q30;
    reg signed [63:0] numerator_q30;

    reg signed [63:0] exp_remainder;
    reg [4:0] exp_shift;
    reg [2:0] polynomial_stage;
    reg signed [63:0] polynomial;
    reg signed [63:0] exponential;
    reg signed [63:0] alpha_denominator;

    reg multiply_start;
    reg multiply_waiting;
    reg signed [63:0] multiply_left;
    reg signed [63:0] multiply_right;
    wire multiply_busy;
    wire multiply_done;
    wire multiply_overflow;
    wire signed [63:0] multiply_result;

    reg divide_negative;
    reg [93:0] divide_dividend;
    reg [63:0] divide_divisor;
    reg [64:0] divide_remainder;
    reg [93:0] divide_quotient;
    reg [6:0] divide_bit;
    wire [64:0] divide_shifted =
        {divide_remainder[63:0], divide_dividend[divide_bit]};
    wire divide_take = divide_shifted >= {1'b0, divide_divisor};
    wire [64:0] divide_remainder_next = divide_take
        ? divide_shifted - {1'b0, divide_divisor} : divide_shifted;
    wire [93:0] divide_quotient_next = divide_take
        ? divide_quotient | (94'd1 << divide_bit) : divide_quotient;
    wire divide_round_up = ({divide_remainder_next, 1'b0}
            > {2'b0, divide_divisor})
        || (({divide_remainder_next, 1'b0} == {2'b0, divide_divisor})
            && divide_quotient_next[0]);
    wire [93:0] divide_rounded = divide_quotient_next + divide_round_up;

    always @* begin
        multiply_left = 64'sd0;
        multiply_right = 64'sd0;
        case (state)
            ST_EXP_POLY: begin
                multiply_left = polynomial;
                multiply_right = exp_remainder;
            end
            ST_DEN_ALPHA: begin
                multiply_left = denominator_q30;
                multiply_right = exponential;
            end
            ST_NUM_ALPHA: begin
                multiply_left = numerator_q30;
                multiply_right = exponential;
            end
            ST_VALUE_BETA: begin
                multiply_left = value;
                multiply_right = exponential;
            end
            default: begin end
        endcase
    end

    truega_q30_mul_seq sequential_multiply (
        .clk(clk), .reset_n(reset_n), .start_i(multiply_start),
        .left_q30_i(multiply_left), .right_q30_i(multiply_right),
        .busy_o(multiply_busy), .done_o(multiply_done),
        .overflow_o(multiply_overflow), .result_q30_o(multiply_result)
    );

    task finish_sample;
        input signed [63:0] next_maximum;
        input signed [63:0] next_denominator;
        input signed [63:0] next_numerator;
        reg [63:0] numerator_magnitude;
        begin
            if (next_denominator <= 0) begin
                busy_o <= 1'b0;
                done_o <= 1'b1;
                error_o <= 1'b1;
                sequence_active <= 1'b0;
                state <= ST_IDLE;
            end else if (last_latched) begin
                divide_negative <= next_numerator[63];
                numerator_magnitude = next_numerator[63]
                    ? (~next_numerator + 64'd1) : next_numerator;
                divide_dividend <= {numerator_magnitude, 30'd0};
                divide_divisor <= next_denominator;
                divide_remainder <= 65'd0;
                divide_quotient <= 94'd0;
                divide_bit <= 7'd93;
                state <= ST_DIVIDE;
            end else begin
                maximum_q30 <= next_maximum;
                denominator_q30 <= next_denominator;
                numerator_q30 <= next_numerator;
                sequence_active <= 1'b1;
                busy_o <= 1'b0;
                done_o <= 1'b1;
                state <= ST_IDLE;
            end
        end
    endtask

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            sequence_active <= 1'b0;
            last_latched <= 1'b0;
            score_is_new_max <= 1'b0;
            score <= 64'sd0;
            value <= 64'sd0;
            maximum_q30 <= 64'sd0;
            denominator_q30 <= 64'sd0;
            numerator_q30 <= 64'sd0;
            exp_remainder <= 64'sd0;
            exp_shift <= 5'd0;
            polynomial_stage <= 3'd0;
            polynomial <= 64'sd0;
            exponential <= 64'sd0;
            alpha_denominator <= 64'sd0;
            multiply_start <= 1'b0;
            multiply_waiting <= 1'b0;
            divide_negative <= 1'b0;
            divide_dividend <= 94'd0;
            divide_divisor <= 64'd0;
            divide_remainder <= 65'd0;
            divide_quotient <= 94'd0;
            divide_bit <= 7'd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            result_valid_o <= 1'b0;
            result_q30_o <= 64'sd0;
        end else begin
            done_o <= 1'b0;
            result_valid_o <= 1'b0;
            multiply_start <= 1'b0;
            if (start_i && !busy_o) begin
                score <= score_q30_i;
                value <= value_q30_i;
                last_latched <= last_i;
                error_o <= 1'b0;
                if ((!sequence_active && !begin_i)
                    || (sequence_active && begin_i)) begin
                    done_o <= 1'b1;
                    error_o <= 1'b1;
                    sequence_active <= 1'b0;
                end else if (begin_i) begin
                    if (last_i) begin
                        result_q30_o <= value_q30_i;
                        result_valid_o <= 1'b1;
                        done_o <= 1'b1;
                        sequence_active <= 1'b0;
                    end else begin
                        maximum_q30 <= score_q30_i;
                        denominator_q30 <= Q30_ONE;
                        numerator_q30 <= value_q30_i;
                        sequence_active <= 1'b1;
                        done_o <= 1'b1;
                    end
                end else begin
                    score_is_new_max <= score_q30_i > maximum_q30;
                    exp_remainder <= score_q30_i > maximum_q30
                        ? maximum_q30 - score_q30_i
                        : score_q30_i - maximum_q30;
                    exp_shift <= 5'd0;
                    multiply_waiting <= 1'b0;
                    busy_o <= 1'b1;
                    state <= ST_EXP_RANGE;
                end
            end else if (busy_o) begin
                case (state)
                    ST_EXP_RANGE: begin
                        if (exp_remainder <= NEG_SIXTEEN_Q30) begin
                            exponential <= 64'sd0;
                            multiply_waiting <= 1'b0;
                            state <= score_is_new_max ? ST_DEN_ALPHA
                                                      : ST_VALUE_BETA;
                        end else if (exp_remainder <= NEG_LN2_Q30) begin
                            exp_remainder <= exp_remainder + LN2_Q30;
                            exp_shift <= exp_shift + 5'd1;
                        end else begin
                            polynomial <= C5_Q30;
                            polynomial_stage <= 3'd0;
                            multiply_waiting <= 1'b0;
                            state <= ST_EXP_POLY;
                        end
                    end
                    ST_EXP_POLY: begin
                        if (!multiply_waiting) begin
                            multiply_start <= 1'b1;
                            multiply_waiting <= 1'b1;
                        end else if (multiply_done) begin
                            multiply_waiting <= 1'b0;
                            if (multiply_overflow) begin
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                error_o <= 1'b1;
                                sequence_active <= 1'b0;
                                state <= ST_IDLE;
                            end else begin
                                case (polynomial_stage)
                                    3'd0: polynomial <= C4_Q30 + multiply_result;
                                    3'd1: polynomial <= C3_Q30 + multiply_result;
                                    3'd2: polynomial <= C2_Q30 + multiply_result;
                                    3'd3: polynomial <= Q30_ONE + multiply_result;
                                    3'd4: begin
                                        exponential <= $signed(rounded_pow2_shift(
                                            Q30_ONE + multiply_result, exp_shift));
                                        state <= score_is_new_max ? ST_DEN_ALPHA
                                                                  : ST_VALUE_BETA;
                                    end
                                    default: begin end
                                endcase
                                if (polynomial_stage != 3'd4)
                                    polynomial_stage <= polynomial_stage + 3'd1;
                            end
                        end
                    end
                    ST_DEN_ALPHA: begin
                        if (!multiply_waiting) begin
                            multiply_start <= 1'b1;
                            multiply_waiting <= 1'b1;
                        end else if (multiply_done) begin
                            multiply_waiting <= 1'b0;
                            if (multiply_overflow) begin
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                error_o <= 1'b1;
                                sequence_active <= 1'b0;
                                state <= ST_IDLE;
                            end else begin
                                alpha_denominator <= multiply_result;
                                state <= ST_NUM_ALPHA;
                            end
                        end
                    end
                    ST_NUM_ALPHA: begin
                        if (!multiply_waiting) begin
                            multiply_start <= 1'b1;
                            multiply_waiting <= 1'b1;
                        end else if (multiply_done) begin
                            multiply_waiting <= 1'b0;
                            if (multiply_overflow) begin
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                error_o <= 1'b1;
                                sequence_active <= 1'b0;
                                state <= ST_IDLE;
                            end else begin
                                finish_sample(score,
                                    alpha_denominator + Q30_ONE,
                                    multiply_result + value);
                            end
                        end
                    end
                    ST_VALUE_BETA: begin
                        if (!multiply_waiting) begin
                            multiply_start <= 1'b1;
                            multiply_waiting <= 1'b1;
                        end else if (multiply_done) begin
                            multiply_waiting <= 1'b0;
                            if (multiply_overflow) begin
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                error_o <= 1'b1;
                                sequence_active <= 1'b0;
                                state <= ST_IDLE;
                            end else begin
                                finish_sample(maximum_q30,
                                    denominator_q30 + exponential,
                                    numerator_q30 + multiply_result);
                            end
                        end
                    end
                    ST_DIVIDE: begin
                        divide_remainder <= divide_remainder_next;
                        divide_quotient <= divide_quotient_next;
                        if (divide_bit == 7'd0) begin
                            if (divide_rounded[93:63] != 31'd0) begin
                                error_o <= 1'b1;
                                result_q30_o <= 64'sd0;
                            end else if (divide_negative) begin
                                result_q30_o <= -$signed(divide_rounded[63:0]);
                            end else begin
                                result_q30_o <= $signed(divide_rounded[63:0]);
                            end
                            result_valid_o <= 1'b1;
                            sequence_active <= 1'b0;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            state <= ST_IDLE;
                        end else begin
                            divide_bit <= divide_bit - 7'd1;
                        end
                    end
                    default: begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= 1'b1;
                        sequence_active <= 1'b0;
                        state <= ST_IDLE;
                    end
                endcase
            end
        end
    end

    wire unused_multiply_busy = multiply_busy;
endmodule
