`timescale 1ns/1ps

module truega_lfm25_rmsnorm_vector_slot_tb;
    localparam signed [63:0] Q30_ONE = 64'sd1073741824;
    localparam signed [63:0] EXPECTED_MEAN = 64'sd1073752561;
    localparam signed [63:0] EXPECTED_INV = 64'sd1073736456;

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg start_i = 1'b0;
    reg input_valid = 1'b0;
    wire input_ready;
    reg signed [63:0] x_q30 = Q30_ONE;
    reg weight_format_bf16 = 1'b1;
    reg [31:0] weight_bits = 32'h00003f80;
    wire output_valid;
    reg output_ready = 1'b0;
    wire [4:0] output_block_index;
    wire output_last;
    wire [271:0] output_q8_block;
    wire busy_o;
    wire done_o;
    wire error_o;
    wire [10:0] inputs_accepted;
    wire [5:0] blocks_retired;
    wire signed [63:0] mean_square;
    wire signed [63:0] inv_rms;
    integer sent;
    integer received;
    integer cycles;
    integer quant;
    integer failures = 0;
    reg [4:0] stalled_index;
    reg stalled;

    always #5 clk = ~clk;

    truega_lfm25_rmsnorm_vector_slot dut (
        .clk(clk), .reset_n(reset_n), .start_i(start_i),
        .input_valid_i(input_valid), .input_ready_o(input_ready),
        .x_q30_i(x_q30), .weight_format_bf16_i(weight_format_bf16),
        .weight_bits_i(weight_bits),
        .output_valid_o(output_valid), .output_ready_i(output_ready),
        .output_block_index_o(output_block_index), .output_last_o(output_last),
        .output_q8_block_o(output_q8_block),
        .busy_o(busy_o), .done_o(done_o), .error_o(error_o),
        .inputs_accepted_o(inputs_accepted), .blocks_retired_o(blocks_retired),
        .mean_square_q30_o(mean_square), .inv_rms_q30_o(inv_rms)
    );

    task check_constant_block;
        begin
            if (output_q8_block[15:0] !== 16'h2008) begin
                $display("FAIL rms_vector block=%0d scale=%h", output_block_index,
                    output_q8_block[15:0]);
                failures = failures + 1;
            end
            for (quant = 0; quant < 32; quant = quant + 1) begin
                if (output_q8_block[16 + quant * 8 +: 8] !== 8'h7f) begin
                    $display("FAIL rms_vector block=%0d quant=%0d value=%h",
                        output_block_index, quant,
                        output_q8_block[16 + quant * 8 +: 8]);
                    failures = failures + 1;
                end
            end
        end
    endtask

    initial begin
        repeat (4) @(negedge clk);
        reset_n = 1'b1;
        @(negedge clk);
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;

        sent = 0;
        received = 0;
        cycles = 0;
        stalled = 1'b0;
        while (!done_o && cycles < 500000) begin
            @(negedge clk);
            input_valid = input_ready && sent < 1024 && ((cycles % 7) != 2);
            x_q30 = Q30_ONE;
            weight_format_bf16 = sent[0];
            weight_bits = sent[0] ? 32'h00003f80 : 32'h3f800000;
            if (input_valid)
                sent = sent + 1;

            output_ready = (cycles % 5) != 1;
            if (output_valid) begin
                check_constant_block();
                if (stalled && output_block_index !== stalled_index) begin
                    $display("FAIL rms_vector changed index under stall old=%0d new=%0d",
                        stalled_index, output_block_index);
                    failures = failures + 1;
                end
                if (output_ready) begin
                    if (output_block_index !== received[4:0]
                        || output_last !== (received == 31)) begin
                        $display("FAIL rms_vector output order got=%0d last=%b expected=%0d",
                            output_block_index, output_last, received);
                        failures = failures + 1;
                    end
                    received = received + 1;
                    stalled = 1'b0;
                end else begin
                    stalled = 1'b1;
                    stalled_index = output_block_index;
                end
            end
            cycles = cycles + 1;
        end
        input_valid = 1'b0;
        output_ready = 1'b0;

        if (!done_o || error_o || sent != 1024 || received != 32
            || inputs_accepted != 1024 || blocks_retired != 32
            || mean_square !== EXPECTED_MEAN || inv_rms !== EXPECTED_INV) begin
            $display("FAIL rms_vector done=%b error=%b sent=%0d recv=%0d accepted=%0d retired=%0d mean=%0d inv=%0d cycles=%0d",
                done_o, error_o, sent, received, inputs_accepted, blocks_retired,
                mean_square, inv_rms, cycles);
            failures = failures + 1;
        end

        // Invalid F32 weight is buffered but rejected by the FPGA normalization
        // element before any block can be published.
        @(negedge clk);
        @(negedge clk);
        start_i = 1'b1;
        @(negedge clk);
        start_i = 1'b0;
        sent = 0;
        cycles = 0;
        while (!done_o && cycles < 100000) begin
            @(negedge clk);
            input_valid = input_ready && sent < 1024;
            x_q30 = Q30_ONE;
            weight_format_bf16 = 1'b0;
            weight_bits = sent == 0 ? 32'h7fc00000 : 32'h3f800000;
            if (input_valid)
                sent = sent + 1;
            output_ready = 1'b1;
            cycles = cycles + 1;
        end
        input_valid = 1'b0;
        output_ready = 1'b0;
        if (!done_o || !error_o || sent != 1024 || blocks_retired != 0) begin
            $display("FAIL rms_vector invalid-weight done=%b error=%b sent=%0d retired=%0d",
                done_o, error_o, sent, blocks_retired);
            failures = failures + 1;
        end

        if (failures == 0) begin
            $display("PASS lfm25_rmsnorm_vector input_stalls output_stalls elements=1024 q8_blocks=32 inv_rms=fpga weights=f32+bf16 error_guard");
            $finish;
        end
        $display("FAIL lfm25_rmsnorm_vector failures=%0d", failures);
        $finish_and_return(1);
    end
endmodule
