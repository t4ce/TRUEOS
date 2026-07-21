`timescale 1ns/1ps

// Proves the fixed feeder boundary using the 32 sealed native blocks for
// layer-0 gate row 0.  No synthetic ROM is inferred and no heartbeat/top-level
// source is instantiated or modified.
module truega_lfm25_gate_row_slot_tb;
    localparam BLOCKS_PER_ROW = 32;

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg start_i = 1'b0;
    reg feeder_valid_i = 1'b0;
    reg [271:0] feeder_activation_block_i = 272'd0;
    reg [271:0] feeder_weight_block_i = 272'd0;
    wire feeder_ready_o;
    wire [4:0] feeder_block_index_o;
    wire busy_o;
    wire done_o;
    wire error_o;
    wire [5:0] blocks_accepted_o;
    wire signed [63:0] row_q30_o;

    wire disabled_feeder_ready;
    wire disabled_busy;
    wire disabled_done;
    wire disabled_error;
    wire [4:0] disabled_block_index;
    wire [5:0] disabled_blocks_accepted;
    wire signed [63:0] disabled_row_q30;

    reg [15:0] vector_activation_scale [0:BLOCKS_PER_ROW-1];
    reg [15:0] vector_weight_scale [0:BLOCKS_PER_ROW-1];
    reg [255:0] vector_activation_quants [0:BLOCKS_PER_ROW-1];
    reg [255:0] vector_weight_quants [0:BLOCKS_PER_ROW-1];
    reg signed [63:0] expected_row_q30;
    reg signed [63:0] expected_fp_q30;
    integer expected_fp_bound;
    integer ignored_row;
    integer ignored_block;
    integer ignored_first;
    integer ignored_last;
    integer ignored_dot;
    reg [63:0] scanned_term;
    reg [63:0] scanned_fp;
    integer scanned_bound;
    integer vector_count = 0;
    integer call_index;
    integer drive_index;
    integer failures = 0;
    integer file_descriptor;
    integer scan_result;
    integer wait_cycles;
    reg signed [63:0] fp_difference;
    reg [1023:0] vector_path;
    reg [1023:0] line;

    always #5 clk = ~clk;

    truega_lfm25_gate_row_slot #(
        .DIAGNOSTIC_ENABLE(1)
    ) dut (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(start_i),
        .feeder_ready_o(feeder_ready_o),
        .feeder_block_index_o(feeder_block_index_o),
        .feeder_valid_i(feeder_valid_i),
        .feeder_activation_block_i(feeder_activation_block_i),
        .feeder_weight_block_i(feeder_weight_block_i),
        .busy_o(busy_o),
        .done_o(done_o),
        .error_o(error_o),
        .blocks_accepted_o(blocks_accepted_o),
        .row_q30_o(row_q30_o)
    );

    // The default instance proves that merely adding the module to a project
    // cannot start it or request model data.
    truega_lfm25_gate_row_slot disabled_dut (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(start_i),
        .feeder_ready_o(disabled_feeder_ready),
        .feeder_block_index_o(disabled_block_index),
        .feeder_valid_i(feeder_valid_i),
        .feeder_activation_block_i(feeder_activation_block_i),
        .feeder_weight_block_i(feeder_weight_block_i),
        .busy_o(disabled_busy),
        .done_o(disabled_done),
        .error_o(disabled_error),
        .blocks_accepted_o(disabled_blocks_accepted),
        .row_q30_o(disabled_row_q30)
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
        expected_row_q30 = 64'sd0;
        while (!$feof(file_descriptor) && vector_count < BLOCKS_PER_ROW) begin
            scan_result = $fscanf(file_descriptor,
                "%d %d %d %d %h %h %h %h %d %h %h %d\n",
                ignored_row, ignored_block, ignored_first, ignored_last,
                vector_activation_scale[vector_count],
                vector_weight_scale[vector_count],
                vector_activation_quants[vector_count],
                vector_weight_quants[vector_count], ignored_dot, scanned_term,
                scanned_fp, scanned_bound);
            if (scan_result == 12) begin
                if (ignored_row != 0 || ignored_block != vector_count) begin
                    $display("FAIL expected gate row=0 block=%0d, got row=%0d block=%0d",
                        vector_count, ignored_row, ignored_block);
                    $finish_and_return(1);
                end
                expected_row_q30 = expected_row_q30 + $signed(scanned_term);
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

        for (call_index = 0; call_index < 2; call_index = call_index + 1) begin
            @(negedge clk);
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            if (!busy_o || !feeder_ready_o || feeder_block_index_o !== 5'd0) begin
                $display("FAIL call=%0d initial contract busy=%b ready=%b index=%0d",
                    call_index, busy_o, feeder_ready_o, feeder_block_index_o);
                failures = failures + 1;
            end
            if (disabled_busy || disabled_feeder_ready || disabled_done
                    || disabled_error || disabled_blocks_accepted != 0
                    || disabled_row_q30 != 0) begin
                $display("FAIL default-disabled slot became active");
                failures = failures + 1;
            end

            for (drive_index = 0; drive_index < BLOCKS_PER_ROW; drive_index = drive_index + 1) begin
                // Deterministic gaps prove that the future DDR reader may stall.
                if ((drive_index % 5) == 2)
                    @(negedge clk);
                feeder_activation_block_i = {
                    vector_activation_quants[drive_index],
                    vector_activation_scale[drive_index]
                };
                feeder_weight_block_i = {
                    vector_weight_quants[drive_index],
                    vector_weight_scale[drive_index]
                };
                feeder_valid_i = 1'b1;
                while (!feeder_ready_o)
                    @(negedge clk);
                if (feeder_block_index_o !== drive_index[4:0]) begin
                    $display("FAIL call=%0d expected feeder index=%0d got=%0d",
                        call_index, drive_index, feeder_block_index_o);
                    failures = failures + 1;
                end
                @(negedge clk);
                feeder_valid_i = 1'b0;

                // A busy start is intentionally ignored and cannot restart row state.
                if (drive_index == 8) begin
                    start_i = 1'b1;
                    @(negedge clk);
                    start_i = 1'b0;
                end
            end

            wait_cycles = 0;
            while (!done_o && wait_cycles < 100) begin
                @(negedge clk);
                wait_cycles = wait_cycles + 1;
            end
            if (!done_o) begin
                $display("FAIL call=%0d completion timeout", call_index);
                failures = failures + 1;
            end else begin
                if (busy_o || feeder_ready_o || error_o || blocks_accepted_o != 6'd32) begin
                    $display("FAIL call=%0d retire busy=%b ready=%b error=%b blocks=%0d",
                        call_index, busy_o, feeder_ready_o, error_o, blocks_accepted_o);
                    failures = failures + 1;
                end
                if (row_q30_o !== expected_row_q30) begin
                    $display("FAIL call=%0d row got=%h expected=%h",
                        call_index, row_q30_o, expected_row_q30);
                    failures = failures + 1;
                end
                fp_difference = row_q30_o - expected_fp_q30;
                if (fp_difference < 0)
                    fp_difference = -fp_difference;
                if (fp_difference > expected_fp_bound) begin
                    $display("FAIL call=%0d fp difference=%0d bound=%0d",
                        call_index, fp_difference, expected_fp_bound);
                    failures = failures + 1;
                end
            end
            @(negedge clk);
            if (done_o) begin
                $display("FAIL call=%0d done was not a one-cycle pulse", call_index);
                failures = failures + 1;
            end
        end

        if (failures == 0) begin
            $display("PASS lfm25_gate_row_slot calls=2 blocks_per_row=32 exact_q30 bounded_fp default_disabled");
            $finish;
        end
        $display("FAIL lfm25_gate_row_slot failures=%0d", failures);
        $finish_and_return(1);
    end

    initial begin
        #100000;
        $display("FAIL lfm25_gate_row_slot simulation timeout");
        $finish_and_return(1);
    end
endmodule
