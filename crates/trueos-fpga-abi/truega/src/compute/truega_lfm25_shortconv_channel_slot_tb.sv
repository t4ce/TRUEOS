`timescale 1ns/1ps

module truega_lfm25_shortconv_channel_slot_tb;
    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg start_i = 1'b0;
    wire busy_o;
    wire done_o;
    wire error_o;
    wire signed [63:0] bx_q30;
    wire signed [63:0] conv_q30;
    wire signed [63:0] y_q30;
    wire signed [63:0] state_oldest;
    wire signed [63:0] state_newest;
    integer cycles;
    integer failures = 0;

    always #5 clk = ~clk;

    truega_lfm25_shortconv_channel_slot dut (
        .clk(clk), .reset_n(reset_n), .start_i(start_i),
        .b_q30_i(64'sd536870912),              //  0.5
        .c_q30_i(-64'sd1610612736),            // -1.5
        .x_q30_i(-64'sd268435456),             // -0.25
        .state_oldest_q30_i(64'sd1073741824),  //  1.0
        .state_newest_q30_i(64'sd2147483648),  //  2.0
        .kernel_oldest_bf16_i(16'h3f00),       //  0.5
        .kernel_newest_bf16_i(16'hbe80),       // -0.25
        .kernel_current_bf16_i(16'h4000),      //  2.0
        .busy_o(busy_o), .done_o(done_o), .error_o(error_o),
        .bx_q30_o(bx_q30), .conv_q30_o(conv_q30), .y_q30_o(y_q30),
        .state_oldest_q30_o(state_oldest),
        .state_newest_q30_o(state_newest)
    );

    initial begin
        repeat (4) @(negedge clk);
        reset_n = 1'b1;
        @(negedge clk);
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;
        cycles = 0;
        while (!done_o && cycles < 500) begin
            @(negedge clk);
            cycles = cycles + 1;
        end

        // bx=-0.125, conv=0.5-0.5-0.25=-0.25, y=(-1.5)*(-0.25)=0.375.
        if (!done_o || error_o
            || bx_q30 !== -64'sd134217728
            || conv_q30 !== -64'sd268435456
            || y_q30 !== 64'sd402653184
            || state_oldest !== 64'sd2147483648
            || state_newest !== -64'sd134217728) begin
            $display("FAIL shortconv_channel bx=%0d conv=%0d y=%0d state={%0d,%0d} error=%b cycles=%0d",
                bx_q30, conv_q30, y_q30, state_oldest, state_newest, error_o, cycles);
            failures = failures + 1;
        end

        // Non-finite BF16 kernel must fail before mutating causal state.
        @(negedge clk);
        force dut.kernel_current_bf16_i = 16'h7f80;
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;
        release dut.kernel_current_bf16_i;
        if (!done_o || !error_o || busy_o) begin
            $display("FAIL shortconv_channel invalid-kernel guard");
            failures = failures + 1;
        end

        if (failures == 0) begin
            $display("PASS lfm25_shortconv_channel order=oldest,newest,current l_cache=3 state_shift=exact y=outproj_q8_boundary");
            $finish;
        end
        $display("FAIL lfm25_shortconv_channel failures=%0d", failures);
        $finish_and_return(1);
    end
endmodule
