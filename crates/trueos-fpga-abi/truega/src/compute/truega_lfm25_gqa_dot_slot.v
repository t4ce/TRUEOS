// LFM2.5 one-head Q.K score with fixed 16:8 GQA mapping.
//
// Exactly 64 Q30 element pairs are consumed.  Each product is ties-even Q30,
// the wide sum is divided by sqrt(64)=8 with ties-even rounding, and the
// selected KV head must equal query_head >> 1.
module truega_lfm25_gqa_dot_slot (
    input  wire                clk,
    input  wire                reset_n,
    input  wire                start_i,
    input  wire [3:0]          query_head_i,
    input  wire [2:0]          kv_head_i,
    input  wire                sample_valid_i,
    input  wire                sample_last_i,
    input  wire signed [63:0]  query_q30_i,
    input  wire signed [63:0]  key_q30_i,
    output wire                sample_ready_o,
    output reg                 busy_o,
    output reg                 done_o,
    output reg                 error_o,
    output reg [2:0]           mapped_kv_head_o,
    output reg signed [63:0]   score_q30_o
);
    reg [6:0] sample_count;
    reg multiply_start;
    reg multiply_waiting;
    reg sample_last;
    reg signed [63:0] query;
    reg signed [63:0] key;
    reg signed [70:0] accumulator;
    wire multiply_busy;
    wire multiply_done;
    wire multiply_overflow;
    wire signed [63:0] product_q30;
    wire signed [70:0] sum_next = accumulator
        + {{7{product_q30[63]}}, product_q30};

    function signed [63:0] round_shift_three;
        input signed [70:0] value;
        reg negative;
        reg [70:0] magnitude;
        reg [67:0] quotient;
        reg [2:0] remainder;
        begin
            negative = value[70];
            magnitude = negative ? (~value + 71'd1) : value;
            quotient = magnitude[70:3];
            remainder = magnitude[2:0];
            if ((remainder > 3'd4) || ((remainder == 3'd4) && quotient[0]))
                quotient = quotient + 68'd1;
            round_shift_three = negative ? -$signed(quotient[63:0])
                                         :  $signed(quotient[63:0]);
        end
    endfunction

    assign sample_ready_o = busy_o && !multiply_waiting;

    truega_q30_mul_seq dot_multiply (
        .clk(clk), .reset_n(reset_n), .start_i(multiply_start),
        .left_q30_i(query), .right_q30_i(key),
        .busy_o(multiply_busy), .done_o(multiply_done),
        .overflow_o(multiply_overflow), .result_q30_o(product_q30)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            sample_count <= 7'd0;
            multiply_start <= 1'b0;
            multiply_waiting <= 1'b0;
            sample_last <= 1'b0;
            query <= 64'sd0;
            key <= 64'sd0;
            accumulator <= 71'sd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            mapped_kv_head_o <= 3'd0;
            score_q30_o <= 64'sd0;
        end else begin
            done_o <= 1'b0;
            multiply_start <= 1'b0;
            if (start_i && !busy_o) begin
                mapped_kv_head_o <= query_head_i[3:1];
                sample_count <= 7'd0;
                multiply_waiting <= 1'b0;
                accumulator <= 71'sd0;
                score_q30_o <= 64'sd0;
                if (query_head_i > 4'd15 || kv_head_i != query_head_i[3:1]) begin
                    error_o <= 1'b1;
                    done_o <= 1'b1;
                end else begin
                    error_o <= 1'b0;
                    busy_o <= 1'b1;
                end
            end else if (busy_o) begin
                if (sample_valid_i && !multiply_waiting) begin
                    query <= query_q30_i;
                    key <= key_q30_i;
                    sample_last <= sample_last_i;
                    multiply_start <= 1'b1;
                    multiply_waiting <= 1'b1;
                end else if (multiply_waiting && multiply_done) begin
                    multiply_waiting <= 1'b0;
                    if (multiply_overflow
                        || (sample_last && sample_count != 7'd63)
                        || (!sample_last && sample_count == 7'd63)) begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= 1'b1;
                    end else if (sample_last) begin
                        score_q30_o <= round_shift_three(sum_next);
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                    end else begin
                        accumulator <= sum_next;
                        sample_count <= sample_count + 7'd1;
                    end
                end
            end
        end
    end

    wire unused_multiply_busy = multiply_busy;
endmodule
