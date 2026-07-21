// Streaming Q8_0 GEMV row accumulator. The native weight/activation blocks are
// accepted unchanged: FP16 scale plus 32 signed bytes. Results are deterministic Q30.
module truega_q8_0_gemv (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 valid_i,
    input  wire                 row_first_i,
    input  wire                 row_last_i,
    input  wire [15:0]          activation_scale_f16_i,
    input  wire [15:0]          weight_scale_f16_i,
    input  wire [255:0]         activation_quants_i,
    input  wire [255:0]         weight_quants_i,
    output reg                  block_valid_o,
    output reg  signed [20:0]   block_dot_o,
    output reg  signed [63:0]   block_term_q30_o,
    output reg                  row_valid_o,
    output reg  signed [63:0]   row_q30_o,
    output reg                  scale_error_o
);
    wire dot_valid;
    wire signed [20:0] dot;
    wire signed [63:0] scaled_term;
    wire scale_error;
    reg [5:0] first_pipe;
    reg [5:0] last_pipe;
    reg [15:0] activation_scale_pipe [0:5];
    reg [15:0] weight_scale_pipe [0:5];
    reg signed [63:0] accumulator;
    integer stage;

    truega_q8_0_dot32 dot32 (
        .clk(clk),
        .reset_n(reset_n),
        .valid_i(valid_i),
        .activation_quants_i(activation_quants_i),
        .weight_quants_i(weight_quants_i),
        .valid_o(dot_valid),
        .dot_o(dot)
    );

    truega_q8_0_scale_q30 scale_q30 (
        .dot_i(dot),
        .activation_scale_f16_i(activation_scale_pipe[5]),
        .weight_scale_f16_i(weight_scale_pipe[5]),
        .term_q30_o(scaled_term),
        .scale_error_o(scale_error)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            first_pipe <= 6'b0;
            last_pipe <= 6'b0;
            accumulator <= 64'sd0;
            row_valid_o <= 1'b0;
            row_q30_o <= 64'sd0;
            scale_error_o <= 1'b0;
            block_valid_o <= 1'b0;
            block_dot_o <= 21'sd0;
            block_term_q30_o <= 64'sd0;
            for (stage = 0; stage < 6; stage = stage + 1) begin
                activation_scale_pipe[stage] <= 16'd0;
                weight_scale_pipe[stage] <= 16'd0;
            end
        end else begin
            first_pipe <= {first_pipe[4:0], row_first_i && valid_i};
            last_pipe <= {last_pipe[4:0], row_last_i && valid_i};
            activation_scale_pipe[0] <= activation_scale_f16_i;
            weight_scale_pipe[0] <= weight_scale_f16_i;
            for (stage = 1; stage < 6; stage = stage + 1) begin
                activation_scale_pipe[stage] <= activation_scale_pipe[stage - 1];
                weight_scale_pipe[stage] <= weight_scale_pipe[stage - 1];
            end

            row_valid_o <= 1'b0;
            block_valid_o <= dot_valid;
            if (dot_valid) begin
                block_dot_o <= dot;
                block_term_q30_o <= scaled_term;
                if (scale_error)
                    scale_error_o <= 1'b1;
                if (first_pipe[5]) begin
                    if (last_pipe[5]) begin
                        row_q30_o <= scaled_term;
                        row_valid_o <= 1'b1;
                        accumulator <= 64'sd0;
                    end else begin
                        accumulator <= scaled_term;
                    end
                end else if (last_pipe[5]) begin
                    row_q30_o <= accumulator + scaled_term;
                    row_valid_o <= 1'b1;
                    accumulator <= 64'sd0;
                end else begin
                    accumulator <= accumulator + scaled_term;
                end
            end
        end
    end
endmodule
