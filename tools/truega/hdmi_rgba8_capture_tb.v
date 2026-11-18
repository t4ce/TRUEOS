module hdmi_rgba8_capture_tb;
    reg clk = 0;
    always #1 clk = !clk;
    reg reset = 1, arm = 0, vsync = 0, de = 0, ready = 1;
    wire valid, complete, dropped;
    wire [31:0] rgba;
    integer accepted = 0, completed = 0, discarded = 0;

    hdmi_rgba8_capture #(.WIDTH(4), .HEIGHT(3)) dut (
        .pixel_clk(clk), .reset(reset), .arm(arm),
        .vsync(vsync), .de(de), .red(8'h21), .green(8'h43), .blue(8'h65),
        .pixel_ready(ready), .pixel_valid(valid), .rgba(rgba),
        .frame_complete(complete), .frame_dropped(dropped)
    );

    always @(posedge clk) begin
        if (valid && ready) begin
            if (rgba !== 32'hff654321) $fatal(1, "Incorrect pixel byte order");
            accepted = accepted + 1;
        end
        if (complete) completed = completed + 1;
        if (dropped) discarded = discarded + 1;
    end

    task boundary;
        begin
            vsync = 1; repeat (2) @(negedge clk);
            vsync = 0; @(negedge clk);
        end
    endtask

    task line(input integer pixels, input stall);
        begin
            de = 1; ready = !stall;
            repeat (pixels) @(negedge clk);
            de = 0; ready = 1;
            repeat (2) @(negedge clk);
        end
    endtask

    task start;
        begin
            arm = 1; @(negedge clk); arm = 0;
            boundary;
        end
    endtask

    initial begin
        repeat (2) @(negedge clk); reset = 0;
        start; repeat (3) line(4, 0); boundary;
        if (accepted != 12 || completed != 1 || discarded != 0)
            $fatal(1, "Complete raster was not accepted");
        start; line(3, 0); repeat (2) line(4, 0); boundary;
        if (completed != 1 || discarded != 1)
            $fatal(1, "Short line was not rejected");
        start; line(4, 1); repeat (2) line(4, 0); boundary;
        if (completed != 1 || discarded != 2)
            $fatal(1, "Stalled raster was not rejected");
        start; repeat (3) line(4, 0); boundary;
        if (completed != 2 || discarded != 2)
            $fatal(1, "Failed to recover at the next frame");
        $display("Verified RGBA byte order, frame dimensions and overflow recovery");
        $finish;
    end
endmodule
