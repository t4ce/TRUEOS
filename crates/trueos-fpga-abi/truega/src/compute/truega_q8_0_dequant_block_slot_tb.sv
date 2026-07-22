`timescale 1ns/1ps

module truega_q8_0_dequant_block_slot_tb;
    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg start = 1'b0;
    reg [271:0] block = 272'd0;
    wire output_valid;
    reg output_ready = 1'b0;
    wire [4:0] output_index;
    wire output_last;
    wire signed [63:0] output_q30;
    wire busy;
    wire done;
    wire error;
    wire [5:0] samples;
    integer failures = 0;
    integer index;
    reg signed [7:0] quant;
    reg signed [63:0] expected;
    reg signed [63:0] held_data;
    reg [4:0] held_index;
    reg held_last;

    always #5 clk = ~clk;

    truega_q8_0_dequant_block_slot dut (
        .clk(clk), .reset_n(reset_n), .start_i(start), .q8_block_i(block),
        .output_valid_o(output_valid), .output_ready_i(output_ready),
        .output_index_o(output_index), .output_last_o(output_last),
        .output_q30_o(output_q30), .busy_o(busy), .done_o(done),
        .error_o(error), .samples_retired_o(samples)
    );

    task automatic pulse_start;
        begin
            @(negedge clk);
            start = 1'b1;
            @(negedge clk);
            start = 1'b0;
        end
    endtask

    task automatic retire_block;
        input [15:0] scale;
        input subnormal_case;
        begin
            block[15:0] = scale;
            for (index = 0; index < 32; index = index + 1) begin
                quant = subnormal_case ? 8'sd127 : $signed(index[7:0]) - 8'sd16;
                block[16 + index * 8 +: 8] = quant;
            end
            pulse_start();
            for (index = 0; index < 32; index = index + 1) begin
                while (!output_valid)
                    @(negedge clk);
                quant = block[16 + index * 8 +: 8];
                expected = subnormal_case
                    ? 64'sd8128
                    : $signed(quant) * 64'sd536870912;
                if (output_index !== index[4:0] || output_q30 !== expected
                        || output_last !== (index == 31))
                    failures = failures + 1;
                if (index == 7) begin
                    held_data = output_q30;
                    held_index = output_index;
                    held_last = output_last;
                    repeat (4) begin
                        @(negedge clk);
                        if (!output_valid || output_q30 !== held_data
                                || output_index !== held_index
                                || output_last !== held_last)
                            failures = failures + 1;
                    end
                end
                output_ready = 1'b1;
                @(negedge clk);
                output_ready = 1'b0;
            end
            while (!done)
                @(negedge clk);
            if (error || busy || samples != 6'd32)
                failures = failures + 1;
        end
    endtask

    initial begin
        repeat (4) @(negedge clk);
        reset_n = 1'b1;
        repeat (2) @(negedge clk);

        retire_block(16'h3800, 1'b0); // signed lanes * exact 0.5
        retire_block(16'h0001, 1'b1); // 127 * minimum F16 subnormal

        block = 272'd0;
        block[15:0] = 16'hbc00; // negative Q8 scale is invalid
        block[23:16] = 8'h01;
        pulse_start();
        while (!done)
            @(negedge clk);
        if (!error || busy || samples != 6'd0 || output_valid)
            failures = failures + 1;

        if (failures == 0)
            $display("PASS q8_0_dequant_block exact_normal+subnormal signed_lanes stable_backpressure invalid_scale");
        else begin
            $display("FAIL q8_0_dequant_block failures=%0d", failures);
            $fatal(1);
        end
        $finish;
    end

    initial begin
        #2000000;
        $display("FAIL q8_0_dequant_block timeout");
        $fatal(1);
    end
endmodule
