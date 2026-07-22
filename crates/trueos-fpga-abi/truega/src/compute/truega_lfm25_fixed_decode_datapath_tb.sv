`timescale 1ns/1ps
module truega_lfm25_fixed_decode_datapath_tb;
    reg clk = 0;
    always #5 clk = ~clk;
    reg reset_n = 0;
    reg clear = 0;
    reg abort = 0;
    reg start = 0;
    wire start_ready;
    reg [3:0] operation = 0;
    reg [7:0] layer = 8'hff;
    reg [7:0] input_slot = 8'hff;
    reg [7:0] residual_slot = 8'hff;
    reg [7:0] destination_slot = 0;
    reg stage_valid = 0;
    wire stage_ready;
    reg [7:0] mode = 0;
    reg [31:0] item = 0;
    reg [1:0] bank = 0;
    reg [7:0] stage = 0;
    reg [511:0] payload = 0;
    reg item_finish = 0;
    wire item_effect_done;
    wire result_valid;
    reg result_ready = 0;
    wire result_error;
    wire [7:0] result_error_code;
    wire [7:0] result_slot;
    integer failures = 0;
    integer i;

    truega_lfm25_fixed_decode_datapath dut (
        .clk(clk), .reset_n(reset_n), .clear_i(clear), .abort_i(abort),
        .start_i(start), .start_ready_o(start_ready),
        .operation_i(operation), .layer_i(layer), .position_i(32'd0),
        .session_epoch_i(32'd7), .input_slot_i(input_slot),
        .residual_slot_i(residual_slot),
        .destination_slot_i(destination_slot),
        .feed_stage_valid_i(stage_valid), .feed_stage_ready_o(stage_ready),
        .feed_mode_i(mode), .feed_item_i(item), .feed_bank_i(bank),
        .feed_stage_i(stage), .feed_payload_i(payload),
        .feed_item_finish_i(item_finish),
        .feed_item_effect_done_o(item_effect_done),
        .result_valid_o(result_valid), .result_ready_i(result_ready),
        .result_error_o(result_error),
        .result_error_code_o(result_error_code),
        .result_slot_o(result_slot), .result_token_o(), .result_rows_o(),
        .result_score_q30_o()
    );

    task automatic launch;
        input integer op;
        input integer src;
        input integer dst;
        begin
            @(negedge clk);
            operation = op;
            input_slot = src;
            destination_slot = dst;
            while (!start_ready) @(negedge clk);
            start = 1;
            @(negedge clk);
            start = 0;
        end
    endtask

    task automatic send_stage;
        input integer feed_mode;
        input integer feed_stage;
        input [511:0] feed_payload;
        begin
            mode = feed_mode;
            stage = feed_stage;
            payload = feed_payload;
            stage_valid = 1;
            while (!stage_ready) @(negedge clk);
            @(negedge clk);
            stage_valid = 0;
        end
    endtask

    task automatic consume_result;
        input integer expected_slot;
        begin
            while (!result_valid) @(negedge clk);
            if (result_error || result_slot != expected_slot) begin
                $display("datapath result error=%0d code=%0d slot=%0d expected=%0d",
                    result_error, result_error_code, result_slot, expected_slot);
                failures = failures + 1;
            end
            result_ready = 1;
            @(negedge clk);
            result_ready = 0;
        end
    endtask

    task automatic finish_item_before_result;
        begin
            item_finish = 1;
            @(negedge clk);
            item_finish = 0;
            if (item_effect_done || result_valid) begin
                $display("final item retired before the held numerical result");
                failures = failures + 1;
            end
        end
    endtask

    reg [511:0] bf16_ones;
    reg [511:0] nonzero_q8;
    initial begin
        bf16_ones = 0;
        nonzero_q8 = 0;
        for (i = 0; i < 32; i = i + 1)
            bf16_ones[i * 16 +: 16] = 16'h3f80;
        // fp16 scale=1, first signed sample=1. This catches payload clearing
        // or next-index skew at the controller/datapath handshake boundary.
        nonzero_q8[15:0] = 16'h3c00;
        nonzero_q8[23:16] = 8'h01;
        repeat (4) @(posedge clk);
        reset_n = 1;

        // Install epoch 7 through the one shared resident engine.
        launch(0, 8'hff, 0);
        for (i = 0; i < 32; i = i + 1)
            send_stage(0, i, nonzero_q8);
        mode = 0;
        item = 0;
        finish_item_before_result();
        consume_result(0);

        // Route the same shared resident Q30 slot through the RMS join and
        // scalarized 32x64-byte BF16 TGF2 payloads into Q8 slot zero.
        layer = 0;
        launch(1, 0, 0);
        for (i = 0; i < 32; i = i + 1)
            send_stage(1, i, bf16_ones);
        mode = 1;
        item = 0;
        finish_item_before_result();
        consume_result(0);

        if (failures == 0)
            $display("PASS fixed_decode_datapath shared_resident=1 embedding=32xQ8 rmsnorm=1024xBF16 typed_slots=strict");
        else
            $display("FAIL fixed_decode_datapath failures=%0d", failures);
        $finish;
    end
endmodule
