module rgba8_frame_store_tb;
    localparam BURSTS = 259200;
    reg clk = 0;
    always #1 clk = !clk;
    reg reset = 1, calibrated = 0, capture_valid = 0, release_frame = 0;
    wire capture_ready, frame_ready, read_ready, reply_valid;
    reg read_valid = 0, reply_ready = 0;
    reg [1:0] capture_kind;
    reg [255:0] capture_data;
    reg [18:0] read_index = 0;
    wire [255:0] reply_data, wr_data;
    wire cmd_en, wr_data_en, wr_data_end;
    wire [2:0] cmd;
    wire [28:0] addr;
    reg rd_data_valid = 0;
    reg [255:0] rd_data;
    reg [255:0] memory [0:BURSTS-1];
    integer cycle = 0, i;
    wire cmd_ready = cycle % 5 != 1;
    wire wr_data_rdy = cycle % 7 != 1;

    rgba8_frame_store dut (.*);

    function [255:0] pattern(input integer index);
        pattern = {8{index ^ 32'h9abcf135}};
    endfunction

    always @(posedge clk) begin
        cycle <= cycle + 1;
        if (cycle > 5_000_000) $fatal(1, "Timed out");
        rd_data_valid <= 0;
        if (cmd_en) begin
            if (!cmd_ready || addr[2:0] != 0 || addr / 8 >= BURSTS)
                $fatal(1, "Invalid DDR command/address");
            if (cmd == 0) begin
                if (!wr_data_rdy || !wr_data_en || !wr_data_end)
                    $fatal(1, "Write command and data must be accepted together");
                memory[addr / 8] <= wr_data;
            end else if (cmd == 1) begin
                rd_data <= memory[addr / 8];
                rd_data_valid <= 1;
            end else $fatal(1, "Unexpected DDR command");
        end
    end

    task token(input [1:0] kind, input [255:0] data);
        begin
            capture_valid = 1;
            capture_kind = kind;
            capture_data = data;
            @(posedge clk);
            while (!capture_ready) @(posedge clk);
            @(negedge clk);
            capture_valid = 0;
        end
    endtask

    task read_burst(input integer index, input stall, input release_during_reply);
        begin
            read_index = index;
            read_valid = 1;
            @(posedge clk);
            while (!read_ready) @(posedge clk);
            @(negedge clk);
            read_valid = 0;
            while (!reply_valid) @(negedge clk);
            if (release_during_reply) begin
                release_frame = 1;
                @(negedge clk);
                release_frame = 0;
                if (!frame_ready || !reply_valid)
                    $fatal(1, "Released a frame before its pending reply was consumed");
            end
            if (stall) repeat (8) @(negedge clk);
            if (!reply_valid || reply_data !== pattern(index))
                $fatal(1, "Read mismatch at burst %0d", index);
            reply_ready = 1;
            @(negedge clk);
            reply_ready = 0;
        end
    endtask

    initial begin
        repeat (3) @(negedge clk);
        reset = 0;
        if (capture_ready || read_ready) $fatal(1, "Used DDR before calibration");
        calibrated = 1;
        token(0, 0); token(1, 0); token(2, 0);
        if (frame_ready) $fatal(1, "Short frame was published");
        token(0, 0); token(1, 0); token(3, 0);
        if (frame_ready) $fatal(1, "Aborted frame was published");
        token(0, 0);
        for (i = 0; i < BURSTS; i = i + 1) token(1, pattern(i));
        token(2, 0);
        if (!frame_ready || capture_ready) $fatal(1, "Full frame was not frozen");
        read_index = BURSTS;
        #0;
        if (read_ready) $fatal(1, "Accepted a read beyond the frame");
        read_burst(BURSTS-1, 1, 0);
        for (i = 0; i < BURSTS - 1; i = i + 1) read_burst(i, 0, 0);
        read_burst(BURSTS-1, 1, 1);
        if (frame_ready) $fatal(1, "Release did not free the frame slot");
        $display("Verified every burst of a Full HD RGBA8 frame through the DDR interface model");
        $finish;
    end
endmodule
