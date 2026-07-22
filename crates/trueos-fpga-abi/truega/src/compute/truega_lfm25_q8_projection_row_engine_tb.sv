`timescale 1ns/1ps

module truega_lfm25_q8_projection_row_engine_tb;
    localparam integer ROW_COUNT = 512;

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg abort = 1'b0;
    reg state_reset = 1'b0;
    wire state_reset_ready;
    wire state_reset_done;
    reg start = 1'b0;
    wire start_ready;
    reg activation_valid = 1'b0;
    wire activation_ready;
    wire [4:0] expected_activation_index;
    reg [4:0] activation_index = 5'd0;
    reg [271:0] activation_block = 272'd0;
    reg weight_valid = 1'b0;
    wire weight_ready;
    wire [12:0] expected_weight_row;
    wire [4:0] expected_weight_block;
    reg [12:0] weight_row = 13'd0;
    reg [4:0] weight_block_index = 5'd0;
    reg [271:0] weight_block = 272'd0;
    wire result_valid;
    reg result_ready = 1'b0;
    wire [12:0] result_row;
    wire signed [63:0] result_q30;
    wire result_first;
    wire result_last;
    wire busy;
    wire done;
    wire error;
    wire poisoned;
    wire [7:0] error_code;
    wire [12:0] rows_retired;

    integer failures = 0;
    integer row;
    integer block_index;
    integer cycles = 0;
    reg signed [63:0] held_result;
    reg [12:0] held_row;
    reg held_first;
    reg held_last;

    always #5 clk = ~clk;

    function automatic [271:0] native_block;
        input [15:0] scale_f16;
        input signed [7:0] first_quant;
        reg [271:0] value;
        begin
            value = 272'd0;
            value[15:0] = scale_f16;
            value[23:16] = first_quant;
            native_block = value;
        end
    endfunction

    function automatic signed [63:0] expected_row_q30;
        input integer requested_row;
        begin
            case (requested_row)
                0: expected_row_q30 = 64'sd1073741824;
                1: expected_row_q30 = -64'sd1073741824;
                2: expected_row_q30 = 64'sd536870912;
                default: expected_row_q30 = 64'sd0;
            endcase
        end
    endfunction

    function automatic [271:0] row_weight_block;
        input integer requested_row;
        input integer requested_block;
        begin
            if (requested_block != 0) begin
                row_weight_block = 272'd0;
            end else begin
                case (requested_row)
                    0: row_weight_block = native_block(16'h3800, 8'sd2);
                    1: row_weight_block = native_block(16'h3800, -8'sd2);
                    2: row_weight_block = native_block(16'h3800, 8'sd1);
                    default: row_weight_block = 272'd0;
                endcase
            end
        end
    endfunction

    truega_lfm25_q8_projection_row_engine #(
        .ROW_COUNT(ROW_COUNT)
    ) dut (
        .clk(clk), .reset_n(reset_n),
        .abort_i(abort),
        .state_reset_i(state_reset),
        .state_reset_ready_o(state_reset_ready),
        .state_reset_done_o(state_reset_done),
        .start_i(start), .start_ready_o(start_ready),
        .activation_valid_i(activation_valid),
        .activation_ready_o(activation_ready),
        .activation_block_index_o(expected_activation_index),
        .activation_block_index_i(activation_index),
        .activation_q8_block_i(activation_block),
        .weight_valid_i(weight_valid), .weight_ready_o(weight_ready),
        .weight_row_index_o(expected_weight_row),
        .weight_block_index_o(expected_weight_block),
        .weight_row_index_i(weight_row),
        .weight_block_index_i(weight_block_index),
        .weight_q8_block_i(weight_block),
        .result_valid_o(result_valid), .result_ready_i(result_ready),
        .result_row_index_o(result_row), .result_q30_o(result_q30),
        .result_first_o(result_first), .result_last_o(result_last),
        .busy_o(busy), .done_o(done), .error_o(error),
        .poisoned_o(poisoned), .error_code_o(error_code),
        .rows_retired_o(rows_retired)
    );

    // Dormant instances force elaboration of every allowed production shape.
    truega_lfm25_q8_projection_shape_probe #(.ROW_COUNT(1024)) shape_1024(.clk(clk));
    truega_lfm25_q8_projection_shape_probe #(.ROW_COUNT(2048)) shape_2048(.clk(clk));
    truega_lfm25_q8_projection_shape_probe #(.ROW_COUNT(3072)) shape_3072(.clk(clk));
    truega_lfm25_q8_projection_shape_probe #(.ROW_COUNT(4608)) shape_4608(.clk(clk));

    task automatic pulse_start;
        begin
            while (!start_ready) @(negedge clk);
            start = 1'b1;
            @(negedge clk);
            start = 1'b0;
        end
    endtask

    task automatic pulse_state_reset;
        begin
            while (!state_reset_ready) @(negedge clk);
            state_reset = 1'b1;
            @(negedge clk);
            state_reset = 1'b0;
            if (!state_reset_done)
                failures = failures + 1;
            @(negedge clk);
        end
    endtask

    task automatic load_activations;
        integer activation_number;
        begin
            for (activation_number = 0; activation_number < 32;
                 activation_number = activation_number + 1) begin
                while (!activation_ready) @(negedge clk);
                if (expected_activation_index !== activation_number[4:0])
                    failures = failures + 1;
                activation_index = activation_number[4:0];
                activation_block = activation_number == 0
                    ? native_block(16'h3800, 8'sd2) : 272'd0;
                activation_valid = 1'b1;
                @(negedge clk);
                activation_valid = 1'b0;
            end
        end
    endtask

    task automatic feed_weight;
        input integer requested_row;
        input integer requested_block;
        input [271:0] requested_weight;
        begin
            while (!weight_ready) @(negedge clk);
            if (expected_weight_row !== requested_row[12:0]
                    || expected_weight_block !== requested_block[4:0])
                failures = failures + 1;
            weight_row = requested_row[12:0];
            weight_block_index = requested_block[4:0];
            weight_block = requested_weight;
            weight_valid = 1'b1;
            @(negedge clk);
            weight_valid = 1'b0;
        end
    endtask

    task automatic feed_row;
        input integer requested_row;
        begin
            for (block_index = 0; block_index < 32;
                 block_index = block_index + 1)
                feed_weight(requested_row, block_index,
                            row_weight_block(requested_row, block_index));
        end
    endtask

    task automatic consume_row;
        input integer requested_row;
        input integer stall_cycles;
        integer stall;
        begin
            while (!result_valid) @(negedge clk);
            if (result_row !== requested_row[12:0]
                    || result_q30 !== expected_row_q30(requested_row)
                    || result_first !== (requested_row == 0)
                    || result_last !== (requested_row == ROW_COUNT - 1))
                failures = failures + 1;
            held_result = result_q30;
            held_row = result_row;
            held_first = result_first;
            held_last = result_last;
            for (stall = 0; stall < stall_cycles; stall = stall + 1) begin
                @(negedge clk);
                if (!result_valid || result_q30 !== held_result
                        || result_row !== held_row
                        || result_first !== held_first
                        || result_last !== held_last)
                    failures = failures + 1;
            end
            result_ready = 1'b1;
            @(negedge clk);
            result_ready = 1'b0;
        end
    endtask

    always @(posedge clk) begin
        cycles <= cycles + 1;
        if (cycles > 2_000_000) begin
            $display("FAIL q8_projection timeout state=%0d row=%0d block=%0d",
                     dut.state, expected_weight_row, expected_weight_block);
            $fatal(1);
        end
    end

    initial begin
        repeat (5) @(negedge clk);
        reset_n = 1'b1;
        repeat (2) @(negedge clk);

        // Malformed activation ordering poisons until explicit recovery.
        pulse_start();
        while (!activation_ready) @(negedge clk);
        activation_index = 5'd1;
        activation_block = 272'd0;
        activation_valid = 1'b1;
        @(negedge clk);
        activation_valid = 1'b0;
        if (!poisoned || !error || error_code != 8'd2)
            failures = failures + 1;
        pulse_state_reset();
        if (poisoned || error)
            failures = failures + 1;

        // Malformed row/block tag poisons before entering GEMV.
        pulse_start();
        load_activations();
        while (!weight_ready) @(negedge clk);
        weight_row = 13'd0;
        weight_block_index = 5'd1;
        weight_block = 272'd0;
        weight_valid = 1'b1;
        @(negedge clk);
        weight_valid = 1'b0;
        if (!poisoned || !error || error_code != 8'd3)
            failures = failures + 1;
        pulse_state_reset();

        // An external operation abort is fail-closed just like an arithmetic
        // protocol error: the partial projection is poisoned and reaches idle
        // only through the explicit state-reset recovery contract.
        pulse_start();
        load_activations();
        feed_weight(0, 0, 272'd0);
        abort = 1'b1;
        @(negedge clk);
        abort = 1'b0;
        if (!done || !poisoned || !error || error_code != 8'd6
                || result_valid)
            failures = failures + 1;
        pulse_state_reset();

        // A negative native scale reaches the reused GEMV scale guard and
        // poisons after the otherwise ordered row drains.
        pulse_start();
        load_activations();
        for (block_index = 0; block_index < 32;
             block_index = block_index + 1)
            feed_weight(0, block_index,
                block_index == 0
                    ? native_block(16'hbc00, 8'sd1) : 272'd0);
        while (!done) @(negedge clk);
        if (!poisoned || !error || error_code != 8'd4
                || result_valid)
            failures = failures + 1;
        pulse_state_reset();

        // Full smallest production shape. First/last tags and full signed-i64
        // values are checked on every row; first and last outputs are stalled.
        pulse_start();
        load_activations();
        for (row = 0; row < ROW_COUNT; row = row + 1) begin
            feed_row(row);
            consume_row(row, (row == 0 || row == ROW_COUNT - 1) ? 4 : 0);
            if ((row & 127) == 127)
                $display("q8_projection progress=%0d/%0d", row + 1, ROW_COUNT);
        end

        if (!done || busy || error || poisoned
                || rows_retired != ROW_COUNT)
            failures = failures + 1;
        if (!dut.parameter_contract_valid
                || !shape_1024.implementation.parameter_contract_valid
                || !shape_2048.implementation.parameter_contract_valid
                || !shape_3072.implementation.parameter_contract_valid
                || !shape_4608.implementation.parameter_contract_valid)
            failures = failures + 1;

        if (failures == 0)
            $display("PASS lfm25_q8_projection rows=512 exact_signed_i64_q30 ordered_32_blocks stable_backpressure first_last poison=tag+scale reset_recovery production_shapes=512+1024+2048+3072+4608");
        else begin
            $display("FAIL lfm25_q8_projection failures=%0d", failures);
            $fatal(1);
        end
        $finish;
    end
endmodule

module truega_lfm25_q8_projection_shape_probe #(
    parameter integer ROW_COUNT = 1024
) (
    input wire clk
);
    wire unused;
    truega_lfm25_q8_projection_row_engine #(
        .ROW_COUNT(ROW_COUNT)
    ) implementation (
        .clk(clk), .reset_n(1'b0),
        .abort_i(1'b0),
        .state_reset_i(1'b0), .state_reset_ready_o(),
        .state_reset_done_o(), .start_i(1'b0), .start_ready_o(),
        .activation_valid_i(1'b0), .activation_ready_o(),
        .activation_block_index_o(), .activation_block_index_i(5'd0),
        .activation_q8_block_i(272'd0),
        .weight_valid_i(1'b0), .weight_ready_o(),
        .weight_row_index_o(), .weight_block_index_o(),
        .weight_row_index_i(13'd0), .weight_block_index_i(5'd0),
        .weight_q8_block_i(272'd0),
        .result_valid_o(unused), .result_ready_i(1'b0),
        .result_row_index_o(), .result_q30_o(),
        .result_first_o(), .result_last_o(), .busy_o(), .done_o(),
        .error_o(), .poisoned_o(), .error_code_o(), .rows_retired_o()
    );
endmodule
