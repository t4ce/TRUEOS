`timescale 1ns/1ps

module truega_lfm25_shortconv_token_slot_tb;
    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg state_reset_i = 1'b0;
    reg [3:0] state_reset_layer_i = 4'd0;
    wire state_reset_ready;
    wire state_reset_done;
    reg start_i = 1'b0;
    reg [3:0] layer_slot_i = 4'd0;
    reg [31:0] token_position_i = 32'd0;
    reg activation_valid = 1'b0;
    wire activation_ready;
    wire [4:0] activation_block_index;
    reg [271:0] activation_block = 272'd0;
    reg row_valid = 1'b0;
    wire row_ready;
    wire [9:0] row_channel_index;
    wire [4:0] row_block_index;
    reg [271:0] b_weight_block = 272'd0;
    reg [271:0] c_weight_block = 272'd0;
    reg [271:0] x_weight_block = 272'd0;
    reg [15:0] kernel_oldest = 16'h3f80;
    reg [15:0] kernel_newest = 16'h3f80;
    reg [15:0] kernel_current = 16'h3f80;
    wire output_valid;
    reg output_ready = 1'b0;
    wire [4:0] output_block_index;
    wire output_last;
    wire [271:0] output_block;
    wire busy_o;
    wire done_o;
    wire error_o;
    wire [10:0] channels_retired;
    wire [5:0] blocks_retired;

    integer failures = 0;
    integer cycles;
    integer activation_sent;
    integer rows_sent;
    integer outputs_received;
    integer quant;
    integer expected_channel;
    integer expected_block;

    always #5 clk = ~clk;

    truega_lfm25_shortconv_token_slot dut (
        .clk(clk), .reset_n(reset_n),
        .state_reset_i(state_reset_i),
        .state_reset_layer_i(state_reset_layer_i),
        .state_reset_ready_o(state_reset_ready),
        .state_reset_done_o(state_reset_done),
        .start_i(start_i), .layer_slot_i(layer_slot_i),
        .token_position_i(token_position_i),
        .activation_valid_i(activation_valid),
        .activation_ready_o(activation_ready),
        .activation_block_index_o(activation_block_index),
        .activation_q8_block_i(activation_block),
        .row_valid_i(row_valid), .row_ready_o(row_ready),
        .row_channel_index_o(row_channel_index),
        .row_block_index_o(row_block_index),
        .row_b_weight_q8_block_i(b_weight_block),
        .row_c_weight_q8_block_i(c_weight_block),
        .row_x_weight_q8_block_i(x_weight_block),
        .kernel_oldest_bf16_i(kernel_oldest),
        .kernel_newest_bf16_i(kernel_newest),
        .kernel_current_bf16_i(kernel_current),
        .output_valid_o(output_valid), .output_ready_i(output_ready),
        .output_block_index_o(output_block_index),
        .output_last_o(output_last), .output_y_q8_block_o(output_block),
        .busy_o(busy_o), .done_o(done_o), .error_o(error_o),
        .channels_retired_o(channels_retired),
        .blocks_retired_o(blocks_retired)
    );

    function [271:0] sparse_q8_block;
        input [4:0] block_index;
        begin
            sparse_q8_block = block_index == 5'd0
                ? {248'd0, 8'h01, 16'h3c00}
                : {256'd0, 16'h3c00};
        end
    endfunction

    task reset_layer;
        input [3:0] layer;
        begin
            @(negedge clk);
            state_reset_layer_i = layer;
            state_reset_i = 1'b1;
            @(negedge clk);
            if (!state_reset_done || !state_reset_ready) begin
                $display("FAIL shortconv_token reset layer=%0d done=%b ready=%b",
                    layer, state_reset_done, state_reset_ready);
                failures = failures + 1;
            end
            state_reset_i = 1'b0;
            @(negedge clk);
        end
    endtask

    task pulse_start;
        input [3:0] layer;
        input [31:0] position;
        begin
            layer_slot_i = layer;
            token_position_i = position;
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
        end
    endtask

    task check_output_block;
        input [15:0] expected_scale;
        begin
            if (output_block[15:0] !== expected_scale) begin
                $display("FAIL shortconv_token out=%0d scale=%h expected=%h",
                    output_block_index, output_block[15:0], expected_scale);
                failures = failures + 1;
            end
            for (quant = 0; quant < 32; quant = quant + 1) begin
                if (output_block[16 + quant * 8 +: 8] !== 8'h7f) begin
                    $display("FAIL shortconv_token out=%0d quant=%0d value=%h",
                        output_block_index, quant,
                        output_block[16 + quant * 8 +: 8]);
                    failures = failures + 1;
                end
            end
        end
    endtask

    task run_good_token;
        input [3:0] layer;
        input [31:0] position;
        input [15:0] expected_scale;
        begin
            @(negedge clk);
            pulse_start(layer, position);
            activation_sent = 0;
            rows_sent = 0;
            outputs_received = 0;
            cycles = 0;
            while (!done_o && cycles < 2500000) begin
                @(negedge clk);
                activation_valid = activation_ready && ((cycles % 7) != 2);
                activation_block = sparse_q8_block(activation_block_index);
                if (activation_valid)
                    activation_sent = activation_sent + 1;

                row_valid = row_ready && ((cycles % 11) != 4);
                b_weight_block = sparse_q8_block(row_block_index);
                c_weight_block = sparse_q8_block(row_block_index);
                x_weight_block = sparse_q8_block(row_block_index);
                kernel_oldest = 16'h3f80;
                kernel_newest = 16'h3f80;
                kernel_current = 16'h3f80;
                if (row_valid) begin
                    expected_channel = rows_sent / 32;
                    expected_block = rows_sent % 32;
                    if (row_channel_index !== expected_channel[9:0]
                        || row_block_index !== expected_block[4:0]) begin
                        $display("FAIL shortconv_token row order channel=%0d block=%0d expected=%0d/%0d",
                            row_channel_index, row_block_index,
                            expected_channel, expected_block);
                        failures = failures + 1;
                    end
                    rows_sent = rows_sent + 1;
                end

                output_ready = (cycles % 5) != 1;
                if (output_valid) begin
                    check_output_block(expected_scale);
                    if (output_ready) begin
                        if (output_block_index !== outputs_received[4:0]
                            || output_last !== (outputs_received == 31)) begin
                            $display("FAIL shortconv_token output order got=%0d last=%b expected=%0d",
                                output_block_index, output_last, outputs_received);
                            failures = failures + 1;
                        end
                        outputs_received = outputs_received + 1;
                    end
                end
                cycles = cycles + 1;
            end
            activation_valid = 1'b0;
            row_valid = 1'b0;
            output_ready = 1'b0;
            if (!done_o || error_o || activation_sent != 32
                || rows_sent != 32768 || outputs_received != 32
                || channels_retired != 1024 || blocks_retired != 32) begin
                $display("FAIL shortconv_token position=%0d done=%b error=%b act=%0d rows=%0d outs=%0d channels=%0d blocks=%0d cycles=%0d",
                    position, done_o, error_o, activation_sent, rows_sent,
                    outputs_received, channels_retired, blocks_retired, cycles);
                failures = failures + 1;
            end
            @(negedge clk);
        end
    endtask

    task expect_position_reject;
        input [3:0] layer;
        input [31:0] position;
        begin
            pulse_start(layer, position);
            if (!done_o || !error_o || busy_o) begin
                $display("FAIL shortconv_token position reject layer=%0d pos=%0d done=%b error=%b busy=%b",
                    layer, position, done_o, error_o, busy_o);
                failures = failures + 1;
            end
            @(negedge clk);
        end
    endtask

    task poison_layer;
        input [3:0] layer;
        begin
            @(negedge clk);
            pulse_start(layer, 32'd0);
            activation_sent = 0;
            rows_sent = 0;
            cycles = 0;
            while (!done_o && cycles < 10000) begin
                @(negedge clk);
                activation_valid = activation_ready;
                activation_block = sparse_q8_block(activation_block_index);
                if (activation_valid)
                    activation_sent = activation_sent + 1;
                row_valid = row_ready;
                b_weight_block = sparse_q8_block(row_block_index);
                c_weight_block = sparse_q8_block(row_block_index);
                x_weight_block = sparse_q8_block(row_block_index);
                kernel_oldest = 16'h3f80;
                kernel_newest = 16'h3f80;
                kernel_current = 16'h7f80;
                if (row_valid)
                    rows_sent = rows_sent + 1;
                output_ready = 1'b1;
                cycles = cycles + 1;
            end
            activation_valid = 1'b0;
            row_valid = 1'b0;
            output_ready = 1'b0;
            if (!done_o || !error_o || activation_sent != 32
                || rows_sent != 32 || channels_retired != 0) begin
                $display("FAIL shortconv_token poison done=%b error=%b act=%0d rows=%0d channels=%0d",
                    done_o, error_o, activation_sent, rows_sent, channels_retired);
                failures = failures + 1;
            end
            @(negedge clk);
            expect_position_reject(layer, 32'd0);
        end
    endtask

    initial begin
        repeat (4) @(negedge clk);
        reset_n = 1'b1;
        reset_layer(4'd3);

        // Token 0: state={0,0}, so y=1.  Token 1 reuses committed {0,1},
        // making convolution and y exactly 2 for every channel.
        run_good_token(4'd3, 32'd0, 16'h2008);
        expect_position_reject(4'd3, 32'd0);
        run_good_token(4'd3, 32'd1, 16'h2408);

        // An arithmetic failure poisons partial state until explicit reset.
        reset_layer(4'd4);
        poison_layer(4'd4);
        reset_layer(4'd4);

        if (failures == 0) begin
            $display("PASS lfm25_shortconv_token tokens=2 layers=10 activation_load_once rows=1024x32 q8_blocks=32 causal_state_commit positions=strict stalls poison_reset");
            $finish;
        end
        $display("FAIL lfm25_shortconv_token failures=%0d", failures);
        $finish_and_return(1);
    end
endmodule
