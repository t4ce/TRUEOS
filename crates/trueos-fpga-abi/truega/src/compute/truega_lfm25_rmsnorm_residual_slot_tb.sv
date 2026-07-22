`timescale 1ns/1ps

module truega_lfm25_rmsnorm_residual_slot_tb;
    localparam signed [63:0] Q30_ONE = 64'sd1073741824;

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg start_i = 1'b0;
    reg signed [63:0] x_q30_i = 64'sd0;
    reg signed [63:0] inv_rms_q30_i = 64'sd0;
    reg weight_format_bf16_i = 1'b0;
    reg [31:0] weight_bits_i = 32'd0;
    reg signed [63:0] residual_q30_i = 64'sd0;
    reg signed [63:0] branch_q30_i = 64'sd0;
    wire busy_o;
    wire done_o;
    wire error_o;
    wire signed [63:0] normalized_q30_o;
    wire signed [63:0] residual_sum_q30_o;
    integer failures = 0;
    integer cycles;

    always #5 clk = ~clk;

    truega_lfm25_rmsnorm_residual_slot dut (
        .clk(clk), .reset_n(reset_n), .start_i(start_i),
        .x_q30_i(x_q30_i), .inv_rms_q30_i(inv_rms_q30_i),
        .weight_format_bf16_i(weight_format_bf16_i),
        .weight_bits_i(weight_bits_i),
        .residual_q30_i(residual_q30_i), .branch_q30_i(branch_q30_i),
        .busy_o(busy_o), .done_o(done_o), .error_o(error_o),
        .normalized_q30_o(normalized_q30_o),
        .residual_sum_q30_o(residual_sum_q30_o)
    );

    task run_case;
        input signed [63:0] x;
        input signed [63:0] inv_rms;
        input format_bf16;
        input [31:0] weight_bits;
        input signed [63:0] residual;
        input signed [63:0] branch;
        input signed [63:0] expected_norm;
        input signed [63:0] expected_residual;
        begin
            @(negedge clk);
            x_q30_i = x;
            inv_rms_q30_i = inv_rms;
            weight_format_bf16_i = format_bf16;
            weight_bits_i = weight_bits;
            residual_q30_i = residual;
            branch_q30_i = branch;
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            cycles = 0;
            while (!done_o && cycles < 200) begin
                @(negedge clk);
                cycles = cycles + 1;
            end
            if (!done_o || error_o || normalized_q30_o !== expected_norm
                || residual_sum_q30_o !== expected_residual) begin
                $display("FAIL rmsnorm x=%0d inv=%0d norm=%0d expected=%0d residual=%0d expected_residual=%0d error=%b cycles=%0d",
                    x, inv_rms, normalized_q30_o, expected_norm,
                    residual_sum_q30_o, expected_residual, error_o, cycles);
                failures = failures + 1;
            end
        end
    endtask

    initial begin
        repeat (4) @(negedge clk);
        reset_n = 1'b1;

        // BF16 1.5: (0.75 * 1.25) * 1.5 = 1.40625 exactly.
        run_case(64'sd805306368, 64'sd1342177280, 1'b1, 32'h00003fc0,
            64'sd268435456, -64'sd134217728,
            64'sd1509949440, 64'sd134217728);

        // Source F32 -0.5 weight, negative x, and the second residual site.
        run_case(-64'sd536870912, Q30_ONE, 1'b0, 32'hbf000000,
            -64'sd1073741824, 64'sd268435456,
            64'sd268435456, -64'sd805306368);

        // NaN must be rejected without starting the multiplier.
        @(negedge clk);
        weight_format_bf16_i = 1'b0;
        weight_bits_i = 32'h7fc00000;
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;
        if (!done_o || !error_o || busy_o) begin
            $display("FAIL rmsnorm invalid-weight guard");
            failures = failures + 1;
        end

        if (failures == 0) begin
            $display("PASS lfm25_rmsnorm_residual per_element=f32+bf16 q30_rne residual=exact overflow_guard");
            $finish;
        end
        $display("FAIL lfm25_rmsnorm_residual failures=%0d", failures);
        $finish_and_return(1);
    end
endmodule
