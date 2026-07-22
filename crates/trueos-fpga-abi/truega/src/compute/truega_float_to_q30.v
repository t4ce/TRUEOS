// Deterministic IEEE-754 weight decode for fixed TRUEGA tensor slots.
//
// format_bf16_i=0 consumes a binary32 bit pattern. format_bf16_i=1 consumes
// BF16 in bits_i[15:0] (the sealed TRUEGA model format) and restores its zero
// low mantissa bits. Finite values are converted to signed Q30 with
// round-to-nearest, ties-to-even. NaN, infinity, and Q30 overflow assert
// error_o. IEEE subnormals are far below one Q30 LSB and therefore round to 0.
module truega_float_to_q30 (
    input  wire               format_bf16_i,
    input  wire [31:0]        bits_i,
    output reg signed [63:0]  q30_o,
    output reg                error_o
);
    reg [31:0] fp_bits;
    reg sign;
    reg [7:0] exponent;
    reg [22:0] fraction;
    reg [23:0] significand;
    reg [127:0] magnitude;
    reg [127:0] quotient;
    reg [127:0] remainder;
    reg [127:0] remainder_mask;
    reg [127:0] halfway;
    reg [127:0] rounded;
    integer shift;
    integer right_shift;

    always @* begin
        fp_bits = format_bf16_i ? {bits_i[15:0], 16'd0} : bits_i;
        sign = fp_bits[31];
        exponent = fp_bits[30:23];
        fraction = fp_bits[22:0];
        significand = {1'b1, fraction};
        magnitude = 128'd0;
        quotient = 128'd0;
        remainder = 128'd0;
        remainder_mask = 128'd0;
        halfway = 128'd0;
        rounded = 128'd0;
        shift = 0;
        right_shift = 0;
        q30_o = 64'sd0;
        error_o = 1'b0;

        if (exponent == 8'hff) begin
            error_o = 1'b1;
        end else if (exponent == 8'd0) begin
            // Zero and all binary32/BF16 subnormals are < 0.5 Q30 LSB.
            q30_o = 64'sd0;
        end else begin
            // significand represents 1.fraction * 2^23. Multiplication by
            // 2^30 makes the exact integer shift exponent - 120.
            shift = $signed({1'b0, exponent}) - 120;
            if (shift >= 0) begin
                if (shift > 103) begin
                    error_o = 1'b1;
                end else begin
                    magnitude = {104'd0, significand} << shift;
                    rounded = magnitude;
                end
            end else begin
                right_shift = -shift;
                if (right_shift >= 128) begin
                    rounded = 128'd0;
                end else begin
                    magnitude = {104'd0, significand};
                    quotient = magnitude >> right_shift;
                    remainder_mask = (128'd1 << right_shift) - 128'd1;
                    remainder = magnitude & remainder_mask;
                    halfway = 128'd1 << (right_shift - 1);
                    rounded = quotient
                        + ((remainder > halfway)
                           || ((remainder == halfway) && quotient[0]));
                end
            end

            if (!error_o) begin
                if ((!sign && rounded > 128'h00000000000000007fffffffffffffff)
                    || (sign && rounded > 128'h00000000000000008000000000000000)) begin
                    error_o = 1'b1;
                    q30_o = 64'sd0;
                end else if (sign) begin
                    q30_o = -$signed(rounded[63:0]);
                end else begin
                    q30_o = $signed(rounded[63:0]);
                end
            end
        end
    end
endmodule
