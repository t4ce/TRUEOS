`timescale 1ns/1ps

module truega_q30_to_q8_0_block_slot_tb;
    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg start_i = 1'b0;
    reg sample_valid_i = 1'b0;
    reg signed [63:0] sample_q30_i = 64'sd0;
    wire sample_ready_o;
    wire busy_o;
    wire done_o;
    wire error_o;
    wire [5:0] samples_accepted_o;
    wire [271:0] q8_block_o;

    integer vectors_fd;
    integer scan_result;
    integer case_count;
    integer case_index;
    integer case_id;
    integer sample_index;
    integer cycles;
    integer failures = 0;
    reg [15:0] expected_scale;
    reg [7:0] expected_quants [0:31];
    reg [63:0] samples [0:31];
    reg [1023:0] vectors_path;

    truega_q30_to_q8_0_block_slot dut (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(start_i),
        .sample_valid_i(sample_valid_i),
        .sample_q30_i(sample_q30_i),
        .sample_ready_o(sample_ready_o),
        .busy_o(busy_o),
        .done_o(done_o),
        .error_o(error_o),
        .samples_accepted_o(samples_accepted_o),
        .q8_block_o(q8_block_o)
    );

    always #5 clk = ~clk;

    task automatic read_case;
        begin
            scan_result = $fscanf(vectors_fd, "%d %h", case_id, expected_scale);
            if (scan_result != 2) begin
                $display("FAIL q30_to_q8 unable to read case header index=%0d", case_index);
                $fatal(1);
            end
            for (sample_index = 0; sample_index < 32; sample_index = sample_index + 1) begin
                scan_result = $fscanf(vectors_fd, "%h", expected_quants[sample_index]);
                if (scan_result != 1) begin
                    $display("FAIL q30_to_q8 unable to read quant case=%0d sample=%0d",
                             case_id, sample_index);
                    $fatal(1);
                end
            end
            for (sample_index = 0; sample_index < 32; sample_index = sample_index + 1) begin
                scan_result = $fscanf(vectors_fd, "%h", samples[sample_index]);
                if (scan_result != 1) begin
                    $display("FAIL q30_to_q8 unable to read sample case=%0d sample=%0d",
                             case_id, sample_index);
                    $fatal(1);
                end
            end
        end
    endtask

    task automatic run_case;
        begin
            @(negedge clk);
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;

            for (sample_index = 0; sample_index < 32; sample_index = sample_index + 1) begin
                cycles = 0;
                while (!sample_ready_o && cycles < 16) begin
                    @(negedge clk);
                    cycles = cycles + 1;
                end
                if (!sample_ready_o) begin
                    $display("FAIL q30_to_q8 ready timeout case=%0d sample=%0d",
                             case_id, sample_index);
                    $fatal(1);
                end
                sample_q30_i = samples[sample_index];
                sample_valid_i = 1'b1;
                @(negedge clk);
                sample_valid_i = 1'b0;
            end

            cycles = 0;
            while (!done_o && cycles < 4096) begin
                @(negedge clk);
                cycles = cycles + 1;
            end
            if (!done_o) begin
                $display("FAIL q30_to_q8 completion timeout case=%0d accepted=%0d",
                         case_id, samples_accepted_o);
                $fatal(1);
            end
            if (error_o || busy_o || samples_accepted_o != 6'd32) begin
                $display("FAIL q30_to_q8 status case=%0d error=%b busy=%b accepted=%0d",
                         case_id, error_o, busy_o, samples_accepted_o);
                failures = failures + 1;
            end
            if (q8_block_o[15:0] !== expected_scale) begin
                $display("FAIL q30_to_q8 scale case=%0d got=%04x expected=%04x",
                         case_id, q8_block_o[15:0], expected_scale);
                failures = failures + 1;
            end
            for (sample_index = 0; sample_index < 32; sample_index = sample_index + 1) begin
                if (q8_block_o[16 + sample_index * 8 +: 8] !== expected_quants[sample_index]) begin
                    $display("FAIL q30_to_q8 quant case=%0d sample=%0d got=%02x expected=%02x input=%016x",
                             case_id, sample_index,
                             q8_block_o[16 + sample_index * 8 +: 8],
                             expected_quants[sample_index], samples[sample_index]);
                    failures = failures + 1;
                end
            end
        end
    endtask

    initial begin
        if (!$value$plusargs("VECTORS=%s", vectors_path)) begin
            $display("FAIL q30_to_q8 missing +VECTORS=path");
            $fatal(1);
        end
        vectors_fd = $fopen(vectors_path, "r");
        if (vectors_fd == 0) begin
            $display("FAIL q30_to_q8 cannot open vectors=%s", vectors_path);
            $fatal(1);
        end
        scan_result = $fscanf(vectors_fd, "%d", case_count);
        if (scan_result != 1 || case_count < 10) begin
            $display("FAIL q30_to_q8 invalid case count=%0d", case_count);
            $fatal(1);
        end

        repeat (4) @(negedge clk);
        reset_n = 1'b1;
        repeat (2) @(negedge clk);

        for (case_index = 0; case_index < case_count; case_index = case_index + 1) begin
            read_case();
            run_case();
        end
        $fclose(vectors_fd);

        if (failures == 0) begin
            $display("PASS q30_to_q8 cases=%0d exact_rust_ggml_contract real_trace_blocks=4",
                     case_count);
            $finish;
        end
        $display("FAIL q30_to_q8 failures=%0d cases=%0d", failures, case_count);
        $fatal(1);
    end

    initial begin
        #1000000;
        $display("FAIL q30_to_q8 global timeout");
        $fatal(1);
    end
endmodule
