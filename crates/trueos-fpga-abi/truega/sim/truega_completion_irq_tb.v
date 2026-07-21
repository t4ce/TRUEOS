`timescale 1ns/1ps

module truega_completion_irq_tb;
    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg retire = 1'b0;
    reg interrupt_enable = 1'b0;
    reg bar_ack = 1'b0;
    reg controller_ack = 1'b0;
    wire status;
    wire request;
    wire [4:0] msinum;
    integer request_pulses = 0;

    always #5 clk = ~clk;
    always @(posedge clk) begin
        if (request)
            request_pulses = request_pulses + 1;
    end

    truega_completion_irq dut (
        .clk(clk),
        .reset_n(reset_n),
        .retire_i(retire),
        .interrupt_enable_i(interrupt_enable),
        .bar_ack_i(bar_ack),
        .controller_ack_i(controller_ack),
        .status_o(status),
        .request_o(request),
        .msinum_o(msinum)
    );

    task tick;
        begin
            @(posedge clk);
            #1;
        end
    endtask

    initial begin
        tick;
        reset_n = 1'b1;
        tick;
        if (status || request || msinum != 0) $fatal(1, "bad reset state");

        // A completion without the package flag must remain silent.
        retire = 1'b1;
        tick;
        retire = 1'b0;
        tick;
        if (status || request || request_pulses != 0) $fatal(1, "disabled interrupt fired");

        interrupt_enable = 1'b1;
        retire = 1'b1;
        tick;
        retire = 1'b0;
        if (!status || request) $fatal(1, "retirement was not staged");
        tick;
        if (!status || !request || request_pulses != 1) $fatal(1, "missing request pulse");
        tick;
        if (!status || request || request_pulses != 1) $fatal(1, "request was not one cycle");

        // An early BAR ACK cannot retire a request the controller has not accepted.
        bar_ack = 1'b1;
        tick;
        bar_ack = 1'b0;
        if (!status) $fatal(1, "early BAR ACK cleared status");

        controller_ack = 1'b1;
        tick;
        controller_ack = 1'b0;
        bar_ack = 1'b1;
        tick;
        bar_ack = 1'b0;
        if (status || request) $fatal(1, "BAR ACK did not clear accepted interrupt");

        // A second retirement gets one new request, with no sticky pulse.
        retire = 1'b1;
        tick;
        retire = 1'b0;
        tick;
        tick;
        if (!status || request || request_pulses != 2) $fatal(1, "second request malformed");

        $display("truega_completion_irq_tb: PASS request_pulses=%0d", request_pulses);
        $finish;
    end
endmodule
