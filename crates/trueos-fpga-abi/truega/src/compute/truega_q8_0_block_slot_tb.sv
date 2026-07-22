`timescale 1ns/1ps

module truega_q8_0_block_slot_tb;
    localparam MAX_VECTORS = 256;

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg start_i = 1'b0;
    reg [271:0] activation_block_i = 272'd0;
    reg [271:0] weight_block_i = 272'd0;
    wire busy_o;
    wire done_o;
    wire signed [31:0] dot_o;
    wire signed [63:0] term_q30_o;
    wire scale_error_o;

    integer vector_dot [0:MAX_VECTORS-1];
    reg [15:0] vector_activation_scale [0:MAX_VECTORS-1];
    reg [15:0] vector_weight_scale [0:MAX_VECTORS-1];
    reg [255:0] vector_activation_quants [0:MAX_VECTORS-1];
    reg [255:0] vector_weight_quants [0:MAX_VECTORS-1];
    reg [63:0] vector_term [0:MAX_VECTORS-1];
    integer ignored_row;
    integer ignored_block;
    integer ignored_first;
    integer ignored_last;
    reg [63:0] ignored_fp_expected;
    integer ignored_fp_bound;
    integer vector_count = 0;
    integer vector_index;
    integer failures = 0;
    integer file_descriptor;
    integer scan_result;
    integer wait_cycles;
    reg [1023:0] vector_path;
    reg [1023:0] line;

    always #5 clk = ~clk;

    truega_q8_0_block_slot dut (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(start_i),
        .activation_block_i(activation_block_i),
        .weight_block_i(weight_block_i),
        .busy_o(busy_o),
        .done_o(done_o),
        .dot_o(dot_o),
        .term_q30_o(term_q30_o),
        .scale_error_o(scale_error_o)
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
        while (!$feof(file_descriptor)) begin
            scan_result = $fscanf(file_descriptor,
                "%d %d %d %d %h %h %h %h %d %h %h %d\n",
                ignored_row, ignored_block, ignored_first, ignored_last,
                vector_activation_scale[vector_count],
                vector_weight_scale[vector_count],
                vector_activation_quants[vector_count],
                vector_weight_quants[vector_count],
                vector_dot[vector_count], vector_term[vector_count],
                ignored_fp_expected, ignored_fp_bound);
            if (scan_result == 12)
                vector_count = vector_count + 1;
        end
        $fclose(file_descriptor);
        if (vector_count != 210) begin
            $display("FAIL loaded %0d vectors, expected 210", vector_count);
            $finish_and_return(1);
        end

        repeat (4) @(negedge clk);
        reset_n = 1'b1;

        for (vector_index = 0; vector_index < vector_count; vector_index = vector_index + 1) begin
            @(negedge clk);
            activation_block_i = {
                vector_activation_quants[vector_index],
                vector_activation_scale[vector_index]
            };
            weight_block_i = {
                vector_weight_quants[vector_index],
                vector_weight_scale[vector_index]
            };
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            if (!busy_o) begin
                $display("FAIL vector=%0d busy did not assert", vector_index);
                failures = failures + 1;
            end

            // Overwrite the live input pins and pulse start during the first call.
            // The accepted native blocks must remain the only retired operation.
            if (vector_index == 0) begin
                activation_block_i = {{32{8'h7f}}, 16'h3c00};
                weight_block_i = {{32{8'h80}}, 16'h3c00};
                start_i = 1'b1;
                @(negedge clk);
                start_i = 1'b0;
            end

            wait_cycles = 0;
            while (!done_o && wait_cycles < 160) begin
                @(negedge clk);
                wait_cycles = wait_cycles + 1;
            end
            if (!done_o) begin
                $display("FAIL vector=%0d completion timeout", vector_index);
                failures = failures + 1;
            end else begin
                if (dot_o !== vector_dot[vector_index]) begin
                    $display("FAIL vector=%0d dot got=%0d expected=%0d",
                        vector_index, dot_o, vector_dot[vector_index]);
                    failures = failures + 1;
                end
                if (term_q30_o !== vector_term[vector_index]) begin
                    $display("FAIL vector=%0d term got=%h expected=%h",
                        vector_index, term_q30_o, vector_term[vector_index]);
                    failures = failures + 1;
                end
                if (scale_error_o) begin
                    $display("FAIL vector=%0d unexpected scale error", vector_index);
                    failures = failures + 1;
                end
                if (busy_o) begin
                    $display("FAIL vector=%0d busy remained asserted with done", vector_index);
                    failures = failures + 1;
                end
            end
            @(negedge clk);
            if (done_o) begin
                $display("FAIL vector=%0d done was not a one-cycle pulse", vector_index);
                failures = failures + 1;
            end
        end

        repeat (40) begin
            @(negedge clk);
            if (done_o) begin
                $display("FAIL ignored busy start produced an extra completion");
                failures = failures + 1;
            end
        end

        // Physical preflight regression: gate row 125 block 0 has three
        // negative 18-bit partial sums. Zero-extending them at the next tree
        // level produces 797237 instead of the exact signed dot 10805.
        activation_block_i = {
            256'h211da756d317082a81dab021e7cfd24a8925f6e7a8253cb3b616491f4ed4a80d,
            16'h1830
        };
        weight_block_i = {
            256'h113c6228c67f0cdc21664d1048c0ef099f76180b1cb54225c3e022ffc4e9c15b,
            16'h0b7f
        };
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;
        wait_cycles = 0;
        while (!done_o && wait_cycles < 160) begin
            @(negedge clk);
            wait_cycles = wait_cycles + 1;
        end
        if (!done_o || dot_o !== 32'sd10805 || term_q30_o !== 64'sd5426685
            || scale_error_o) begin
            $display("FAIL preflight row=125 block=0 dot=%0d term=%0d error=%0d",
                dot_o, term_q30_o, scale_error_o);
            failures = failures + 1;
        end

        if (failures == 0) begin
            $display("PASS q8_0_block_slot blocks=%0d preflight_row125 exact_dot exact_q30 ignored_busy_start",
                vector_count);
            $finish;
        end
        $display("FAIL q8_0_block_slot failures=%0d", failures);
        $finish_and_return(1);
    end

    initial begin
        #500000;
        $display("FAIL q8_0_block_slot simulation timeout");
        $finish_and_return(1);
    end
endmodule
