`timescale 1ns/1ps

module truega_lfm25_attention_norm_rope_tb;
    reg clk = 1'b0;
    reg reset_n = 1'b0;
    integer failures = 0;
    integer cycles;
    integer i;

    reg rms_start = 1'b0;
    reg rms_valid = 1'b0;
    reg signed [63:0] rms_sample = 64'sd0;
    wire rms_ready;
    wire rms_busy;
    wire rms_done;
    wire rms_error;
    wire signed [63:0] inv_rms;

    reg rope_start = 1'b0;
    reg signed [63:0] x_lo = 64'sd0;
    reg signed [63:0] x_hi = 64'sd0;
    reg signed [63:0] weight_lo = 64'sd0;
    reg signed [63:0] weight_hi = 64'sd0;
    reg signed [63:0] rope_cos = 64'sd0;
    reg signed [63:0] rope_sin = 64'sd0;
    wire rope_busy;
    wire rope_done;
    wire rope_error;
    wire signed [63:0] y_lo;
    wire signed [63:0] y_hi;

    always #5 clk = ~clk;

    truega_lfm25_head_rms_inverse_slot rms (
        .clk(clk), .reset_n(reset_n), .start_i(rms_start),
        .sample_valid_i(rms_valid), .sample_q30_i(rms_sample),
        .sample_ready_o(rms_ready), .busy_o(rms_busy), .done_o(rms_done),
        .error_o(rms_error), .inv_rms_q30_o(inv_rms)
    );

    truega_lfm25_qk_norm_rope_slot rope (
        .clk(clk), .reset_n(reset_n), .start_i(rope_start),
        .x_lo_q30_i(x_lo), .x_hi_q30_i(x_hi),
        .inv_rms_q30_i(inv_rms),
        .weight_lo_q30_i(weight_lo), .weight_hi_q30_i(weight_hi),
        .cos_q30_i(rope_cos), .sin_q30_i(rope_sin),
        .busy_o(rope_busy), .done_o(rope_done), .error_o(rope_error),
        .y_lo_q30_o(y_lo), .y_hi_q30_o(y_hi)
    );

    initial begin
        repeat (4) @(negedge clk);
        reset_n = 1'b1;

        // One complete 64-element reduction.  For x[i]=1, the exact fixed
        // equation is 1/sqrt(1 + round_q30(1e-5)).
        @(negedge clk);
        rms_start = 1'b1;
        @(negedge clk);
        rms_start = 1'b0;
        for (i = 0; i < 64; i = i + 1) begin
            while (!rms_ready) @(negedge clk);
            rms_sample = 64'sd1073741824;
            rms_valid = 1'b1;
            @(negedge clk);
            rms_valid = 1'b0;
        end
        cycles = 0;
        while (!rms_done && cycles < 6000) begin
            @(negedge clk);
            cycles = cycles + 1;
        end
        if (!rms_done || rms_error || inv_rms !== 64'sd1073736456) begin
            $display("FAIL head_rms inv=%0d expected=1073736456 error=%b cycles=%0d",
                inv_rms, rms_error, cycles);
            failures = failures + 1;
        end

        // LFM2 uses NEOX pairing: this pair represents dimensions i and i+32.
        // The just-computed FPGA inv_rms is wired directly into this slot.
        @(negedge clk);
        x_lo = 64'sd805306368;       // 0.75
        x_hi = -64'sd536870912;      // -0.5
        weight_lo = 64'sd1610612736; // 1.5
        weight_hi = 64'sd805306368;  // 0.75
        rope_cos = 64'sd858993459;   // round_q30(0.8)
        rope_sin = 64'sd644245094;   // round_q30(0.6)
        rope_start = 1'b1;
        @(negedge clk);
        rope_start = 1'b0;
        cycles = 0;
        while (!rope_done && cycles < 1000) begin
            @(negedge clk);
            cycles = cycles + 1;
        end
        if (!rope_done || rope_error
            || y_lo !== 64'sd1207953512 || y_hi !== 64'sd402651170) begin
            $display("FAIL qk_norm_neox_rope y=(%0d,%0d) expected=(1207953512,402651170) error=%b cycles=%0d",
                y_lo, y_hi, rope_error, cycles);
            failures = failures + 1;
        end

        if (failures == 0) begin
            $display("PASS lfm25_attention_norm_rope head=64 rms=fpga-sumsq-epsilon-isqrt-div rope=neox q30=rne");
            $finish;
        end
        $display("FAIL lfm25_attention_norm_rope failures=%0d", failures);
        $finish_and_return(1);
    end
endmodule
