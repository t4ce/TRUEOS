// Fixed 32-element signed-Q30 to native GGML Q8_0 block quantizer.
//
// This implements the pinned Rust/ggml arithmetic order exactly:
//   F32 sample = (i64 sample) / 2^30
//   F32 inverse = 127.0f / max(abs(sample))
//   quant = RNE(F32(sample * inverse))
//   F16 scale = F16(F32(max / 127.0f))
// Integer-to-F32, both F32 operations, final ties-to-even, and F16 conversion
// are represented explicitly. One divider and one shift/add multiplier are
// shared by all 32 values; no soft processor or runtime format machinery exists.
module truega_q30_to_q8_0_block_slot (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire                sample_valid_i,
    input  wire signed [63:0]  sample_q30_i,
    output wire                sample_ready_o,
    output reg                 busy_o,
    output reg                 done_o,
    output reg                 error_o,
    output reg [5:0]           samples_accepted_o,
    output reg [271:0]         q8_block_o
);
    localparam [3:0] ST_IDLE           = 4'd0;
    localparam [3:0] ST_COLLECT        = 4'd1;
    localparam [3:0] ST_SCALE_PREP     = 4'd2;
    localparam [3:0] ST_SCALE_DIVIDE   = 4'd3;
    localparam [3:0] ST_INVERSE_PREP   = 4'd4;
    localparam [3:0] ST_INVERSE_DIVIDE = 4'd5;
    localparam [3:0] ST_QUANT_PREP     = 4'd6;
    localparam [3:0] ST_QUANT_MULTIPLY = 4'd7;
    localparam [3:0] ST_QUANT_ROUND    = 4'd8;

    reg [3:0] state;
    reg [63:0] sample_magnitudes [0:31];
    reg sample_negatives [0:31];
    reg [63:0] maximum;
    reg [5:0] quant_index;

    reg [47:0] divide_numerator;
    reg [24:0] divide_remainder;
    reg [47:0] divide_quotient;
    reg [23:0] divide_divisor;
    reg [5:0] divide_bit;
    reg [23:0] scale_significand;
    reg signed [8:0] scale_exponent;
    reg [23:0] inverse_significand;
    reg signed [8:0] inverse_exponent;

    reg [47:0] product_accumulator;
    reg [47:0] product_multiplicand;
    reg [4:0] multiply_bit;
    reg [6:0] product_sample_msb;
    integer index;

    wire [63:0] sample_magnitude = sample_q30_i[63]
        ? (~sample_q30_i[63:0] + 64'd1)
        : sample_q30_i[63:0];

    // IEEE-754 integer-to-F32 RNE, kept as the equivalent rounded integer.
    function automatic [63:0] round_magnitude_to_fp32;
        input [63:0] magnitude;
        reg [6:0] msb;
        reg found;
        reg [6:0] precision_shift;
        reg [63:0] truncated;
        reg [63:0] discard_mask;
        reg [63:0] discarded;
        reg [63:0] halfway;
        reg [64:0] rounded_significand;
        integer magnitude_scan;
        begin
            msb = 7'd0;
            found = 1'b0;
            for (magnitude_scan = 63; magnitude_scan >= 0;
                 magnitude_scan = magnitude_scan - 1) begin
                if (!found && magnitude[magnitude_scan]) begin
                    msb = magnitude_scan[6:0];
                    found = 1'b1;
                end
            end
            round_magnitude_to_fp32 = magnitude;
            if (found && msb > 7'd23) begin
                precision_shift = msb - 7'd23;
                truncated = magnitude >> precision_shift;
                discard_mask = (64'd1 << precision_shift) - 64'd1;
                discarded = magnitude & discard_mask;
                halfway = 64'd1 << (precision_shift - 1'b1);
                rounded_significand = {1'b0, truncated}
                    + ((discarded > halfway)
                       || ((discarded == halfway) && truncated[0]));
                round_magnitude_to_fp32 =
                    rounded_significand << precision_shift;
            end
        end
    endfunction

    wire [63:0] sample_fp32_magnitude =
        round_magnitude_to_fp32(sample_magnitude);
    wire sample_accept = sample_valid_i && sample_ready_o;
    wire [63:0] maximum_with_sample = sample_fp32_magnitude > maximum
        ? sample_fp32_magnitude : maximum;

    // Normalize the F32-rounded maximum into a 24-bit significand.
    reg [6:0] maximum_msb;
    reg maximum_found;
    reg [23:0] maximum_significand;
    integer maximum_scan;
    always @* begin
        maximum_msb = 7'd0;
        maximum_found = 1'b0;
        for (maximum_scan = 63; maximum_scan >= 0;
             maximum_scan = maximum_scan - 1) begin
            if (!maximum_found && maximum[maximum_scan]) begin
                maximum_msb = maximum_scan[6:0];
                maximum_found = 1'b1;
            end
        end
        maximum_significand = 24'd0;
        if (maximum_found) begin
            if (maximum_msb > 7'd23)
                maximum_significand = maximum >> (maximum_msb - 7'd23);
            else
                maximum_significand = maximum << (7'd23 - maximum_msb);
        end
    end

    // 127/max has normalized exponent -17 except for the very top of the
    // 24-bit significand interval, where it is -18.
    wire inverse_low_interval = maximum_significand <= 24'd16646144;
    wire [47:0] inverse_dividend = inverse_low_interval
        ? (48'd127 << 40) : (48'd127 << 41);
    wire signed [8:0] inverse_base_exponent = inverse_low_interval
        ? (9'sd36 - $signed({2'b00, maximum_msb}))
        : (9'sd35 - $signed({2'b00, maximum_msb}));

    wire [25:0] divide_shifted_remainder =
        {1'b0, divide_remainder[23:0], divide_numerator[47]};
    wire divide_take =
        divide_shifted_remainder >= {2'b00, divide_divisor};
    wire [24:0] divide_remainder_next = divide_take
        ? divide_shifted_remainder - {2'b00, divide_divisor}
        : divide_shifted_remainder[24:0];
    wire [47:0] divide_quotient_next = divide_take
        ? divide_quotient | (48'd1 << divide_bit) : divide_quotient;
    wire divide_round =
        ({divide_remainder_next, 1'b0} > {2'b00, divide_divisor})
        || (({divide_remainder_next, 1'b0} == {2'b00, divide_divisor})
            && divide_quotient_next[0]);
    wire [48:0] divide_rounded = {1'b0, divide_quotient_next}
        + (divide_round ? 49'd1 : 49'd0);

    // F32 max/127 followed by exact F32-to-F16 RNE for the two block bytes.
    wire scale_high_interval = maximum_significand >= 24'd16646144;
    wire [30:0] scale_dividend = scale_high_interval
        ? ({7'd0, maximum_significand} << 6)
        : ({7'd0, maximum_significand} << 7);
    wire signed [8:0] scale_base_exponent = scale_high_interval
        ? ($signed({2'b00, maximum_msb}) - 9'sd36)
        : ($signed({2'b00, maximum_msb}) - 9'sd37);

    reg [15:0] scale_f16;
    reg [24:0] scale_f16_truncated;
    reg [24:0] scale_f16_discarded;
    reg [24:0] scale_f16_halfway;
    reg [24:0] scale_f16_rounded;
    reg [5:0] scale_f16_exponent;
    integer scale_subnormal_shift;
    always @* begin
        scale_f16 = 16'd0;
        scale_f16_truncated = 25'd0;
        scale_f16_discarded = 25'd0;
        scale_f16_halfway = 25'd0;
        scale_f16_rounded = 25'd0;
        scale_f16_exponent = 6'd0;
        scale_subnormal_shift = 0;
        if (!maximum_found) begin
            scale_f16 = 16'd0;
        end else if (scale_exponent > 9'sd15) begin
            scale_f16 = 16'h7c00;
        end else if (scale_exponent >= -9'sd14) begin
            scale_f16_truncated = scale_significand >> 13;
            scale_f16_discarded = scale_significand & 25'h0001fff;
            scale_f16_halfway = 25'h0001000;
            scale_f16_rounded = scale_f16_truncated
                + ((scale_f16_discarded > scale_f16_halfway)
                   || ((scale_f16_discarded == scale_f16_halfway)
                       && scale_f16_truncated[0]));
            scale_f16_exponent = scale_exponent + 9'sd15;
            if (scale_f16_rounded >= 25'd2048) begin
                scale_f16_rounded = scale_f16_rounded >> 1;
                scale_f16_exponent = scale_f16_exponent + 6'd1;
            end
            if (scale_f16_exponent >= 6'd31)
                scale_f16 = 16'h7c00;
            else
                scale_f16 = {1'b0, scale_f16_exponent[4:0],
                             scale_f16_rounded[9:0]};
        end else begin
            // F16 subnormal units are 2^-24: shift the normalized F32
            // significand by -(exponent+1), retaining RNE.
            scale_subnormal_shift = -($signed(scale_exponent) + 1);
            if (scale_subnormal_shift <= 24) begin
                scale_f16_truncated =
                    scale_significand >> scale_subnormal_shift;
                scale_f16_discarded = scale_significand
                    & ((25'd1 << scale_subnormal_shift) - 25'd1);
                scale_f16_halfway = 25'd1 << (scale_subnormal_shift - 1);
                scale_f16_rounded = scale_f16_truncated
                    + ((scale_f16_discarded > scale_f16_halfway)
                       || ((scale_f16_discarded == scale_f16_halfway)
                           && scale_f16_truncated[0]));
                if (scale_f16_rounded >= 25'd1024)
                    scale_f16 = 16'h0400;
                else
                    scale_f16 = {6'd0, scale_f16_rounded[9:0]};
            end
        end
    end

    // Normalize the selected F32-rounded sample.
    reg [6:0] active_sample_msb;
    reg active_sample_found;
    reg [23:0] active_sample_significand;
    integer active_scan;
    always @* begin
        active_sample_msb = 7'd0;
        active_sample_found = 1'b0;
        for (active_scan = 63; active_scan >= 0; active_scan = active_scan - 1) begin
            if (!active_sample_found
                    && sample_magnitudes[quant_index][active_scan]) begin
                active_sample_msb = active_scan[6:0];
                active_sample_found = 1'b1;
            end
        end
        active_sample_significand = 24'd0;
        if (active_sample_found) begin
            if (active_sample_msb > 7'd23)
                active_sample_significand =
                    sample_magnitudes[quant_index]
                    >> (active_sample_msb - 7'd23);
            else
                active_sample_significand =
                    sample_magnitudes[quant_index]
                    << (7'd23 - active_sample_msb);
        end
    end

    wire [47:0] product_accumulator_next =
        active_sample_significand[multiply_bit]
        ? product_accumulator + product_multiplicand : product_accumulator;

    // Round the exact 24x24 product to F32, then RNE that F32 to an integer.
    reg [5:0] product_msb;
    reg product_found;
    reg [5:0] product_precision_shift;
    reg [48:0] product_truncated;
    reg [47:0] product_discard_mask;
    reg [47:0] product_discarded;
    reg [47:0] product_halfway;
    reg [48:0] product_significand_rounded;
    reg [23:0] product_significand;
    reg signed [9:0] product_exponent;
    reg [5:0] integer_shift;
    reg [23:0] integer_truncated;
    reg [23:0] integer_discard_mask;
    reg [23:0] integer_discarded;
    reg [23:0] integer_halfway;
    reg [8:0] integer_rounded;
    reg product_error;
    integer product_scan;
    always @* begin
        product_msb = 6'd0;
        product_found = 1'b0;
        for (product_scan = 47; product_scan >= 0; product_scan = product_scan - 1) begin
            if (!product_found && product_accumulator[product_scan]) begin
                product_msb = product_scan[5:0];
                product_found = 1'b1;
            end
        end
        product_precision_shift = product_msb - 6'd23;
        product_truncated = {1'b0, product_accumulator}
            >> product_precision_shift;
        product_discard_mask = (48'd1 << product_precision_shift) - 48'd1;
        product_discarded = product_accumulator & product_discard_mask;
        product_halfway = 48'd1 << (product_precision_shift - 1'b1);
        product_significand_rounded = product_truncated
            + ((product_discarded > product_halfway)
               || ((product_discarded == product_halfway)
                   && product_truncated[0]));
        product_exponent = $signed({3'b000, product_sample_msb})
            + inverse_exponent
            + $signed({4'b0000, product_msb}) - 10'sd76;
        if (product_significand_rounded[24]) begin
            product_significand = product_significand_rounded[24:1];
            product_exponent = product_exponent + 10'sd1;
        end else begin
            product_significand = product_significand_rounded[23:0];
        end

        integer_shift = 6'd0;
        integer_truncated = 24'd0;
        integer_discard_mask = 24'd0;
        integer_discarded = 24'd0;
        integer_halfway = 24'd0;
        integer_rounded = 9'd0;
        product_error = 1'b0;
        if (!product_found) begin
            integer_rounded = 9'd0;
        end else if (product_exponent < -10'sd1) begin
            integer_rounded = 9'd0;
        end else if (product_exponent == -10'sd1) begin
            integer_rounded = product_significand > 24'h800000
                ? 9'd1 : 9'd0;
        end else if (product_exponent > 10'sd6) begin
            integer_rounded = 9'd256;
            product_error = 1'b1;
        end else begin
            integer_shift = 6'd23 - product_exponent[5:0];
            integer_truncated = product_significand >> integer_shift;
            integer_discard_mask = (24'd1 << integer_shift) - 24'd1;
            integer_discarded = product_significand & integer_discard_mask;
            integer_halfway = 24'd1 << (integer_shift - 1'b1);
            integer_rounded = {1'b0, integer_truncated[7:0]}
                + ((integer_discarded > integer_halfway)
                   || ((integer_discarded == integer_halfway)
                       && integer_truncated[0]));
            if (integer_rounded > 9'd127)
                product_error = 1'b1;
        end
    end

    wire [7:0] signed_quant = sample_negatives[quant_index]
        ? (~integer_rounded[7:0] + 8'd1) : integer_rounded[7:0];
    assign sample_ready_o = state == ST_COLLECT;

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            maximum <= 64'd0;
            quant_index <= 6'd0;
            divide_numerator <= 48'd0;
            divide_remainder <= 25'd0;
            divide_quotient <= 48'd0;
            divide_divisor <= 24'd0;
            divide_bit <= 6'd0;
            scale_significand <= 24'd0;
            scale_exponent <= 9'sd0;
            inverse_significand <= 24'd0;
            inverse_exponent <= 9'sd0;
            product_accumulator <= 48'd0;
            product_multiplicand <= 48'd0;
            multiply_bit <= 5'd0;
            product_sample_msb <= 7'd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            samples_accepted_o <= 6'd0;
            q8_block_o <= 272'd0;
            for (index = 0; index < 32; index = index + 1) begin
                sample_magnitudes[index] <= 64'd0;
                sample_negatives[index] <= 1'b0;
            end
        end else begin
            done_o <= 1'b0;
            case (state)
                ST_IDLE: begin
                    busy_o <= 1'b0;
                    if (start_i) begin
                        state <= ST_COLLECT;
                        busy_o <= 1'b1;
                        error_o <= 1'b0;
                        maximum <= 64'd0;
                        samples_accepted_o <= 6'd0;
                        q8_block_o <= 272'd0;
                    end
                end

                ST_COLLECT: begin
                    if (sample_accept) begin
                        sample_magnitudes[samples_accepted_o[4:0]]
                            <= sample_fp32_magnitude;
                        sample_negatives[samples_accepted_o[4:0]]
                            <= sample_q30_i[63];
                        maximum <= maximum_with_sample;
                        samples_accepted_o <= samples_accepted_o + 6'd1;
                        if (samples_accepted_o == 6'd31)
                            state <= ST_SCALE_PREP;
                    end
                end

                ST_SCALE_PREP: begin
                    if (!maximum_found) begin
                        q8_block_o <= 272'd0;
                        state <= ST_IDLE;
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                    end else begin
                        // Align bit 30 of the 31-bit numerator with the shared
                        // divider's fixed input bit. This is max/127 rounded
                        // once to F32, before the later F16 conversion.
                        divide_numerator <= {scale_dividend, 17'd0};
                        divide_remainder <= 25'd0;
                        divide_quotient <= 48'd0;
                        divide_divisor <= 24'd127;
                        divide_bit <= 6'd30;
                        state <= ST_SCALE_DIVIDE;
                    end
                end

                ST_SCALE_DIVIDE: begin
                    divide_numerator <= {divide_numerator[46:0], 1'b0};
                    divide_remainder <= divide_remainder_next;
                    divide_quotient <= divide_quotient_next;
                    if (divide_bit == 6'd0) begin
                        if (divide_rounded == 49'h0000001000000) begin
                            scale_significand <= 24'h800000;
                            scale_exponent <= scale_base_exponent + 9'sd1;
                            state <= ST_INVERSE_PREP;
                        end else if (divide_rounded < 49'h0000000800000
                                || divide_rounded > 49'h0000000ffffff) begin
                            state <= ST_IDLE;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                        end else begin
                            scale_significand <= divide_rounded[23:0];
                            scale_exponent <= scale_base_exponent;
                            state <= ST_INVERSE_PREP;
                        end
                    end else begin
                        divide_bit <= divide_bit - 6'd1;
                    end
                end

                ST_INVERSE_PREP: begin
                    q8_block_o[15:0] <= scale_f16;
                    divide_numerator <= inverse_dividend;
                    divide_remainder <= 25'd0;
                    divide_quotient <= 48'd0;
                    divide_divisor <= maximum_significand;
                    divide_bit <= 6'd47;
                    inverse_exponent <= inverse_base_exponent;
                    state <= ST_INVERSE_DIVIDE;
                end

                ST_INVERSE_DIVIDE: begin
                    divide_numerator <= {divide_numerator[46:0], 1'b0};
                    divide_remainder <= divide_remainder_next;
                    divide_quotient <= divide_quotient_next;
                    if (divide_bit == 6'd0) begin
                        if (divide_rounded == 49'h0000001000000) begin
                            inverse_significand <= 24'h800000;
                            inverse_exponent <= inverse_exponent + 9'sd1;
                            quant_index <= 6'd0;
                            state <= ST_QUANT_PREP;
                        end else if (divide_rounded < 49'h0000000800000
                                || divide_rounded > 49'h0000000ffffff) begin
                            state <= ST_IDLE;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                        end else begin
                            inverse_significand <= divide_rounded[23:0];
                            quant_index <= 6'd0;
                            state <= ST_QUANT_PREP;
                        end
                    end else begin
                        divide_bit <= divide_bit - 6'd1;
                    end
                end

                ST_QUANT_PREP: begin
                    if (!active_sample_found) begin
                        q8_block_o[16 + quant_index * 8 +: 8] <= 8'd0;
                        if (quant_index == 6'd31) begin
                            state <= ST_IDLE;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                        end else begin
                            quant_index <= quant_index + 6'd1;
                        end
                    end else begin
                        product_accumulator <= 48'd0;
                        product_multiplicand <= {24'd0, inverse_significand};
                        multiply_bit <= 5'd0;
                        product_sample_msb <= active_sample_msb;
                        state <= ST_QUANT_MULTIPLY;
                    end
                end

                ST_QUANT_MULTIPLY: begin
                    product_accumulator <= product_accumulator_next;
                    product_multiplicand <= {product_multiplicand[46:0], 1'b0};
                    if (multiply_bit == 5'd23)
                        state <= ST_QUANT_ROUND;
                    else
                        multiply_bit <= multiply_bit + 5'd1;
                end

                ST_QUANT_ROUND: begin
                    if (product_error) begin
                        state <= ST_IDLE;
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= 1'b1;
                    end else begin
                        q8_block_o[16 + quant_index * 8 +: 8] <= signed_quant;
                        if (quant_index == 6'd31) begin
                            state <= ST_IDLE;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b0;
                        end else begin
                            quant_index <= quant_index + 6'd1;
                            state <= ST_QUANT_PREP;
                        end
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
endmodule
