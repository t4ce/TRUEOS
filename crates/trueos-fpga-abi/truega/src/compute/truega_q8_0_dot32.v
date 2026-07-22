// Exact 32-lane signed Q8_0 integer dot product.
//
// The lanes are accumulated serially so synthesis cannot infer signed partial-
// sum RAMs or silently zero-extend a negative tree node.  Each 16-bit product
// is sign-extended by explicit bit replication, then added as raw two's-
// complement bits in a 21-bit accumulator.  Lane selection, multiplication,
// and accumulation occupy separate registered stages.  The enclosing serialized
// block slot keeps both quant inputs stable until valid_o.  One accepted block
// completes after 33 work cycles; valid_i is ignored while a block is active.
module truega_q8_0_dot32 (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 valid_i,
    input  wire [255:0]         activation_quants_i,
    input  wire [255:0]         weight_quants_i,
    output wire                 valid_o,
    output wire signed [20:0]   dot_o
);
    localparam STATE_IDLE = 2'd0;
    localparam STATE_RUN = 2'd1;
    localparam STATE_LAST_PRODUCT = 2'd2;
    localparam STATE_DRAIN = 2'd3;

    reg [5:0] lane_index;
    reg [1:0] state;
    reg valid_reg;
    reg [7:0] activation_lane_reg;
    reg [7:0] weight_lane_reg;
    reg [15:0] product_reg;
    reg [20:0] accumulator;
    reg [20:0] dot_reg;

    wire [7:0] activation_lane_bits;
    wire [7:0] weight_lane_bits;
    wire signed [7:0] activation_lane;
    wire signed [7:0] weight_lane;
    wire signed [15:0] lane_product;
    wire [15:0] current_product_bits;
    wire [20:0] registered_product_extended;
    wire [20:0] accumulator_next;

    assign activation_lane_bits = activation_quants_i[lane_index*8 +: 8];
    assign weight_lane_bits = weight_quants_i[lane_index*8 +: 8];
    assign activation_lane = activation_lane_reg;
    assign weight_lane = weight_lane_reg;
    assign lane_product = activation_lane * weight_lane;
    assign current_product_bits = lane_product;
    assign registered_product_extended = {{5{product_reg[15]}}, product_reg};
    assign accumulator_next = accumulator + registered_product_extended;

    assign valid_o = valid_reg;
    assign dot_o = dot_reg;

    always @(posedge clk) begin
        if (!reset_n) begin
            lane_index <= 6'd0;
            state <= STATE_IDLE;
            valid_reg <= 1'b0;
            activation_lane_reg <= 8'd0;
            weight_lane_reg <= 8'd0;
            product_reg <= 16'd0;
            accumulator <= 21'd0;
            dot_reg <= 21'd0;
        end else begin
            valid_reg <= 1'b0;

            case (state)
                STATE_IDLE: begin
                    if (valid_i) begin
                        activation_lane_reg <= activation_quants_i[7:0];
                        weight_lane_reg <= weight_quants_i[7:0];
                        lane_index <= 6'd1;
                        product_reg <= 16'd0;
                        accumulator <= 21'd0;
                        state <= STATE_RUN;
                    end
                end

                STATE_RUN: begin
                    product_reg <= current_product_bits;
                    activation_lane_reg <= activation_lane_bits;
                    weight_lane_reg <= weight_lane_bits;
                    if (lane_index != 6'd1)
                        accumulator <= accumulator_next;
                    if (lane_index == 6'd31) begin
                        state <= STATE_LAST_PRODUCT;
                    end else begin
                        lane_index <= lane_index + 1'b1;
                    end
                end

                STATE_LAST_PRODUCT: begin
                    product_reg <= current_product_bits;
                    accumulator <= accumulator_next;
                    state <= STATE_DRAIN;
                end

                STATE_DRAIN: begin
                    dot_reg <= accumulator_next;
                    accumulator <= 21'd0;
                    lane_index <= 6'd0;
                    state <= STATE_IDLE;
                    valid_reg <= 1'b1;
                end

                default: begin
                    state <= STATE_IDLE;
                    valid_reg <= 1'b0;
                    product_reg <= 16'd0;
                    accumulator <= 21'd0;
                end
            endcase
        end
    end
endmodule
