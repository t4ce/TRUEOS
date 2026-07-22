// Exact streaming dequantization of one native GGML Q8_0 block to signed Q30.
//
// The 16-bit block scale is multiplied by each signed byte through the same
// sequential binary16-to-Q30 scale circuit used by the Q8 dot-product path.
// Setting the second scale to binary16 1.0 and dot to the signed quant gives
// exact RNE_Q30(quant * scale), including normal/subnormal scale handling.
// Output index/data/last remain stable while output_ready_i is deasserted.
module truega_q8_0_dequant_block_slot (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire [271:0]        q8_block_i,
    output wire                output_valid_o,
    input  wire                output_ready_i,
    output wire [4:0]          output_index_o,
    output wire                output_last_o,
    output wire signed [63:0]  output_q30_o,
    output reg                 busy_o,
    output reg                 done_o,
    output reg                 error_o,
    output reg [5:0]           samples_retired_o
);
    localparam [1:0] ST_IDLE       = 2'd0;
    localparam [1:0] ST_SCALE_START = 2'd1;
    localparam [1:0] ST_SCALE_WAIT  = 2'd2;
    localparam [1:0] ST_OUTPUT      = 2'd3;

    reg [1:0] state;
    reg [271:0] block_reg;
    reg [4:0] sample_index;
    reg scale_start;
    reg signed [63:0] output_q30;

    wire signed [7:0] quant =
        block_reg[16 + sample_index * 8 +: 8];
    wire signed [20:0] quant_dot = {{13{quant[7]}}, quant};
    wire scale_busy;
    wire scale_done;
    wire signed [63:0] scale_q30;
    wire scale_error;
    wire output_accept = output_valid_o && output_ready_i;

    assign output_valid_o = state == ST_OUTPUT;
    assign output_index_o = sample_index;
    assign output_last_o = sample_index == 5'd31;
    assign output_q30_o = output_q30;

    truega_q8_0_scale_q30_seq scale_to_q30 (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(scale_start),
        .dot_i(quant_dot),
        .activation_scale_f16_i(block_reg[15:0]),
        .weight_scale_f16_i(16'h3c00),
        .busy_o(scale_busy),
        .done_o(scale_done),
        .term_q30_o(scale_q30),
        .scale_error_o(scale_error)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            block_reg <= 272'd0;
            sample_index <= 5'd0;
            scale_start <= 1'b0;
            output_q30 <= 64'sd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            samples_retired_o <= 6'd0;
        end else begin
            done_o <= 1'b0;
            scale_start <= 1'b0;
            case (state)
                ST_IDLE: begin
                    busy_o <= 1'b0;
                    if (start_i) begin
                        block_reg <= q8_block_i;
                        sample_index <= 5'd0;
                        output_q30 <= 64'sd0;
                        samples_retired_o <= 6'd0;
                        busy_o <= 1'b1;
                        error_o <= 1'b0;
                        state <= ST_SCALE_START;
                    end
                end

                ST_SCALE_START: begin
                    scale_start <= 1'b1;
                    state <= ST_SCALE_WAIT;
                end

                ST_SCALE_WAIT: begin
                    if (scale_done) begin
                        if (scale_error) begin
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                            state <= ST_IDLE;
                        end else begin
                            output_q30 <= scale_q30;
                            state <= ST_OUTPUT;
                        end
                    end
                end

                ST_OUTPUT: begin
                    if (output_accept) begin
                        samples_retired_o <= samples_retired_o + 6'd1;
                        if (sample_index == 5'd31) begin
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b0;
                            state <= ST_IDLE;
                        end else begin
                            sample_index <= sample_index + 5'd1;
                            state <= ST_SCALE_START;
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

    wire unused_scale_busy = scale_busy;
endmodule
