// Exact signed Q30 multiply for fixed TRUEGA element slots.
//
// A 64-cycle shift/add multiplier avoids a 64x64 combinational timing path.
// The 128-bit magnitude is shifted by 30 and rounded to nearest, ties to even.
// Inputs outside the signed-Q30 result range complete with overflow_o asserted.
module truega_q30_mul_seq (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire signed [63:0]  left_q30_i,
    input  wire signed [63:0]  right_q30_i,
    output reg                 busy_o,
    output reg                 done_o,
    output reg                 overflow_o,
    output reg signed [63:0]   result_q30_o
);
    localparam [1:0] PHASE_MULTIPLY = 2'd0;
    localparam [1:0] PHASE_ROUND = 2'd1;
    localparam [1:0] PHASE_COMMIT = 2'd2;

    reg [1:0] phase;
    reg [6:0] bit_index;
    reg negative;
    reg [127:0] multiplicand;
    reg [63:0] multiplier;
    reg [127:0] accumulator;
    reg [127:0] product_magnitude;
    reg [97:0] rounded_magnitude;

    wire [63:0] left_magnitude = left_q30_i[63]
        ? (~left_q30_i[63:0] + 64'd1) : left_q30_i[63:0];
    wire [63:0] right_magnitude = right_q30_i[63]
        ? (~right_q30_i[63:0] + 64'd1) : right_q30_i[63:0];
    wire [127:0] accumulator_next = accumulator
        + (multiplier[0] ? multiplicand : 128'd0);
    wire [97:0] quotient = product_magnitude[127:30];
    wire [29:0] remainder = product_magnitude[29:0];
    wire round_increment = (remainder > 30'h20000000)
        || ((remainder == 30'h20000000) && quotient[0]);
    wire [97:0] rounded_next = quotient + round_increment;
    wire result_overflow = negative
        ? (rounded_magnitude > {34'd0, 64'h8000000000000000})
        : (rounded_magnitude > {34'd0, 64'h7fffffffffffffff});

    always @(posedge clk) begin
        if (!reset_n) begin
            phase <= PHASE_MULTIPLY;
            bit_index <= 7'd0;
            negative <= 1'b0;
            multiplicand <= 128'd0;
            multiplier <= 64'd0;
            accumulator <= 128'd0;
            product_magnitude <= 128'd0;
            rounded_magnitude <= 98'd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            overflow_o <= 1'b0;
            result_q30_o <= 64'sd0;
        end else begin
            done_o <= 1'b0;
            if (start_i && !busy_o) begin
                phase <= PHASE_MULTIPLY;
                bit_index <= 7'd0;
                negative <= left_q30_i[63] ^ right_q30_i[63];
                multiplicand <= {64'd0, left_magnitude};
                multiplier <= right_magnitude;
                accumulator <= 128'd0;
                busy_o <= 1'b1;
                overflow_o <= 1'b0;
                result_q30_o <= 64'sd0;
            end else if (busy_o) begin
                case (phase)
                    PHASE_MULTIPLY: begin
                        accumulator <= accumulator_next;
                        multiplicand <= multiplicand << 1;
                        multiplier <= multiplier >> 1;
                        if (bit_index == 7'd63) begin
                            product_magnitude <= accumulator_next;
                            phase <= PHASE_ROUND;
                        end else begin
                            bit_index <= bit_index + 7'd1;
                        end
                    end
                    PHASE_ROUND: begin
                        rounded_magnitude <= rounded_next;
                        phase <= PHASE_COMMIT;
                    end
                    PHASE_COMMIT: begin
                        overflow_o <= result_overflow;
                        if (result_overflow) begin
                            result_q30_o <= 64'sd0;
                        end else if (negative) begin
                            result_q30_o <= -$signed(rounded_magnitude[63:0]);
                        end else begin
                            result_q30_o <= $signed(rounded_magnitude[63:0]);
                        end
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        phase <= PHASE_MULTIPLY;
                    end
                    default: begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        overflow_o <= 1'b1;
                        result_q30_o <= 64'sd0;
                        phase <= PHASE_MULTIPLY;
                    end
                endcase
            end
        end
    end
endmodule
