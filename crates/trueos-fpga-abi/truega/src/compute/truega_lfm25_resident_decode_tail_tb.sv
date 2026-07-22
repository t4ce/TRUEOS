`timescale 1ns/1ps

module truega_lfm25_resident_decode_tail_tb;
`ifdef TRUEGA_TAIL_QUICK
    localparam integer PRODUCTION_ROWS = 64;
`else
    localparam integer PRODUCTION_ROWS = 65536;
`endif
    localparam [31:0] EPOCH = 32'h00001234;
    localparam [36:0] STREAM_HANDLE = {EPOCH, 1'b1, 4'd0};
    localparam [36:0] SOURCE_HANDLE = {EPOCH, 1'b0, 4'd0};
    localparam [36:0] NORM_HANDLE = {EPOCH, 1'b1, 4'd1};

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg clear = 1'b0;
    reg abort = 1'b0;
    reg start = 1'b0;
    wire start_ready;
    reg [36:0] source_handle = SOURCE_HANDLE;
    reg [36:0] norm_handle = NORM_HANDLE;

    reg norm_valid = 1'b0;
    wire norm_ready;
    wire [9:0] expected_norm_index;
    reg [9:0] norm_index = 10'd0;
    reg norm_bf16 = 1'b1;
    reg [31:0] norm_bits = 32'h00003f80;
    wire norm_result_valid;
    reg norm_result_ready = 1'b0;
    wire [36:0] norm_result_handle;

    reg lm_valid = 1'b0;
    wire lm_ready;
    wire [31:0] expected_lm_row;
    wire [4:0] expected_lm_block;
    reg [31:0] lm_row = 32'd0;
    reg [4:0] lm_block = 5'd0;
    reg [271:0] lm_weight = 272'd0;
    wire lm_row_done;
    wire lm_row_error;
    wire [31:0] retired_row;
    wire signed [63:0] retired_score;
    reg activation_pause = 1'b0;

    wire result_valid;
    reg result_ready = 1'b0;
    wire result_error;
    wire [7:0] result_error_code;
    wire [31:0] result_token;
    wire signed [63:0] result_score;
    wire [16:0] result_rows;
    wire poisoned;
    wire busy;

    // Tail side of the shared resident-vector interfaces.
    wire tail_command_valid;
    wire tail_command_ready;
    wire [1:0] tail_command_operation;
    wire [36:0] tail_command_source0;
    wire [36:0] tail_command_source1;
    wire [36:0] tail_command_destination;
    wire tail_result_valid;
    wire tail_result_ready;
    wire tail_result_error;
    wire [36:0] tail_result_handle;
    wire tail_resident_abort;
    wire tail_weight_valid;
    wire tail_weight_ready;
    wire [9:0] tail_weight_index;
    wire tail_weight_bf16;
    wire [31:0] tail_weight_bits;
    wire tail_inspect_valid;
    wire tail_inspect_ready;
    wire [36:0] tail_inspect_handle;
    wire [9:0] tail_inspect_index;
    wire tail_inspect_rsp_valid;
    wire tail_inspect_rsp_ready;
    wire tail_inspect_rsp_error;
    wire [271:0] tail_inspect_rsp_data;

    // Initial source creation uses only the typed TokenEmbedding operation.
    reg setup_mode = 1'b1;
    reg setup_command_valid = 1'b0;
    reg [1:0] setup_command_operation = 2'd0;
    reg [36:0] setup_command_source0 = STREAM_HANDLE;
    reg [36:0] setup_command_destination = SOURCE_HANDLE;
    reg setup_embedding_valid = 1'b0;
    wire setup_embedding_ready;
    reg [4:0] setup_embedding_index = 5'd0;
    reg [271:0] setup_embedding_block = 272'd0;
    reg setup_result_ready = 1'b0;

    wire rv_command_valid = setup_mode
        ? setup_command_valid : tail_command_valid;
    wire rv_command_ready;
    wire [1:0] rv_command_operation = setup_mode
        ? setup_command_operation : tail_command_operation;
    wire [36:0] rv_command_source0 = setup_mode
        ? setup_command_source0 : tail_command_source0;
    wire [36:0] rv_command_source1 = setup_mode
        ? 37'd0 : tail_command_source1;
    wire [36:0] rv_command_destination = setup_mode
        ? setup_command_destination : tail_command_destination;
    wire rv_result_valid;
    wire rv_result_ready = setup_mode
        ? setup_result_ready : tail_result_ready;
    wire rv_result_error;
    wire [36:0] rv_result_handle;

    assign tail_command_ready = !setup_mode && rv_command_ready;
    assign tail_result_valid = !setup_mode && rv_result_valid;
    assign tail_result_error = rv_result_error;
    assign tail_result_handle = rv_result_handle;
    assign setup_embedding_ready = setup_mode && rv_embedding_ready;
    assign tail_weight_ready = !setup_mode && rv_weight_ready;
    assign tail_inspect_ready = !setup_mode && rv_inspect_ready;
    assign tail_inspect_rsp_valid = !setup_mode && rv_inspect_rsp_valid;
    assign tail_inspect_rsp_error = rv_inspect_rsp_error;
    assign tail_inspect_rsp_data = rv_inspect_rsp_data;

    wire rv_embedding_ready;
    wire rv_weight_ready;
    wire rv_inspect_ready;
    wire rv_inspect_rsp_valid;
    wire rv_inspect_rsp_error;
    wire [271:0] rv_inspect_rsp_data;

    integer failures = 0;
    integer block_number;
    integer row_number;
    integer weight_number;
    integer cycles = 0;
    reg signed [63:0] tie_score;
    reg [31:0] held_token;
    reg signed [63:0] held_score;
    reg [16:0] held_rows;
    reg [271:0] held_activation;

    always #5 clk = ~clk;

    function automatic [271:0] native_block;
        input [15:0] scale_f16;
        input signed [7:0] fill_quant;
        integer q;
        reg [255:0] quants;
        begin
            quants = 256'd0;
            for (q = 0; q < 32; q = q + 1)
                quants[q * 8 +: 8] = fill_quant;
            native_block = {quants, scale_f16};
        end
    endfunction

    function automatic [271:0] sparse_weight_block;
        input signed [7:0] first_quant;
        reg [255:0] quants;
        begin
            quants = 256'd0;
            quants[7:0] = first_quant;
            sparse_weight_block = {quants, 16'h3c00};
        end
    endfunction

    truega_lfm25_resident_decode_tail #(
        .LM_HEAD_ROWS(PRODUCTION_ROWS)
    ) dut (
        .clk(clk), .reset_n(reset_n), .clear_i(clear), .abort_i(abort),
        .start_i(start), .start_ready_o(start_ready),
        .source_q30_handle_i(source_handle),
        .normalized_q8_handle_i(norm_handle),
        .norm_weight_valid_i(norm_valid),
        .norm_weight_ready_o(norm_ready),
        .expected_norm_weight_index_o(expected_norm_index),
        .norm_weight_index_i(norm_index),
        .norm_weight_format_bf16_i(norm_bf16),
        .norm_weight_bits_i(norm_bits),
        .norm_result_valid_o(norm_result_valid),
        .norm_result_ready_i(norm_result_ready),
        .norm_result_handle_o(norm_result_handle),
        .lm_weight_valid_i(lm_valid), .lm_weight_ready_o(lm_ready),
        .expected_lm_row_o(expected_lm_row),
        .expected_lm_block_o(expected_lm_block),
        .lm_weight_row_i(lm_row), .lm_weight_block_i(lm_block),
        .lm_weight_q8_block_i(lm_weight),
        .lm_row_done_o(lm_row_done), .lm_row_error_o(lm_row_error),
        .lm_row_retired_index_o(retired_row),
        .lm_row_score_q30_o(retired_score),
        .activation_pause_i(activation_pause),
        .result_valid_o(result_valid), .result_ready_i(result_ready),
        .result_error_o(result_error),
        .result_error_code_o(result_error_code),
        .result_token_o(result_token),
        .result_score_q30_o(result_score),
        .result_rows_retired_o(result_rows), .poisoned_o(poisoned),
        .busy_o(busy),
        .resident_command_valid_o(tail_command_valid),
        .resident_command_ready_i(tail_command_ready),
        .resident_command_operation_o(tail_command_operation),
        .resident_command_source0_handle_o(tail_command_source0),
        .resident_command_source1_handle_o(tail_command_source1),
        .resident_command_destination_handle_o(tail_command_destination),
        .resident_result_valid_i(tail_result_valid),
        .resident_result_ready_o(tail_result_ready),
        .resident_result_error_i(tail_result_error),
        .resident_result_handle_i(tail_result_handle),
        .resident_abort_o(tail_resident_abort),
        .resident_weight_valid_o(tail_weight_valid),
        .resident_weight_ready_i(tail_weight_ready),
        .resident_weight_index_o(tail_weight_index),
        .resident_weight_format_bf16_o(tail_weight_bf16),
        .resident_weight_bits_o(tail_weight_bits),
        .resident_inspect_valid_o(tail_inspect_valid),
        .resident_inspect_ready_i(tail_inspect_ready),
        .resident_inspect_handle_o(tail_inspect_handle),
        .resident_inspect_index_o(tail_inspect_index),
        .resident_inspect_rsp_valid_i(tail_inspect_rsp_valid),
        .resident_inspect_rsp_ready_o(tail_inspect_rsp_ready),
        .resident_inspect_rsp_error_i(tail_inspect_rsp_error),
        .resident_inspect_rsp_data_i(tail_inspect_rsp_data)
    );

    truega_lfm25_resident_vector_engine resident (
        .clk(clk), .reset_n(reset_n),
        .abort_i(!setup_mode && tail_resident_abort),
        .command_valid_i(rv_command_valid),
        .command_ready_o(rv_command_ready),
        .command_operation_i(rv_command_operation),
        .command_source0_handle_i(rv_command_source0),
        .command_source1_handle_i(rv_command_source1),
        .command_destination_handle_i(rv_command_destination),
        .embedding_block_valid_i(setup_mode && setup_embedding_valid),
        .embedding_block_ready_o(rv_embedding_ready),
        .embedding_block_index_i(setup_embedding_index),
        .embedding_q8_block_i(setup_embedding_block),
        .weight_valid_i(!setup_mode && tail_weight_valid),
        .weight_ready_o(rv_weight_ready),
        .weight_index_i(tail_weight_index),
        .weight_format_bf16_i(tail_weight_bf16),
        .weight_bits_i(tail_weight_bits),
        .import_valid_i(1'b0), .import_ready_o(),
        .import_index_i(10'd0), .import_q30_i(64'sd0),
        .result_valid_o(rv_result_valid), .result_ready_i(rv_result_ready),
        .result_error_o(rv_result_error), .result_handle_o(rv_result_handle),
        .inspect_valid_i(!setup_mode && tail_inspect_valid),
        .inspect_ready_o(rv_inspect_ready),
        .inspect_handle_i(tail_inspect_handle),
        .inspect_index_i(tail_inspect_index),
        .inspect_rsp_valid_o(rv_inspect_rsp_valid),
        .inspect_rsp_ready_i(!setup_mode && tail_inspect_rsp_ready),
        .inspect_rsp_error_o(rv_inspect_rsp_error),
        .inspect_rsp_data_o(rv_inspect_rsp_data),
        .session_epoch_o(), .busy_o()
    );

    task automatic establish_source;
        begin
            setup_mode = 1'b1;
            setup_command_operation = 2'd0;
            setup_command_source0 = STREAM_HANDLE;
            setup_command_destination = SOURCE_HANDLE;
            while (!rv_command_ready) @(negedge clk);
            setup_command_valid = 1'b1;
            @(negedge clk);
            setup_command_valid = 1'b0;
            for (block_number = 0; block_number < 32;
                    block_number = block_number + 1) begin
                while (!setup_embedding_ready) @(negedge clk);
                setup_embedding_index = block_number[4:0];
                setup_embedding_block = native_block(16'h3c00, 8'sd1);
                setup_embedding_valid = 1'b1;
                @(negedge clk);
                setup_embedding_valid = 1'b0;
            end
            while (!rv_result_valid) @(negedge clk);
            if (rv_result_error || rv_result_handle !== SOURCE_HANDLE) begin
                $display("FAIL source establishment error=%b handle=%h",
                    rv_result_error, rv_result_handle);
                failures = failures + 1;
            end
            setup_result_ready = 1'b1;
            @(negedge clk);
            setup_result_ready = 1'b0;
            setup_mode = 1'b0;
            @(negedge clk);
        end
    endtask

    task automatic pulse_start;
        begin
            while (!start_ready) @(negedge clk);
            start = 1'b1;
            @(negedge clk);
            start = 1'b0;
        end
    endtask

    task automatic consume_result;
        input expected_error;
        input [7:0] expected_code;
        begin
            while (!result_valid) @(negedge clk);
            held_token = result_token;
            held_score = result_score;
            held_rows = result_rows;
            repeat (3) begin
                @(negedge clk);
                if (!result_valid || result_token !== held_token
                        || result_score !== held_score
                        || result_rows !== held_rows) begin
                    $display("FAIL result changed under backpressure");
                    failures = failures + 1;
                end
            end
            if (result_error !== expected_error
                    || result_error_code !== expected_code) begin
                $display("FAIL result error=%b/%0d expected=%b/%0d",
                    result_error, result_error_code,
                    expected_error, expected_code);
                failures = failures + 1;
            end
            result_ready = 1'b1;
            @(negedge clk);
            result_ready = 1'b0;
        end
    endtask

    task automatic pulse_clear;
        begin
            clear = 1'b1;
            @(negedge clk);
            clear = 1'b0;
            repeat (2) @(negedge clk);
        end
    endtask

    task automatic feed_full_norm;
        input exercise_reply_backpressure;
        begin
            for (weight_number = 0; weight_number < 1024;
                    weight_number = weight_number + 1) begin
                while (!norm_ready) @(negedge clk);
                if (expected_norm_index !== weight_number[9:0]) begin
                    $display("FAIL norm expected index=%0d got=%0d",
                        weight_number, expected_norm_index);
                    failures = failures + 1;
                end
                if ((weight_number & 255) == 17) begin
                    repeat (2) begin
                        @(negedge clk);
                        if (!norm_ready
                                || expected_norm_index !== weight_number[9:0]) begin
                            $display("FAIL norm request changed under source stall index=%0d",
                                weight_number);
                            failures = failures + 1;
                        end
                    end
                end
                norm_index = weight_number[9:0];
                norm_bf16 = 1'b1;
                norm_bits = 32'h00003f80;
                norm_valid = 1'b1;
                @(negedge clk);
                norm_valid = 1'b0;
            end
            while (!norm_result_valid) @(negedge clk);
            repeat (4) begin
                @(negedge clk);
                if (!norm_result_valid
                        || norm_result_handle !== NORM_HANDLE
                        || lm_ready || tail_inspect_valid) begin
                    $display("FAIL FinalRmsNorm publication boundary valid=%b handle=%h lm_ready=%b inspect=%b",
                        norm_result_valid, norm_result_handle,
                        lm_ready, tail_inspect_valid);
                    failures = failures + 1;
                end
            end
            norm_result_ready = 1'b1;
            @(negedge clk);
            norm_result_ready = 1'b0;
            if (exercise_reply_backpressure) begin
                activation_pause = 1'b1;
                while (!tail_inspect_rsp_valid) @(negedge clk);
                held_activation = tail_inspect_rsp_data;
                repeat (4) begin
                    @(negedge clk);
                    if (!tail_inspect_rsp_valid
                            || tail_inspect_rsp_data !== held_activation) begin
                        $display("FAIL normalized resident reply changed under backpressure");
                        failures = failures + 1;
                    end
                end
                activation_pause = 1'b0;
            end
        end
    endtask

    task automatic feed_bad_head_tag;
        begin
            while (!lm_ready) @(negedge clk);
            lm_row = expected_lm_row;
            lm_block = expected_lm_block + 5'd1;
            lm_weight = sparse_weight_block(-8'sd1);
            lm_valid = 1'b1;
            @(negedge clk);
            lm_valid = 1'b0;
        end
    endtask

    task automatic feed_production_head;
        reg signed [7:0] q;
        begin
            tie_score = 64'sd0;
            for (row_number = 0; row_number < PRODUCTION_ROWS;
                    row_number = row_number + 1) begin
                for (block_number = 0; block_number < 32;
                        block_number = block_number + 1) begin
                    while (!lm_ready) @(negedge clk);
                    if (expected_lm_row !== row_number[31:0]
                            || expected_lm_block !== block_number[4:0]) begin
                        $display("FAIL lm expected=%0d/%0d got=%0d/%0d",
                            row_number, block_number,
                            expected_lm_row, expected_lm_block);
                        failures = failures + 1;
                    end
                    if ((row_number == 0 && block_number == 0)
                            || (row_number == 42 && block_number == 0)
                            || (row_number == PRODUCTION_ROWS - 1
                                && block_number == 31)) begin
                        repeat (2) begin
                            @(negedge clk);
                            if (!lm_ready
                                    || expected_lm_row !== row_number[31:0]
                                    || expected_lm_block
                                        !== block_number[4:0]) begin
                                $display("FAIL lm request changed under source stall expected=%0d/%0d got=%0d/%0d",
                                    row_number, block_number,
                                    expected_lm_row, expected_lm_block);
                                failures = failures + 1;
                            end
                        end
                    end
                    lm_row = row_number[31:0];
                    lm_block = block_number[4:0];
                    if (block_number == 0) begin
                        q = (row_number == 42 || row_number == 43)
                            ? -8'sd1 : -8'sd3;
                        lm_weight = sparse_weight_block(q);
                    end else begin
                        lm_weight = sparse_weight_block(8'sd0);
                    end
                    lm_valid = 1'b1;
                    @(negedge clk);
                    lm_valid = 1'b0;
                end
                while (!lm_row_done) @(negedge clk);
                if (lm_row_error || retired_row !== row_number[31:0]) begin
                    $display("FAIL row retire row=%0d got=%0d error=%b",
                        row_number, retired_row, lm_row_error);
                    failures = failures + 1;
                end
                if (row_number == 42)
                    tie_score = retired_score;
                if (row_number == 43 && retired_score !== tie_score) begin
                    $display("FAIL tie row scores differ first=%0d second=%0d",
                        tie_score, retired_score);
                    failures = failures + 1;
                end
                if ((row_number & 16383) == 16383)
                    $display("resident_decode_tail lm_rows=%0d/%0d",
                        row_number + 1, PRODUCTION_ROWS);
            end
        end
    endtask

    always @(posedge clk) begin
        cycles <= cycles + 1;
        if (cycles > 100_000_000) begin
            $display("FAIL resident_decode_tail timeout state=%0d head_state=%0d rows=%0d",
                dut.state, dut.tied_lm_head.state, result_rows);
            $fatal(1);
        end
    end

    initial begin
        repeat (5) @(negedge clk);
        reset_n = 1'b1;
        repeat (2) @(negedge clk);
        establish_source();

        // Typed-handle domain is checked before the shared engine is touched.
        source_handle = {EPOCH, 1'b1, 4'd2};
        pulse_start();
        consume_result(1'b1, 8'd1);
        source_handle = SOURCE_HANDLE;

        // Only canonical low-half BF16 and exact index order are accepted.
        pulse_start();
        while (!norm_ready) @(negedge clk);
        norm_index = 10'd0;
        norm_bf16 = 1'b0;
        norm_bits = 32'h3f800000;
        norm_valid = 1'b1;
        @(negedge clk);
        norm_valid = 1'b0;
        consume_result(1'b1, 8'd2);
        if (!poisoned || start_ready) begin
            $display("FAIL BF16 domain violation did not poison lane");
            failures = failures + 1;
        end
        pulse_clear();
        if (poisoned || !start_ready) begin
            $display("FAIL clear did not restore poisoned lane");
            failures = failures + 1;
        end

        // Abort owns and drains the resident RMSNorm result before completing.
        pulse_start();
        for (weight_number = 0; weight_number < 9;
                weight_number = weight_number + 1) begin
            while (!norm_ready) @(negedge clk);
            norm_index = weight_number[9:0];
            norm_bf16 = 1'b1;
            norm_bits = 32'h00003f80;
            norm_valid = 1'b1;
            @(negedge clk);
            norm_valid = 1'b0;
        end
        abort = 1'b1;
        @(negedge clk);
        abort = 1'b0;
        consume_result(1'b1, 8'd6);
        if (poisoned) begin
            $display("FAIL cooperative abort poisoned lane");
            failures = failures + 1;
        end

        // A wrong row response tag poisons the tied-head lane; clear is its
        // sole recovery path.  This also exercises a complete resident RMS.
        pulse_start();
        feed_full_norm(1'b1);
        feed_bad_head_tag();
        consume_result(1'b1, 8'd5);
        if (!poisoned || start_ready) begin
            $display("FAIL head order violation did not poison lane");
            failures = failures + 1;
        end
        pulse_clear();

        // Full production scan.  All scores are negative.  Rows 42 and 43
        // tie exactly for the maximum, proving signed-i64 comparison and the
        // deterministic earliest-index rule without exporting a host tensor.
        pulse_start();
        feed_full_norm(1'b1);
        feed_production_head();
        while (!result_valid) @(negedge clk);
        if (result_error || result_error_code != 8'd0
                || result_rows != PRODUCTION_ROWS
                || result_token != 32'd42
                || result_score !== tie_score || tie_score >= 0) begin
            $display("FAIL production result error=%b code=%0d rows=%0d token=%0d score=%0d tie=%0d",
                result_error, result_error_code, result_rows,
                result_token, result_score, tie_score);
            failures = failures + 1;
        end
        consume_result(1'b0, 8'd0);

        if (failures == 0) begin
            $display("PASS resident_decode_tail typed_q30 final_rms_bf16=1024 norm_publication=explicit resident_q8_blocks=32 tied_lm_rows=%0d signed_i64_argmax=negative earliest_tie_token42 stalls=stable abort=drained poison_reset=explicit output=token_only no_host_tensor",
                PRODUCTION_ROWS);
            $finish;
        end
        $display("FAIL resident_decode_tail failures=%0d", failures);
        $fatal(1);
    end
endmodule
