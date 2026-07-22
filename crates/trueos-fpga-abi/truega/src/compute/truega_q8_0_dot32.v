// Exact 32-lane signed Q8_0 integer dot product.
//
// The lanes are accumulated serially so synthesis cannot infer signed partial-
// sum RAMs or silently zero-extend a negative tree node.  Each 16-bit product
// is sign-extended by explicit bit replication, then added as raw two's-
// complement bits in a 21-bit accumulator.  One accepted block completes after
// 32 accumulation cycles; valid_i is ignored while a block is active.
module truega_q8_0_dot32 (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 valid_i,
    input  wire [255:0]         activation_quants_i,
    input  wire [255:0]         weight_quants_i,
    output wire                 valid_o,
    output wire signed [20:0]   dot_o
);
    reg [255:0] activation_quants_reg;
    reg [255:0] weight_quants_reg;
    reg [5:0] lane_index;
    reg active;
    reg valid_reg;
    reg [20:0] accumulator;
    reg [20:0] dot_reg;

    wire [7:0] activation_lane_bits;
    wire [7:0] weight_lane_bits;
    wire signed [7:0] activation_lane;
    wire signed [7:0] weight_lane;
    wire signed [15:0] lane_product;
    wire [15:0] lane_product_bits;
    wire [20:0] lane_product_extended;
    wire [20:0] accumulator_next;

    assign activation_lane_bits = activation_quants_reg[lane_index*8 +: 8];
    assign weight_lane_bits = weight_quants_reg[lane_index*8 +: 8];
    assign activation_lane = activation_lane_bits;
    assign weight_lane = weight_lane_bits;
    assign lane_product = activation_lane * weight_lane;
    assign lane_product_bits = lane_product;
    assign lane_product_extended = {{5{lane_product_bits[15]}}, lane_product_bits};
    assign accumulator_next = accumulator + lane_product_extended;

    assign valid_o = valid_reg;
    assign dot_o = dot_reg;

    always @(posedge clk) begin
        if (!reset_n) begin
            activation_quants_reg <= 256'd0;
            weight_quants_reg <= 256'd0;
            lane_index <= 6'd0;
            active <= 1'b0;
            valid_reg <= 1'b0;
            accumulator <= 21'd0;
            dot_reg <= 21'd0;
        end else begin
            valid_reg <= 1'b0;

            if (!active) begin
                if (valid_i) begin
                    activation_quants_reg <= activation_quants_i;
                    weight_quants_reg <= weight_quants_i;
                    lane_index <= 6'd0;
                    accumulator <= 21'd0;
                    active <= 1'b1;
                end
            end else if (lane_index == 6'd31) begin
                dot_reg <= accumulator_next;
                accumulator <= 21'd0;
                lane_index <= 6'd0;
                active <= 1'b0;
                valid_reg <= 1'b1;
            end else begin
                accumulator <= accumulator_next;
                lane_index <= lane_index + 1'b1;
            end
        end
    end
endmodule
