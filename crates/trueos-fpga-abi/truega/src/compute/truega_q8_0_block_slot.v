// One serialized Q8_0 block operation with reusable start/busy/done signalling.
// Native blocks are unchanged: bits [15:0] hold the little-endian binary16
// scale and bits [16 + lane*8 +: 8] hold signed quant lane `lane`.
// A rising edge accepts start_i only with busy_o low.  Attempts while busy are
// ignored.  Accepted block inputs must remain stable while busy_o is high.
// done_o pulses for one cycle after both registered outputs are valid.
module truega_q8_0_block_slot (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 start_i,
    input  wire [271:0]         activation_block_i,
    input  wire [271:0]         weight_block_i,
    output reg                  busy_o,
    output reg                  done_o,
    output reg  signed [31:0]   dot_o,
    output reg  signed [63:0]   term_q30_o,
    output reg                  scale_error_o
);
    wire accept = start_i && !busy_o;
    wire dot_valid;
    wire signed [20:0] dot;
    reg [15:0] activation_scale_reg;
    reg [15:0] weight_scale_reg;
    wire scaler_start;
    wire scaler_busy;
    wire scaler_done;
    wire signed [63:0] scaler_term;
    wire scaler_error;

    assign scaler_start = dot_valid && busy_o && !scaler_busy;

    truega_q8_0_dot32 dot32 (
        .clk(clk),
        .reset_n(reset_n),
        .valid_i(accept),
        .activation_quants_i(activation_block_i[271:16]),
        .weight_quants_i(weight_block_i[271:16]),
        .valid_o(dot_valid),
        .dot_o(dot)
    );

    truega_q8_0_scale_q30_seq scale_q30 (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(scaler_start),
        .dot_i(dot),
        .activation_scale_f16_i(activation_scale_reg),
        .weight_scale_f16_i(weight_scale_reg),
        .busy_o(scaler_busy),
        .done_o(scaler_done),
        .term_q30_o(scaler_term),
        .scale_error_o(scaler_error)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            activation_scale_reg <= 16'd0;
            weight_scale_reg <= 16'd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            dot_o <= 32'sd0;
            term_q30_o <= 64'sd0;
            scale_error_o <= 1'b0;
        end else begin
            done_o <= 1'b0;
            if (accept) begin
                activation_scale_reg <= activation_block_i[15:0];
                weight_scale_reg <= weight_block_i[15:0];
                busy_o <= 1'b1;
                dot_o <= 32'sd0;
                term_q30_o <= 64'sd0;
                scale_error_o <= 1'b0;
            end
            if (dot_valid && busy_o)
                dot_o <= {{11{dot[20]}}, dot};
            if (scaler_done) begin
                busy_o <= 1'b0;
                done_o <= 1'b1;
                term_q30_o <= scaler_term;
                scale_error_o <= scaler_error;
            end
        end
    end
endmodule
