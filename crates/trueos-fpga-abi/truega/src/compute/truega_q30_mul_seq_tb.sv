`timescale 1ns/1ps

module truega_q30_mul_seq_tb;
    localparam signed [63:0] Q30_ONE = 64'sd1073741824;
    localparam integer EXPECTED_LATENCY = 66;

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg start_i = 1'b0;
    reg signed [63:0] left_q30_i = 64'sd0;
    reg signed [63:0] right_q30_i = 64'sd0;
    wire busy_o;
    wire done_o;
    wire overflow_o;
    wire signed [63:0] result_q30_o;

    integer failures = 0;
    integer assertions = 0;
    integer case_count = 0;
    integer random_index;
    reg [63:0] random_left;
    reg [63:0] random_right;

    always #5 clk = ~clk;

    truega_q30_mul_seq dut (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(start_i),
        .left_q30_i(left_q30_i),
        .right_q30_i(right_q30_i),
        .busy_o(busy_o),
        .done_o(done_o),
        .overflow_o(overflow_o),
        .result_q30_o(result_q30_o)
    );

    function automatic [64:0] reference_multiply;
        input signed [63:0] left;
        input signed [63:0] right;
        reg negative;
        reg [63:0] left_magnitude;
        reg [63:0] right_magnitude;
        reg [127:0] product;
        reg [97:0] quotient;
        reg [29:0] remainder;
        reg [97:0] rounded;
        reg overflow;
        reg signed [63:0] result;
        begin
            negative = left[63] ^ right[63];
            left_magnitude = left[63] ? (~left[63:0] + 64'd1)
                                      : left[63:0];
            right_magnitude = right[63] ? (~right[63:0] + 64'd1)
                                        : right[63:0];
            product = left_magnitude * right_magnitude;
            quotient = product[127:30];
            remainder = product[29:0];
            rounded = quotient
                + ((remainder > 30'h20000000)
                    || ((remainder == 30'h20000000) && quotient[0]));
            overflow = negative
                ? (rounded > {34'd0, 64'h8000000000000000})
                : (rounded > {34'd0, 64'h7fffffffffffffff});
            if (overflow)
                result = 64'sd0;
            else if (negative)
                result = -$signed(rounded[63:0]);
            else
                result = $signed(rounded[63:0]);
            reference_multiply = {overflow, result};
        end
    endfunction

    task automatic check;
        input condition;
        input [8*96-1:0] description;
        begin
            assertions = assertions + 1;
            if (!condition) begin
                failures = failures + 1;
                $display("FAIL q30_mul_seq %0s", description);
            end
        end
    endtask

    task automatic run_case;
        input signed [63:0] left;
        input signed [63:0] right;
        input inject_busy_start;
        reg [64:0] expected;
        integer cycles;
        reg signed [63:0] completed_result;
        reg completed_overflow;
        begin
            expected = reference_multiply(left, right);
            case_count = case_count + 1;

            @(negedge clk);
            left_q30_i = left;
            right_q30_i = right;
            start_i = 1'b1;
            @(posedge clk);
            #1;
            check(busy_o && !done_o,
                "start acceptance must assert busy without done");
            @(negedge clk);
            start_i = 1'b0;

            cycles = 0;
            while (!done_o && cycles < EXPECTED_LATENCY + 2) begin
                @(posedge clk);
                #1;
                cycles = cycles + 1;
                if (!done_o)
                    check(busy_o, "busy must stay asserted before completion");
                if (inject_busy_start && cycles == 17) begin
                    @(negedge clk);
                    left_q30_i = 64'sh7fffffffffffffff;
                    right_q30_i = 64'sh7fffffffffffffff;
                    start_i = 1'b1;
                end
                if (inject_busy_start && cycles == 18) begin
                    check(busy_o && !done_o,
                        "start while busy must not restart the operation");
                    @(negedge clk);
                    start_i = 1'b0;
                end
            end

            check(done_o, "completion must arrive before timeout");
            check(cycles == EXPECTED_LATENCY,
                "multiply/round/commit latency must remain 66 cycles");
            check(!busy_o, "busy must deassert with done");
            check(overflow_o === expected[64],
                "overflow must match exact reference");
            check(result_q30_o === $signed(expected[63:0]),
                "result must match exact ties-even reference");

            completed_result = result_q30_o;
            completed_overflow = overflow_o;
            @(posedge clk);
            #1;
            check(!done_o, "done must be a one-cycle pulse");
            check(!busy_o, "unit must remain idle after completion");
            check(result_q30_o === completed_result
                    && overflow_o === completed_overflow,
                "completed result and overflow must remain stable while idle");
        end
    endtask

    initial begin
        repeat (3) @(posedge clk);
        reset_n = 1'b1;
        @(posedge clk);
        #1;
        check(!busy_o && !done_o && !overflow_o
                && result_q30_o == 64'sd0,
            "reset state");

        run_case(64'sd0, 64'sd0, 1'b0);
        run_case(Q30_ONE, Q30_ONE, 1'b0);
        run_case(-Q30_ONE, Q30_ONE, 1'b0);
        run_case(Q30_ONE, 64'sd1234567890123, 1'b1);

        // Remainders immediately below, exactly at, and immediately above half.
        run_case(64'sd1, 64'sd536870911, 1'b0);
        run_case(64'sd1, 64'sd536870912, 1'b0);
        run_case(64'sd1, 64'sd536870913, 1'b0);
        // Halfway with an odd quotient rounds to the adjacent even magnitude.
        run_case(64'sd1, 64'sd1610612736, 1'b0);
        run_case(-64'sd1, 64'sd1610612736, 1'b0);

        run_case(64'sh7fffffffffffffff, Q30_ONE, 1'b0);
        run_case(64'sh8000000000000000, Q30_ONE, 1'b0);
        run_case(64'sh7fffffffffffffff, Q30_ONE + 64'sd1, 1'b0);
        run_case(64'sh8000000000000000, Q30_ONE + 64'sd1, 1'b0);
        run_case(64'sh8000000000000000, 64'sh8000000000000000, 1'b0);

        for (random_index = 0; random_index < 64;
                random_index = random_index + 1) begin
            random_left = {$random, $random};
            random_right = {$random, $random};
            run_case($signed(random_left), $signed(random_right), 1'b0);
        end

        // Reset during a live transaction must synchronously cancel it.
        @(negedge clk);
        left_q30_i = Q30_ONE;
        right_q30_i = 64'sd42;
        start_i = 1'b1;
        @(posedge clk);
        #1;
        @(negedge clk);
        start_i = 1'b0;
        repeat (8) @(posedge clk);
        @(negedge clk);
        reset_n = 1'b0;
        @(posedge clk);
        #1;
        check(!busy_o && !done_o && !overflow_o
                && result_q30_o == 64'sd0,
            "reset must cancel an active transaction");
        @(negedge clk);
        reset_n = 1'b1;
        run_case(Q30_ONE, 64'sd42, 1'b0);

        if (failures == 0) begin
            $display("PASS q30_mul_seq cases=%0d assertions=%0d exact_signed_q30_rne latency=66 busy_start_ignored reset_cancel",
                case_count, assertions);
            $finish;
        end
        $display("FAIL q30_mul_seq failures=%0d cases=%0d assertions=%0d",
            failures, case_count, assertions);
        $fatal(1);
    end

    initial begin
        #1000000;
        $display("FAIL q30_mul_seq global timeout");
        $fatal(1);
    end
endmodule
