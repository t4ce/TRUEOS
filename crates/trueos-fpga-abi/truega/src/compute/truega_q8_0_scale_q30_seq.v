// Multi-cycle conversion of one exact Q8_0 integer dot product to signed Q30.
//
// Each Q8_0 scale is a non-negative IEEE binary16 value.  Normal and
// subnormal values are decoded without converting through floating point.
// Right shifts use round-to-nearest, ties-to-even.  The iterative shifter keeps
// the combinational path short enough for use beside the 100 MHz PCIe shell.
// start_i is accepted only while idle; later starts are ignored until done_o.
// done_o pulses for one cycle with busy_o low and the result already registered.
module truega_q8_0_scale_q30_seq (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 start_i,
    input  wire signed [20:0]   dot_i,
    input  wire [15:0]          activation_scale_f16_i,
    input  wire [15:0]          weight_scale_f16_i,
    output reg                  busy_o,
    output reg                  done_o,
    output reg  signed [63:0]   term_q30_o,
    output reg                  scale_error_o
);
    localparam [3:0] STATE_IDLE        = 4'd0;
    localparam [3:0] STATE_VALIDATE    = 4'd1;
    localparam [3:0] STATE_MUL_SCALE   = 4'd2;
    localparam [3:0] STATE_MUL_DOT     = 4'd3;
    localparam [3:0] STATE_PREP_SHIFT  = 4'd4;
    localparam [3:0] STATE_SHIFT_LEFT  = 4'd5;
    localparam [3:0] STATE_SHIFT_RIGHT = 4'd6;
    localparam [3:0] STATE_ROUND       = 4'd7;
    localparam [3:0] STATE_COMMIT      = 4'd8;

    wire activation_invalid = activation_scale_f16_i[15]
                            || activation_scale_f16_i[14:10] == 5'h1f;
    wire weight_invalid = weight_scale_f16_i[15]
                        || weight_scale_f16_i[14:10] == 5'h1f;
    wire [10:0] activation_significand_decoded =
        activation_scale_f16_i[14:10] == 5'd0
            ? {1'b0, activation_scale_f16_i[9:0]}
            : {1'b1, activation_scale_f16_i[9:0]};
    wire [10:0] weight_significand_decoded =
        weight_scale_f16_i[14:10] == 5'd0
            ? {1'b0, weight_scale_f16_i[9:0]}
            : {1'b1, weight_scale_f16_i[9:0]};
    wire [5:0] activation_exponent_decoded =
        activation_scale_f16_i[14:10] == 5'd0
            ? 6'd1
            : {1'b0, activation_scale_f16_i[14:10]};
    wire [5:0] weight_exponent_decoded =
        weight_scale_f16_i[14:10] == 5'd0
            ? 6'd1
            : {1'b0, weight_scale_f16_i[14:10]};
    wire signed [7:0] scale_shift_decoded =
        $signed({2'b00, activation_exponent_decoded})
      + $signed({2'b00, weight_exponent_decoded}) - 8'sd20;

    reg [3:0] state;
    reg signed [20:0] dot_reg;
    reg [10:0] activation_significand_reg;
    reg [10:0] weight_significand_reg;
    reg [21:0] significand_product_reg;
    reg signed [42:0] raw_product_reg;
    reg signed [7:0] scale_shift_reg;
    reg invalid_reg;
    reg [5:0] shift_count_reg;
    reg [63:0] magnitude_reg;
    reg negative_reg;
    reg guard_reg;
    reg sticky_reg;

    wire signed [22:0] significand_product_signed =
        $signed({1'b0, significand_product_reg});
    wire signed [42:0] dot_scale_product =
        dot_reg * significand_product_signed;
    wire signed [63:0] raw_extended =
        {{21{raw_product_reg[42]}}, raw_product_reg};
    wire [63:0] raw_magnitude = raw_extended[63]
        ? (~raw_extended + 64'd1)
        : raw_extended;
    wire round_increment = guard_reg && (sticky_reg || magnitude_reg[0]);
    wire [63:0] rounded_magnitude = magnitude_reg
        + (round_increment ? 64'd1 : 64'd0);

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= STATE_IDLE;
            dot_reg <= 21'sd0;
            activation_significand_reg <= 11'd0;
            weight_significand_reg <= 11'd0;
            significand_product_reg <= 22'd0;
            raw_product_reg <= 43'sd0;
            scale_shift_reg <= 8'sd0;
            invalid_reg <= 1'b0;
            shift_count_reg <= 6'd0;
            magnitude_reg <= 64'd0;
            negative_reg <= 1'b0;
            guard_reg <= 1'b0;
            sticky_reg <= 1'b0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            term_q30_o <= 64'sd0;
            scale_error_o <= 1'b0;
        end else begin
            done_o <= 1'b0;
            case (state)
                STATE_IDLE: begin
                    busy_o <= 1'b0;
                    if (start_i) begin
                        busy_o <= 1'b1;
                        term_q30_o <= 64'sd0;
                        scale_error_o <= 1'b0;
                        guard_reg <= 1'b0;
                        sticky_reg <= 1'b0;
                        dot_reg <= dot_i;
                        activation_significand_reg <= activation_significand_decoded;
                        weight_significand_reg <= weight_significand_decoded;
                        scale_shift_reg <= scale_shift_decoded;
                        invalid_reg <= activation_invalid || weight_invalid;
                        state <= STATE_VALIDATE;
                    end
                end

                STATE_VALIDATE: begin
                    if (invalid_reg) begin
                        scale_error_o <= 1'b1;
                        state <= STATE_COMMIT;
                    end else if (activation_significand_reg == 11'd0
                              || weight_significand_reg == 11'd0
                              || dot_reg == 21'sd0) begin
                        state <= STATE_COMMIT;
                    end else if (scale_shift_reg > 8'sd20) begin
                        scale_error_o <= 1'b1;
                        state <= STATE_COMMIT;
                    end else begin
                        state <= STATE_MUL_SCALE;
                    end
                end

                STATE_MUL_SCALE: begin
                    significand_product_reg <= activation_significand_reg
                                             * weight_significand_reg;
                    state <= STATE_MUL_DOT;
                end

                STATE_MUL_DOT: begin
                    raw_product_reg <= dot_scale_product;
                    state <= STATE_PREP_SHIFT;
                end

                STATE_PREP_SHIFT: begin
                    magnitude_reg <= raw_magnitude;
                    negative_reg <= raw_extended[63];
                    guard_reg <= 1'b0;
                    sticky_reg <= 1'b0;
                    if (scale_shift_reg > 0) begin
                        shift_count_reg <= scale_shift_reg[5:0];
                        state <= STATE_SHIFT_LEFT;
                    end else if (scale_shift_reg < 0) begin
                        shift_count_reg <= 6'd0 - scale_shift_reg[5:0];
                        state <= STATE_SHIFT_RIGHT;
                    end else begin
                        shift_count_reg <= 6'd0;
                        state <= STATE_ROUND;
                    end
                end

                STATE_SHIFT_LEFT: begin
                    magnitude_reg <= magnitude_reg << 1;
                    shift_count_reg <= shift_count_reg - 1'b1;
                    if (shift_count_reg == 6'd1)
                        state <= STATE_ROUND;
                end

                STATE_SHIFT_RIGHT: begin
                    magnitude_reg <= magnitude_reg >> 1;
                    shift_count_reg <= shift_count_reg - 1'b1;
                    if (shift_count_reg == 6'd1) begin
                        guard_reg <= magnitude_reg[0];
                        state <= STATE_ROUND;
                    end else begin
                        sticky_reg <= sticky_reg || magnitude_reg[0];
                    end
                end

                STATE_ROUND: begin
                    term_q30_o <= negative_reg
                        ? -$signed(rounded_magnitude)
                        : $signed(rounded_magnitude);
                    state <= STATE_COMMIT;
                end

                STATE_COMMIT: begin
                    busy_o <= 1'b0;
                    done_o <= 1'b1;
                    state <= STATE_IDLE;
                end

                default: begin
                    state <= STATE_IDLE;
                    busy_o <= 1'b0;
                    done_o <= 1'b0;
                    term_q30_o <= 64'sd0;
                    scale_error_o <= 1'b1;
                end
            endcase
        end
    end
endmodule
