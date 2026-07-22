// Exact 32-lane signed Q8_0 integer dot product.
// Six-cycle latency, one unchanged 34-byte native-image block per cycle.
module truega_q8_0_dot32 (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 valid_i,
    input  wire [255:0]         activation_quants_i,
    input  wire [255:0]         weight_quants_i,
    output wire                 valid_o,
    output wire signed [20:0]   dot_o
);
    reg signed [15:0] product [0:31];
    reg signed [16:0] sum_1 [0:15];
    reg signed [17:0] sum_2 [0:7];
    reg signed [18:0] sum_3 [0:3];
    reg signed [19:0] sum_4 [0:1];
    reg signed [20:0] sum_5;
    reg [5:0] valid_pipe;
    integer lane;

    assign valid_o = valid_pipe[5];
    assign dot_o = sum_5;

    always @(posedge clk) begin
        if (!reset_n) begin
            valid_pipe <= 6'b0;
            sum_5 <= 21'sd0;
            for (lane = 0; lane < 32; lane = lane + 1)
                product[lane] <= 16'sd0;
            for (lane = 0; lane < 16; lane = lane + 1)
                sum_1[lane] <= 17'sd0;
            for (lane = 0; lane < 8; lane = lane + 1)
                sum_2[lane] <= 18'sd0;
            for (lane = 0; lane < 4; lane = lane + 1)
                sum_3[lane] <= 19'sd0;
            for (lane = 0; lane < 2; lane = lane + 1)
                sum_4[lane] <= 20'sd0;
        end else begin
            valid_pipe <= {valid_pipe[4:0], valid_i};

            for (lane = 0; lane < 32; lane = lane + 1)
                product[lane] <= $signed(activation_quants_i[lane*8 +: 8])
                               * $signed(weight_quants_i[lane*8 +: 8]);

            // A concatenation is unsigned in Verilog even when all of its
            // members came from signed registers.  Cast every widened operand
            // explicitly so synthesis cannot zero-extend a negative partial
            // sum at a tree boundary.
            for (lane = 0; lane < 16; lane = lane + 1)
                sum_1[lane] <= $signed({product[lane*2][15], product[lane*2]})
                             + $signed({product[lane*2 + 1][15], product[lane*2 + 1]});
            for (lane = 0; lane < 8; lane = lane + 1)
                sum_2[lane] <= $signed({sum_1[lane*2][16], sum_1[lane*2]})
                             + $signed({sum_1[lane*2 + 1][16], sum_1[lane*2 + 1]});
            for (lane = 0; lane < 4; lane = lane + 1)
                sum_3[lane] <= $signed({sum_2[lane*2][17], sum_2[lane*2]})
                             + $signed({sum_2[lane*2 + 1][17], sum_2[lane*2 + 1]});
            for (lane = 0; lane < 2; lane = lane + 1)
                sum_4[lane] <= $signed({sum_3[lane*2][18], sum_3[lane*2]})
                             + $signed({sum_3[lane*2 + 1][18], sum_3[lane*2 + 1]});
            sum_5 <= $signed({sum_4[0][19], sum_4[0]})
                   + $signed({sum_4[1][19], sum_4[1]});
        end
    end
endmodule
