`timescale 1ns/1ps

module truega_lfm25_tied_lm_head_argmax_slot_tb;
    localparam integer TEST_ROWS = 6;
    localparam signed [63:0] Q30_ONE = 64'sd1073741824;

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg state_reset_i = 1'b0;
    wire state_reset_ready_o;
    wire state_reset_done_o;
    reg start_i = 1'b0;
    wire start_ready_o;

    reg activation_valid_i = 1'b0;
    wire activation_ready_o;
    wire [4:0] activation_block_index_o;
    reg [4:0] activation_block_index_i = 5'd0;
    reg [271:0] activation_q8_block_i = 272'd0;

    reg row_valid_i = 1'b0;
    wire row_ready_o;
    wire [31:0] row_index_o;
    wire [4:0] row_block_index_o;
    reg [31:0] row_index_i = 32'd0;
    reg [4:0] row_block_index_i = 5'd0;
    reg [271:0] row_weight_q8_block_i = 272'd0;

    wire row_done_o;
    wire row_error_o;
    wire [31:0] row_retired_index_o;
    wire signed [63:0] row_score_q30_o;
    wire busy_o;
    wire done_o;
    wire error_o;
    wire poisoned_o;
    wire [16:0] rows_retired_o;
    wire [31:0] token_o;
    wire signed [63:0] score_q30_o;

    integer failures = 0;
    integer block_index;
    integer row_index;
    integer wait_cycles;
    reg signed [7:0] row_scores [0:TEST_ROWS-1];
    reg signed [63:0] expected_row_score;
    reg [31:0] stalled_row;
    reg [4:0] stalled_block;

    always #5 clk = ~clk;

    function automatic [271:0] q8_block;
        input [15:0] scale_f16;
        input signed [7:0] first_quant;
        reg [255:0] quants;
        begin
            quants = 256'd0;
            quants[7:0] = first_quant;
            q8_block = {quants, scale_f16};
        end
    endfunction

    truega_lfm25_tied_lm_head_argmax_slot #(
        .ROW_COUNT(TEST_ROWS)
    ) dut (
        .clk(clk),
        .reset_n(reset_n),
        .state_reset_i(state_reset_i),
        .state_reset_ready_o(state_reset_ready_o),
        .state_reset_done_o(state_reset_done_o),
        .start_i(start_i),
        .start_ready_o(start_ready_o),
        .activation_valid_i(activation_valid_i),
        .activation_ready_o(activation_ready_o),
        .activation_block_index_o(activation_block_index_o),
        .activation_block_index_i(activation_block_index_i),
        .activation_q8_block_i(activation_q8_block_i),
        .row_valid_i(row_valid_i),
        .row_ready_o(row_ready_o),
        .row_index_o(row_index_o),
        .row_block_index_o(row_block_index_o),
        .row_index_i(row_index_i),
        .row_block_index_i(row_block_index_i),
        .row_weight_q8_block_i(row_weight_q8_block_i),
        .row_done_o(row_done_o),
        .row_error_o(row_error_o),
        .row_retired_index_o(row_retired_index_o),
        .row_score_q30_o(row_score_q30_o),
        .busy_o(busy_o),
        .done_o(done_o),
        .error_o(error_o),
        .poisoned_o(poisoned_o),
        .rows_retired_o(rows_retired_o),
        .token_o(token_o),
        .score_q30_o(score_q30_o)
    );

    task begin_operation;
        begin
            while (!start_ready_o) @(negedge clk);
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            if (!busy_o || !activation_ready_o || error_o
                || activation_block_index_o != 0) begin
                $display("FAIL start busy=%b activation_ready=%b error=%b index=%0d",
                    busy_o, activation_ready_o, error_o,
                    activation_block_index_o);
                failures = failures + 1;
            end
        end
    endtask

    task load_activation;
        begin
            for (block_index = 0; block_index < 32;
                 block_index = block_index + 1) begin
                while (!activation_ready_o) @(negedge clk);
                if (activation_block_index_o !== block_index[4:0]) begin
                    $display("FAIL activation order expected=%0d got=%0d",
                        block_index, activation_block_index_o);
                    failures = failures + 1;
                end
                if (block_index == 3) begin
                    // Reset is backpressured during an active command.
                    state_reset_i = 1'b1;
                    if (state_reset_ready_o || state_reset_done_o) begin
                        $display("FAIL state reset accepted while busy");
                        failures = failures + 1;
                    end
                    @(negedge clk);
                    state_reset_i = 1'b0;
                end
                activation_block_index_i = block_index[4:0];
                activation_q8_block_i = q8_block(16'h3c00, 8'sd1);
                activation_valid_i = 1'b1;
                @(negedge clk);
                activation_valid_i = 1'b0;
                if (block_index % 7 == 2)
                    @(negedge clk);
            end
        end
    endtask

    task reset_poison;
        begin
            while (!state_reset_ready_o) @(negedge clk);
            state_reset_i = 1'b1;
            @(negedge clk);
            state_reset_i = 1'b0;
            if (!state_reset_done_o || poisoned_o || error_o || busy_o) begin
                $display("FAIL poison reset done=%b poisoned=%b error=%b busy=%b",
                    state_reset_done_o, poisoned_o, error_o, busy_o);
                failures = failures + 1;
            end
            @(negedge clk);
            if (state_reset_done_o) begin
                $display("FAIL state_reset_done was not a pulse");
                failures = failures + 1;
            end
        end
    endtask

    task verify_poison_rejects_start;
        begin
            if (!poisoned_o) begin
                $display("FAIL expected poisoned lane");
                failures = failures + 1;
            end
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            if (!done_o || !error_o || busy_o) begin
                $display("FAIL poisoned start done=%b error=%b busy=%b",
                    done_o, error_o, busy_o);
                failures = failures + 1;
            end
            @(negedge clk);
        end
    endtask

    task activation_order_error;
        begin
            begin_operation();
            activation_block_index_i = 5'd1;
            activation_q8_block_i = q8_block(16'h3c00, 8'sd1);
            activation_valid_i = 1'b1;
            @(negedge clk);
            activation_valid_i = 1'b0;
            if (!done_o || !error_o || !poisoned_o || busy_o
                || row_done_o) begin
                $display("FAIL activation order fault done=%b error=%b poison=%b busy=%b row_done=%b",
                    done_o, error_o, poisoned_o, busy_o, row_done_o);
                failures = failures + 1;
            end
            @(negedge clk);
        end
    endtask

    task row_order_error;
        begin
            begin_operation();
            load_activation();
            while (!row_ready_o) @(negedge clk);
            if (row_index_o != 0 || row_block_index_o != 0) begin
                $display("FAIL initial row request row=%0d block=%0d",
                    row_index_o, row_block_index_o);
                failures = failures + 1;
            end
            row_index_i = 32'd0;
            row_block_index_i = 5'd1;
            row_weight_q8_block_i = q8_block(16'h3c00, 8'sd0);
            row_valid_i = 1'b1;
            @(negedge clk);
            row_valid_i = 1'b0;
            if (!row_done_o || !row_error_o || !done_o || !error_o
                || !poisoned_o || rows_retired_o != 0) begin
                $display("FAIL row order fault row_done=%b row_error=%b done=%b error=%b poison=%b rows=%0d",
                    row_done_o, row_error_o, done_o, error_o,
                    poisoned_o, rows_retired_o);
                failures = failures + 1;
            end
            @(negedge clk);
        end
    endtask

    task row_scale_error;
        begin
            begin_operation();
            load_activation();
            for (block_index = 0; block_index < 32;
                 block_index = block_index + 1) begin
                while (!row_ready_o) @(negedge clk);
                row_index_i = 32'd0;
                row_block_index_i = block_index[4:0];
                row_weight_q8_block_i = q8_block(
                    block_index == 0 ? 16'hbc00 : 16'h3c00,
                    block_index == 0 ? 8'sd1 : 8'sd0);
                row_valid_i = 1'b1;
                @(negedge clk);
                row_valid_i = 1'b0;
            end
            wait_cycles = 0;
            while (!row_done_o && wait_cycles < 1000) begin
                @(negedge clk);
                wait_cycles = wait_cycles + 1;
            end
            if (!row_done_o || !row_error_o || !done_o || !error_o
                || !poisoned_o || rows_retired_o != 0
                || row_retired_index_o != 0) begin
                $display("FAIL scale fault row_done=%b row_error=%b done=%b error=%b poison=%b rows=%0d index=%0d",
                    row_done_o, row_error_o, done_o, error_o,
                    poisoned_o, rows_retired_o, row_retired_index_o);
                failures = failures + 1;
            end
            @(negedge clk);
        end
    endtask

    task drive_success;
        input [31:0] expected_token;
        input signed [63:0] expected_score;
        begin
            begin_operation();
            load_activation();

            for (row_index = 0; row_index < TEST_ROWS;
                 row_index = row_index + 1) begin
                for (block_index = 0; block_index < 32;
                     block_index = block_index + 1) begin
                    while (!row_ready_o) @(negedge clk);
                    if (row_index_o !== row_index[31:0]
                        || row_block_index_o !== block_index[4:0]) begin
                        $display("FAIL row order expected=%0d/%0d got=%0d/%0d",
                            row_index, block_index, row_index_o,
                            row_block_index_o);
                        failures = failures + 1;
                    end

                    if ((row_index + block_index) % 11 == 4) begin
                        stalled_row = row_index_o;
                        stalled_block = row_block_index_o;
                        repeat (2) begin
                            @(negedge clk);
                            if (!row_ready_o || row_index_o !== stalled_row
                                || row_block_index_o !== stalled_block) begin
                                $display("FAIL request index changed under source stall expected=%0d/%0d got=%0d/%0d ready=%b",
                                    stalled_row, stalled_block, row_index_o,
                                    row_block_index_o, row_ready_o);
                                failures = failures + 1;
                            end
                        end
                    end

                    row_index_i = row_index[31:0];
                    row_block_index_i = block_index[4:0];
                    row_weight_q8_block_i = q8_block(16'h3c00,
                        block_index == 0 ? row_scores[row_index] : 8'sd0);
                    row_valid_i = 1'b1;
                    @(negedge clk);
                    row_valid_i = 1'b0;
                end

                wait_cycles = 0;
                while (!row_done_o && wait_cycles < 1000) begin
                    @(negedge clk);
                    wait_cycles = wait_cycles + 1;
                end
                expected_row_score = row_scores[row_index] * Q30_ONE;
                if (!row_done_o || row_error_o || error_o
                    || row_retired_index_o !== row_index[31:0]
                    || row_score_q30_o !== expected_row_score
                    || rows_retired_o !== row_index + 1) begin
                    $display("FAIL row retire row=%0d done=%b row_error=%b error=%b retired_index=%0d score=%0d expected=%0d rows=%0d",
                        row_index, row_done_o, row_error_o, error_o,
                        row_retired_index_o, row_score_q30_o,
                        expected_row_score, rows_retired_o);
                    failures = failures + 1;
                end

                if (row_index == TEST_ROWS - 1) begin
                    if (!done_o || busy_o || poisoned_o
                        || rows_retired_o != TEST_ROWS
                        || token_o !== expected_token
                        || score_q30_o !== expected_score) begin
                        $display("FAIL final done=%b busy=%b poison=%b rows=%0d token=%0d score=%0d expected_token=%0d expected_score=%0d",
                            done_o, busy_o, poisoned_o, rows_retired_o,
                            token_o, score_q30_o, expected_token,
                            expected_score);
                        failures = failures + 1;
                    end
                end else if (done_o || !busy_o) begin
                    $display("FAIL premature final completion row=%0d done=%b busy=%b",
                        row_index, done_o, busy_o);
                    failures = failures + 1;
                end
                @(negedge clk);
                if (row_done_o || done_o) begin
                    $display("FAIL completion was not a one-cycle pulse row=%0d",
                        row_index);
                    failures = failures + 1;
                end
            end
        end
    endtask

    initial begin
        repeat (4) @(negedge clk);
        reset_n = 1'b1;
        @(negedge clk);

        activation_order_error();
        verify_poison_rejects_start();
        reset_poison();

        row_order_error();
        verify_poison_rejects_start();
        reset_poison();

        row_scale_error();
        verify_poison_rejects_start();
        reset_poison();

        // All-negative scores prove signed comparison and max initialization;
        // rows 1 and 2 tie, so deterministic first-index behavior selects 1.
        row_scores[0] = -8'sd5;
        row_scores[1] = -8'sd2;
        row_scores[2] = -8'sd2;
        row_scores[3] = -8'sd7;
        row_scores[4] = -8'sd3;
        row_scores[5] = -8'sd4;
        drive_success(32'd1, -64'sd2147483648);

        // A second command proves the retained result is replaced only by the
        // new ordered scan and that positive ties also keep the first token.
        row_scores[0] = -8'sd2;
        row_scores[1] = 8'sd3;
        row_scores[2] = 8'sd3;
        row_scores[3] = -8'sd1;
        row_scores[4] = 8'sd5;
        row_scores[5] = 8'sd4;
        drive_success(32'd4, 64'sd5368709120);

        if (failures == 0) begin
            $display("PASS lfm25_tied_lm_head_argmax test_rows=%0d blocks_per_row=32 activation_load_once ordering=strict poison_reset row_retire=per-row signed_max=full-i64 tie=first-index",
                TEST_ROWS);
            $finish;
        end
        $display("FAIL lfm25_tied_lm_head_argmax failures=%0d", failures);
        $finish_and_return(1);
    end

    initial begin
        #5000000;
        $display("FAIL lfm25_tied_lm_head_argmax simulation timeout");
        $finish_and_return(1);
    end
endmodule

// Elaboration-only production contract.  It proves the default instance is
// 65,536 rows x 32 blocks and that its externally retired result widths cannot
// truncate the vocabulary index, row count, or signed Q30 score.
module truega_lfm25_tied_lm_head_argmax_contract_tb;
    wire [16:0] rows_retired;
    wire [31:0] token;
    wire signed [63:0] score;
    wire [31:0] row_index;
    wire [4:0] row_block_index;

    truega_lfm25_tied_lm_head_argmax_slot dut (
        .clk(1'b0), .reset_n(1'b0),
        .state_reset_i(1'b0), .state_reset_ready_o(),
        .state_reset_done_o(), .start_i(1'b0), .start_ready_o(),
        .activation_valid_i(1'b0), .activation_ready_o(),
        .activation_block_index_o(), .activation_block_index_i(5'd0),
        .activation_q8_block_i(272'd0),
        .row_valid_i(1'b0), .row_ready_o(), .row_index_o(row_index),
        .row_block_index_o(row_block_index), .row_index_i(32'd0),
        .row_block_index_i(5'd0), .row_weight_q8_block_i(272'd0),
        .row_done_o(), .row_error_o(), .row_retired_index_o(),
        .row_score_q30_o(), .busy_o(), .done_o(), .error_o(),
        .poisoned_o(), .rows_retired_o(rows_retired),
        .token_o(token), .score_q30_o(score)
    );

    initial begin
        #1;
        if (dut.ROW_COUNT != 65536 || dut.BLOCKS_PER_ROW != 32
            || $bits(rows_retired) != 17 || $bits(token) != 32
            || $bits(score) != 64 || $bits(row_index) != 32
            || $bits(row_block_index) != 5) begin
            $display("FAIL lfm25_tied_lm_head_argmax production contract rows=%0d blocks=%0d row_count_bits=%0d token_bits=%0d score_bits=%0d",
                dut.ROW_COUNT, dut.BLOCKS_PER_ROW, $bits(rows_retired),
                $bits(token), $bits(score));
            $finish_and_return(1);
        end
        $display("PASS lfm25_tied_lm_head_argmax production_contract rows=65536 blocks_per_row=32 token=u32 score=i64 payload_ram=unreset-sync");
        $finish;
    end
endmodule
