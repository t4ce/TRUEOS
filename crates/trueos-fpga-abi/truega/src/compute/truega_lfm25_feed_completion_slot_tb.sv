`timescale 1ns/1ps

module truega_lfm25_feed_completion_slot_tb;
    reg clk = 1'b0;
    always #5 clk = ~clk;

    reg reset_n = 1'b0;
    reg item_valid = 1'b0;
    reg item_ready = 1'b0;
    reg [7:0] item_mode = 8'd0;
    reg [7:0] item_layer = 8'hff;
    reg [31:0] item_epoch = 32'd0;
    reg [31:0] item_sequence = 32'd0;
    reg [31:0] item_index = 32'd0;
    reg item_error = 1'b0;
    reg [31:0] item_error_code = 32'd0;
    reg frontend_poisoned = 1'b0;
    reg irq_ack = 1'b0;
    reg control_write = 1'b0;
    reg [31:0] control_value = 32'd0;
    wire frontend_state_reset;
    wire [31:0] state_value;
    wire [31:0] retired_mode_layer;
    wire [31:0] retired_epoch;
    wire [31:0] retired_sequence;
    wire [31:0] retired_item;
    wire [31:0] error_code;
    wire [31:0] completion_count;
    wire irq_retire;

    integer assertions = 0;
    integer irq_pulses = 0;

    truega_lfm25_feed_completion_slot dut (
        .clk(clk),
        .reset_n(reset_n),
        .item_valid_i(item_valid),
        .item_ready_i(item_ready),
        .item_mode_i(item_mode),
        .item_layer_i(item_layer),
        .item_session_epoch_i(item_epoch),
        .item_sequence_i(item_sequence),
        .item_index_i(item_index),
        .item_error_i(item_error),
        .item_error_code_i(item_error_code),
        .frontend_poisoned_i(frontend_poisoned),
        .irq_ack_i(irq_ack),
        .control_write_i(control_write),
        .control_value_i(control_value),
        .frontend_state_reset_o(frontend_state_reset),
        .state_o(state_value),
        .retired_mode_layer_o(retired_mode_layer),
        .retired_session_epoch_o(retired_epoch),
        .retired_sequence_o(retired_sequence),
        .retired_item_o(retired_item),
        .error_code_o(error_code),
        .completion_count_o(completion_count),
        .irq_retire_o(irq_retire)
    );

    always @(posedge clk)
        if (reset_n && irq_retire)
            irq_pulses = irq_pulses + 1;

    task automatic check;
        input condition;
        input [8*96-1:0] message;
        begin
            assertions = assertions + 1;
            if (!condition) begin
                $display("FAIL: %0s", message);
                $fatal(1);
            end
        end
    endtask

    task automatic next_cycle;
        begin
            @(negedge clk);
            #1;
        end
    endtask

    task automatic acknowledge;
        begin
            @(negedge clk);
            irq_ack = 1'b1;
            @(negedge clk);
            irq_ack = 1'b0;
            #1;
        end
    endtask

    task automatic control;
        input [31:0] value;
        begin
            @(negedge clk);
            control_write = 1'b1;
            control_value = value;
            @(negedge clk);
            control_write = 1'b0;
            control_value = 32'd0;
            #1;
        end
    endtask

    initial begin
        repeat (3) @(negedge clk);
        reset_n = 1'b1;
        next_cycle();
        check(state_value == 0 && completion_count == 0,
            "physical reset publishes IDLE and clears count");
        check(retired_mode_layer == 32'h0000_ff00 && error_code == 0,
            "physical reset publishes no-retirement sentinel");
        check(!irq_retire && !frontend_state_reset,
            "physical reset emits no event pulse");

        // Successful item: item_valid alone means BUSY, with arbitrary
        // backpressure duration and no premature interrupt.
        @(negedge clk);
        item_mode = 13;
        item_layer = 4;
        item_epoch = 32'h1234_5678;
        item_sequence = 32'd91;
        item_index = 32'd91;
        item_error = 1'b0;
        item_error_code = 32'hffff_ffff;
        item_valid = 1'b1;
        repeat (3) begin
            next_cycle();
            check(state_value == 1, "published item remains BUSY under backpressure");
            check(completion_count == 0 && !irq_retire,
                "BUSY item neither counts nor interrupts");
        end
        @(negedge clk);
        item_ready = 1'b1;
        @(negedge clk);
        item_ready = 1'b0;
        item_valid = 1'b0;
        #1;
        check(state_value == 2, "successful downstream retirement publishes COMPLETE");
        check(retired_mode_layer == 32'h0000_040d,
            "successful retirement packs exact mode/layer tag");
        check(retired_epoch == 32'h1234_5678 && retired_sequence == 91
                && retired_item == 91,
            "successful retirement latches exact session/sequence/item");
        check(error_code == 0 && completion_count == 1,
            "success publishes FEED_ERROR_NONE and increments count once");
        check(irq_retire, "success emits one retirement IRQ pulse");
        next_cycle();
        check(!irq_retire && state_value == 2,
            "IRQ pulse is single-cycle and terminal state is sticky");

        // Input mutation cannot change a held terminal envelope.
        item_mode = 1;
        item_layer = 15;
        item_epoch = 32'hdead_beef;
        item_sequence = 32'd999;
        item_index = 32'd888;
        repeat (3) next_cycle();
        check(retired_mode_layer == 32'h0000_040d
                && retired_epoch == 32'h1234_5678
                && retired_sequence == 91 && retired_item == 91,
            "terminal tags remain stable until and after ACK");
        check(completion_count == 1 && irq_pulses == 1,
            "backpressure/terminal hold cannot duplicate completion");
        acknowledge();
        check(state_value == 0, "shared BAR0 ACK returns COMPLETE to IDLE");
        check(retired_mode_layer == 32'h0000_040d,
            "ACK preserves last-retirement diagnostics");

        // Downstream compute failure. A coincident stale ACK must not erase it.
        @(negedge clk);
        item_mode = 14;
        item_layer = 7;
        item_epoch = 32'h2222_0001;
        item_sequence = 32'd12;
        item_index = 32'd12;
        item_error = 1'b1;
        item_error_code = 32'hbad5_1234;
        item_valid = 1'b1;
        item_ready = 1'b1;
        irq_ack = 1'b1;
        @(negedge clk);
        item_valid = 1'b0;
        item_ready = 1'b0;
        irq_ack = 1'b0;
        #1;
        check(state_value == 3 && error_code == 32'hbad5_1234,
            "compute error publishes FAILED and exact engine error");
        check(retired_mode_layer == 32'h0000_070e
                && retired_epoch == 32'h2222_0001
                && retired_sequence == 12 && retired_item == 12,
            "failure latches exact retired tags");
        check(completion_count == 2 && irq_retire,
            "failure counts and emits one IRQ despite coincident stale ACK");
        next_cycle();
        check(!irq_retire && irq_pulses == 2,
            "failure retirement IRQ is exactly one cycle");
        acknowledge();
        check(state_value == 0, "failure ACK returns IDLE");

        // A frontend poison with no published item becomes a terminal event,
        // preventing the host worker from timing out waiting for an item IRQ.
        @(negedge clk);
        frontend_poisoned = 1'b1;
        @(negedge clk);
        #1;
        check(state_value == 4, "frontend poison publishes POISONED");
        check(error_code == 32'hbad4_0001,
            "poison publishes ABI FEED_ERROR_FRONTEND_POISON");
        check(retired_mode_layer == 32'h0000_ff00
                && retired_epoch == 0 && retired_sequence == 0 && retired_item == 0,
            "poison without item publishes no-item tag envelope");
        check(completion_count == 3 && irq_retire,
            "poison counts and emits retirement IRQ");
        next_cycle();
        check(!irq_retire && irq_pulses == 3,
            "held poison cannot emit repeated IRQ pulses");
        acknowledge();
        check(state_value == 0, "poison terminal ACK returns IDLE");
        repeat (3) next_cycle();
        check(state_value == 0 && completion_count == 3 && irq_pulses == 3,
            "still-high poison is edge-qualified after ACK");

        // Unknown control values are deterministic no-ops. RST2 clears poison,
        // emits one frontend reset pulse, and preserves the monotonic count.
        control(32'h3254_5353);
        check(state_value == 0 && !frontend_state_reset
                && completion_count == 3,
            "unknown control value is ignored");
        control(32'h3254_5352);
        check(state_value == 0 && frontend_state_reset
                && completion_count == 3,
            "RST2 pulses frontend reset and preserves completion count");
        next_cycle();
        check(state_value == 0 && completion_count == 3 && irq_pulses == 3,
            "held poison is suppressed across registered frontend reset pulse");
        frontend_poisoned = 1'b0;
        next_cycle();
        check(!frontend_state_reset && !irq_retire,
            "reset and IRQ outputs are single-cycle pulses");

        // RST2 is also the explicit abort between fixed feed items and wins
        // over a BUSY item deterministically without fabricating retirement.
        @(negedge clk);
        item_valid = 1'b1;
        item_ready = 1'b0;
        item_mode = 8;
        item_layer = 2;
        next_cycle();
        check(state_value == 1, "new published item enters BUSY");
        @(negedge clk);
        control_write = 1'b1;
        control_value = 32'h3254_5352;
        item_ready = 1'b1;
        @(negedge clk);
        control_write = 1'b0;
        control_value = 0;
        item_valid = 1'b0;
        item_ready = 1'b0;
        #1;
        check(state_value == 0 && frontend_state_reset,
            "RST2 wins over coincident BUSY handshake");
        check(completion_count == 3 && !irq_retire && irq_pulses == 3,
            "explicit abort does not fabricate a completion");

        $display("PASS truega_lfm25_feed_completion_slot assertions=%0d irq_pulses=%0d",
            assertions, irq_pulses);
        $finish;
    end
endmodule
