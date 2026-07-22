// Fixed LFM2.5-350M RMSNorm reduction and reciprocal-square-root slot.
//
// The pinned model has 1024 elements and epsilon=1e-5.  Inputs and outputs use
// signed Q30.  Each x*x is rounded to nearest/ties-even by the shared sequential
// multiplier, the 1024 terms are averaged with the same rounding rule, and the
// fixed Q30 epsilon (round(1e-5 * 2^30) = 10737) is added.  The reciprocal square
// root is then calculated entirely in gates:
//
//   root_q30 = floor(sqrt(mean_q30 << 30))
//   inv_q30  = RNE((1 << 60) / root_q30)
//
// No host-provided normalization scalar is accepted by this circuit.  The result
// feeds truega_lfm25_rmsnorm_residual_slot, which performs the per-element weight
// multiply and the two LFM2.5 residual sites.
module truega_lfm25_rmsnorm_reduce_slot #(
    parameter integer VECTOR_ELEMENTS = 1024,
    parameter [63:0] EPSILON_Q30 = 64'd10737
) (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire                sample_valid_i,
    input  wire signed [63:0]  sample_q30_i,
    output wire                sample_ready_o,
    output reg                 busy_o,
    output reg                 done_o,
    output reg                 error_o,
    output reg [10:0]          samples_accepted_o,
    output reg signed [63:0]   mean_square_q30_o,
    output reg signed [63:0]   inv_rms_q30_o
);
    localparam [2:0] ST_IDLE    = 3'd0;
    localparam [2:0] ST_COLLECT = 3'd1;
    localparam [2:0] ST_SQRT    = 3'd2;
    localparam [2:0] ST_DIVIDE  = 3'd3;

    reg [2:0] state;
    reg signed [63:0] square_operand;
    reg square_start;
    reg square_waiting;
    wire square_busy;
    wire square_done;
    wire square_overflow;
    wire signed [63:0] square_q30;
    reg [73:0] square_sum;

    // 94-bit unsigned integer square root, two radicand bits per cycle.
    reg [93:0] sqrt_radicand;
    reg [48:0] sqrt_remainder;
    reg [46:0] sqrt_root;
    reg [5:0] sqrt_iteration;
    wire [48:0] sqrt_shifted_remainder =
        {sqrt_remainder[46:0], sqrt_radicand[93:92]};
    wire [48:0] sqrt_trial = ({2'b00, sqrt_root} << 2) | 49'd1;
    wire sqrt_take = sqrt_shifted_remainder >= sqrt_trial;
    wire [48:0] sqrt_remainder_next = sqrt_take
        ? sqrt_shifted_remainder - sqrt_trial
        : sqrt_shifted_remainder;
    wire [46:0] sqrt_root_next = {sqrt_root[45:0], sqrt_take};

    // Unsigned restoring divide for RNE((1<<60) / sqrt_root).
    reg [63:0] divide_numerator;
    reg [46:0] divide_denominator;
    reg [47:0] divide_remainder;
    reg [63:0] divide_quotient;
    reg [6:0] divide_bit;
    wire [47:0] divide_shifted_remainder =
        {divide_remainder[46:0], divide_numerator[divide_bit]};
    wire divide_take = divide_shifted_remainder >= {1'b0, divide_denominator};
    wire [47:0] divide_remainder_next = divide_take
        ? divide_shifted_remainder - {1'b0, divide_denominator}
        : divide_shifted_remainder;
    wire [63:0] divide_bit_mask = 64'd1 << divide_bit;
    wire [63:0] divide_quotient_next = divide_take
        ? (divide_quotient | divide_bit_mask)
        : divide_quotient;
    wire [48:0] divide_twice_remainder = {divide_remainder_next, 1'b0};
    wire divide_round = (divide_twice_remainder > {2'b00, divide_denominator})
        || ((divide_twice_remainder == {2'b00, divide_denominator})
            && divide_quotient_next[0]);
    wire [64:0] divide_rounded =
        {1'b0, divide_quotient_next} + (divide_round ? 65'd1 : 65'd0);

    wire sample_accept = sample_valid_i && sample_ready_o;
    wire [73:0] square_sum_next =
        square_sum + {10'd0, square_q30[63:0]};
    wire [63:0] mean_quotient = square_sum_next[73:10];
    wire mean_round = (square_sum_next[9:0] > 10'h200)
        || ((square_sum_next[9:0] == 10'h200) && mean_quotient[0]);
    wire [64:0] mean_rounded =
        {1'b0, mean_quotient} + (mean_round ? 65'd1 : 65'd0);
    wire [64:0] mean_with_epsilon = mean_rounded + {1'b0, EPSILON_Q30};

    assign sample_ready_o = (state == ST_COLLECT) && !square_waiting;

    truega_q30_mul_seq square (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(square_start),
        .left_q30_i(square_operand),
        .right_q30_i(square_operand),
        .busy_o(square_busy),
        .done_o(square_done),
        .overflow_o(square_overflow),
        .result_q30_o(square_q30)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            square_operand <= 64'sd0;
            square_start <= 1'b0;
            square_waiting <= 1'b0;
            square_sum <= 74'd0;
            sqrt_radicand <= 94'd0;
            sqrt_remainder <= 49'd0;
            sqrt_root <= 47'd0;
            sqrt_iteration <= 6'd0;
            divide_numerator <= 64'd0;
            divide_denominator <= 47'd0;
            divide_remainder <= 48'd0;
            divide_quotient <= 64'd0;
            divide_bit <= 7'd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            samples_accepted_o <= 11'd0;
            mean_square_q30_o <= 64'sd0;
            inv_rms_q30_o <= 64'sd0;
        end else begin
            done_o <= 1'b0;
            square_start <= 1'b0;

            case (state)
                ST_IDLE: begin
                    busy_o <= 1'b0;
                    if (start_i) begin
                        state <= ST_COLLECT;
                        busy_o <= 1'b1;
                        error_o <= 1'b0;
                        square_waiting <= 1'b0;
                        square_sum <= 74'd0;
                        samples_accepted_o <= 11'd0;
                        mean_square_q30_o <= 64'sd0;
                        inv_rms_q30_o <= 64'sd0;
                    end
                end

                ST_COLLECT: begin
                    if (sample_accept) begin
                        square_operand <= sample_q30_i;
                        square_start <= 1'b1;
                        square_waiting <= 1'b1;
                    end else if (square_waiting && square_done) begin
                        square_waiting <= 1'b0;
                        if (square_overflow || square_q30[63]) begin
                            state <= ST_IDLE;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                        end else if (samples_accepted_o == VECTOR_ELEMENTS - 1) begin
                            samples_accepted_o <= samples_accepted_o + 11'd1;
                            if (mean_with_epsilon[64]
                                || mean_with_epsilon[63]) begin
                                state <= ST_IDLE;
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                error_o <= 1'b1;
                            end else begin
                                mean_square_q30_o <= $signed(mean_with_epsilon[63:0]);
                                sqrt_radicand <= {mean_with_epsilon[63:0], 30'd0};
                                sqrt_remainder <= 49'd0;
                                sqrt_root <= 47'd0;
                                sqrt_iteration <= 6'd0;
                                state <= ST_SQRT;
                            end
                        end else begin
                            square_sum <= square_sum_next;
                            samples_accepted_o <= samples_accepted_o + 11'd1;
                        end
                    end
                end

                ST_SQRT: begin
                    sqrt_radicand <= sqrt_radicand << 2;
                    sqrt_remainder <= sqrt_remainder_next;
                    sqrt_root <= sqrt_root_next;
                    if (sqrt_iteration == 6'd46) begin
                        if (sqrt_root_next == 47'd0) begin
                            state <= ST_IDLE;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                        end else begin
                            divide_numerator <= 64'h1000000000000000;
                            divide_denominator <= sqrt_root_next;
                            divide_remainder <= 48'd0;
                            divide_quotient <= 64'd0;
                            divide_bit <= 7'd63;
                            state <= ST_DIVIDE;
                        end
                    end else begin
                        sqrt_iteration <= sqrt_iteration + 6'd1;
                    end
                end

                ST_DIVIDE: begin
                    divide_remainder <= divide_remainder_next;
                    divide_quotient <= divide_quotient_next;
                    if (divide_bit == 7'd0) begin
                        state <= ST_IDLE;
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        if (divide_rounded[64] || divide_rounded[63]) begin
                            error_o <= 1'b1;
                            inv_rms_q30_o <= 64'sd0;
                        end else begin
                            error_o <= 1'b0;
                            inv_rms_q30_o <= $signed(divide_rounded[63:0]);
                        end
                    end else begin
                        divide_bit <= divide_bit - 7'd1;
                    end
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

    wire unused_square_busy = square_busy;
endmodule
