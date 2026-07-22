`timescale 1ns/1ps
module truega_lfm25_fixed_decode_controller_production_tb;
    reg clk = 0;
    always #5 clk = ~clk;
    reg reset_n = 0;
    reg clear = 0;
    reg item_valid = 0;
    wire item_ready;
    wire item_error;
    wire [31:0] item_error_code;
    wire payload_read_valid;
    wire [1:0] payload_read_bank;
    wire [7:0] payload_read_slot;
    wire [3:0] payload_read_word;
    reg payload_rsp_valid = 0;
    reg [31:0] payload_rsp_data = 0;
    reg execute_start = 0;
    wire engine_done, engine_error;
    wire [31:0] engine_error_code;
    wire [7:0] result_slot;
    wire [6:0] ordinal;
    wire [31:0] feed_items;
    wire poisoned;
    integer failures = 0;
    reg stall_active = 0;
    reg stall_seen = 0;
    reg [7:0] held_stage;
    reg [1:0] held_bank;
    reg [511:0] held_payload;

    truega_lfm25_fixed_decode_controller #(.FAST_SCHEDULE_SIM(0)) dut (
        .clk(clk), .reset_n(reset_n), .clear_i(clear), .abort_i(1'b0),
        .feed_item_valid_i(item_valid), .feed_item_ready_o(item_ready),
        .feed_item_mode_i(8'd0), .feed_item_layer_i(8'hff),
        .feed_item_lane_mask_i(8'd1),
        .feed_item_payload_format_i(8'd3),
        .feed_item_session_epoch_i(32'd7),
        .feed_item_sequence_i(32'd0), .feed_item_position_i(32'd0),
        .feed_item_token_i(32'd1), .feed_item_index_i(32'd0),
        .feed_item_stages_per_lane_i(16'd32),
        .feed_item_payload_bytes_per_stage_i(16'd34),
        .feed_frontend_poisoned_i(1'b0),
        .feed_item_error_o(item_error),
        .feed_item_error_code_o(item_error_code),
        .payload_read_valid_o(payload_read_valid),
        .payload_read_bank_o(payload_read_bank),
        .payload_read_slot_o(payload_read_slot),
        .payload_read_word_o(payload_read_word),
        .payload_read_ready_i(1'b1),
        .payload_read_rsp_valid_i(payload_rsp_valid),
        .payload_read_data_i(payload_rsp_data),
        .payload_read_error_i(1'b0),
        .execute_start_i(execute_start), .execute_operation_i(4'd0),
        .execute_layer_i(8'hff), .execute_position_i(32'd0),
        .execute_input_slot_i(8'hff),
        .execute_residual_slot_i(8'hff),
        .execute_session_epoch_i(32'd7),
        .execute_session_begin_i(1'b1),
        .engine_done_o(engine_done), .engine_error_o(engine_error),
        .engine_error_code_o(engine_error_code),
        .engine_result_slot_o(result_slot),
        .engine_result_position_o(), .engine_argmax_token_o(),
        .engine_argmax_rows_o(), .engine_argmax_score_q30_o(),
        .operation_ordinal_o(ordinal),
        .feed_items_retired_o(feed_items), .poisoned_o(poisoned),
        .busy_o()
    );

    // Synchronous stand-in for the already independently verified strict TGF2
    // frontend. Every block carries fp16 scale=1 and signed sample[0]=1.
    always @(posedge clk) begin
        payload_rsp_valid <= 0;
        if (payload_read_valid) begin
            payload_rsp_valid <= 1;
            payload_rsp_data <= payload_read_word == 0
                ? 32'h0001_3c00 : 32'd0;
        end

        // The real dequantizer backpressures between blocks. During that hold,
        // the controller must retain the same bank/stage/nonzero payload.
        if (reset_n && dut.state == 6'd4 && !dut.datapath_stage_ready) begin
            if (!stall_active) begin
                stall_active <= 1;
                stall_seen <= 1;
                held_stage <= dut.read_stage;
                held_bank <= dut.read_bank;
                held_payload <= dut.payload_buffer;
            end else if (dut.read_stage != held_stage
                    || dut.read_bank != held_bank
                    || dut.payload_buffer != held_payload) begin
                $display("controller changed payload/index under datapath backpressure");
                failures <= failures + 1;
            end
        end else
            stall_active <= 0;
        if (item_ready && !dut.datapath_result_valid) begin
            $display("embedding feed retired before held resident result");
            failures <= failures + 1;
        end
    end

    initial begin
        repeat (4) @(posedge clk);
        reset_n = 1;
        @(negedge clk);
        item_valid = 1;
        while (!item_ready) @(negedge clk);
        if (item_error || item_error_code != 0 || poisoned) begin
            $display("production feed error item_error=%0d code=%08x poison=%0d",
                item_error, item_error_code, poisoned);
            failures = failures + 1;
        end
        item_valid = 0;

        // Numerical evidence from the actual shared resident store: each
        // block's first signed sample reached Q30 and adjacent zeros stayed zero.
        if (dut.gen_datapath.datapath.resident.store.q30_memory[0] <= 0
                || dut.gen_datapath.datapath.resident.store.q30_memory[1] != 0
                || dut.gen_datapath.datapath.resident.store.q30_memory[32] <= 0) begin
            $display("production numeric mismatch q30[0]=%0d q30[1]=%0d q30[32]=%0d",
                dut.gen_datapath.datapath.resident.store.q30_memory[0],
                dut.gen_datapath.datapath.resident.store.q30_memory[1],
                dut.gen_datapath.datapath.resident.store.q30_memory[32]);
            failures = failures + 1;
        end

        @(negedge clk);
        execute_start = 1;
        @(negedge clk);
        execute_start = 0;
        while (!engine_done) @(negedge clk);
        if (engine_error || engine_error_code != 0 || result_slot != 0
                || ordinal != 1 || feed_items != 1 || !stall_seen) begin
            $display("production execute mismatch error=%0d code=%08x slot=%0d ordinal=%0d feeds=%0d stall=%0d",
                engine_error, engine_error_code, result_slot, ordinal,
                feed_items, stall_seen);
            failures = failures + 1;
        end

        if (failures == 0)
            $display("PASS fixed_decode_controller_production frontend_dwords=32x9 payload_hold=stable shared_resident=1 numeric_q30=nonzero result=TGD1");
        else
            $display("FAIL fixed_decode_controller_production failures=%0d", failures);
        $finish;
    end
endmodule
