`timescale 1ns/1ps

module truega_lfm25_feed_decode_slice_tb;
    localparam [31:0] EPOCH_ONE = 32'h5e55_0001;
    localparam [31:0] EPOCH_TWO = 32'h5e55_0002;

    reg clk = 1'b0;
    always #5 clk = ~clk;
    reg reset_n = 1'b0;
    reg bar_valid = 1'b0;
    reg [18:0] bar_address = 19'd0;
    reg [31:0] bar_data = 32'd0;
    reg [3:0] bar_strobe = 4'd0;
    wire bar_ready;
    reg irq_ack = 1'b0;
    reg control_write = 1'b0;
    reg [31:0] control_value = 32'd0;
    wire irq_retire;
    wire [31:0] capability_magic;
    wire [31:0] capability_version_bytes;
    wire [31:0] capability_bits;
    wire [31:0] capability_generation;
    wire [31:0] capability_shape_tag;
    wire [31:0] feed_state;
    wire [31:0] retired_mode_layer;
    wire [31:0] retired_epoch;
    wire [31:0] retired_sequence;
    wire [31:0] retired_item;
    wire [31:0] feed_error;
    wire [31:0] completion_count;
    wire final_result_valid;
    reg final_result_ready = 1'b0;
    wire final_result_error;
    wire [7:0] final_result_error_code;
    wire [36:0] final_result_handle;
    wire [12:0] projection_rows_retired;
    reg output_read_valid = 1'b0;
    wire output_read_ready;
    reg [9:0] output_read_index = 10'd0;
    wire output_read_rsp_valid;
    reg output_read_rsp_ready = 1'b0;
    wire output_read_error;
    wire signed [63:0] output_read_q30;

    integer assertions = 0;
    integer writes = 0;
    integer irq_pulses = 0;
    integer cycles = 0;
    integer slot_number;
    integer word_number;
    integer row_number;
    reg signed [63:0] captured_row0 = 64'sd0;
    reg signed [63:0] captured_row1 = 64'sd0;
    reg signed [63:0] captured_row1023 = 64'sd0;

    truega_lfm25_feed_decode_slice dut (
        .clk(clk), .reset_n(reset_n),
        .bar2_write_valid_i(bar_valid),
        .bar2_write_address_i(bar_address),
        .bar2_write_data_i(bar_data),
        .bar2_write_strobe_i(bar_strobe),
        .bar2_write_ready_o(bar_ready),
        .irq_ack_i(irq_ack), .feed_control_write_i(control_write),
        .feed_control_value_i(control_value), .irq_retire_o(irq_retire),
        .capability_magic_o(capability_magic),
        .capability_version_record_bytes_o(capability_version_bytes),
        .capability_bits_o(capability_bits),
        .capability_model_generation_o(capability_generation),
        .capability_shape_set_tag_o(capability_shape_tag),
        .feed_state_o(feed_state),
        .retired_mode_layer_o(retired_mode_layer),
        .retired_session_epoch_o(retired_epoch),
        .retired_sequence_o(retired_sequence),
        .retired_item_o(retired_item),
        .feed_error_code_o(feed_error),
        .completion_count_o(completion_count),
        .final_result_valid_o(final_result_valid),
        .final_result_ready_i(final_result_ready),
        .final_result_error_o(final_result_error),
        .final_result_error_code_o(final_result_error_code),
        .final_result_handle_o(final_result_handle),
        .projection_rows_retired_o(projection_rows_retired),
        .output_read_valid_i(output_read_valid),
        .output_read_ready_o(output_read_ready),
        .output_read_index_i(output_read_index),
        .output_read_rsp_valid_o(output_read_rsp_valid),
        .output_read_rsp_ready_i(output_read_rsp_ready),
        .output_read_error_o(output_read_error),
        .output_read_q30_o(output_read_q30)
    );

    task automatic check;
        input condition;
        input [8*112-1:0] message;
        begin
            assertions = assertions + 1;
            if (!condition) begin
                $display("FAIL: %0s cycle=%0d slice_state=%0d phase=%0d feed_state=%0d row=%0d count=%0d",
                    message, cycles, dut.state, dut.phase, feed_state,
                    dut.projection_row, completion_count);
                $fatal(1);
            end
        end
    endtask

    function automatic [271:0] constant_native_block;
        input [15:0] scale_f16;
        input signed [7:0] quant_value;
        integer quant;
        reg [271:0] value;
        begin
            value = 272'd0;
            value[15:0] = scale_f16;
            for (quant = 0; quant < 32; quant = quant + 1)
                value[16 + quant * 8 +: 8] = quant_value;
            constant_native_block = value;
        end
    endfunction

    function automatic [271:0] sparse_native_block;
        input [15:0] scale_f16;
        input signed [7:0] first_quant;
        reg [271:0] value;
        begin
            value = 272'd0;
            value[15:0] = scale_f16;
            value[23:16] = first_quant;
            sparse_native_block = value;
        end
    endfunction

    function automatic [271:0] projection_block;
        input integer row;
        input integer block;
        begin
            if (block != 0)
                projection_block = 272'd0;
            else begin
                case (row)
                    0: projection_block = sparse_native_block(16'h3800, 8'sd2);
                    1: projection_block = sparse_native_block(16'h3800, -8'sd2);
                    1023: projection_block = sparse_native_block(16'h3400, 8'sd1);
                    default: projection_block = 272'd0;
                endcase
            end
        end
    endfunction

    task automatic bar_write;
        input [18:0] address;
        input [31:0] data;
        input [3:0] strobe;
        begin
            while (!bar_ready)
                @(negedge clk);
            bar_address = address;
            bar_data = data;
            bar_strobe = strobe;
            bar_valid = 1'b1;
            @(negedge clk);
            bar_valid = 1'b0;
            bar_strobe = 4'd0;
            writes = writes + 1;
        end
    endtask

    task automatic write_q8_slot;
        input integer slot;
        input [271:0] block;
        begin
            for (word_number = 0; word_number < 9;
                 word_number = word_number + 1)
                bar_write(slot * 64 + word_number * 4,
                    block[word_number * 32 +: 32],
                    word_number == 8 ? 4'h3 : 4'hf);
        end
    endtask

    task automatic write_embedding_payload;
        reg [271:0] block;
        begin
            block = constant_native_block(16'h3800, 8'sd2);
            for (slot_number = 0; slot_number < 32;
                 slot_number = slot_number + 1)
                write_q8_slot(slot_number, block);
        end
    endtask

    task automatic write_norm_payload;
        begin
            for (slot_number = 0; slot_number < 32;
                 slot_number = slot_number + 1)
                for (word_number = 0; word_number < 16;
                     word_number = word_number + 1)
                    bar_write(slot_number * 64 + word_number * 4,
                        32'h3f80_3f80, 4'hf);
        end
    endtask

    task automatic write_projection_payload;
        input integer row;
        begin
            for (slot_number = 0; slot_number < 32;
                 slot_number = slot_number + 1)
                write_q8_slot(slot_number, projection_block(row, slot_number));
        end
    endtask

    task automatic write_commit_header;
        input [7:0] mode;
        input [7:0] layer;
        input [7:0] lane_mask;
        input [7:0] payload_format;
        input [31:0] epoch;
        input [31:0] sequence_value;
        input [31:0] token;
        input [31:0] item;
        input [15:0] payload_bytes;
        input [31:0] generation;
        input [31:0] shape_tag;
        begin
            bar_write(19'h7f000, 32'h3244_4654, 4'hf);
            bar_write(19'h7f004, 32'h0040_0002, 4'hf);
            bar_write(19'h7f008, 32'h0000_01ff, 4'hf);
            bar_write(19'h7f00c,
                {payload_format, lane_mask, layer, mode}, 4'hf);
            bar_write(19'h7f010, epoch, 4'hf);
            bar_write(19'h7f014, sequence_value, 4'hf);
            bar_write(19'h7f018, 32'd0, 4'hf);
            bar_write(19'h7f01c, token, 4'hf);
            bar_write(19'h7f020, item, 4'hf);
            bar_write(19'h7f024, {16'd31, 16'd32}, 4'hf);
            bar_write(19'h7f028, {16'd0, payload_bytes}, 4'hf);
            bar_write(19'h7f02c, generation, 4'hf);
            bar_write(19'h7f030, shape_tag, 4'hf);
            bar_write(19'h7f034, 32'd1, 4'hf);
            bar_write(19'h7f038, 32'd0, 4'hf);
        end
    endtask

    task automatic publish_commit;
        begin
            bar_write(19'h7f03c, 32'h324d_4346, 4'hf);
        end
    endtask

    task automatic wait_and_ack;
        input [31:0] expected_state;
        input [7:0] expected_mode;
        input [7:0] expected_layer;
        input [31:0] expected_epoch;
        input [31:0] expected_sequence;
        input [31:0] expected_item;
        input [31:0] expected_error;
        input [31:0] expected_count;
        begin
            while (!irq_retire)
                @(negedge clk);
            check(feed_state == expected_state, "terminal state exact at IRQ");
            check(retired_mode_layer == {16'd0, expected_layer, expected_mode},
                "completion packs exact mode/layer");
            check(retired_epoch == expected_epoch
                    && retired_sequence == expected_sequence
                    && retired_item == expected_item,
                "completion tags exact epoch/sequence/item");
            check(feed_error == expected_error, "completion error exact");
            check(completion_count == expected_count,
                "completion count increments exactly once");
            @(negedge clk);
            check(!irq_retire, "retirement IRQ is one cycle");
            irq_ack = 1'b1;
            @(negedge clk);
            irq_ack = 1'b0;
            while (feed_state != 0)
                @(negedge clk);
        end
    endtask

    task automatic inspect_output;
        input [9:0] index;
        input signed [63:0] expected;
        reg signed [63:0] held;
        begin
            output_read_index = index;
            output_read_valid = 1'b1;
            while (!output_read_ready)
                @(negedge clk);
            @(negedge clk);
            output_read_valid = 1'b0;
            while (!output_read_rsp_valid)
                @(negedge clk);
            held = output_read_q30;
            check(!output_read_error && held === expected,
                "resident signed Q30 readback equals imported projection value");
            repeat (2) begin
                @(negedge clk);
                check(output_read_rsp_valid && output_read_q30 === held,
                    "resident read response stable under backpressure");
            end
            output_read_rsp_ready = 1'b1;
            @(negedge clk);
            output_read_rsp_ready = 1'b0;
        end
    endtask

    task automatic send_reset;
        begin
            @(negedge clk);
            control_write = 1'b1;
            control_value = 32'h3254_5352;
            @(negedge clk);
            control_write = 1'b0;
            control_value = 32'd0;
            repeat (3) @(negedge clk);
            check(feed_state == 0 && dut.phase == 0 && dut.state == 0
                    && dut.engine.state == 0 && !dut.frontend.poisoned_o,
                "RST2 clears completion, frontend, sequencer, and engine");
        end
    endtask

    always @(posedge clk) begin
        cycles <= cycles + 1;
        if (reset_n && irq_retire)
            irq_pulses <= irq_pulses + 1;
        if (dut.engine.projection_result_valid
                && dut.engine.projection_result_ready) begin
            case (dut.engine.projection_result_row)
                13'd0: captured_row0 <= dut.engine.projection_result_q30;
                13'd1: captured_row1 <= dut.engine.projection_result_q30;
                13'd1023: captured_row1023 <= dut.engine.projection_result_q30;
                default: begin end
            endcase
        end
        if (cycles > 15_000_000) begin
            $display("FAIL timeout slice=%0d phase=%0d engine=%0d projection_row=%0d block=%0d feed=%0d",
                dut.state, dut.phase, dut.engine.state,
                dut.engine_projection_row, dut.engine_projection_block,
                feed_state);
            $fatal(1);
        end
    end

    initial begin
        repeat (5) @(negedge clk);
        reset_n = 1'b1;
        repeat (2) @(negedge clk);
        check(capability_magic == 32'h3246_4754
                && capability_version_bytes == 32'h0040_0002
                && capability_bits == 32'h0000_01ff
                && capability_generation == 1
                && capability_shape_tag == 32'h03c6_2299,
            "slice exposes exact final TGF2 capability");

        write_embedding_payload();
        write_commit_header(0, 8'hff, 1, 3, EPOCH_ONE, 0, 17, 0,
            34, 32, 32'h46ea_2684);
        publish_commit();
        wait_and_ack(2, 0, 8'hff, EPOCH_ONE, 0, 0, 0, 1);

        write_norm_payload();
        write_commit_header(1, 2, 1, 1, EPOCH_ONE, 0,
            32'hffff_ffff, 0, 64, 32, 32'hf27a_4365);
        publish_commit();
        wait_and_ack(2, 1, 2, EPOCH_ONE, 0, 0, 0, 2);

        for (row_number = 0; row_number < 1024;
             row_number = row_number + 1) begin
            write_projection_payload(row_number);
            write_commit_header(8, 2, 1, 3, EPOCH_ONE, row_number,
                32'hffff_ffff, row_number, 34,
                (row_number + 1) * 32, 32'h15d6_8491);
            publish_commit();
            wait_and_ack(2, 8, 2, EPOCH_ONE, row_number, row_number,
                0, row_number + 3);
            check(projection_rows_retired == row_number + 1,
                "projection item retires only after rows_retired advances");
            if ((row_number & 255) == 255)
                $display("feed_decode_slice progress=%0d/1024 completions=%0d",
                    row_number + 1, completion_count);
        end

        check(completion_count == 1026 && irq_pulses == 1026,
            "1+1+1024 items produce exactly 1026 MSI retirements");
        while (!final_result_valid)
            @(negedge clk);
        check(!final_result_error && final_result_error_code == 0,
            "joined engine publishes successful final result");
        check(final_result_handle == {EPOCH_ONE, 1'b0, 4'd1},
            "final result is exact resident Q30 handle");
        check(captured_row0 != 0 && captured_row1 != 0
                && captured_row1023 != 0
                && !captured_row0[63] && captured_row1[63]
                && !captured_row1023[63],
            "projection produced exact signed-path witnesses");
        inspect_output(0, captured_row0);
        inspect_output(1, captured_row1);
        inspect_output(1023, captured_row1023);
        final_result_ready = 1'b1;
        @(negedge clk);
        final_result_ready = 1'b0;

        // Reset back to the only accepted graph start, then publish a valid
        // TGF2 norm record in the wrong graph order. The slice must retire it
        // FAILED, lock out further BAR writes, and recover only through RST2.
        send_reset();
        write_norm_payload();
        write_commit_header(1, 2, 1, 1, EPOCH_TWO, 0,
            32'hffff_ffff, 0, 64, 32, 32'hf27a_4365);
        publish_commit();
        wait_and_ack(3, 1, 2, EPOCH_TWO, 0, 0,
            32'hbad4_1001, 1027);
        repeat (3) @(negedge clk);
        check(!bar_ready && dut.sequence_failed,
            "malformed graph order fails closed after ACK");
        send_reset();
        check(completion_count == 1027 && bar_ready,
            "RST2 preserves count and reopens exact embedding start");

        $display("PASS truega_lfm25_feed_decode_slice assertions=%0d writes=%0d irq_pulses=%0d completions=%0d handle=%h q30=[%0d,%0d,%0d]",
            assertions, writes, irq_pulses, completion_count,
            {EPOCH_ONE, 1'b0, 4'd1},
            captured_row0, captured_row1, captured_row1023);
        $finish;
    end
endmodule
