`timescale 1ns/1ps

module truega_q8_0_row_block_slot_tb;
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

    wire disabled_busy;
    wire disabled_done;
    wire disabled_error;
    wire signed [31:0] disabled_dot;
    wire signed [63:0] disabled_term;
    wire signed [63:0] disabled_row;

    integer vector_dot [0:BLOCKS_PER_ROW-1];
    reg [15:0] vector_activation_scale [0:BLOCKS_PER_ROW-1];
    reg [15:0] vector_weight_scale [0:BLOCKS_PER_ROW-1];
    reg [255:0] vector_activation_quants [0:BLOCKS_PER_ROW-1];
    reg [255:0] vector_weight_quants [0:BLOCKS_PER_ROW-1];
    reg signed [63:0] vector_term [0:BLOCKS_PER_ROW-1];
    reg signed [63:0] expected_accumulator;
    reg signed [63:0] expected_fp_q30;
    integer expected_fp_bound;
    integer ignored_row;
    integer ignored_block;
    integer ignored_first;
    integer ignored_last;
    reg [63:0] scanned_fp;
    integer scanned_bound;
    integer vector_count = 0;
    integer vector_index;
    integer failures = 0;
    integer file_descriptor;
    integer scan_result;
    integer wait_cycles;
    reg signed [63:0] fp_difference;
    reg [1023:0] vector_path;
    reg [1023:0] line;

    always #5 clk = ~clk;

    truega_q8_0_row_block_slot #(
        .ROW_DIAGNOSTIC_ENABLE(1)
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

    truega_q8_0_row_block_slot disabled_dut (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(start_i),
        .control_i(control_i),
        .activation_block_i(activation_block_i),
        .weight_block_i(weight_block_i),
        .busy_o(disabled_busy),
        .done_o(disabled_done),
        .error_o(disabled_error),
        .dot_o(disabled_dot),
        .term_q30_o(disabled_term),
        .row_q30_o(disabled_row)
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
                vector_term[vector_count], scanned_fp, scanned_bound);
            if (scan_result == 12) begin
                if (ignored_row != 0 || ignored_block != vector_count) begin
                    $display("FAIL expected row=0 block=%0d, got row=%0d block=%0d",
                        vector_count, ignored_row, ignored_block);
                    $finish_and_return(1);
                end
                expected_fp_q30 = $signed(scanned_fp);
                expected_fp_bound = scanned_bound;
                vector_count = vector_count + 1;
            end
        end
        $fclose(file_descriptor);
        if (vector_count != BLOCKS_PER_ROW) begin
            $display("FAIL loaded %0d row blocks, expected %0d",
                vector_count, BLOCKS_PER_ROW);
            $finish_and_return(1);
        end

        repeat (4) @(negedge clk);
        reset_n = 1'b1;

        // Compatibility call: first|last,index=0 behaves as the original one-block q8.
        activation_block_i = {vector_activation_quants[0], vector_activation_scale[0]};
        weight_block_i = {vector_weight_quants[0], vector_weight_scale[0]};
        control_i = 32'h0000_0003;
        @(negedge clk);
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;
        // Arguments belong to the accepted start edge and need not remain live.
        activation_block_i = {{32{8'h7f}}, 16'h3c00};
        weight_block_i = {{32{8'h80}}, 16'h3c00};
        wait_cycles = 0;
        while (!done_o && wait_cycles < 100) begin
            @(negedge clk);
            wait_cycles = wait_cycles + 1;
        end
        if (!done_o || error_o || dot_o !== vector_dot[0]
                || term_q30_o !== vector_term[0] || row_q30_o !== vector_term[0]) begin
            $display("FAIL compatibility call done=%b error=%b dot=%0d term=%h row=%h",
                done_o, error_o, dot_o, term_q30_o, row_q30_o);
            failures = failures + 1;
        end
        if (disabled_busy || disabled_done || disabled_error
                || disabled_dot != 0 || disabled_term != 0 || disabled_row != 0) begin
            $display("FAIL default-disabled row slot became active");
            failures = failures + 1;
        end

        // A malformed continuation must fail and leave the sequencer restartable.
        @(negedge clk);
        control_i = 32'h0000_0500;
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;
        if (!done_o || !error_o || busy_o) begin
            $display("FAIL malformed continuation did not retire as error");
            failures = failures + 1;
        end

        expected_accumulator = 64'sd0;
        for (vector_index = 0; vector_index < BLOCKS_PER_ROW; vector_index = vector_index + 1) begin
            @(negedge clk);
            activation_block_i = {
                vector_activation_quants[vector_index],
                vector_activation_scale[vector_index]
            };
            weight_block_i = {
                vector_weight_quants[vector_index],
                vector_weight_scale[vector_index]
            };
            control_i = (vector_index << 8)
                      | ((vector_index == 0) ? 1 : 0)
                      | ((vector_index == 31) ? 2 : 0);
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            if (!busy_o) begin
                $display("FAIL block=%0d busy did not assert", vector_index);
                failures = failures + 1;
            end

            wait_cycles = 0;
            while (!done_o && wait_cycles < 100) begin
                @(negedge clk);
                wait_cycles = wait_cycles + 1;
            end
            expected_accumulator = expected_accumulator + vector_term[vector_index];
            if (!done_o || error_o || busy_o) begin
                $display("FAIL block=%0d retire done=%b error=%b busy=%b",
                    vector_index, done_o, error_o, busy_o);
                failures = failures + 1;
            end
            if (dot_o !== vector_dot[vector_index]
                    || term_q30_o !== vector_term[vector_index]
                    || row_q30_o !== expected_accumulator) begin
                $display("FAIL block=%0d dot=%0d/%0d term=%h/%h row=%h/%h",
                    vector_index, dot_o, vector_dot[vector_index],
                    term_q30_o, vector_term[vector_index],
                    row_q30_o, expected_accumulator);
                failures = failures + 1;
            end
        end

        fp_difference = row_q30_o - expected_fp_q30;
        if (fp_difference < 0)
            fp_difference = -fp_difference;
        if (fp_difference > expected_fp_bound) begin
            $display("FAIL fp difference=%0d bound=%0d",
                fp_difference, expected_fp_bound);
            failures = failures + 1;
        end

        // The down projection uses 4,608 inputs: 144 native Q8_0 blocks.
        // Repeating the sealed 32-block fixture proves the wider sequence and
        // accumulator without inventing a second block encoding.
        expected_accumulator = 64'sd0;
        for (vector_index = 0; vector_index < 144; vector_index = vector_index + 1) begin
            @(negedge clk);
            activation_block_i = {
                vector_activation_quants[vector_index % BLOCKS_PER_ROW],
                vector_activation_scale[vector_index % BLOCKS_PER_ROW]
            };
            weight_block_i = {
                vector_weight_quants[vector_index % BLOCKS_PER_ROW],
                vector_weight_scale[vector_index % BLOCKS_PER_ROW]
            };
            control_i = (vector_index << 8)
                      | 4
                      | ((vector_index == 0) ? 1 : 0)
                      | ((vector_index == 143) ? 2 : 0);
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            wait_cycles = 0;
            while (!done_o && wait_cycles < 100) begin
                @(negedge clk);
                wait_cycles = wait_cycles + 1;
            end
            expected_accumulator = expected_accumulator
                                 + vector_term[vector_index % BLOCKS_PER_ROW];
            if (!done_o || error_o || row_q30_o !== expected_accumulator) begin
                $display("FAIL wide block=%0d row=%h/%h done=%b error=%b",
                    vector_index, row_q30_o, expected_accumulator, done_o, error_o);
                failures = failures + 1;
            end
        end
        @(negedge clk);
        if (done_o) begin
            $display("FAIL done was not a one-cycle pulse");
            failures = failures + 1;
        end

        if (failures == 0) begin
            $display("PASS q8_0_row_block_slot calls=32+144 exact_dot exact_term exact_row_q30 bounded_fp compatibility default_disabled");
            $finish;
        end
        $display("FAIL q8_0_row_block_slot failures=%0d", failures);
        $finish_and_return(1);
    end

    initial begin
        #200000;
        $display("FAIL q8_0_row_block_slot simulation timeout");
        $finish_and_return(1);
    end
endmodule
