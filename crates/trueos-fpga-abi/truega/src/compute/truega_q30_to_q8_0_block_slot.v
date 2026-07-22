// Fixed 32-element signed-Q30 to native GGML Q8_0 block quantizer.
//
// This closes the on-device activation boundary used by the LFM2.5 shortconv
// out projection.  max(abs(x)) and every signed quant are calculated in this
// state machine; no host float conversion is involved.  Quants use
// RNE(x*127/max).  The non-negative FP16 scale is generated from the Q30 value
// RNE(max/127), again with ties-to-even.
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
    localparam [2:0] ST_IDLE    = 3'd0;
    localparam [2:0] ST_COLLECT = 3'd1;
    localparam [2:0] ST_PREPARE = 3'd2;
    localparam [2:0] ST_DIVIDE  = 3'd3;

    reg [2:0] state;
    reg signed [63:0] samples [0:31];
    reg [63:0] maximum;
    reg [5:0] quant_index;
    reg [70:0] numerator;
    reg [64:0] remainder;
    reg [70:0] quotient;
    reg [6:0] divide_bit;
    reg quant_negative;
    integer index;

    wire [63:0] sample_magnitude = sample_q30_i[63]
        ? (~sample_q30_i[63:0] + 64'd1)
        : sample_q30_i[63:0];
    wire sample_accept = sample_valid_i && sample_ready_o;
    wire [63:0] maximum_with_sample = sample_magnitude > maximum
        ? sample_magnitude : maximum;

    wire [64:0] shifted_remainder =
        {remainder[63:0], numerator[divide_bit]};
    wire divide_take = shifted_remainder >= {1'b0, maximum};
    wire [64:0] remainder_next = divide_take
        ? shifted_remainder - {1'b0, maximum}
        : shifted_remainder;
    wire [70:0] quotient_mask = 71'd1 << divide_bit;
    wire [70:0] quotient_next = divide_take
        ? quotient | quotient_mask : quotient;
    wire [65:0] twice_remainder = {remainder_next, 1'b0};
    wire quant_round = (twice_remainder > {2'b00, maximum})
        || ((twice_remainder == {2'b00, maximum}) && quotient_next[0]);
    wire [71:0] quant_rounded =
        {1'b0, quotient_next} + (quant_round ? 72'd1 : 72'd0);
    wire [7:0] quant_magnitude = quant_rounded[7:0];
    wire [7:0] signed_quant = quant_negative
        ? (~quant_magnitude + 8'd1) : quant_magnitude;

    // Constant division is build-time fixed logic, not a runtime math service.
    wire [63:0] scale_quotient = maximum / 64'd127;
    wire [6:0] scale_remainder = maximum % 64'd127;
    wire [64:0] scale_q30_ext = {1'b0, scale_quotient}
        + (({1'b0, scale_remainder} << 1) > 8'd127 ? 65'd1 : 65'd0);
    wire [63:0] scale_q30 = scale_q30_ext[63:0];

    // Positive Q30 -> binary16 RNE.  LFM activations are in the normal branch;
    // subnormal and zero handling is included so the block format is total.
    reg [6:0] scale_msb;
    reg scale_found;
    reg [63:0] scale_shifted;
    reg [63:0] scale_discard_mask;
    reg [63:0] scale_remainder_bits;
    reg [63:0] scale_halfway;
    reg [11:0] scale_significand;
    reg [5:0] scale_exponent;
    reg [15:0] scale_f16;
    integer scan;
    integer scale_shift;

    always @* begin
        scale_msb = 7'd0;
        scale_found = 1'b0;
        for (scan = 63; scan >= 0; scan = scan - 1) begin
            if (!scale_found && scale_q30[scan]) begin
                scale_msb = scan[6:0];
                scale_found = 1'b1;
            end
        end
        scale_shifted = 64'd0;
        scale_discard_mask = 64'd0;
        scale_remainder_bits = 64'd0;
        scale_halfway = 64'd0;
        scale_significand = 12'd0;
        scale_exponent = 6'd0;
        scale_f16 = 16'd0;
        scale_shift = 0;

        if (scale_q30 != 64'd0) begin
            if ($signed({1'b0, scale_msb}) - 8'sd30 < -8'sd14) begin
                // binary16 subnormal: one LSB is 2^-24, or 64 Q30 units.
                scale_significand = {1'b0, scale_q30[63:6]};
                if ((scale_q30[5:0] > 6'd32)
                    || ((scale_q30[5:0] == 6'd32) && scale_significand[0]))
                    scale_significand = scale_significand + 12'd1;
                if (scale_significand >= 12'd1024)
                    scale_f16 = 16'h0400;
                else
                    scale_f16 = {6'd0, scale_significand[9:0]};
            end else if ($signed({1'b0, scale_msb}) - 8'sd30 > 8'sd15) begin
                scale_f16 = 16'h7c00;
            end else begin
                scale_exponent = scale_msb - 7'd15;
                if (scale_msb > 7'd10) begin
                    scale_shift = scale_msb - 7'd10;
                    scale_shifted = scale_q30 >> scale_shift;
                    scale_discard_mask = (64'd1 << scale_shift) - 64'd1;
                    scale_remainder_bits = scale_q30 & scale_discard_mask;
                    scale_halfway = 64'd1 << (scale_shift - 1);
                    scale_significand = scale_shifted[11:0]
                        + ((scale_remainder_bits > scale_halfway)
                           || ((scale_remainder_bits == scale_halfway)
                               && scale_shifted[0]));
                end else begin
                    scale_significand = scale_q30[11:0] << (10 - scale_msb);
                end
                if (scale_significand == 12'd2048) begin
                    scale_exponent = scale_exponent + 6'd1;
                    scale_f16 = {(scale_exponent + 6'd1), 10'd0};
                end else begin
                    scale_f16 = {scale_exponent[4:0], scale_significand[9:0]};
                end
            end
        end
    end

    assign sample_ready_o = state == ST_COLLECT;

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            maximum <= 64'd0;
            quant_index <= 6'd0;
            numerator <= 71'd0;
            remainder <= 65'd0;
            quotient <= 71'd0;
            divide_bit <= 7'd0;
            quant_negative <= 1'b0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            samples_accepted_o <= 6'd0;
            q8_block_o <= 272'd0;
            for (index = 0; index < 32; index = index + 1)
                samples[index] <= 64'sd0;
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
                        samples[samples_accepted_o[4:0]] <= sample_q30_i;
                        maximum <= maximum_with_sample;
                        samples_accepted_o <= samples_accepted_o + 6'd1;
                        if (samples_accepted_o == 6'd31) begin
                            quant_index <= 6'd0;
                            state <= ST_PREPARE;
                        end
                    end
                end

                ST_PREPARE: begin
                    q8_block_o[15:0] <= scale_f16;
                    if (maximum == 64'd0) begin
                        q8_block_o <= 272'd0;
                        state <= ST_IDLE;
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                    end else begin
                        numerator <= (samples[quant_index][63]
                            ? (~samples[quant_index][63:0] + 64'd1)
                            : samples[quant_index][63:0]) * 7'd127;
                        remainder <= 65'd0;
                        quotient <= 71'd0;
                        divide_bit <= 7'd70;
                        quant_negative <= samples[quant_index][63];
                        state <= ST_DIVIDE;
                    end
                end

                ST_DIVIDE: begin
                    remainder <= remainder_next;
                    quotient <= quotient_next;
                    if (divide_bit == 7'd0) begin
                        if (quant_rounded > 72'd127) begin
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
                                state <= ST_PREPARE;
                            end
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
endmodule
