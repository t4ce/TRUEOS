`timescale 1ns/1ps

module truega_lfm25_silu_q30_slot_tb;
    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg start_i = 1'b0;
    reg signed [63:0] gate_q30_i = 64'sd0;
    reg signed [63:0] up_q30_i = 64'sd0;
    wire busy_o;
    wire done_o;
    wire error_o;
    wire signed [63:0] result_q30_o;
    integer failures = 0;
    integer cycles;

    always #5 clk = ~clk;

    truega_lfm25_silu_q30_slot #(.SILU_ENABLE(1)) dut (
        .clk(clk), .reset_n(reset_n), .start_i(start_i),
        .gate_q30_i(gate_q30_i), .up_q30_i(up_q30_i),
        .busy_o(busy_o), .done_o(done_o), .error_o(error_o),
        .result_q30_o(result_q30_o)
    );

    task run_case;
        input signed [63:0] gate;
        input signed [63:0] up;
        input signed [63:0] expected;
        begin
            @(negedge clk);
            gate_q30_i = gate;
            up_q30_i = up;
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            cycles = 0;
            while (!done_o && cycles < 20) begin
                @(negedge clk);
                cycles = cycles + 1;
            end
            if (!done_o || error_o || result_q30_o !== expected) begin
                $display("FAIL gate=%0d up=%0d result=%0d expected=%0d done=%b error=%b",
                    gate, up, result_q30_o, expected, done_o, error_o);
                failures = failures + 1;
            end
        end
    endtask

    initial begin
        repeat (4) @(negedge clk);
        reset_n = 1'b1;
        run_case(64'sd0, 64'sd1073741824, 64'sd0);
        run_case(64'sd29481209, -64'sd10250472, -64'sd142653);

        @(negedge clk);
        gate_q30_i = 64'sd1207959553;
        up_q30_i = 64'sd0;
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;
        if (!done_o || !error_o || busy_o) begin
            $display("FAIL out-of-range input was not rejected");
            failures = failures + 1;
        end

        if (failures == 0) begin
            $display("PASS lfm25_silu_q30 exact_fixed_pipeline range_guard");
            $finish;
        end
        $display("FAIL lfm25_silu_q30 failures=%0d", failures);
        $finish_and_return(1);
    end
endmodule
