// Exact fixed 1024-element residual vector addition for LFM2.5.
//
// This companion keeps both residual sites independent from RMSNorm and Q8
// quantization.  Every accepted pair produces one signed-Q30 sum with the same
// index.  A pending output backpressures input; overflow terminates the command
// with error_o and never publishes a wrapped value.
module truega_lfm25_residual_vector_slot (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire                input_valid_i,
    output wire                input_ready_o,
    input  wire signed [63:0]  residual_q30_i,
    input  wire signed [63:0]  branch_q30_i,
    output wire                output_valid_o,
    input  wire                output_ready_i,
    output wire [9:0]          output_index_o,
    output wire signed [63:0]  output_q30_o,
    output reg                 busy_o,
    output reg                 done_o,
    output reg                 error_o,
    output reg [10:0]          elements_retired_o
);
    reg output_pending;
    reg [9:0] element_index;
    reg signed [63:0] output_value;
    wire signed [64:0] sum_ext =
        $signed({residual_q30_i[63], residual_q30_i})
        + $signed({branch_q30_i[63], branch_q30_i});
    wire sum_overflow = sum_ext[64] != sum_ext[63];
    wire input_accept = input_valid_i && input_ready_o;
    wire output_accept = output_valid_o && output_ready_i;

    assign input_ready_o = busy_o && !output_pending;
    assign output_valid_o = output_pending;
    assign output_index_o = element_index;
    assign output_q30_o = output_value;

    always @(posedge clk) begin
        if (!reset_n) begin
            output_pending <= 1'b0;
            element_index <= 10'd0;
            output_value <= 64'sd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            elements_retired_o <= 11'd0;
        end else begin
            done_o <= 1'b0;
            if (!busy_o) begin
                output_pending <= 1'b0;
                if (start_i) begin
                    busy_o <= 1'b1;
                    error_o <= 1'b0;
                    element_index <= 10'd0;
                    output_value <= 64'sd0;
                    elements_retired_o <= 11'd0;
                end
            end else begin
                if (input_accept) begin
                    if (sum_overflow) begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= 1'b1;
                        output_pending <= 1'b0;
                    end else begin
                        output_value <= sum_ext[63:0];
                        output_pending <= 1'b1;
                    end
                end

                if (output_accept) begin
                    output_pending <= 1'b0;
                    elements_retired_o <= elements_retired_o + 11'd1;
                    if (element_index == 10'd1023) begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= 1'b0;
                    end else begin
                        element_index <= element_index + 10'd1;
                    end
                end
            end
        end
    end
endmodule
