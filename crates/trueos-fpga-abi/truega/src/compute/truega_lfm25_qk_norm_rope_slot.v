// Fixed LFM2.5 Q/K per-head RMSNorm application followed by NEOX RoPE.
//
// The pinned llama.cpp graph applies independent 64-element RMSNorms to every
// Q and K head, then LFM2's NEOX RoPE pairs element i with element i + 32.
// This element-pair slot is the second pass of that vector operation.  The
// reduction circuit supplies inv_rms_q30_i and a fixed RoPE table supplies
// cos_q30_i/sin_q30_i.  No host math or runtime-programmed operation exists.
//
// All signed values are Q30.  Every multiply is round-to-nearest, ties-even
// through truega_q30_mul_seq.  A transaction is eight sequential multiplies.
module truega_lfm25_qk_norm_rope_slot (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire signed [63:0]  x_lo_q30_i,
    input  wire signed [63:0]  x_hi_q30_i,
    input  wire signed [63:0]  inv_rms_q30_i,
    input  wire signed [63:0]  weight_lo_q30_i,
    input  wire signed [63:0]  weight_hi_q30_i,
    input  wire signed [63:0]  cos_q30_i,
    input  wire signed [63:0]  sin_q30_i,
    output reg                 busy_o,
    output reg                 done_o,
    output reg                 error_o,
    output reg signed [63:0]   y_lo_q30_o,
    output reg signed [63:0]   y_hi_q30_o
);
    localparam [3:0] ST_IDLE    = 4'd0;
    localparam [3:0] ST_X0_INV  = 4'd1;
    localparam [3:0] ST_X0_W    = 4'd2;
    localparam [3:0] ST_X1_INV  = 4'd3;
    localparam [3:0] ST_X1_W    = 4'd4;
    localparam [3:0] ST_N0_COS  = 4'd5;
    localparam [3:0] ST_N1_SIN  = 4'd6;
    localparam [3:0] ST_N0_SIN  = 4'd7;
    localparam [3:0] ST_N1_COS  = 4'd8;

    reg [3:0] state;
    reg multiply_start;
    reg multiply_waiting;
    reg signed [63:0] x0;
    reg signed [63:0] x1;
    reg signed [63:0] inv_rms;
    reg signed [63:0] weight0;
    reg signed [63:0] weight1;
    reg signed [63:0] rope_cos;
    reg signed [63:0] rope_sin;
    reg signed [63:0] scaled0;
    reg signed [63:0] scaled1;
    reg signed [63:0] norm0;
    reg signed [63:0] norm1;
    reg signed [63:0] norm0_cos;
    reg signed [63:0] norm1_sin;
    reg signed [63:0] norm0_sin;
    reg signed [63:0] multiply_left;
    reg signed [63:0] multiply_right;
    wire multiply_busy;
    wire multiply_done;
    wire multiply_overflow;
    wire signed [63:0] multiply_result;

    wire signed [64:0] y0_ext =
        $signed({norm0_cos[63], norm0_cos})
        - $signed({norm1_sin[63], norm1_sin});
    wire signed [64:0] y1_ext =
        $signed({norm0_sin[63], norm0_sin})
        + $signed({multiply_result[63], multiply_result});
    wire y0_overflow = y0_ext[64] != y0_ext[63];
    wire y1_overflow = y1_ext[64] != y1_ext[63];

    always @* begin
        multiply_left = 64'sd0;
        multiply_right = 64'sd0;
        case (state)
            ST_X0_INV: begin multiply_left = x0;      multiply_right = inv_rms; end
            ST_X0_W:   begin multiply_left = scaled0; multiply_right = weight0; end
            ST_X1_INV: begin multiply_left = x1;      multiply_right = inv_rms; end
            ST_X1_W:   begin multiply_left = scaled1; multiply_right = weight1; end
            ST_N0_COS: begin multiply_left = norm0;   multiply_right = rope_cos; end
            ST_N1_SIN: begin multiply_left = norm1;   multiply_right = rope_sin; end
            ST_N0_SIN: begin multiply_left = norm0;   multiply_right = rope_sin; end
            ST_N1_COS: begin multiply_left = norm1;   multiply_right = rope_cos; end
            default: begin end
        endcase
    end

    truega_q30_mul_seq multiply (
        .clk(clk), .reset_n(reset_n), .start_i(multiply_start),
        .left_q30_i(multiply_left), .right_q30_i(multiply_right),
        .busy_o(multiply_busy), .done_o(multiply_done),
        .overflow_o(multiply_overflow), .result_q30_o(multiply_result)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            multiply_start <= 1'b0;
            multiply_waiting <= 1'b0;
            x0 <= 64'sd0;
            x1 <= 64'sd0;
            inv_rms <= 64'sd0;
            weight0 <= 64'sd0;
            weight1 <= 64'sd0;
            rope_cos <= 64'sd0;
            rope_sin <= 64'sd0;
            scaled0 <= 64'sd0;
            scaled1 <= 64'sd0;
            norm0 <= 64'sd0;
            norm1 <= 64'sd0;
            norm0_cos <= 64'sd0;
            norm1_sin <= 64'sd0;
            norm0_sin <= 64'sd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            y_lo_q30_o <= 64'sd0;
            y_hi_q30_o <= 64'sd0;
        end else begin
            done_o <= 1'b0;
            multiply_start <= 1'b0;
            if (start_i && !busy_o) begin
                x0 <= x_lo_q30_i;
                x1 <= x_hi_q30_i;
                inv_rms <= inv_rms_q30_i;
                weight0 <= weight_lo_q30_i;
                weight1 <= weight_hi_q30_i;
                rope_cos <= cos_q30_i;
                rope_sin <= sin_q30_i;
                multiply_waiting <= 1'b0;
                busy_o <= 1'b1;
                error_o <= 1'b0;
                y_lo_q30_o <= 64'sd0;
                y_hi_q30_o <= 64'sd0;
                state <= ST_X0_INV;
            end else if (busy_o) begin
                if (!multiply_waiting) begin
                    multiply_start <= 1'b1;
                    multiply_waiting <= 1'b1;
                end else if (multiply_done) begin
                    multiply_waiting <= 1'b0;
                    if (multiply_overflow) begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= 1'b1;
                        state <= ST_IDLE;
                    end else begin
                        case (state)
                            ST_X0_INV: begin scaled0 <= multiply_result; state <= ST_X0_W; end
                            ST_X0_W:   begin norm0 <= multiply_result; state <= ST_X1_INV; end
                            ST_X1_INV: begin scaled1 <= multiply_result; state <= ST_X1_W; end
                            ST_X1_W:   begin norm1 <= multiply_result; state <= ST_N0_COS; end
                            ST_N0_COS: begin norm0_cos <= multiply_result; state <= ST_N1_SIN; end
                            ST_N1_SIN: begin norm1_sin <= multiply_result; state <= ST_N0_SIN; end
                            ST_N0_SIN: begin norm0_sin <= multiply_result; state <= ST_N1_COS; end
                            ST_N1_COS: begin
                                y_lo_q30_o <= y0_ext[63:0];
                                y_hi_q30_o <= y1_ext[63:0];
                                error_o <= y0_overflow | y1_overflow;
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                state <= ST_IDLE;
                            end
                            default: begin
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                error_o <= 1'b1;
                                state <= ST_IDLE;
                            end
                        endcase
                    end
                end
            end
        end
    end

    wire unused_multiply_busy = multiply_busy;
endmodule
