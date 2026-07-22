// Stable online softmax and value accumulation for one LFM2.5 head element.
//
// One instance is reused for each of the 64 value dimensions.  Scores and
// values are Q30.  The state follows the online recurrence (m, denominator,
// numerator), so no score buffer and no host-side softmax are needed.
// exp(x), x <= 0, uses ln(2) range reduction and a fifth-order polynomial on
// [-ln(2), 0].  Its worst scalar absolute error is below 1.6e-4; tails below
// -16 are zero because they contribute less than 1.2e-7.
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
    localparam signed [63:0] NEG_SIXTEEN_Q30 = -64'sd17179869184;

    function signed [63:0] q30_mul;
        input signed [63:0] left;
        input signed [63:0] right;
        reg negative;
        reg [63:0] left_magnitude;
        reg [63:0] right_magnitude;
        reg [127:0] product;
        reg [97:0] quotient;
        reg [29:0] remainder;
        begin
            negative = left[63] ^ right[63];
            left_magnitude = left[63] ? (~left + 64'd1) : left;
            right_magnitude = right[63] ? (~right + 64'd1) : right;
            product = left_magnitude * right_magnitude;
            quotient = product[127:30];
            remainder = product[29:0];
            if ((remainder > 30'h20000000)
                || ((remainder == 30'h20000000) && quotient[0]))
                quotient = quotient + 98'd1;
            q30_mul = negative ? -$signed(quotient[63:0])
                               :  $signed(quotient[63:0]);
        end
    endfunction

    function signed [63:0] exp_negative_q30;
        input signed [63:0] x;
        integer shift;
        reg signed [63:0] remainder_x;
        reg signed [63:0] polynomial;
        reg [63:0] positive_polynomial;
        reg [63:0] shifted;
        reg [63:0] discarded_mask;
        reg [63:0] discarded;
        reg [63:0] half;
        begin
            if (x >= 0) begin
                exp_negative_q30 = Q30_ONE;
            end else if (x <= NEG_SIXTEEN_Q30) begin
                exp_negative_q30 = 64'sd0;
            end else begin
                shift = (-x) / LN2_Q30;
                remainder_x = x + shift * LN2_Q30;
                polynomial = C5_Q30;
                polynomial = C4_Q30 + q30_mul(polynomial, remainder_x);
                polynomial = C3_Q30 + q30_mul(polynomial, remainder_x);
                polynomial = C2_Q30 + q30_mul(polynomial, remainder_x);
                polynomial = Q30_ONE + q30_mul(polynomial, remainder_x);
                polynomial = Q30_ONE + q30_mul(polynomial, remainder_x);
                positive_polynomial = polynomial[63:0];
                if (shift == 0) begin
                    shifted = positive_polynomial;
                end else begin
                    shifted = positive_polynomial >> shift;
                    discarded_mask = (64'd1 << shift) - 64'd1;
                    discarded = positive_polynomial & discarded_mask;
                    half = 64'd1 << (shift - 1);
                    if ((discarded > half)
                        || ((discarded == half) && shifted[0]))
                        shifted = shifted + 64'd1;
                end
                exp_negative_q30 = $signed(shifted);
            end
        end
    endfunction

    function signed [63:0] q30_div;
        input signed [63:0] numerator;
        input signed [63:0] denominator;
        reg negative;
        reg [63:0] numerator_magnitude;
        reg [63:0] denominator_magnitude;
        reg [127:0] dividend;
        reg [127:0] quotient;
        reg [63:0] remainder;
        begin
            negative = numerator[63] ^ denominator[63];
            numerator_magnitude = numerator[63]
                ? (~numerator + 64'd1) : numerator;
            denominator_magnitude = denominator[63]
                ? (~denominator + 64'd1) : denominator;
            dividend = {34'd0, numerator_magnitude, 30'd0};
            quotient = dividend / denominator_magnitude;
            remainder = dividend % denominator_magnitude;
            if (({remainder, 1'b0} > denominator_magnitude)
                || (({remainder, 1'b0} == denominator_magnitude)
                    && quotient[0]))
                quotient = quotient + 128'd1;
            q30_div = negative ? -$signed(quotient[63:0])
                               :  $signed(quotient[63:0]);
        end
    endfunction

    reg active;
    reg begin_latched;
    reg last_latched;
    reg signed [63:0] score_latched;
    reg signed [63:0] value_latched;
    reg signed [63:0] maximum_q30;
    reg signed [63:0] denominator_q30;
    reg signed [63:0] numerator_q30;

    reg signed [63:0] alpha;
    reg signed [63:0] beta;
    reg signed [63:0] next_maximum;
    reg signed [63:0] next_denominator;
    reg signed [63:0] next_numerator;
    always @* begin
        alpha = Q30_ONE;
        beta = Q30_ONE;
        next_maximum = score_latched;
        next_denominator = Q30_ONE;
        next_numerator = value_latched;
        if (!begin_latched) begin
            if (score_latched > maximum_q30) begin
                alpha = exp_negative_q30(maximum_q30 - score_latched);
                next_maximum = score_latched;
                next_denominator = q30_mul(denominator_q30, alpha) + Q30_ONE;
                next_numerator = q30_mul(numerator_q30, alpha) + value_latched;
            end else begin
                beta = exp_negative_q30(score_latched - maximum_q30);
                next_maximum = maximum_q30;
                next_denominator = denominator_q30 + beta;
                next_numerator = numerator_q30 + q30_mul(value_latched, beta);
            end
        end
    end

    always @(posedge clk) begin
        if (!reset_n) begin
            active <= 1'b0;
            begin_latched <= 1'b0;
            last_latched <= 1'b0;
            score_latched <= 64'sd0;
            value_latched <= 64'sd0;
            maximum_q30 <= 64'sd0;
            denominator_q30 <= 64'sd0;
            numerator_q30 <= 64'sd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            result_valid_o <= 1'b0;
            result_q30_o <= 64'sd0;
        end else begin
            done_o <= 1'b0;
            result_valid_o <= 1'b0;
            if (start_i && !busy_o) begin
                begin_latched <= begin_i;
                last_latched <= last_i;
                score_latched <= score_q30_i;
                value_latched <= value_q30_i;
                busy_o <= 1'b1;
                error_o <= 1'b0;
            end else if (busy_o) begin
                busy_o <= 1'b0;
                done_o <= 1'b1;
                if ((!active && !begin_latched) || (active && begin_latched)) begin
                    error_o <= 1'b1;
                    active <= 1'b0;
                end else if (next_denominator <= 0) begin
                    error_o <= 1'b1;
                    active <= 1'b0;
                end else if (last_latched) begin
                    result_q30_o <= q30_div(next_numerator, next_denominator);
                    result_valid_o <= 1'b1;
                    active <= 1'b0;
                end else begin
                    maximum_q30 <= next_maximum;
                    denominator_q30 <= next_denominator;
                    numerator_q30 <= next_numerator;
                    active <= 1'b1;
                end
            end
        end
    end
endmodule
