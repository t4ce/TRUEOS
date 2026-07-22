`timescale 1ns/1ps

module truega_lfm25_resident_ffn_row_engine_tb;
    reg clk = 1'b0;
    always #5 clk = ~clk;

    reg reset_n = 1'b0;
    reg clear = 1'b0;
    reg activation_valid = 1'b0;
    reg [4:0] activation_index = 5'd0;
    reg [271:0] activation_block = 272'd0;
    wire activation_ready;
    wire [4:0] expected_activation_index;
    reg row_start = 1'b0;
    reg row_down = 1'b0;
    reg [12:0] row_index = 13'd0;
    wire row_ready;
    wire expected_row_down;
    wire [12:0] expected_row_index;
    reg weight_valid = 1'b0;
    reg [7:0] weight_index = 8'd0;
    reg [271:0] weight0 = 272'd0;
    reg [271:0] weight1 = 272'd0;
    wire weight_ready;
    wire [7:0] expected_weight_index;
    wire row_done;
    wire row_error;
    wire row_done_down;
    wire [12:0] row_done_index;
    wire poison;
    wire [7:0] error_code;
    wire busy;
    wire complete;
    reg output_read = 1'b0;
    reg [9:0] output_read_index = 10'd0;
    wire output_read_valid;
    wire output_read_error;
    wire signed [63:0] output_read_q30;
    wire [5:0] activation_count;
    wire [12:0] gate_up_count;
    wire [7:0] down_activation_count;
    wire [10:0] down_count;

    truega_lfm25_resident_ffn_row_engine dut (
        .clk(clk), .reset_n(reset_n), .clear_i(clear),
        .activation_valid_i(activation_valid),
        .activation_block_index_i(activation_index),
        .activation_block_i(activation_block),
        .activation_ready_o(activation_ready),
        .activation_block_index_o(expected_activation_index),
        .row_start_i(row_start), .row_down_i(row_down),
        .row_index_i(row_index), .row_ready_o(row_ready),
        .row_down_o(expected_row_down), .row_index_o(expected_row_index),
        .weight_valid_i(weight_valid),
        .weight_block_index_i(weight_index),
        .weight0_block_i(weight0), .weight1_block_i(weight1),
        .weight_ready_o(weight_ready),
        .weight_block_index_o(expected_weight_index),
        .row_done_o(row_done), .row_error_o(row_error),
        .row_done_down_o(row_done_down),
        .row_done_index_o(row_done_index),
        .poison_o(poison), .error_code_o(error_code),
        .busy_o(busy), .complete_o(complete),
        .output_read_i(output_read),
        .output_read_index_i(output_read_index),
        .output_read_valid_o(output_read_valid),
        .output_read_error_o(output_read_error),
        .output_read_q30_o(output_read_q30),
        .activation_blocks_loaded_o(activation_count),
        .gate_up_rows_completed_o(gate_up_count),
        .down_activation_blocks_o(down_activation_count),
        .down_rows_completed_o(down_count)
    );

    integer failures = 0;
    integer row;
    integer block_index;
    integer cycles = 0;
    reg signed [63:0] output0;
    reg signed [63:0] output1;
    reg signed [63:0] output2;
    reg signed [63:0] output_last;

    function automatic [271:0] native_block;
        input [15:0] scale_f16;
        input signed [7:0] first_quant;
        reg [271:0] value;
        begin
            value = 272'd0;
            value[15:0] = scale_f16;
            value[23:16] = first_quant;
            native_block = value;
        end
    endfunction

    task automatic pulse_clear;
        begin
            @(negedge clk);
            clear = 1'b1;
            @(negedge clk);
            clear = 1'b0;
            @(negedge clk);
        end
    endtask

    task automatic load_activation_image;
        integer activation_number;
        begin
            for (activation_number = 0; activation_number < 32;
                 activation_number = activation_number + 1) begin
                while (!activation_ready) @(negedge clk);
                if (expected_activation_index !== activation_number[4:0])
                    failures = failures + 1;
                activation_index = activation_number[4:0];
                activation_block = activation_number == 0
                    ? native_block(16'h3800, 8'sd1) : 272'd0;
                activation_valid = 1'b1;
                @(negedge clk);
                activation_valid = 1'b0;
            end
            while (!row_ready) @(negedge clk);
            if (activation_count != 6'd32)
                failures = failures + 1;
        end
    endtask

    task automatic begin_expected_row;
        input requested_down;
        input integer requested_row;
        begin
            while (!row_ready) @(negedge clk);
            if (expected_row_down !== requested_down
                    || expected_row_index !== requested_row[12:0])
                failures = failures + 1;
            row_down = requested_down;
            row_index = requested_row[12:0];
            row_start = 1'b1;
            @(negedge clk);
            row_start = 1'b0;
        end
    endtask

    task automatic feed_weight_block;
        input integer requested_block;
        input [271:0] requested_weight0;
        input [271:0] requested_weight1;
        begin
            while (!weight_ready) @(negedge clk);
            if (expected_weight_index !== requested_block[7:0])
                failures = failures + 1;
            weight_index = requested_block[7:0];
            weight0 = requested_weight0;
            weight1 = requested_weight1;
            weight_valid = 1'b1;
            @(negedge clk);
            weight_valid = 1'b0;
        end
    endtask

    task automatic finish_expected_row;
        input requested_down;
        input integer requested_row;
        begin
            while (!row_done) @(negedge clk);
            if (row_error || poison
                    || row_done_down !== requested_down
                    || row_done_index !== requested_row[12:0]) begin
                $display("row failure down=%0d row=%0d error=%0d poison=%0d code=%0d",
                         requested_down, requested_row, row_error, poison,
                         error_code);
                failures = failures + 1;
            end
        end
    endtask

    task automatic read_output;
        input integer requested_index;
        output reg signed [63:0] value;
        begin
            @(negedge clk);
            output_read_index = requested_index[9:0];
            output_read = 1'b1;
            @(negedge clk);
            output_read = 1'b0;
            if (!output_read_valid || output_read_error) begin
                failures = failures + 1;
                value = 64'sd0;
            end else begin
                value = output_read_q30;
            end
        end
    endtask

    always @(posedge clk) begin
        cycles <= cycles + 1;
        if (cycles > 20_000_000) begin
            $display("FAIL resident_ffn timeout state=%0d gu=%0d down=%0d",
                     dut.state, gate_up_count, down_count);
            $finish;
        end
    end

    initial begin
        repeat (4) @(posedge clk);
        reset_n = 1'b1;
        @(negedge clk);

        // A malformed activation index poisons immediately. Only explicit
        // clear recovers; payload RAM contents are irrelevant after metadata reset.
        activation_index = 5'd1;
        activation_block = 272'd0;
        activation_valid = 1'b1;
        @(negedge clk);
        activation_valid = 1'b0;
        if (!poison || error_code != 8'd1)
            failures = failures + 1;
        pulse_clear();
        if (poison || !activation_ready || expected_activation_index != 5'd0)
            failures = failures + 1;

        // Exercise weight ordering poison before the full fixed-shape pass.
        load_activation_image();
        begin_expected_row(1'b0, 0);
        while (!weight_ready) @(negedge clk);
        weight_index = 8'd1;
        weight0 = 272'd0;
        weight1 = 272'd0;
        weight_valid = 1'b1;
        @(negedge clk);
        weight_valid = 1'b0;
        if (!poison || error_code != 8'd3 || !row_error)
            failures = failures + 1;
        pulse_clear();

        // Full 4,608-row gate/up shape. Rows 0..31 carry the same small
        // deterministic value, producing one nonzero resident Q8 block;
        // every later group is the all-zero quantizer path.
        load_activation_image();
        for (row = 0; row < 4608; row = row + 1) begin
            begin_expected_row(1'b0, row);
            for (block_index = 0; block_index < 32;
                 block_index = block_index + 1) begin
                if (row < 32 && block_index == 0)
                    feed_weight_block(block_index,
                        native_block(16'h3800, 8'sd1),
                        native_block(16'h3800, 8'sd1));
                else
                    feed_weight_block(block_index, 272'd0, 272'd0);
            end
            finish_expected_row(1'b0, row);
            if ((row & 1023) == 1023)
                $display("resident_ffn gate_up progress=%0d/4608", row + 1);
        end
        if (gate_up_count != 13'd4608
                || down_activation_count != 8'd144
                || !row_ready || !expected_row_down)
            failures = failures + 1;

        // Full 1,024-row down shape. The first two rows are identical and
        // touch only the first nonzero resident block. Remaining zero rows
        // must store exact zero outputs.
        for (row = 0; row < 1024; row = row + 1) begin
            begin_expected_row(1'b1, row);
            for (block_index = 0; block_index < 144;
                 block_index = block_index + 1) begin
                if (row < 2 && block_index == 0)
                    feed_weight_block(block_index,
                        native_block(16'h3800, 8'sd1), 272'd0);
                else
                    feed_weight_block(block_index, 272'd0, 272'd0);
            end
            finish_expected_row(1'b1, row);
            if ((row & 255) == 255)
                $display("resident_ffn down progress=%0d/1024", row + 1);
        end

        if (!complete || poison || busy
                || down_count != 11'd1024)
            failures = failures + 1;

        read_output(0, output0);
        read_output(1, output1);
        read_output(2, output2);
        read_output(1023, output_last);
        if (output0 == 64'sd0 || output0 !== output1
                || output2 !== 64'sd0 || output_last !== 64'sd0) begin
            $display("output mismatch first=%0d second=%0d zero=%0d last=%0d",
                     output0, output1, output2, output_last);
            failures = failures + 1;
        end

        if (failures == 0)
            $display("PASS resident_ffn full_shape gate_up=4608 q8=144 down=1024 first_q30=%0d poison_recovery=pass",
                     output0);
        else
            $display("FAIL resident_ffn failures=%0d", failures);
        $finish;
    end
endmodule
