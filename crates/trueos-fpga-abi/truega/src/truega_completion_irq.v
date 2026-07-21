// Sticky completion interrupt bridge for the single inline work-package slot.
//
// A retirement is staged for one clock before request_o pulses. This guarantees
// that the terminal work-package state and output registers are visible before
// the PCIe controller can emit the interrupt. status_o remains asserted until
// software consumes the result and writes BAR0_CALL_IRQ_ACK_OFFSET.
module truega_completion_irq (
    input  wire       clk,
    input  wire       reset_n,
    input  wire       retire_i,
    input  wire       interrupt_enable_i,
    input  wire       bar_ack_i,
    input  wire       controller_ack_i,
    output wire       status_o,
    output reg        request_o,
    output wire [4:0] msinum_o
);
    reg pending;
    reg request_issued;
    reg controller_accepted;

    assign status_o = pending;
    assign msinum_o = 5'd0;

    always @(posedge clk or negedge reset_n) begin
        if (!reset_n) begin
            pending <= 1'b0;
            request_issued <= 1'b0;
            controller_accepted <= 1'b0;
            request_o <= 1'b0;
        end else begin
            request_o <= 1'b0;

            // A new retirement wins over a coincident stale host ACK. The
            // request is deliberately emitted on the following clock.
            if (retire_i) begin
                pending <= interrupt_enable_i;
                request_issued <= 1'b0;
                controller_accepted <= 1'b0;
            end else begin
                if (pending && !request_issued) begin
                    request_o <= 1'b1;
                    request_issued <= 1'b1;
                end

                if (controller_ack_i && request_issued) begin
                    controller_accepted <= 1'b1;
                end

                // An interrupt delivered to software has necessarily passed
                // the controller ACK point. Requiring that ACK also protects
                // against an accidental early BAR write.
                if (bar_ack_i && (!pending || controller_accepted)) begin
                    pending <= 1'b0;
                    request_issued <= 1'b0;
                    controller_accepted <= 1'b0;
                end
            end
        end
    end
endmodule
