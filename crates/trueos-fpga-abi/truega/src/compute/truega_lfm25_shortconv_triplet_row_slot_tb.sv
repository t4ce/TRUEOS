`timescale 1ns/1ps

module truega_lfm25_shortconv_triplet_row_slot_tb;
    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg start_i = 1'b0;
    wire feeder_ready;
    wire [4:0] feeder_index;
    reg feeder_valid = 1'b0;
    reg [271:0] activation_block = 272'd0;
    reg [271:0] b_weight_block = 272'd0;
    reg [271:0] c_weight_block = 272'd0;
    reg [271:0] x_weight_block = 272'd0;
    wire busy_o;
    wire done_o;
    wire error_o;
    wire [5:0] blocks_accepted;
    wire signed [63:0] b_q30;
    wire signed [63:0] c_q30;
    wire signed [63:0] x_q30;
    integer sent;
    integer cycles;
    integer failures = 0;

    always #5 clk = ~clk;

    truega_lfm25_shortconv_triplet_row_slot dut (
        .clk(clk), .reset_n(reset_n), .start_i(start_i),
        .feeder_ready_o(feeder_ready),
        .feeder_block_index_o(feeder_index),
        .feeder_valid_i(feeder_valid),
        .feeder_activation_block_i(activation_block),
        .feeder_b_weight_block_i(b_weight_block),
        .feeder_c_weight_block_i(c_weight_block),
        .feeder_x_weight_block_i(x_weight_block),
        .busy_o(busy_o), .done_o(done_o), .error_o(error_o),
        .blocks_accepted_o(blocks_accepted),
        .b_q30_o(b_q30), .c_q30_o(c_q30), .x_q30_o(x_q30)
    );

    initial begin
        // FP16 scale 1.0.  The 32 blocks cover the exact 1024-wide input.
        activation_block = {{32{8'h01}}, 16'h3c00};
        b_weight_block = {{32{8'h01}}, 16'h3c00};
        c_weight_block = {{32{8'h02}}, 16'h3c00};
        x_weight_block = {{32{8'hff}}, 16'h3c00};

        repeat (4) @(negedge clk);
        reset_n = 1'b1;
        @(negedge clk);
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;

        sent = 0;
        cycles = 0;
        while (!done_o && cycles < 200) begin
            @(negedge clk);
            feeder_valid = feeder_ready && sent < 32;
            if (feeder_valid) begin
                if (feeder_index !== sent[4:0]) begin
                    $display("FAIL shortconv_triplet feeder index=%0d expected=%0d",
                        feeder_index, sent);
                    failures = failures + 1;
                end
                sent = sent + 1;
            end
            cycles = cycles + 1;
        end
        feeder_valid = 1'b0;

        if (!done_o || error_o || blocks_accepted != 32 || sent != 32
            || b_q30 !== 64'sd1099511627776
            || c_q30 !== 64'sd2199023255552
            || x_q30 !== -64'sd1099511627776) begin
            $display("FAIL shortconv_triplet sent=%0d blocks=%0d b=%0d c=%0d x=%0d error=%b cycles=%0d",
                sent, blocks_accepted, b_q30, c_q30, x_q30, error_o, cycles);
            failures = failures + 1;
        end

        if (failures == 0) begin
            $display("PASS lfm25_shortconv_inproj q8_0_width=1024 split=b,c,x ordering=exact");
            $finish;
        end
        $display("FAIL lfm25_shortconv_triplet failures=%0d", failures);
        $finish_and_return(1);
    end
endmodule
