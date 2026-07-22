`timescale 1ns/1ps
module truega_lfm25_fixed_decode_controller_tb;
    reg clk = 0;
    always #5 clk = ~clk;
    reg reset_n = 0;
    reg clear = 0;
    reg abort = 0;

    reg item_valid = 0;
    wire item_ready;
    reg [7:0] item_mode = 0;
    reg [7:0] item_layer = 8'hff;
    reg [7:0] item_lane_mask = 0;
    reg [7:0] item_format = 0;
    reg [31:0] item_epoch = 7;
    reg [31:0] item_sequence = 0;
    reg [31:0] item_position = 0;
    reg [31:0] item_token = 32'hffff_ffff;
    reg [31:0] item_index = 0;
    reg [15:0] item_stages = 0;
    reg [15:0] item_bytes = 0;
    reg frontend_poisoned = 0;
    wire item_error;
    wire [31:0] item_error_code;

    reg execute_start = 0;
    reg [3:0] execute_operation = 0;
    reg [7:0] execute_layer = 8'hff;
    reg [31:0] execute_position = 0;
    reg [7:0] execute_input = 8'hff;
    reg [7:0] execute_residual = 8'hff;
    reg [31:0] execute_epoch = 7;
    reg execute_session_begin = 0;
    wire engine_done, engine_error;
    wire [31:0] engine_error_code;
    wire [7:0] result_slot;
    wire [31:0] result_position, result_token, result_rows;
    wire signed [63:0] result_score;
    wire [6:0] ordinal;
    wire [31:0] feed_items;
    wire poisoned;
    integer failures = 0;
    integer commands = 0;
    integer expected_feeds = 0;

    truega_lfm25_fixed_decode_controller #(.FAST_SCHEDULE_SIM(1)) dut (
        .clk(clk), .reset_n(reset_n), .clear_i(clear), .abort_i(abort),
        .feed_item_valid_i(item_valid), .feed_item_ready_o(item_ready),
        .feed_item_mode_i(item_mode), .feed_item_layer_i(item_layer),
        .feed_item_lane_mask_i(item_lane_mask),
        .feed_item_payload_format_i(item_format),
        .feed_item_session_epoch_i(item_epoch),
        .feed_item_sequence_i(item_sequence),
        .feed_item_position_i(item_position), .feed_item_token_i(item_token),
        .feed_item_index_i(item_index),
        .feed_item_stages_per_lane_i(item_stages),
        .feed_item_payload_bytes_per_stage_i(item_bytes),
        .feed_frontend_poisoned_i(frontend_poisoned),
        .feed_item_error_o(item_error),
        .feed_item_error_code_o(item_error_code),
        .payload_read_valid_o(), .payload_read_bank_o(),
        .payload_read_slot_o(), .payload_read_word_o(),
        .payload_read_ready_i(1'b0), .payload_read_rsp_valid_i(1'b0),
        .payload_read_data_i(32'd0), .payload_read_error_i(1'b0),
        .execute_start_i(execute_start),
        .execute_operation_i(execute_operation),
        .execute_layer_i(execute_layer),
        .execute_position_i(execute_position),
        .execute_input_slot_i(execute_input),
        .execute_residual_slot_i(execute_residual),
        .execute_session_epoch_i(execute_epoch),
        .execute_session_begin_i(execute_session_begin),
        .engine_done_o(engine_done), .engine_error_o(engine_error),
        .engine_error_code_o(engine_error_code),
        .engine_result_slot_o(result_slot),
        .engine_result_position_o(result_position),
        .engine_argmax_token_o(result_token),
        .engine_argmax_rows_o(result_rows),
        .engine_argmax_score_q30_o(result_score),
        .operation_ordinal_o(ordinal),
        .feed_items_retired_o(feed_items), .poisoned_o(poisoned),
        .busy_o()
    );

    function automatic is_attention;
        input integer layer;
        begin
            is_attention = (16'h5524 & (16'h1 << layer)) != 0;
        end
    endfunction

    task automatic mode_shape;
        input integer mode;
        output integer items;
        output integer stages;
        output integer lanes;
        output integer bytes;
        output integer format;
        begin
            case (mode)
                5, 6, 8, 12, 14: items = 1024;
                9, 10: items = 512;
                13: items = 4608;
                15: items = 65536;
                default: items = 1;
            endcase
            case (mode)
                4: stages = 96;
                7: stages = 2;
                11: stages = 0;
                14: stages = 144;
                default: stages = 32;
            endcase
            if (mode == 5) lanes = 3;
            else if (mode == 7 || mode == 13) lanes = 2;
            else if (mode == 11) lanes = 0;
            else lanes = 1;
            if (mode == 11) bytes = 0;
            else if (mode == 1 || mode == 2 || mode == 3
                    || mode == 4 || mode == 7) bytes = 64;
            else bytes = 34;
            if (mode == 11) format = 0;
            else if (mode == 1 || mode == 2 || mode == 3 || mode == 7)
                format = 1;
            else if (mode == 4) format = 2;
            else format = 3;
        end
    endtask

    task automatic send_feed;
        input integer mode;
        input integer layer;
        integer items, stages, lanes, bytes, format;
        integer item;
        begin
            mode_shape(mode, items, stages, lanes, bytes, format);
            for (item = 0; item < items; item = item + 1) begin
                @(negedge clk);
                item_mode = mode;
                item_layer = layer;
                item_lane_mask = lanes == 0 ? 0 : (1 << lanes) - 1;
                item_format = format;
                item_sequence = item;
                item_index = item;
                item_stages = stages;
                item_bytes = bytes;
                item_token = mode == 0 ? 1 : 32'hffff_ffff;
                item_valid = 1;
                while (!item_ready) @(negedge clk);
                if (item_error) begin
                    $display("unexpected feed error mode=%0d item=%0d code=%08x",
                        mode, item, item_error_code);
                    failures = failures + 1;
                end
                item_valid = 0;
                expected_feeds = expected_feeds + 1;
            end
        end
    endtask

    task automatic execute;
        input integer op;
        input integer layer;
        input integer input_slot;
        input integer residual_slot;
        input integer output_slot;
        begin
            @(negedge clk);
            execute_operation = op;
            execute_layer = layer;
            execute_input = input_slot;
            execute_residual = residual_slot;
            execute_session_begin = commands == 0;
            execute_start = 1;
            @(negedge clk);
            execute_start = 0;
            while (!engine_done) @(negedge clk);
            if (engine_error) begin
                $display("unexpected execute error ordinal=%0d op=%0d code=%08x",
                    commands, op, engine_error_code);
                failures = failures + 1;
            end
            if (op == 9) begin
                if (result_token != 1 || result_rows != 65536)
                    failures = failures + 1;
            end else if (result_slot != output_slot || result_position != 0) begin
                $display("wrong slot ordinal=%0d got=%0d expected=%0d",
                    commands, result_slot, output_slot);
                failures = failures + 1;
            end
            commands = commands + 1;
        end
    endtask

    integer layer;
    integer h, b, d;
    initial begin
        repeat (4) @(posedge clk);
        reset_n = 1;

        send_feed(0, 8'hff);
        execute(0, 8'hff, 8'hff, 8'hff, 0);
        for (layer = 0; layer < 16; layer = layer + 1) begin
            h = layer % 3;
            b = (h + 1) % 3;
            d = (h + 2) % 3;
            send_feed(1, layer);
            execute(1, layer, h, 8'hff, 0);
            if (is_attention(layer)) begin
                send_feed(7, layer);
                send_feed(8, layer);
                send_feed(9, layer);
                send_feed(10, layer);
                send_feed(11, layer);
                send_feed(12, layer);
                execute(3, layer, 0, 8'hff, b);
            end else begin
                send_feed(4, layer);
                send_feed(5, layer);
                send_feed(6, layer);
                execute(2, layer, 0, 8'hff, b);
            end
            execute(4, layer, b, h, d);
            send_feed(2, layer);
            execute(5, layer, d, 8'hff, 0);
            send_feed(13, layer);
            send_feed(14, layer);
            execute(6, layer, 0, 8'hff, h);
            execute(7, layer, h, d, b);
        end
        send_feed(3, 8'hff);
        execute(8, 8'hff, 1, 8'hff, 0);
        send_feed(15, 8'hff);
        execute(9, 8'hff, 0, 8'hff, 8'hff);

        if (commands != 99 || expected_feeds != 194616
                || feed_items != 194616 || ordinal != 0 || poisoned) begin
            $display("bad totals commands=%0d expected_feeds=%0d hw_feeds=%0d ordinal=%0d poison=%0d",
                commands, expected_feeds, feed_items, ordinal, poisoned);
            failures = failures + 1;
        end

        // Malformed first mode poisons before any payload read and remains
        // fail-closed until the explicit TGF2 reset path drives clear_i.
        @(negedge clk); clear = 1;
        @(negedge clk); clear = 0;
        item_mode = 1;
        item_layer = 8'hff;
        item_lane_mask = 1;
        item_format = 1;
        item_sequence = 0;
        item_index = 0;
        item_stages = 32;
        item_bytes = 64;
        item_token = 32'hffff_ffff;
        item_valid = 1;
        while (!item_ready) @(negedge clk);
        if (!item_error || !poisoned || item_error_code != 32'hbad4_3001)
            failures = failures + 1;
        item_valid = 0;

        // Clear really resets the installed epoch/ordinal. Reach operation 1,
        // then prove a malformed TGD1 resident slot retires as an engine error
        // and sticks the same fail-closed poison state.
        @(negedge clk); clear = 1;
        @(negedge clk); clear = 0;
        commands = 0;
        item_epoch = 7;
        execute_epoch = 7;
        send_feed(0, 8'hff);
        execute(0, 8'hff, 8'hff, 8'hff, 0);
        send_feed(1, 0);
        @(negedge clk);
        execute_operation = 1;
        execute_layer = 0;
        execute_input = 1; // exact schedule requires Q30 slot h=0
        execute_residual = 8'hff;
        execute_session_begin = 0;
        execute_start = 1;
        @(negedge clk);
        execute_start = 0;
        while (!engine_done) @(negedge clk);
        if (!engine_error || !poisoned
                || engine_error_code != 32'hbad4_3102)
            failures = failures + 1;

        // A second explicit clear admits a different nonzero epoch at ordinal
        // zero, demonstrating that poison/domain state is not resurrected.
        @(negedge clk); clear = 1;
        @(negedge clk); clear = 0;
        item_epoch = 9;
        execute_epoch = 9;
        send_feed(0, 8'hff);
        if (poisoned || ordinal != 0 || feed_items != 1)
            failures = failures + 1;

        if (failures == 0)
            $display("PASS fixed_decode_controller ops=99 feeds=194616 schedule=0x5524 slots=mod3 split_tail=strict feed+tgd1_poison=sticky clear=domain-reset");
        else
            $display("FAIL fixed_decode_controller failures=%0d", failures);
        $finish;
    end
endmodule
