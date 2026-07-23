`timescale 1ns/1ps

module truega_q8_0_gemv_tb;
    localparam MAX_VECTORS = 256;

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg valid_i = 1'b0;
    reg row_first_i = 1'b0;
    reg row_last_i = 1'b0;
    reg [15:0] activation_scale_f16_i = 16'd0;
    reg [15:0] weight_scale_f16_i = 16'd0;
    reg [255:0] activation_quants_i = 256'd0;
    reg [255:0] weight_quants_i = 256'd0;
    wire ready_o;
    wire block_valid_o;
    wire signed [20:0] block_dot_o;
    wire signed [63:0] block_term_q30_o;
    wire row_valid_o;
    wire signed [63:0] row_q30_o;
    wire scale_error_o;

    integer vector_row [0:MAX_VECTORS-1];
    integer vector_block [0:MAX_VECTORS-1];
    integer vector_first [0:MAX_VECTORS-1];
    integer vector_last [0:MAX_VECTORS-1];
    integer vector_dot [0:MAX_VECTORS-1];
    reg [15:0] vector_activation_scale [0:MAX_VECTORS-1];
    reg [15:0] vector_weight_scale [0:MAX_VECTORS-1];
    reg [255:0] vector_activation_quants [0:MAX_VECTORS-1];
    reg [255:0] vector_weight_quants [0:MAX_VECTORS-1];
    reg [63:0] vector_term [0:MAX_VECTORS-1];
    reg [63:0] vector_row_result [0:MAX_VECTORS-1];
    reg [63:0] vector_fp_expected [0:MAX_VECTORS-1];
    integer vector_fp_bound [0:MAX_VECTORS-1];
    reg signed [63:0] load_accumulator;
    reg signed [63:0] fp_difference;
    integer vector_count = 0;
    integer output_index = 0;
    integer failures = 0;
    integer file_descriptor;
    integer scan_result;
    integer drive_index;
    reg [1023:0] vector_path;
    reg [1023:0] line;

    always #5 clk = ~clk;

    truega_q8_0_gemv dut (
        .clk(clk),
        .reset_n(reset_n),
        .valid_i(valid_i),
        .ready_o(ready_o),
        .row_first_i(row_first_i),
        .row_last_i(row_last_i),
        .activation_scale_f16_i(activation_scale_f16_i),
        .weight_scale_f16_i(weight_scale_f16_i),
        .activation_quants_i(activation_quants_i),
        .weight_quants_i(weight_quants_i),
        .block_valid_o(block_valid_o),
        .block_dot_o(block_dot_o),
        .block_term_q30_o(block_term_q30_o),
        .row_valid_o(row_valid_o),
        .row_q30_o(row_q30_o),
        .scale_error_o(scale_error_o)
    );

    always @(negedge clk) begin
        if (reset_n && block_valid_o) begin
            if (block_dot_o !== vector_dot[output_index]) begin
                $display("FAIL dot row=%0d block=%0d got=%0d expected=%0d",
                    vector_row[output_index], vector_block[output_index],
                    block_dot_o, vector_dot[output_index]);
                failures = failures + 1;
            end
            if (block_term_q30_o !== vector_term[output_index]) begin
                $display("FAIL term row=%0d block=%0d got=%h expected=%h",
                    vector_row[output_index], vector_block[output_index],
                    block_term_q30_o, vector_term[output_index]);
                failures = failures + 1;
            end
            if (vector_last[output_index]) begin
                if (!row_valid_o || row_q30_o !== vector_row_result[output_index]) begin
                    $display("FAIL row=%0d got_valid=%0d got=%h expected=%h",
                        vector_row[output_index], row_valid_o,
                        row_q30_o, vector_row_result[output_index]);
                    failures = failures + 1;
                end
                fp_difference = $signed(row_q30_o) - $signed(vector_fp_expected[output_index]);
                if (fp_difference < 0)
                    fp_difference = -fp_difference;
                if (fp_difference > vector_fp_bound[output_index]) begin
                    $display("FAIL fp-bound row=%0d difference_q30=%0d bound_q30=%0d",
                        vector_row[output_index], fp_difference,
                        vector_fp_bound[output_index]);
                    failures = failures + 1;
                end
            end else if (row_valid_o) begin
                $display("FAIL unexpected row_valid row=%0d block=%0d",
                    vector_row[output_index], vector_block[output_index]);
                failures = failures + 1;
            end
            output_index = output_index + 1;
        end
    end

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
        load_accumulator = 64'sd0;
        while (!$feof(file_descriptor)) begin
            scan_result = $fscanf(file_descriptor,
                "%d %d %d %d %h %h %h %h %d %h %h %d\n",
                vector_row[vector_count], vector_block[vector_count],
                vector_first[vector_count], vector_last[vector_count],
                vector_activation_scale[vector_count], vector_weight_scale[vector_count],
                vector_activation_quants[vector_count], vector_weight_quants[vector_count],
                vector_dot[vector_count], vector_term[vector_count],
                vector_fp_expected[vector_count], vector_fp_bound[vector_count]);
            if (scan_result == 12) begin
                if (vector_first[vector_count])
                    load_accumulator = $signed(vector_term[vector_count]);
                else
                    load_accumulator = load_accumulator + $signed(vector_term[vector_count]);
                if (vector_last[vector_count])
                    vector_row_result[vector_count] = load_accumulator;
                vector_count = vector_count + 1;
            end
        end
        $fclose(file_descriptor);
        if (vector_count != 210) begin
            $display("FAIL loaded %0d vectors, expected 210", vector_count);
            $finish_and_return(1);
        end

        repeat (4) @(negedge clk);
        reset_n = 1'b1;
        for (drive_index = 0; drive_index < vector_count; drive_index = drive_index + 1) begin
            while (!ready_o)
                @(negedge clk);
            @(negedge clk);
            valid_i = 1'b1;
            row_first_i = vector_first[drive_index];
            row_last_i = vector_last[drive_index];
            activation_scale_f16_i = vector_activation_scale[drive_index];
            weight_scale_f16_i = vector_weight_scale[drive_index];
            activation_quants_i = vector_activation_quants[drive_index];
            weight_quants_i = vector_weight_quants[drive_index];
            // The dot and exact sequential scale converter share one explicit
            // busy interval. Present one accepted pulse, then wait for the
            // scaled term to retire before advancing the fixture index.
            @(negedge clk);
            valid_i = 1'b0;
            while (!block_valid_o)
                @(negedge clk);
        end
        @(negedge clk);
        valid_i = 1'b0;
        row_first_i = 1'b0;
        row_last_i = 1'b0;

        repeat (16) @(negedge clk);
        if (output_index != vector_count) begin
            $display("FAIL observed %0d of %0d outputs", output_index, vector_count);
            failures = failures + 1;
        end
        if (scale_error_o) begin
            $display("FAIL scale_error_o asserted");
            failures = failures + 1;
        end
        if (failures == 0) begin
            $display("PASS q8_0_gemv blocks=%0d rows=5 exact_integer_dot exact_q30 bounded_fp", vector_count);
            $finish;
        end
        $display("FAIL q8_0_gemv failures=%0d", failures);
        $finish_and_return(1);
    end

    initial begin
        #200000;
        $display("FAIL simulation timeout");
        $finish_and_return(1);
    end
endmodule
