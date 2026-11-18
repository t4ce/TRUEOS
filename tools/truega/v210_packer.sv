// Pack three active-video 4:2:2 pairs into one little-endian v210 block.
// 3840 active pixels produce 640 blocks per row with no row padding.
// Feed complete rows of active samples from an elastic buffer.
module v210_packer (
    input  wire         clk,
    input  wire         reset,
    input  wire         pair_valid,
    output wire         pair_ready,
    input  wire [9:0]   cb,
    input  wire [9:0]   y_even,
    input  wire [9:0]   cr,
    input  wire [9:0]   y_odd,
    output reg          block_valid,
    input  wire         block_ready,
    output reg  [127:0] block_data
);
    reg [1:0] pair_index;
    reg [31:0] first_word;
    reg [31:0] second_word;
    reg [19:0] third_word_low;

    // One complete block can wait at the output while the first two
    // pairs of the following block are collected.
    assign pair_ready = !reset && (pair_index != 2 || !block_valid || block_ready);

    always @(posedge clk) begin
        if (reset) begin
            pair_index <= 0;
            block_valid <= 0;
        end else begin
            if (block_ready)
                block_valid <= 0;

            if (pair_valid && pair_ready) begin
                case (pair_index)
                    0: begin
                        first_word <= {2'b00, cr, y_even, cb};
                        second_word[9:0] <= y_odd;
                        pair_index <= 1;
                    end
                    1: begin
                        second_word[31:10] <= {2'b00, y_even, cb};
                        third_word_low <= {y_odd, cr};
                        pair_index <= 2;
                    end
                    2: begin
                        block_data <= {
                            2'b00, y_odd, cr, y_even,
                            2'b00, cb, third_word_low,
                            second_word, first_word
                        };
                        block_valid <= 1;
                        pair_index <= 0;
                    end
                    default: pair_index <= 0;
                endcase
            end
        end
    end
endmodule
