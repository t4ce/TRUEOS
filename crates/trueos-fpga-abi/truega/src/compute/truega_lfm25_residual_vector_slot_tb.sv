`timescale 1ns/1ps

module truega_lfm25_residual_vector_slot_tb;
    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg start_i = 1'b0;
    reg input_valid = 1'b0;
    wire input_ready;
    reg signed [63:0] residual_q30 = 64'sd0;
    reg signed [63:0] branch_q30 = 64'sd0;
    wire output_valid;
    reg output_ready = 1'b0;
    wire [9:0] output_index;
    wire signed [63:0] output_q30;
    wire busy_o;
    wire done_o;
    wire error_o;
    wire [10:0] elements_retired;
    integer sent;
    integer received;
    integer cycles;
    integer failures = 0;
    reg stalled;
    reg [9:0] stalled_index;
    reg signed [63:0] stalled_value;

    always #5 clk = ~clk;

    truega_lfm25_residual_vector_slot dut (
        .clk(clk), .reset_n(reset_n), .start_i(start_i),
        .input_valid_i(input_valid), .input_ready_o(input_ready),
        .residual_q30_i(residual_q30), .branch_q30_i(branch_q30),
        .output_valid_o(output_valid), .output_ready_i(output_ready),
        .output_index_o(output_index), .output_q30_o(output_q30),
        .busy_o(busy_o), .done_o(done_o), .error_o(error_o),
        .elements_retired_o(elements_retired)
    );

    initial begin
        repeat (4) @(negedge clk);
        reset_n = 1'b1;
        @(negedge clk);
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;

        sent = 0;
        received = 0;
        cycles = 0;
        stalled = 1'b0;
        while (!done_o && cycles < 10000) begin
            @(negedge clk);
            input_valid = input_ready && sent < 1024 && ((cycles % 4) != 1);
            residual_q30 = sent * 64'sd1000000;
            branch_q30 = -(sent * 64'sd300000);
            if (input_valid)
                sent = sent + 1;

            output_ready = (cycles % 6) != 2;
            if (output_valid) begin
                if (stalled && (output_index !== stalled_index
                    || output_q30 !== stalled_value)) begin
                    $display("FAIL residual changed under stall index=%0d value=%0d",
                        output_index, output_q30);
                    failures = failures + 1;
                end
                if (output_ready) begin
                    if (output_index !== received[9:0]
                        || output_q30 !== received * 64'sd700000) begin
                        $display("FAIL residual index=%0d value=%0d expected_index=%0d expected=%0d",
                            output_index, output_q30, received,
                            received * 64'sd700000);
                        failures = failures + 1;
                    end
                    received = received + 1;
                    stalled = 1'b0;
                end else begin
                    stalled = 1'b1;
                    stalled_index = output_index;
                    stalled_value = output_q30;
                end
            end
            cycles = cycles + 1;
        end
        input_valid = 1'b0;
        output_ready = 1'b0;

        if (!done_o || error_o || sent != 1024 || received != 1024
            || elements_retired != 1024) begin
            $display("FAIL residual done=%b error=%b sent=%0d received=%0d retired=%0d cycles=%0d",
                done_o, error_o, sent, received, elements_retired, cycles);
            failures = failures + 1;
        end

        // Positive signed overflow must retire as an error without output.
        @(negedge clk);
        @(negedge clk);
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;
        input_valid = 1'b1;
        residual_q30 = 64'sh7fffffffffffffff;
        branch_q30 = 64'sd1;
        @(negedge clk);
        input_valid = 1'b0;
        if (!done_o || !error_o || busy_o || output_valid) begin
            $display("FAIL residual overflow done=%b error=%b busy=%b valid=%b",
                done_o, error_o, busy_o, output_valid);
            failures = failures + 1;
        end

        if (failures == 0) begin
            $display("PASS lfm25_residual_vector elements=1024 input_stalls output_stalls stable_backpressure exact_q30 overflow_guard");
            $finish;
        end
        $display("FAIL lfm25_residual_vector failures=%0d", failures);
        $finish_and_return(1);
    end
endmodule
