`timescale 1ns/1ps

module truega_q8_0_cached_pair_slot_tb;
    localparam BLOCKS_PER_ROW = 32;

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg start_i = 1'b0;
    reg [31:0] control_i = 32'd0;
    reg [271:0] activation_block_i = 272'd0;
    reg [271:0] weight_block_i = 272'd0;
    wire busy_o;
    wire done_o;
    wire error_o;
    wire signed [31:0] dot_o;
    wire signed [63:0] term_q30_o;
    wire signed [63:0] row_q30_o;

    integer vector_dot [0:BLOCKS_PER_ROW-1];
    reg [15:0] vector_activation_scale [0:BLOCKS_PER_ROW-1];
    reg [15:0] vector_weight_scale [0:BLOCKS_PER_ROW-1];
    reg [255:0] vector_activation_quants [0:BLOCKS_PER_ROW-1];
    reg [255:0] vector_weight_quants [0:BLOCKS_PER_ROW-1];
    reg signed [63:0] vector_term [0:BLOCKS_PER_ROW-1];
    reg signed [63:0] expected_accumulator;
    integer ignored_row;
    integer ignored_block;
    integer ignored_first;
    integer ignored_last;
    reg [63:0] ignored_fp;
    integer ignored_bound;
    integer vector_count = 0;
    integer vector_index;
    integer failures = 0;
    integer file_descriptor;
    integer scan_result;
    integer wait_cycles;
    reg [1023:0] vector_path;
    reg [1023:0] line;

    always #5 clk = ~clk;

    truega_q8_0_cached_pair_slot #(
        .CACHED_PAIR_ENABLE(1)
    ) dut (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(start_i),
        .control_i(control_i),
        .activation_block_i(activation_block_i),
        .weight_block_i(weight_block_i),
        .busy_o(busy_o),
        .done_o(done_o),
        .error_o(error_o),
        .dot_o(dot_o),
        .term_q30_o(term_q30_o),
        .row_q30_o(row_q30_o)
    );

    initial begin
        if (!$value$plusargs("VECTORS=%s", vector_path)) begin
            $display("FAIL missing +VECTORS=path");
            $finish_and_return(1);
        end
        file_descriptor = $fopen(vector_path, "r");
        if (file_descriptor == 0) begin
            $display("FAIL cannot open %0s", vector_path);
            $finish_and_return(1);
        end
        scan_result = $fgets(line, file_descriptor);
        scan_result = $fgets(line, file_descriptor);
        while (!$feof(file_descriptor) && vector_count < BLOCKS_PER_ROW) begin
            scan_result = $fscanf(file_descriptor,
                "%d %d %d %d %h %h %h %h %d %h %h %d\n",
                ignored_row, ignored_block, ignored_first, ignored_last,
                vector_activation_scale[vector_count],
                vector_weight_scale[vector_count],
                vector_activation_quants[vector_count],
                vector_weight_quants[vector_count], vector_dot[vector_count],
                vector_term[vector_count], ignored_fp, ignored_bound);
            if (scan_result == 12) begin
                if (ignored_row != 0 || ignored_block != vector_count) begin
                    $display("FAIL non-canonical row fixture");
                    $finish_and_return(1);
                end
                vector_count = vector_count + 1;
            end
        end
        $fclose(file_descriptor);
        if (vector_count != BLOCKS_PER_ROW) begin
            $display("FAIL loaded %0d row blocks", vector_count);
            $finish_and_return(1);
        end

        repeat (4) @(negedge clk);
        reset_n = 1'b1;

        // Cache the row activation once. Every load is itself one ordinary fixed
        // slot call and must retire without touching the row accumulator.
        for (vector_index = 0; vector_index < BLOCKS_PER_ROW;
                vector_index = vector_index + 1) begin
            @(negedge clk);
            control_i = (vector_index << 8) | 32'h10;
            activation_block_i = {
                vector_activation_quants[vector_index],
                vector_activation_scale[vector_index]
            };
            weight_block_i = 272'd0;
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            wait_cycles = 0;
            while (!done_o && wait_cycles < 10) begin
                @(negedge clk);
                wait_cycles = wait_cycles + 1;
            end
            if (!done_o || error_o || busy_o) begin
                $display("FAIL cache block=%0d done=%b error=%b busy=%b",
                    vector_index, done_o, error_o, busy_o);
                failures = failures + 1;
            end
        end

        // Odd pair indices are never valid and must fail without entering busy.
        @(negedge clk);
        control_i = 32'h0000_0120;
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;
        wait_cycles = 0;
        while (!done_o && wait_cycles < 10) begin
            @(negedge clk);
            wait_cycles = wait_cycles + 1;
        end
        if (!done_o || !error_o || busy_o) begin
            $display("FAIL odd cached pair did not retire as error");
            failures = failures + 1;
        end

        expected_accumulator = 64'sd0;
        for (vector_index = 0; vector_index < BLOCKS_PER_ROW;
                vector_index = vector_index + 2) begin
            @(negedge clk);
            control_i = (vector_index << 8)
                      | 32'h20
                      | ((vector_index == 0) ? 1 : 0)
                      | ((vector_index == 30) ? 2 : 0);
            activation_block_i = {
                vector_weight_quants[vector_index],
                vector_weight_scale[vector_index]
            };
            weight_block_i = {
                vector_weight_quants[vector_index + 1],
                vector_weight_scale[vector_index + 1]
            };
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            wait_cycles = 0;
            while (!done_o && wait_cycles < 250) begin
                @(negedge clk);
                wait_cycles = wait_cycles + 1;
            end
            expected_accumulator = expected_accumulator
                                 + vector_term[vector_index]
                                 + vector_term[vector_index + 1];
            if (!done_o || error_o || busy_o
                    || dot_o !== vector_dot[vector_index + 1]
                    || term_q30_o !== vector_term[vector_index + 1]
                    || row_q30_o !== expected_accumulator) begin
                $display("FAIL pair=%0d done=%b error=%b dot=%0d/%0d term=%h/%h row=%h/%h",
                    vector_index / 2, done_o, error_o,
                    dot_o, vector_dot[vector_index + 1],
                    term_q30_o, vector_term[vector_index + 1],
                    row_q30_o, expected_accumulator);
                failures = failures + 1;
            end
        end

        // The old single-block function remains bit-for-bit compatible.
        @(negedge clk);
        control_i = 32'h0000_0003;
        activation_block_i = {vector_activation_quants[0], vector_activation_scale[0]};
        weight_block_i = {vector_weight_quants[0], vector_weight_scale[0]};
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;
        wait_cycles = 0;
        while (!done_o && wait_cycles < 150) begin
            @(negedge clk);
            wait_cycles = wait_cycles + 1;
        end
        if (!done_o || error_o || dot_o !== vector_dot[0]
                || term_q30_o !== vector_term[0] || row_q30_o !== vector_term[0]) begin
            $display("FAIL legacy compatibility through cached wrapper");
            failures = failures + 1;
        end

        if (failures == 0) begin
            $display("PASS q8_0_cached_pair_slot cache_loads=32 pair_calls=16 exact_second_dot exact_terms exact_row legacy_compatible");
            $finish;
        end
        $display("FAIL q8_0_cached_pair_slot failures=%0d", failures);
        $finish_and_return(1);
    end

    initial begin
        #300000;
        $display("FAIL q8_0_cached_pair_slot simulation timeout");
        $finish_and_return(1);
    end
endmodule
