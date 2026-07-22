// Exact fixed-circuit reduction for one LFM2.5 Q or K head (64 elements).
//
// Input and output are signed Q30.  Squares use the shared ties-even Q30
// multiplier, the mean uses ties-even /64, and epsilon is the pinned GGUF F32
// value 9.999999747e-6 rounded to Q30 (10737).  A restoring integer square
// root computes sqrt(mean + epsilon), followed by a restoring divide of 2^60
// by that Q30 root.  Thus inv_rms_o is generated entirely in FPGA logic.
module truega_lfm25_head_rms_inverse_slot (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire                sample_valid_i,
    input  wire signed [63:0]  sample_q30_i,
    output wire                sample_ready_o,
    output reg                 busy_o,
    output reg                 done_o,
    output reg                 error_o,
    output reg signed [63:0]   inv_rms_q30_o
);
    localparam [1:0] ST_IDLE = 2'd0;
    localparam [1:0] ST_LOAD = 2'd1;
    localparam [1:0] ST_SQRT = 2'd2;
    localparam [1:0] ST_DIV  = 2'd3;
    localparam [63:0] EPSILON_Q30 = 64'd10737;

    reg [1:0] state;
    reg [6:0] sample_count;
    reg multiply_start;
    reg multiply_waiting;
    reg signed [63:0] sample;
    wire multiply_busy;
    wire multiply_done;
    wire multiply_overflow;
    wire signed [63:0] square_q30;
    reg [70:0] sum_squares_q30;

    reg [95:0] sqrt_operand;
    reg [95:0] sqrt_result;
    reg [95:0] sqrt_one;
    reg [5:0] sqrt_iteration;
    wire sqrt_take = sqrt_operand >= (sqrt_result + sqrt_one);
    wire [95:0] sqrt_operand_next = sqrt_take
        ? sqrt_operand - (sqrt_result + sqrt_one) : sqrt_operand;
    wire [95:0] sqrt_result_next = sqrt_take
        ? (sqrt_result >> 1) + sqrt_one : (sqrt_result >> 1);

    reg [47:0] root_q30;
    reg [63:0] divide_remainder;
    reg [63:0] divide_quotient;
    reg [6:0] divide_bit;
    wire divide_numerator_bit = divide_bit == 7'd60;
    wire [64:0] divide_shifted =
        {1'b0, divide_remainder, divide_numerator_bit};
    wire divide_take = divide_shifted >= {17'd0, root_q30};
    wire [63:0] divide_remainder_next = divide_take
        ? divide_shifted[63:0] - root_q30 : divide_shifted[63:0];
    wire [63:0] divide_quotient_next = divide_take
        ? divide_quotient | (64'd1 << divide_bit) : divide_quotient;
    wire divide_round_up = ({divide_remainder_next, 1'b0} > root_q30)
        || (({divide_remainder_next, 1'b0} == root_q30)
            && divide_quotient_next[0]);

    wire [70:0] sum_squares_next = sum_squares_q30
        + {{7{square_q30[63]}}, square_q30};
    wire [64:0] mean_truncated = sum_squares_next[70:6];
    wire mean_round_up = (sum_squares_next[5:0] > 6'd32)
        || ((sum_squares_next[5:0] == 6'd32) && mean_truncated[0]);
    wire [64:0] mean_rounded = mean_truncated + mean_round_up;
    wire [65:0] mean_epsilon = {1'b0, mean_rounded}
        + {2'b0, EPSILON_Q30};

    assign sample_ready_o = busy_o && state == ST_LOAD && !multiply_waiting;

    truega_q30_mul_seq square_multiply (
        .clk(clk), .reset_n(reset_n), .start_i(multiply_start),
        .left_q30_i(sample), .right_q30_i(sample),
        .busy_o(multiply_busy), .done_o(multiply_done),
        .overflow_o(multiply_overflow), .result_q30_o(square_q30)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            sample_count <= 7'd0;
            multiply_start <= 1'b0;
            multiply_waiting <= 1'b0;
            sample <= 64'sd0;
            sum_squares_q30 <= 71'd0;
            sqrt_operand <= 96'd0;
            sqrt_result <= 96'd0;
            sqrt_one <= 96'd0;
            sqrt_iteration <= 6'd0;
            root_q30 <= 48'd0;
            divide_remainder <= 64'd0;
            divide_quotient <= 64'd0;
            divide_bit <= 7'd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            inv_rms_q30_o <= 64'sd0;
        end else begin
            done_o <= 1'b0;
            multiply_start <= 1'b0;
            if (start_i && !busy_o) begin
                state <= ST_LOAD;
                sample_count <= 7'd0;
                multiply_waiting <= 1'b0;
                sum_squares_q30 <= 71'd0;
                busy_o <= 1'b1;
                error_o <= 1'b0;
                inv_rms_q30_o <= 64'sd0;
            end else if (busy_o) begin
                case (state)
                    ST_LOAD: begin
                        if (sample_valid_i && !multiply_waiting) begin
                            sample <= sample_q30_i;
                            multiply_start <= 1'b1;
                            multiply_waiting <= 1'b1;
                        end else if (multiply_waiting && multiply_done) begin
                            multiply_waiting <= 1'b0;
                            if (multiply_overflow || square_q30[63]) begin
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                error_o <= 1'b1;
                                state <= ST_IDLE;
                            end else if (sample_count == 7'd63) begin
                                // sqrt input is Q60: (mean_q30 + eps_q30) << 30.
                                sqrt_operand <= {mean_epsilon, 30'd0};
                                sqrt_result <= 96'd0;
                                sqrt_one <= 96'd1 << 94;
                                sqrt_iteration <= 6'd0;
                                state <= ST_SQRT;
                            end else begin
                                sum_squares_q30 <= sum_squares_next;
                                sample_count <= sample_count + 7'd1;
                            end
                        end
                    end
                    ST_SQRT: begin
                        sqrt_operand <= sqrt_operand_next;
                        sqrt_result <= sqrt_result_next;
                        sqrt_one <= sqrt_one >> 2;
                        if (sqrt_iteration == 6'd47) begin
                            if (sqrt_result_next[95:48] != 48'd0
                                || sqrt_result_next[47:0] == 48'd0) begin
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                error_o <= 1'b1;
                                state <= ST_IDLE;
                            end else begin
                                root_q30 <= sqrt_result_next[47:0];
                                divide_remainder <= 64'd0;
                                divide_quotient <= 64'd0;
                                divide_bit <= 7'd60;
                                state <= ST_DIV;
                            end
                        end else begin
                            sqrt_iteration <= sqrt_iteration + 6'd1;
                        end
                    end
                    ST_DIV: begin
                        divide_remainder <= divide_remainder_next;
                        divide_quotient <= divide_quotient_next;
                        if (divide_bit == 7'd0) begin
                            if (divide_quotient_next[63]
                                || (divide_round_up && divide_quotient_next == 64'h7fffffffffffffff)) begin
                                error_o <= 1'b1;
                                inv_rms_q30_o <= 64'sd0;
                            end else begin
                                inv_rms_q30_o <= $signed(divide_quotient_next
                                    + divide_round_up);
                            end
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
                        state <= ST_IDLE;
                    end
                endcase
            end
        end
    end

    wire unused_multiply_busy = multiply_busy;
endmodule
