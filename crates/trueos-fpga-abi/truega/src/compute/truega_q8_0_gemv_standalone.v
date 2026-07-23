// Standalone synthesis boundary. Deliberately absent from min_pci_led.gprj and top.vhd.
module truega_q8_0_gemv_standalone (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 valid_i,
    output wire                 ready_o,
    input  wire                 row_first_i,
    input  wire                 row_last_i,
    input  wire [15:0]          activation_scale_f16_i,
    input  wire [15:0]          weight_scale_f16_i,
    input  wire [255:0]         activation_quants_i,
    input  wire [255:0]         weight_quants_i,
    output wire                 block_valid_o,
    output wire signed [20:0]   block_dot_o,
    output wire signed [63:0]   block_term_q30_o,
    output wire                 row_valid_o,
    output wire signed [63:0]   row_q30_o,
    output wire                 scale_error_o
);
    truega_q8_0_gemv implementation (
        .clk(clk),
        .reset_n(reset_n),
        .valid_i(valid_i),
        .ready_o(ready_o),
        .row_first_i(row_first_i),
        .row_last_i(row_last_i),
        .activation_scale_f16_i(activation_scale_f16_i),
        .weight_scale_f16_i(weight_scale_f16_i),
        .activation_quants_i(activation_quants_i),
        .weight_quants_i(weight_quants_i),
        .block_valid_o(block_valid_o),
        .block_dot_o(block_dot_o),
        .block_term_q30_o(block_term_q30_o),
        .row_valid_o(row_valid_o),
        .row_q30_o(row_q30_o),
        .scale_error_o(scale_error_o)
    );
endmodule
