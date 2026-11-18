module hdmi_frame_ingress_tb;
    localparam WIDTH = 1920, HEIGHT = 1080, BURSTS = WIDTH * HEIGHT / 8;
    reg pixel_clk = 0, ddr_clk = 0, reset = 1;
    always #7 pixel_clk = !pixel_clk;
    always #5 ddr_clk = !ddr_clk;
    reg shot_valid = 0, vsync = 0, de = 0;
    reg [7:0] red = 0, green = 0, blue = 0;
    wire shot_ready, push_valid, push_ready, pop_valid, pop_ready;
    wire [1:0] push_kind, pop_kind;
    wire [255:0] push_data, pop_data;
    reg block_writes = 0, release_frame = 0;
    wire frame_ready, cmd_en, wr_data_en, wr_data_end;
    wire [2:0] cmd;
    wire [28:0] addr;
    wire [255:0] wr_data;
    integer write_count = 0, j, row, column, pixel_number;
    integer pixel_offset = 0;
    reg validate_writes = 0;
    reg abort_seen = 0;

    hdmi_frame_ingress input_frame (
        .pixel_clk(pixel_clk), .reset(reset), .shot_valid(shot_valid),
        .shot_ready(shot_ready), .vsync(vsync), .de(de),
        .red(red), .green(green), .blue(blue),
        .token_valid(push_valid), .token_ready(push_ready),
        .token_kind(push_kind), .token_data(push_data)
    );
    async_frame_tokens #(.ADDR_BITS(4)) tokens (
        .write_clk(pixel_clk), .write_reset(reset),
        .write_valid(push_valid), .write_ready(push_ready), .write_data({push_kind, push_data}),
        .read_clk(ddr_clk), .read_reset(reset),
        .read_valid(pop_valid), .read_ready(pop_ready), .read_data({pop_kind, pop_data})
    );
    rgba8_frame_store store (
        .clk(ddr_clk), .reset(reset), .calibrated(1'b1),
        .capture_valid(pop_valid), .capture_ready(pop_ready),
        .capture_kind(pop_kind), .capture_data(pop_data),
        .frame_ready(frame_ready), .release_frame(release_frame),
        .read_valid(1'b0), .read_index(19'd0), .reply_ready(1'b0),
        .cmd_ready(!block_writes), .wr_data_rdy(!block_writes),
        .cmd_en(cmd_en), .cmd(cmd), .addr(addr), .wr_data_en(wr_data_en),
        .wr_data_end(wr_data_end), .wr_data(wr_data),
        .rd_data_valid(1'b0), .rd_data(256'd0)
    );

    function [31:0] expected_pixel(input integer n);
        expected_pixel = {8'hff, n[23:16], n[15:8], n[7:0]};
    endfunction

    always @(posedge ddr_clk) begin
        if (pop_valid && pop_ready && pop_kind == 3)
            abort_seen <= 1;
        if (validate_writes && cmd_en) begin
            if (cmd != 0 || !wr_data_en || !wr_data_end || addr != 8 * write_count)
                $fatal(1, "Bad DDR write at burst %0d", write_count);
            for (j = 0; j < 8; j = j + 1)
                if (wr_data[32*j +: 32] !== expected_pixel(pixel_offset + 8*write_count + j))
                    $fatal(1, "Pixel mismatch in burst %0d lane %0d", write_count, j);
            write_count <= write_count + 1;
        end
    end

    task boundary;
        begin
            vsync = 1; repeat (3) @(negedge pixel_clk);
            vsync = 0; repeat (3) @(negedge pixel_clk);
        end
    endtask

    task start_shot;
        begin
            while (!shot_ready) @(negedge pixel_clk);
            shot_valid = 1; @(negedge pixel_clk); shot_valid = 0;
            repeat (4) @(negedge pixel_clk);
            boundary;
        end
    endtask

    task pixels(input integer count, input integer offset);
        begin
            de = 1;
            for (column = 0; column < count; column = column + 1) begin
                pixel_number = offset + column;
                {blue, green, red} = pixel_number[23:0];
                @(negedge pixel_clk);
            end
            de = 0; repeat (3) @(negedge pixel_clk);
        end
    endtask

    initial begin
        #100_000_000;
        $fatal(1, "Timed out");
    end

    initial begin
        repeat (6) @(negedge pixel_clk); reset = 0;
        start_shot;
        block_writes = 1;
        pixels(WIDTH, 0);
        block_writes = 0;
        boundary;
        while (!abort_seen) @(negedge pixel_clk);
        if (frame_ready) $fatal(1, "Overflowed frame became visible");

        validate_writes = 1;
        start_shot;
        for (row = 0; row < HEIGHT; row = row + 1)
            pixels(WIDTH, WIDTH * row);
        if (frame_ready) $fatal(1, "Frame was published before the final sync");
        boundary;
        while (!frame_ready) @(negedge pixel_clk);
        if (write_count != BURSTS) $fatal(1, "Incorrect burst count");
        @(negedge ddr_clk);
        release_frame = 1;
        @(negedge ddr_clk);
        release_frame = 0;
        if (frame_ready) $fatal(1, "Released frame still visible");
        write_count = 0;
        pixel_offset = WIDTH * HEIGHT;
        @(negedge pixel_clk);
        start_shot;
        for (row = 0; row < HEIGHT; row = row + 1)
            pixels(WIDTH, pixel_offset + WIDTH * row);
        boundary;
        while (!frame_ready) @(negedge pixel_clk);
        if (write_count != BURSTS) $fatal(1, "Second frame incomplete");
        $display("Verified two distinct Full HD rasters, slot reuse and overflow recovery");
        $finish;
    end
endmodule
