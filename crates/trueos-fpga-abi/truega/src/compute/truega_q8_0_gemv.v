// Streaming Q8_0 GEMV row accumulator. The native weight/activation blocks are
// accepted unchanged: FP16 scale plus 32 signed bytes. Results are deterministic Q30.
module truega_q8_0_gemv (
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
    output reg                  block_valid_o,
    output reg  signed [20:0]   block_dot_o,
    output reg  signed [63:0]   block_term_q30_o,
    output reg                  row_valid_o,
    output reg  signed [63:0]   row_q30_o,
    output reg                  scale_error_o
);
    wire dot_valid;
    wire signed [20:0] dot;
    wire scale_busy;
    wire scale_done;
    wire signed [63:0] scaled_term;
    wire scale_error;
    wire block_accept;
    reg block_active;
    reg block_first;
    reg block_last;
    reg [15:0] activation_scale;
    reg [15:0] weight_scale;
    reg signed [63:0] accumulator;

    assign ready_o = reset_n && !block_active && !scale_busy;
    assign block_accept = valid_i && ready_o;

    truega_q8_0_dot32 dot32 (
        .clk(clk),
        .reset_n(reset_n),
        .valid_i(block_accept),
        .activation_quants_i(activation_quants_i),
        .weight_quants_i(weight_quants_i),
        .valid_o(dot_valid),
        .dot_o(dot)
    );

    // The exact scale conversion is deliberately multi-cycle.  Keeping the
    // block busy through scale_done prevents the following block from
    // replacing the latched row flags/scales before this term retires.
    truega_q8_0_scale_q30_seq scale_q30 (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(dot_valid),
        .dot_i(dot),
        .activation_scale_f16_i(activation_scale),
        .weight_scale_f16_i(weight_scale),
        .busy_o(scale_busy),
        .done_o(scale_done),
        .term_q30_o(scaled_term),
        .scale_error_o(scale_error)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            block_active <= 1'b0;
            block_first <= 1'b0;
            block_last <= 1'b0;
            activation_scale <= 16'd0;
            weight_scale <= 16'd0;
            accumulator <= 64'sd0;
            row_valid_o <= 1'b0;
            row_q30_o <= 64'sd0;
            scale_error_o <= 1'b0;
            block_valid_o <= 1'b0;
            block_dot_o <= 21'sd0;
            block_term_q30_o <= 64'sd0;
        end else begin
            if (block_accept) begin
                block_active <= 1'b1;
                block_first <= row_first_i;
                block_last <= row_last_i;
                activation_scale <= activation_scale_f16_i;
                weight_scale <= weight_scale_f16_i;
            end

            row_valid_o <= 1'b0;
            block_valid_o <= 1'b0;
            if (dot_valid) begin
                block_dot_o <= dot;
            end
            if (scale_done) begin
                block_active <= 1'b0;
                block_valid_o <= 1'b1;
                block_term_q30_o <= scaled_term;
                if (scale_error)
                    scale_error_o <= 1'b1;
                if (block_first) begin
                    if (block_last) begin
                        row_q30_o <= scaled_term;
                        row_valid_o <= 1'b1;
                        accumulator <= 64'sd0;
                    end else begin
                        accumulator <= scaled_term;
                    end
                end else if (block_last) begin
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
