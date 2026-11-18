module v210_packer_tb;
    localparam PAIRS = 3840 * 2160 / 2;
    reg clk = 0;
    always #5 clk = !clk;
    reg reset = 1;
    reg pair_valid = 0;
    wire pair_ready;
    reg [9:0] cb, y_even, cr, y_odd;
    wire block_valid;
    reg block_ready = 0;
    wire [127:0] block_data;
    integer sent = 0, received = 0, cycle = 0;
    integer output_file;
    reg stalled = 0;
    reg [127:0] stalled_data;

    v210_packer dut (.*);

    initial begin
        output_file = $fopen("simulated.v210", "wb");
        if (!output_file) $fatal(1, "Cannot open output");
        repeat (2) @(negedge clk);
        reset = 0;
    end

    always @(negedge clk) begin
        if (!reset) begin
            pair_valid = sent < PAIRS;
            y_even = 64 + (sent % 877);
            y_odd = 64 + ((sent * 3 + 13) % 877);
            cb = 64 + ((sent * 5 + 27) % 897);
            cr = 64 + ((sent * 7 + 53) % 897);
            block_ready = cycle % 31 >= 8;
        end
    end

    always @(posedge clk) begin
        if (!reset) begin
            cycle = cycle + 1;
            if (cycle > PAIRS * 2) $fatal(1, "Timed out");
            if (pair_valid && pair_ready) sent = sent + 1;
            if (stalled && (!block_valid || block_data !== stalled_data))
                $fatal(1, "Output changed under backpressure");
            stalled = block_valid && !block_ready;
            stalled_data = block_data;
            if (block_valid && block_ready) begin
                for (integer octet = 0; octet < 16; octet = octet + 1)
                    $fwrite(output_file, "%c", block_data[8 * octet +: 8]);
                received = received + 1;
                if (received == PAIRS / 3) begin
                    if (sent != PAIRS) $fatal(1, "Unexpected pair count");
                    $fclose(output_file);
                    $finish;
                end
            end
        end
    end
endmodule
