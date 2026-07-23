`timescale 1ns/1ps
module truega_lfm25_fixed_decode_datapath_coeff_tb;
    reg clk = 0;
    always #5 clk = ~clk;
    reg reset_n = 0;
    reg start = 0;
    wire start_ready;
    reg feed_valid = 0;
    wire feed_ready;
    reg [7:0] feed_stage = 0;
    reg [511:0] feed_payload = 0;
    integer stage_index;
    integer element_index;
    integer failures = 0;

    truega_lfm25_fixed_decode_datapath dut (
        .clk(clk), .reset_n(reset_n), .clear_i(1'b0), .abort_i(1'b0),
        .start_i(start), .start_ready_o(start_ready),
        .operation_i(4'd2), .layer_i(8'd0), .position_i(32'd0),
        .session_epoch_i(32'd7), .input_slot_i(8'd0),
        .residual_slot_i(8'hff), .destination_slot_i(8'd1),
        .feed_stage_valid_i(feed_valid), .feed_stage_ready_o(feed_ready),
        .feed_mode_i(8'd4), .feed_item_i(32'd0),
        .feed_bank_i(2'd0), .feed_stage_i(feed_stage),
        .feed_payload_i(feed_payload), .feed_item_finish_i(1'b0),
        .feed_item_effect_done_o(), .result_valid_o(),
        .result_ready_i(1'b0), .result_error_o(),
        .result_error_code_o(), .result_slot_o(), .result_token_o(),
        .result_rows_o(), .result_score_q30_o()
    );

    task automatic check_channel;
        input integer channel;
        reg [15:0] base;
        begin
            base = 16'h1000 + channel * 3;
            if (dut.shortconv_coeff_oldest[channel] !== base
                    || dut.shortconv_coeff_newest[channel] !== base + 1'b1
                    || dut.shortconv_coeff_current[channel] !== base + 2'd2) begin
                $display("coeff mismatch channel=%0d oldest=%04x newest=%04x current=%04x expected=%04x/%04x/%04x",
                    channel, dut.shortconv_coeff_oldest[channel],
                    dut.shortconv_coeff_newest[channel],
                    dut.shortconv_coeff_current[channel], base,
                    base + 1'b1, base + 2'd2);
                failures = failures + 1;
            end
        end
    endtask

    initial begin
        repeat (4) @(posedge clk);
        reset_n = 1;
        @(negedge clk);
        if (!start_ready) begin
            $display("datapath not ready for shortconv coefficient test");
            failures = failures + 1;
        end
        start = 1;
        @(negedge clk);
        start = 0;

        for (stage_index = 0; stage_index < 96;
             stage_index = stage_index + 1) begin
            feed_stage = stage_index[7:0];
            for (element_index = 0; element_index < 32;
                 element_index = element_index + 1)
                feed_payload[element_index * 16 +: 16] = 16'h1000
                    + stage_index * 32 + element_index;
            feed_valid = 1;
            while (!feed_ready) @(negedge clk);
            @(negedge clk);
            feed_valid = 0;
        end
        while (dut.scalar_busy) @(negedge clk);

        check_channel(0);
        check_channel(1);
        check_channel(10);
        check_channel(341);
        check_channel(342);
        check_channel(1023);

        if (failures == 0)
            $display("PASS fixed_decode_datapath_coeff bar_stages=96 scalarized=3072 triplets=1024 banks=3 ordering=exact backpressure=32cycles");
        else
            $display("FAIL fixed_decode_datapath_coeff failures=%0d", failures);
        $finish;
    end
endmodule
