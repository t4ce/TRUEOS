// Converts an exact Q8_0 block dot and two positive IEEE binary16 scales into
// signed Q30. Right shifts use round-to-nearest, ties-to-even.
module truega_q8_0_scale_q30 (
    input  wire signed [20:0] dot_i,
    input  wire [15:0]        activation_scale_f16_i,
    input  wire [15:0]        weight_scale_f16_i,
    output reg  signed [63:0] term_q30_o,
    output reg                  scale_error_o
);
    reg [10:0] activation_significand;
    reg [10:0] weight_significand;
    reg [5:0] activation_exponent;
    reg [5:0] weight_exponent;
    reg signed [11:0] activation_significand_signed;
    reg signed [11:0] weight_significand_signed;
    reg signed [42:0] raw_product;
    reg signed [63:0] raw_extended;
    reg signed [7:0] scale_shift;
    reg [7:0] right_shift;
    reg [63:0] magnitude;
    reg [63:0] quotient;
    reg [63:0] remainder_mask;
    reg [63:0] remainder;
    reg [63:0] halfway;
    reg [63:0] rounded_magnitude;

    always @* begin
        activation_significand = 11'd0;
        weight_significand = 11'd0;
        activation_exponent = 6'd0;
        weight_exponent = 6'd0;
        scale_error_o = 1'b0;

        if (activation_scale_f16_i[15] || activation_scale_f16_i[14:10] == 5'h1f) begin
            scale_error_o = 1'b1;
        end else if (activation_scale_f16_i[14:10] == 5'd0) begin
            activation_significand = {1'b0, activation_scale_f16_i[9:0]};
            activation_exponent = 6'd1;
        end else begin
            activation_significand = {1'b1, activation_scale_f16_i[9:0]};
            activation_exponent = {1'b0, activation_scale_f16_i[14:10]};
        end

        if (weight_scale_f16_i[15] || weight_scale_f16_i[14:10] == 5'h1f) begin
            scale_error_o = 1'b1;
        end else if (weight_scale_f16_i[14:10] == 5'd0) begin
            weight_significand = {1'b0, weight_scale_f16_i[9:0]};
            weight_exponent = 6'd1;
        end else begin
            weight_significand = {1'b1, weight_scale_f16_i[9:0]};
            weight_exponent = {1'b0, weight_scale_f16_i[14:10]};
        end

        activation_significand_signed = {1'b0, activation_significand};
        weight_significand_signed = {1'b0, weight_significand};
        raw_product = dot_i * activation_significand_signed * weight_significand_signed;
        raw_extended = {{21{raw_product[42]}}, raw_product};
        scale_shift = $signed({1'b0, activation_exponent})
                    + $signed({1'b0, weight_exponent}) - 8'sd20;

        term_q30_o = 64'sd0;
        right_shift = 8'd0;
        magnitude = raw_extended[63] ? (~raw_extended + 64'd1) : raw_extended;
        quotient = 64'd0;
        remainder_mask = 64'd0;
        remainder = 64'd0;
        halfway = 64'd0;
        rounded_magnitude = 64'd0;

        if (scale_error_o || activation_significand == 0 || weight_significand == 0 || dot_i == 0) begin
            term_q30_o = 64'sd0;
        end else if (scale_shift >= 0) begin
            if (scale_shift > 8'sd20) begin
                scale_error_o = 1'b1;
                term_q30_o = 64'sd0;
            end else begin
                term_q30_o = raw_extended <<< scale_shift;
            end
        end else begin
            right_shift = -scale_shift;
            if (right_shift >= 64) begin
                term_q30_o = 64'sd0;
            end else begin
                quotient = magnitude >> right_shift;
                remainder_mask = (64'd1 << right_shift) - 64'd1;
                remainder = magnitude & remainder_mask;
                halfway = 64'd1 << (right_shift - 1'b1);
                rounded_magnitude = quotient
                                  + ((remainder > halfway)
                                  || ((remainder == halfway) && quotient[0]));
                term_q30_o = raw_extended[63] ? -$signed(rounded_magnitude)
                                               : $signed(rounded_magnitude);
            end
        end
    end
endmodule
