`timescale 1ns/1ps

module truega_lfm25_rmsnorm_reduce_slot_tb;
    localparam signed [63:0] Q30_ONE = 64'sd1073741824;
    localparam signed [63:0] EXPECTED_MEAN = 64'sd1073752561;
    localparam signed [63:0] EXPECTED_INV = 64'sd1073736456;

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg reduce_start = 1'b0;
    reg sample_valid = 1'b0;
    reg signed [63:0] sample = 64'sd0;
    wire sample_ready;
    wire reduce_busy;
    wire reduce_done;
    wire reduce_error;
    wire [10:0] samples_accepted;
    wire signed [63:0] mean_square;
    wire signed [63:0] inv_rms;

    reg element_start = 1'b0;
    wire element_busy;
    wire element_done;
    wire element_error;
    wire signed [63:0] normalized;
    wire signed [63:0] residual_sum;

    integer sent;
    integer cycles;
    integer failures = 0;

    always #5 clk = ~clk;

    truega_lfm25_rmsnorm_reduce_slot reduce (
        .clk(clk), .reset_n(reset_n), .start_i(reduce_start),
        .sample_valid_i(sample_valid), .sample_q30_i(sample),
        .sample_ready_o(sample_ready), .busy_o(reduce_busy),
        .done_o(reduce_done), .error_o(reduce_error),
        .samples_accepted_o(samples_accepted),
        .mean_square_q30_o(mean_square), .inv_rms_q30_o(inv_rms)
    );

    truega_lfm25_rmsnorm_residual_slot element (
        .clk(clk), .reset_n(reset_n), .start_i(element_start),
        .x_q30_i(Q30_ONE), .inv_rms_q30_i(inv_rms),
        .weight_format_bf16_i(1'b1), .weight_bits_i(32'h00003f80),
        .residual_q30_i(Q30_ONE), .branch_q30_i(-64'sd268435456),
        .busy_o(element_busy), .done_o(element_done), .error_o(element_error),
        .normalized_q30_o(normalized), .residual_sum_q30_o(residual_sum)
    );

    initial begin
        repeat (4) @(negedge clk);
        reset_n = 1'b1;
        @(negedge clk);
        reduce_start = 1'b1;
        @(negedge clk);
        reduce_start = 1'b0;

        sent = 0;
        cycles = 0;
        while (!reduce_done && cycles < 100000) begin
            @(negedge clk);
            sample_valid = sample_ready && sent < 1024;
            sample = Q30_ONE;
            if (sample_valid)
                sent = sent + 1;
            cycles = cycles + 1;
        end
        sample_valid = 1'b0;

        if (!reduce_done || reduce_error || sent != 1024
            || samples_accepted != 1024
            || mean_square !== EXPECTED_MEAN || inv_rms !== EXPECTED_INV) begin
            $display("FAIL rms_reduce sent=%0d accepted=%0d mean=%0d inv=%0d error=%b cycles=%0d",
                sent, samples_accepted, mean_square, inv_rms, reduce_error, cycles);
            failures = failures + 1;
        end

        // Pass the circuit-produced scalar through the committed RMSNorm weight
        // and residual slot.  BF16 weight 1.0 leaves EXPECTED_INV unchanged.
        @(negedge clk);
        element_start = 1'b1;
        @(negedge clk);
        element_start = 1'b0;
        cycles = 0;
        while (!element_done && cycles < 200) begin
            @(negedge clk);
            cycles = cycles + 1;
        end
        if (!element_done || element_error || normalized !== EXPECTED_INV
            || residual_sum !== 64'sd805306368) begin
            $display("FAIL rms_element normalized=%0d residual=%0d error=%b",
                normalized, residual_sum, element_error);
            failures = failures + 1;
        end

        if (failures == 0) begin
            $display("PASS lfm25_rmsnorm vector=1024 epsilon_q30=10737 fpga_rsqrt=integer_rne residual=exact");
            $finish;
        end
        $display("FAIL lfm25_rmsnorm failures=%0d", failures);
        $finish_and_return(1);
    end
endmodule
