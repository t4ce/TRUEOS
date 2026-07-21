`timescale 1ns/1ps

module truega_q8_0_scale_q30_seq_tb;
    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg start_i = 1'b0;
    reg signed [20:0] dot_i = 21'sd0;
    reg [15:0] activation_scale_f16_i = 16'd0;
    reg [15:0] weight_scale_f16_i = 16'd0;
    wire busy_o;
    wire done_o;
    wire signed [63:0] term_q30_o;
    wire scale_error_o;
    integer failures = 0;
    integer completion_cycles;
    integer extra_cycles;

    always #5 clk = ~clk;

    truega_q8_0_scale_q30_seq dut (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(start_i),
        .dot_i(dot_i),
        .activation_scale_f16_i(activation_scale_f16_i),
        .weight_scale_f16_i(weight_scale_f16_i),
        .busy_o(busy_o),
        .done_o(done_o),
        .term_q30_o(term_q30_o),
        .scale_error_o(scale_error_o)
    );

    task automatic run_case;
        input signed [20:0] test_dot;
        input [15:0] test_activation_scale;
        input [15:0] test_weight_scale;
        input signed [63:0] expected_term;
        input expected_error;
        input [255:0] label;
        integer cycles;
        begin
            while (busy_o)
                @(negedge clk);
            @(negedge clk);
            dot_i = test_dot;
            activation_scale_f16_i = test_activation_scale;
            weight_scale_f16_i = test_weight_scale;
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            if (!busy_o) begin
                $display("FAIL %0s busy did not assert", label);
                failures = failures + 1;
            end
            cycles = 0;
            while (!done_o && cycles < 80) begin
                @(negedge clk);
                cycles = cycles + 1;
            end
            if (!done_o) begin
                $display("FAIL %0s timeout", label);
                failures = failures + 1;
            end else begin
                if (term_q30_o !== expected_term) begin
                    $display("FAIL %0s term got=%h expected=%h",
                        label, term_q30_o, expected_term);
                    failures = failures + 1;
                end
                if (scale_error_o !== expected_error) begin
                    $display("FAIL %0s error got=%0d expected=%0d",
                        label, scale_error_o, expected_error);
                    failures = failures + 1;
                end
                if (busy_o) begin
                    $display("FAIL %0s busy remained asserted with done", label);
                    failures = failures + 1;
                end
            end
            @(negedge clk);
            if (done_o) begin
                $display("FAIL %0s done was not a one-cycle pulse", label);
                failures = failures + 1;
            end
        end
    endtask

    initial begin
        repeat (4) @(negedge clk);
        reset_n = 1'b1;

        run_case(21'sd17, 16'h3c00, 16'h3c00,
            64'sd18253611008, 1'b0, "one-scales");
        run_case(21'sd131072, 16'h0001, 16'h0001,
            64'sd0, 1'b0, "positive-half-even-zero");
        run_case(21'sd393216, 16'h0001, 16'h0001,
            64'sd2, 1'b0, "positive-half-odd-up");
        run_case(-21'sd131072, 16'h0001, 16'h0001,
            64'sd0, 1'b0, "negative-half-even-zero");
        run_case(-21'sd393216, 16'h0001, 16'h0001,
            -64'sd2, 1'b0, "negative-half-odd-away");
        run_case(21'sd131073, 16'h0001, 16'h0001,
            64'sd1, 1'b0, "above-half");
        run_case(21'sd131071, 16'h0001, 16'h0001,
            64'sd0, 1'b0, "below-half");
        run_case(21'sd1, 16'h5000, 16'h5000,
            64'sd1099511627776, 1'b0, "maximum-supported-left-shift");
        run_case(21'sd1, 16'h5400, 16'h5000,
            64'sd0, 1'b1, "left-shift-overflow-policy");
        run_case(21'sd9, 16'h0000, 16'h3c00,
            64'sd0, 1'b0, "zero-scale");
        run_case(21'sd9, 16'hbc00, 16'h3c00,
            64'sd0, 1'b1, "negative-scale");
        run_case(21'sd9, 16'h7c00, 16'h3c00,
            64'sd0, 1'b1, "non-finite-scale");

        // A second start pulse while busy must not replace the active operands.
        @(negedge clk);
        dot_i = 21'sd393216;
        activation_scale_f16_i = 16'h0001;
        weight_scale_f16_i = 16'h0001;
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;
        repeat (3) @(negedge clk);
        dot_i = 21'sd17;
        activation_scale_f16_i = 16'h3c00;
        weight_scale_f16_i = 16'h3c00;
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;
        completion_cycles = 0;
        while (!done_o && completion_cycles < 80) begin
            @(negedge clk);
            completion_cycles = completion_cycles + 1;
        end
        if (!done_o || term_q30_o !== 64'sd2 || scale_error_o) begin
            $display("FAIL ignored-start result done=%0d term=%h error=%0d",
                done_o, term_q30_o, scale_error_o);
            failures = failures + 1;
        end
        @(negedge clk);
        for (extra_cycles = 0; extra_cycles < 32; extra_cycles = extra_cycles + 1) begin
            @(negedge clk);
            if (done_o) begin
                $display("FAIL ignored start produced a second completion");
                failures = failures + 1;
            end
        end

        if (failures == 0) begin
            $display("PASS q8_0_scale_q30_seq focused_rne_errors_busy");
            $finish;
        end
        $display("FAIL q8_0_scale_q30_seq failures=%0d", failures);
        $finish_and_return(1);
    end

    initial begin
        #100000;
        $display("FAIL q8_0_scale_q30_seq timeout");
        $finish_and_return(1);
    end
endmodule
