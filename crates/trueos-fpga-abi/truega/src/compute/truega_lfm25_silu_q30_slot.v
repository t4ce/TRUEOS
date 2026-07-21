// Fixed layer-0 LFM2.5 SiLU(gate) * up datapath.
//
// Inputs and output are signed Q30.  The sigmoid is the odd ninth-order
// expansion around zero, evaluated with one shared sequential multiplier:
//   1/2 + x*(1/4 - x^2/48 + x^4/480 - 17*x^6/80640
//              + 31*x^8/1451520)
// The sealed layer-0 gate is inside +/-1.01; +/-1.125 is enforced so this
// circuit cannot silently operate outside its verified approximation domain.
module truega_lfm25_silu_q30_slot #(
    parameter SILU_ENABLE = 0
) (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire signed [63:0]  gate_q30_i,
    input  wire signed [63:0]  up_q30_i,
    output reg                 busy_o,
    output reg                 done_o,
    output reg                 error_o,
    output reg signed [63:0]   result_q30_o
);
    localparam signed [63:0] GATE_LIMIT_Q30 = 64'sd1207959552; // 1.125
    localparam signed [63:0] UP_LIMIT_Q30   = 64'sd2147483648; // 2.0
    localparam signed [63:0] HALF_Q30 = 64'sd536870912;
    localparam signed [63:0] C1_Q30 = 64'sd268435456;
    localparam signed [63:0] C3_Q30 = -64'sd22369621;
    localparam signed [63:0] C5_Q30 = 64'sd2236962;
    localparam signed [63:0] C7_Q30 = -64'sd226359;
    localparam signed [63:0] C9_Q30 = 64'sd22931;

    localparam [3:0] ST_IDLE = 4'd0;
    localparam [3:0] ST_X2   = 4'd1;
    localparam [3:0] ST_P7   = 4'd2;
    localparam [3:0] ST_P5   = 4'd3;
    localparam [3:0] ST_P3   = 4'd4;
    localparam [3:0] ST_P1   = 4'd5;
    localparam [3:0] ST_SIG  = 4'd6;
    localparam [3:0] ST_SILU = 4'd7;
    localparam [3:0] ST_OUT  = 4'd8;

    reg [3:0] state;
    reg signed [63:0] gate_q30;
    reg signed [63:0] up_q30;
    reg signed [63:0] x2_q30;
    reg signed [63:0] polynomial_q30;
    reg signed [63:0] sigmoid_q30;
    reg signed [63:0] silu_q30;

    reg signed [39:0] multiply_left;
    reg signed [39:0] multiply_right;
    reg multiply_start;
    reg multiply_waiting;
    wire multiply_busy;
    wire multiply_done;
    wire signed [63:0] multiply_q30;
    wire input_range_valid = (gate_q30_i >= -GATE_LIMIT_Q30)
                          && (gate_q30_i <= GATE_LIMIT_Q30)
                          && (up_q30_i >= -UP_LIMIT_Q30)
                          && (up_q30_i <= UP_LIMIT_Q30);

    always @* begin
        multiply_left = 40'sd0;
        multiply_right = 40'sd0;
        case (state)
            ST_X2: begin
                multiply_left = gate_q30[39:0];
                multiply_right = gate_q30[39:0];
            end
            ST_P7, ST_P5, ST_P3, ST_P1: begin
                multiply_left = x2_q30[39:0];
                multiply_right = polynomial_q30[39:0];
            end
            ST_SIG: begin
                multiply_left = gate_q30[39:0];
                multiply_right = polynomial_q30[39:0];
            end
            ST_SILU: begin
                multiply_left = gate_q30[39:0];
                multiply_right = sigmoid_q30[39:0];
            end
            ST_OUT: begin
                multiply_left = silu_q30[39:0];
                multiply_right = up_q30[39:0];
            end
            default: begin end
        endcase
    end

    truega_signed_mul_q30_seq multiply (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(multiply_start),
        .left_i(multiply_left),
        .right_i(multiply_right),
        .busy_o(multiply_busy),
        .done_o(multiply_done),
        .result_q30_o(multiply_q30)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            result_q30_o <= 64'sd0;
            gate_q30 <= 64'sd0;
            up_q30 <= 64'sd0;
            x2_q30 <= 64'sd0;
            polynomial_q30 <= 64'sd0;
            sigmoid_q30 <= 64'sd0;
            silu_q30 <= 64'sd0;
            multiply_start <= 1'b0;
            multiply_waiting <= 1'b0;
        end else begin
            done_o <= 1'b0;
            multiply_start <= 1'b0;
            if (SILU_ENABLE && start_i && !busy_o) begin
                result_q30_o <= 64'sd0;
                if (!input_range_valid) begin
                    state <= ST_IDLE;
                    busy_o <= 1'b0;
                    done_o <= 1'b1;
                    error_o <= 1'b1;
                    multiply_waiting <= 1'b0;
                end else begin
                    gate_q30 <= gate_q30_i;
                    up_q30 <= up_q30_i;
                    state <= ST_X2;
                    busy_o <= 1'b1;
                    error_o <= 1'b0;
                    multiply_waiting <= 1'b0;
                end
            end else if (busy_o) begin
                if (!multiply_waiting) begin
                    multiply_start <= 1'b1;
                    multiply_waiting <= 1'b1;
                end else if (multiply_done) begin
                    multiply_waiting <= 1'b0;
                    case (state)
                        ST_X2: begin
                            x2_q30 <= multiply_q30;
                            polynomial_q30 <= C9_Q30;
                            state <= ST_P7;
                        end
                        ST_P7: begin
                            polynomial_q30 <= C7_Q30 + multiply_q30;
                            state <= ST_P5;
                        end
                        ST_P5: begin
                            polynomial_q30 <= C5_Q30 + multiply_q30;
                            state <= ST_P3;
                        end
                        ST_P3: begin
                            polynomial_q30 <= C3_Q30 + multiply_q30;
                            state <= ST_P1;
                        end
                        ST_P1: begin
                            polynomial_q30 <= C1_Q30 + multiply_q30;
                            state <= ST_SIG;
                        end
                        ST_SIG: begin
                            sigmoid_q30 <= HALF_Q30 + multiply_q30;
                            state <= ST_SILU;
                        end
                        ST_SILU: begin
                            silu_q30 <= multiply_q30;
                            state <= ST_OUT;
                        end
                        ST_OUT: begin
                            result_q30_o <= multiply_q30;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            state <= ST_IDLE;
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
    end

    wire unused_multiply_busy = multiply_busy;
endmodule

// Exact signed 40x40 multiply followed by round-to-nearest-ties-even at Q30.
// One multiplier bit is consumed per cycle; magnitude rounding and sign are
// separately registered so no multiplier/round/add chain crosses one clock.
module truega_signed_mul_q30_seq (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire signed [39:0]  left_i,
    input  wire signed [39:0]  right_i,
    output reg                 busy_o,
    output reg                 done_o,
    output reg signed [63:0]   result_q30_o
);
    localparam [1:0] PHASE_MULTIPLY = 2'd0;
    localparam [1:0] PHASE_ROUND    = 2'd1;
    localparam [1:0] PHASE_SIGN     = 2'd2;

    reg [1:0] phase;
    reg [5:0] bit_index;
    reg negative;
    reg [79:0] multiplicand;
    reg [39:0] multiplier;
    reg [79:0] accumulator;
    reg [79:0] product_magnitude;
    reg [49:0] rounded_magnitude;

    wire [39:0] left_magnitude = left_i[39] ? (~left_i[39:0] + 40'd1) : left_i[39:0];
    wire [39:0] right_magnitude = right_i[39] ? (~right_i[39:0] + 40'd1) : right_i[39:0];
    wire [79:0] addend = multiplier[0] ? multiplicand : 80'd0;
    wire [79:0] accumulator_next = accumulator + addend;
    wire [49:0] quotient = product_magnitude[79:30];
    wire [29:0] remainder = product_magnitude[29:0];
    wire round_increment = (remainder > 30'h20000000)
                        || ((remainder == 30'h20000000) && quotient[0]);

    always @(posedge clk) begin
        if (!reset_n) begin
            busy_o <= 1'b0;
            done_o <= 1'b0;
            result_q30_o <= 64'sd0;
            phase <= PHASE_MULTIPLY;
            bit_index <= 6'd0;
            negative <= 1'b0;
            multiplicand <= 80'd0;
            multiplier <= 40'd0;
            accumulator <= 80'd0;
            product_magnitude <= 80'd0;
            rounded_magnitude <= 50'd0;
        end else begin
            done_o <= 1'b0;
            if (start_i && !busy_o) begin
                busy_o <= 1'b1;
                phase <= PHASE_MULTIPLY;
                bit_index <= 6'd0;
                negative <= left_i[39] ^ right_i[39];
                multiplicand <= {40'd0, left_magnitude};
                multiplier <= right_magnitude;
                accumulator <= 80'd0;
            end else if (busy_o) begin
                case (phase)
                    PHASE_MULTIPLY: begin
                        accumulator <= accumulator_next;
                        multiplicand <= multiplicand << 1;
                        multiplier <= multiplier >> 1;
                        if (bit_index == 6'd39) begin
                            product_magnitude <= accumulator_next;
                            phase <= PHASE_ROUND;
                        end else begin
                            bit_index <= bit_index + 6'd1;
                        end
                    end
                    PHASE_ROUND: begin
                        rounded_magnitude <= quotient + round_increment;
                        phase <= PHASE_SIGN;
                    end
                    PHASE_SIGN: begin
                        result_q30_o <= negative
                            ? -$signed({14'd0, rounded_magnitude})
                            :  $signed({14'd0, rounded_magnitude});
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        phase <= PHASE_MULTIPLY;
                    end
                    default: begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        result_q30_o <= 64'sd0;
                        phase <= PHASE_MULTIPLY;
                    end
                endcase
            end
        end
    end
endmodule
