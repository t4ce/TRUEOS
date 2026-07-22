`timescale 1ns/1ps

module truega_lfm25_decode_endpoint_tb;
    reg clk = 1'b0;
    always #5 clk = ~clk;

    reg reset_n = 1'b0;
    reg bar_valid = 1'b0;
    reg [18:0] bar_address = 19'd0;
    reg [31:0] bar_data = 32'd0;
    reg [3:0] bar_strobe = 4'd0;
    wire bar_ready;
    reg irq_ack = 1'b0;
    reg feed_control_write = 1'b0;
    reg [31:0] feed_control_value = 32'd0;
    reg [31:0] decode_command = 32'd0;
    reg [31:0] decode_position = 32'd0;
    reg [31:0] decode_epoch = 32'd0;
    reg decode_doorbell = 1'b0;
    reg [31:0] decode_doorbell_value = 32'd0;

    wire irq_retire;
    wire feed_irq_retire;
    wire decode_irq_retire;
    wire decode_irq_owned;
    wire [31:0] feed_magic;
    wire [31:0] feed_version_bytes;
    wire [31:0] feed_capability;
    wire [31:0] feed_generation;
    wire [31:0] feed_shape_tag;
    wire [31:0] feed_state;
    wire [31:0] feed_mode_layer;
    wire [31:0] feed_retired_epoch;
    wire [31:0] feed_retired_sequence;
    wire [31:0] feed_retired_item;
    wire [31:0] feed_error;
    wire [31:0] feed_completion_count;
    wire [31:0] decode_magic;
    wire [31:0] decode_capability;
    wire [31:0] decode_state;
    wire [31:0] decode_result0;
    wire [31:0] decode_result1;
    wire signed [63:0] decode_score;
    wire [6:0] operation_ordinal;
    wire [31:0] feed_items_retired;
    wire controller_poisoned;
    wire controller_busy;
    wire frontend_poisoned;

    integer assertions = 0;
    integer accepted_writes = 0;
    integer irq_pulses = 0;
    integer feed_irq_pulses = 0;
    integer decode_irq_pulses = 0;
    integer simultaneous_irq_pulses = 0;

    truega_lfm25_decode_endpoint #(.FAST_SCHEDULE_SIM(1)) dut (
        .clk(clk), .reset_n(reset_n),
        .bar2_write_valid_i(bar_valid),
        .bar2_write_address_i(bar_address),
        .bar2_write_data_i(bar_data),
        .bar2_write_strobe_i(bar_strobe),
        .bar2_write_ready_o(bar_ready),
        .irq_ack_i(irq_ack), .feed_control_write_i(feed_control_write),
        .feed_control_value_i(feed_control_value),
        .decode_command_i(decode_command),
        .decode_position_i(decode_position),
        .decode_session_epoch_i(decode_epoch),
        .decode_doorbell_i(decode_doorbell),
        .decode_doorbell_value_i(decode_doorbell_value),
        .irq_retire_o(irq_retire),
        .feed_irq_retire_o(feed_irq_retire),
        .decode_irq_retire_o(decode_irq_retire),
        .decode_irq_owned_o(decode_irq_owned),
        .feed_capability_magic_o(feed_magic),
        .feed_capability_version_record_bytes_o(feed_version_bytes),
        .feed_capability_bits_o(feed_capability),
        .feed_capability_model_generation_o(feed_generation),
        .feed_capability_shape_set_tag_o(feed_shape_tag),
        .feed_state_o(feed_state),
        .feed_retired_mode_layer_o(feed_mode_layer),
        .feed_retired_session_epoch_o(feed_retired_epoch),
        .feed_retired_sequence_o(feed_retired_sequence),
        .feed_retired_item_o(feed_retired_item),
        .feed_error_code_o(feed_error),
        .feed_completion_count_o(feed_completion_count),
        .decode_capability_magic_o(decode_magic),
        .decode_capability_bits_o(decode_capability),
        .decode_state_o(decode_state), .decode_result0_o(decode_result0),
        .decode_result1_o(decode_result1),
        .decode_argmax_score_q30_o(decode_score),
        .operation_ordinal_o(operation_ordinal),
        .feed_items_retired_o(feed_items_retired),
        .controller_poisoned_o(controller_poisoned),
        .controller_busy_o(controller_busy),
        .frontend_poisoned_o(frontend_poisoned)
    );

    always @(posedge clk) begin
        // Sample after nonblocking retirement publication from the DUT.
        #1;
        if (irq_retire)
            irq_pulses = irq_pulses + 1;
        if (feed_irq_retire)
            feed_irq_pulses = feed_irq_pulses + 1;
        if (decode_irq_retire)
            decode_irq_pulses = decode_irq_pulses + 1;
        if (feed_irq_retire && decode_irq_retire)
            simultaneous_irq_pulses = simultaneous_irq_pulses + 1;
    end

    task automatic check;
        input condition;
        input [8*112-1:0] message;
        begin
            assertions = assertions + 1;
            if (!condition) begin
                $display("FAIL: %0s", message);
                $fatal(1);
            end
        end
    endtask

    task automatic bar_write;
        input [18:0] address;
        input [31:0] data;
        input [3:0] strobe;
        begin
            @(negedge clk);
            bar_valid = 1'b1;
            bar_address = address;
            bar_data = data;
            bar_strobe = strobe;
            #1;
            check(bar_ready, "expected BAR2 dword was admitted");
            @(negedge clk);
            bar_valid = 1'b0;
            bar_strobe = 4'd0;
            accepted_writes = accepted_writes + 1;
        end
    endtask

    task automatic blocked_bar_write;
        input [18:0] address;
        input [31:0] data;
        input [8*112-1:0] message;
        begin
            @(negedge clk);
            bar_valid = 1'b1;
            bar_address = address;
            bar_data = data;
            bar_strobe = 4'hf;
            #1;
            check(!bar_ready, message);
            @(negedge clk);
            bar_valid = 1'b0;
            bar_strobe = 4'd0;
        end
    endtask

    task automatic pulse_irq_ack;
        begin
            @(negedge clk);
            irq_ack = 1'b1;
            @(negedge clk);
            irq_ack = 1'b0;
        end
    endtask

    task automatic ring_decode;
        begin
            @(negedge clk);
            decode_doorbell = 1'b1;
            decode_doorbell_value = 32'h4f43_4544;
            @(negedge clk);
            decode_doorbell = 1'b0;
        end
    endtask

    task automatic stage_embedding;
        integer stage_index;
        integer word_index;
        reg [3:0] strobe;
        begin
            for (stage_index = 0; stage_index < 32;
                 stage_index = stage_index + 1) begin
                for (word_index = 0; word_index < 9;
                     word_index = word_index + 1) begin
                    strobe = word_index == 8 ? 4'h3 : 4'hf;
                    bar_write(stage_index * 64 + word_index * 4,
                        {8'he1, stage_index[7:0], word_index[7:0], 8'h5a},
                        strobe);
                end
            end
        end
    endtask

    task automatic publish_embedding;
        begin
            bar_write(19'h7f000, 32'h3244_4654, 4'hf);
            bar_write(19'h7f004, 32'h0040_0002, 4'hf);
            bar_write(19'h7f008, 32'h0000_01ff, 4'hf);
            bar_write(19'h7f00c, 32'h0301_ff00, 4'hf);
            bar_write(19'h7f010, 32'h1234_5678, 4'hf);
            bar_write(19'h7f014, 32'd0, 4'hf);
            bar_write(19'h7f018, 32'd0, 4'hf);
            bar_write(19'h7f01c, 32'd42, 4'hf);
            bar_write(19'h7f020, 32'd0, 4'hf);
            bar_write(19'h7f024, 32'h001f_0020, 4'hf);
            bar_write(19'h7f028, 32'h0000_0022, 4'hf);
            bar_write(19'h7f02c, 32'd32, 4'hf);
            bar_write(19'h7f030, 32'h46ea_2684, 4'hf);
            bar_write(19'h7f034, 32'd1, 4'hf);
            bar_write(19'h7f038, 32'd0, 4'hf);
            bar_write(19'h7f03c, 32'h324d_4346, 4'hf);
        end
    endtask

    task automatic pulse_rst2;
        begin
            @(negedge clk);
            feed_control_value = 32'h3254_5352;
            feed_control_write = 1'b1;
            @(negedge clk);
            feed_control_write = 1'b0;
            feed_control_value = 32'd0;
            repeat (3) @(negedge clk);
        end
    endtask

    integer watchdog;
    integer irq_before_hold;
    initial begin
        repeat (4) @(posedge clk);
        reset_n = 1'b1;
        repeat (2) @(negedge clk);

        check(feed_magic == 32'h3246_4754
            && feed_version_bytes == 32'h0040_0002
            && feed_capability == 32'h0000_01ff
            && feed_generation == 32'd1
            && feed_shape_tag == 32'h03c6_2299,
            "TGF2 exact capability is published");
        check(decode_magic == 32'h3144_4754
            && decode_capability == 32'h0000_03ff,
            "TGD1 exact capability is published with dispatch ENABLE=1");
        check(feed_state == 0 && decode_state == 0
            && !frontend_poisoned && !controller_poisoned,
            "endpoint starts clean and idle");

        stage_embedding();
        publish_embedding();
        watchdog = 0;
        while (feed_state != 32'd2 && watchdog < 30) begin
            @(negedge clk);
            watchdog = watchdog + 1;
        end
        check(feed_state == 32'd2, "embedding feed reaches terminal COMPLETE");
        check(feed_mode_layer == 32'h0000_ff00
            && feed_retired_epoch == 32'h1234_5678
            && feed_retired_sequence == 0 && feed_retired_item == 0
            && feed_error == 0 && feed_completion_count == 1,
            "embedding TGF2 retirement identity is exact");
        check(feed_items_retired == 1 && operation_ordinal == 0,
            "controller retires embedding feed before its TGD1 invocation");
        check(feed_irq_pulses == 1 && decode_irq_pulses == 0
            && irq_pulses == 1,
            "embedding feed emits exactly one shared retirement pulse");

        irq_before_hold = irq_pulses;
        repeat (5) @(negedge clk);
        check(feed_state == 32'd2 && irq_pulses == irq_before_hold,
            "feed terminal envelope persists without duplicate interrupt");
        blocked_bar_write(19'h00000, 32'hfeed_0001,
            "BAR2 remains blocked while feed terminal ownership awaits ACK");

        // A TGD1 command is not admitted until its preceding feed completion is
        // consumed through the shared ACK lane.
        decode_command = 32'hffff_ff00;
        decode_position = 0;
        decode_epoch = 32'h1234_5678;
        ring_decode();
        repeat (2) @(negedge clk);
        check(decode_state == 0 && decode_irq_pulses == 0,
            "TGD1 doorbell is gated while TGF2 owns the shared completion");

        pulse_irq_ack();
        repeat (2) @(negedge clk);
        check(feed_state == 0, "shared ACK returns TGF2 ownership to IDLE");

        ring_decode();
        watchdog = 0;
        while (decode_state != 32'd2 && watchdog < 30) begin
            @(negedge clk);
            watchdog = watchdog + 1;
        end
        check(decode_state == 32'd2 && decode_result0 == 0
            && decode_result1 == 0 && decode_score == 0,
            "matching embedding TGD1 invocation retires resident slot zero");
        check(operation_ordinal == 1,
            "matching TGD1 completion advances the fixed 99-operation schedule");
        check(decode_irq_owned && decode_irq_pulses == 1
            && feed_irq_pulses == 1 && irq_pulses == 2,
            "TGD1 completion emits one pulse and owns shared IRQ until ACK");
        irq_before_hold = irq_pulses;
        repeat (5) @(negedge clk);
        check(decode_state == 32'd2 && decode_irq_owned
            && irq_pulses == irq_before_hold,
            "TGD1 result persists without duplicate interrupt before ACK");
        blocked_bar_write(19'h00000, 32'hfeed_0002,
            "BAR2 remains blocked while TGD1 owns shared completion");
        pulse_irq_ack();
        repeat (2) @(negedge clk);
        check(!decode_irq_owned && decode_state == 32'd2,
            "shared ACK releases TGD1 ownership without erasing its result");

        // Publishing commit magic with no header/payload is a strict ordering
        // violation.  The frontend poisons, the completion slot reports it
        // once, and capability words stay immutable rather than mimicking IDLE.
        bar_write(19'h7f03c, 32'h324d_4346, 4'hf);
        watchdog = 0;
        while (feed_state != 32'd4 && watchdog < 20) begin
            @(negedge clk);
            watchdog = watchdog + 1;
        end
        check(frontend_poisoned && feed_state == 32'd4
            && feed_error == 32'hbad4_0001
            && feed_completion_count == 2,
            "malformed publication reports one exact poisoned TGF2 envelope");
        check(feed_irq_pulses == 2 && decode_irq_pulses == 1
            && irq_pulses == 3 && simultaneous_irq_pulses == 0,
            "malformed order emits one non-colliding shared IRQ pulse");
        irq_before_hold = irq_pulses;
        repeat (5) @(negedge clk);
        check(irq_pulses == irq_before_hold && feed_state == 32'd4,
            "poisoned terminal state cannot spam the shared IRQ");
        check(feed_magic == 32'h3246_4754 && decode_magic == 32'h3144_4754,
            "malformed traffic does not corrupt immutable capabilities");

        pulse_rst2();
        check(feed_state == 0 && feed_error == 0 && !frontend_poisoned,
            "RST2 clears frontend poison and the TGF2 status envelope");
        check(operation_ordinal == 0 && feed_items_retired == 0
            && !controller_poisoned,
            "RST2 clears the complete controller and resident session");
        check(decode_state == 0 && !decode_irq_owned,
            "RST2 clears TGD1 dispatch and shared decode ownership");
        check(feed_completion_count == 2,
            "RST2 preserves the monotonic TGF2 diagnostic completion count");

        $display("PASS lfm25_decode_endpoint assertions=%0d writes=%0d irq=%0d feed=%0d decode=%0d path=TGF2->fixed99->TGD1 shared_ack=held malformed=poison+RST2",
            assertions, accepted_writes, irq_pulses,
            feed_irq_pulses, decode_irq_pulses);
        $finish;
    end
endmodule
