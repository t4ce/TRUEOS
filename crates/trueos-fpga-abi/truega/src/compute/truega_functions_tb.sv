`timescale 1ns/1ps

// Exercises the generated common slot wrapper, including the exact byte order
// seen by the 96-byte BAR work-package envelopes. The lower 72 input bytes are
// the row-control dword plus sealed layer-0 gate row-0/block-0 vector.
module truega_functions_tb;
    localparam [271:0] GOLDEN_ACTIVATION =
        272'h211da756d317082a81dab021e7cfd24a8925f6e7a8253cb3b616491f4ed4a80d1830;
    localparam [271:0] GOLDEN_WEIGHT =
        272'h82a227e1c1f97ffaf176e4fcf803f228360701e305d2b113e3f82cb52f18147a0cb9;
    localparam [159:0] GOLDEN_OUTPUT =
        160'hffffffffff701c80ffffffffff701c80ffffc5cb;

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg start = 1'b0;
    reg [15:0] function_id = 16'd0;
    reg [767:0] input_data = 768'd0;
    reg [4:0] led_state = 5'b00001;
    wire [4:0] next_led;
    wire [767:0] output_data;
    wire [15:0] required_input_bytes;
    wire [15:0] output_bytes;
    wire valid;
    wire busy;
    wire done;
    wire error;

    integer call_index;
    integer wait_cycles;
    integer failures = 0;

    always #5 clk = ~clk;

    truega_functions dut (
        .clk(clk),
        .reset_n(reset_n),
        .start(start),
        .function_id(function_id),
        .input_data(input_data),
        .led_state(led_state),
        .next_led(next_led),
        .output_data(output_data),
        .required_input_bytes(required_input_bytes),
        .output_bytes(output_bytes),
        .valid(valid),
        .busy(busy),
        .done(done),
        .error(error)
    );

    initial begin
        repeat (4) @(negedge clk);
        reset_n = 1'b1;
        function_id = 16'd2;
        input_data[31:0] = 32'h0000_0003;
        input_data[303:32] = GOLDEN_ACTIVATION;
        input_data[575:304] = GOLDEN_WEIGHT;

        #1;
        if (!valid || required_input_bytes !== 16'd72 || output_bytes !== 16'd20) begin
            $display("FAIL functions wrapper descriptor valid=%b input=%0d output=%0d",
                valid, required_input_bytes, output_bytes);
            failures = failures + 1;
        end

        // Run the same physical slot twice to prove the start/busy/done handoff
        // is reusable and does not retain state across work packages.
        for (call_index = 0; call_index < 2; call_index = call_index + 1) begin
            @(negedge clk);
            start = 1'b1;
            @(negedge clk);
            start = 1'b0;
            if (!busy) begin
                $display("FAIL functions wrapper call=%0d busy did not assert", call_index);
                failures = failures + 1;
            end

            wait_cycles = 0;
            while (!done && wait_cycles < 100) begin
                @(negedge clk);
                wait_cycles = wait_cycles + 1;
            end
            if (!done) begin
                $display("FAIL functions wrapper call=%0d completion timeout", call_index);
                failures = failures + 1;
            end else begin
                if (busy || error) begin
                    $display("FAIL functions wrapper call=%0d busy=%b error=%b",
                        call_index, busy, error);
                    failures = failures + 1;
                end
                if (output_data[159:0] !== GOLDEN_OUTPUT) begin
                    $display("FAIL functions wrapper call=%0d output=%h expected=%h",
                        call_index, output_data[159:0], GOLDEN_OUTPUT);
                    failures = failures + 1;
                end
                if (output_data[767:160] !== 608'd0) begin
                    $display("FAIL functions wrapper call=%0d nonzero output padding", call_index);
                    failures = failures + 1;
                end
            end
            @(negedge clk);
            if (done) begin
                $display("FAIL functions wrapper call=%0d done was not a one-cycle pulse",
                    call_index);
                failures = failures + 1;
            end
        end

        // The same fixed slot also owns the chained layer-0 SiLU(gate)*up
        // operation. This sample is the exact row-0 result of the two full
        // projection accumulators.
        input_data = 768'd0;
        input_data[31:0] = 32'h0000_0008;
        input_data[95:32] = 64'sd29481209;
        input_data[159:96] = -64'sd10250472;
        @(negedge clk);
        start = 1'b1;
        @(negedge clk);
        start = 1'b0;
        wait_cycles = 0;
        while (!done && wait_cycles < 100) begin
            @(negedge clk);
            wait_cycles = wait_cycles + 1;
        end
        if (!done || busy || error || output_data[159:96] !== -64'sd142653
                || output_data[95:0] !== 96'd0 || output_data[767:160] !== 608'd0) begin
            $display("FAIL functions wrapper silu done=%b busy=%b error=%b result=%0d",
                done, busy, error, $signed(output_data[159:96]));
            failures = failures + 1;
        end

        if (failures == 0) begin
            $display("PASS functions_wrapper q8_calls=2 silu_calls=1 envelope=96B exact_dot exact_q30");
            $finish;
        end
        $display("FAIL functions_wrapper failures=%0d", failures);
        $finish_and_return(1);
    end

    initial begin
        #10000;
        $display("FAIL functions wrapper simulation timeout");
        $finish_and_return(1);
    end
endmodule
