`timescale 1ns/1ps

module truega_lfm25_resident_attention_join_tb;
    localparam [31:0] EPOCH = 32'ha770_0001;
    localparam [36:0] STREAM_HANDLE = {EPOCH, 1'b1, 4'd3};
    localparam [36:0] EMBEDDING_HANDLE = {EPOCH, 1'b0, 4'd0};
    localparam [36:0] SOURCE_HANDLE = {EPOCH, 1'b1, 4'd0};
    localparam [36:0] DESTINATION_ONE = {EPOCH, 1'b0, 4'd1};
    localparam [36:0] DESTINATION_TWO = {EPOCH, 1'b0, 4'd2};

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg clear = 1'b0;
    reg abort = 1'b0;
    reg start = 1'b0;
    wire start_ready;
    reg [36:0] destination_handle = DESTINATION_ONE;
    reg [3:0] layer = 4'd2;
    reg [16:0] token_position = 17'd0;

    reg norm_valid = 1'b0;
    wire norm_ready;
    wire norm_expected_key;
    wire [5:0] norm_expected_element;
    reg norm_key = 1'b0;
    reg [5:0] norm_element = 6'd0;
    reg [15:0] norm_bf16 = 16'h3f80;

    reg projection_weight_valid = 1'b0;
    wire projection_weight_ready;
    wire [1:0] projection_expected_kind;
    wire [12:0] projection_expected_row;
    wire [4:0] projection_expected_block;
    reg [1:0] projection_kind = 2'd0;
    reg [12:0] projection_row = 13'd0;
    reg [4:0] projection_block = 5'd0;
    reg [271:0] projection_weight = 272'd0;
    reg core_valid = 1'b0;
    wire core_ready;
    wire core_done;
    reg import_pause = 1'b0;
    wire projection_output_valid;
    wire [12:0] projection_output_row;
    wire signed [63:0] projection_output_q30;

    wire result_valid;
    reg result_ready = 1'b0;
    wire result_error;
    wire [7:0] result_error_code;
    wire [36:0] result_handle;
    reg output_read_valid = 1'b0;
    wire output_read_ready;
    reg [9:0] output_read_index = 10'd0;
    wire output_read_rsp_valid;
    reg output_read_rsp_ready = 1'b0;
    wire output_read_error;
    wire signed [63:0] output_read_q30;

    wire join_command_valid, join_command_ready;
    wire [1:0] join_command_operation;
    wire [36:0] join_command_source0, join_command_source1;
    wire [36:0] join_command_destination;
    wire join_result_valid, join_result_ready, join_result_error;
    wire [36:0] join_result_handle;
    wire join_resident_abort;
    wire join_inspect_valid, join_inspect_ready;
    wire [36:0] join_inspect_handle;
    wire [9:0] join_inspect_index;
    wire join_inspect_rsp_valid, join_inspect_rsp_ready;
    wire join_inspect_rsp_error;
    wire [271:0] join_inspect_rsp_data;
    wire join_import_valid, join_import_ready;
    wire [9:0] join_import_index;
    wire signed [63:0] join_import_q30;
    wire [10:0] q_rows;
    wire [9:0] k_rows, v_rows;
    wire [12:0] out_rows;
    wire [10:0] imports;
    wire poisoned, busy;

    reg setup_owner = 1'b1;
    reg setup_command_valid = 1'b0;
    wire setup_command_ready;
    reg [1:0] setup_command_operation = 2'd0;
    reg [36:0] setup_command_source0 = 37'd0;
    reg [36:0] setup_command_destination = 37'd0;
    reg setup_result_ready = 1'b0;
    wire setup_result_valid;
    reg embedding_valid = 1'b0;
    wire embedding_ready;
    reg [4:0] embedding_index = 5'd0;
    reg [271:0] embedding_block = 272'd0;
    reg setup_norm_valid = 1'b0;
    wire setup_norm_ready;
    reg [9:0] setup_norm_index = 10'd0;
    reg setup_norm_bf16 = 1'b0;
    reg [31:0] setup_norm_bits = 32'd0;

    wire resident_command_valid = setup_owner
        ? setup_command_valid : join_command_valid;
    wire [1:0] resident_command_operation = setup_owner
        ? setup_command_operation : join_command_operation;
    wire [36:0] resident_command_source0 = setup_owner
        ? setup_command_source0 : join_command_source0;
    wire [36:0] resident_command_destination = setup_owner
        ? setup_command_destination : join_command_destination;
    wire resident_command_ready;
    wire resident_result_valid;
    wire resident_result_ready = setup_owner
        ? setup_result_ready : join_result_ready;
    wire resident_result_error;
    wire [36:0] resident_result_handle;

    assign setup_command_ready = setup_owner && resident_command_ready;
    assign setup_result_valid = setup_owner && resident_result_valid;
    assign join_command_ready = !setup_owner && resident_command_ready;
    assign join_result_valid = !setup_owner && resident_result_valid;
    assign join_result_error = resident_result_error;
    assign join_result_handle = resident_result_handle;

    integer failures = 0;
    integer setup_number;
    integer row_number;
    integer block_number;
    integer cycles = 0;
    reg signed [63:0] imported0;
    reg signed [63:0] imported1;
    reg signed [63:0] imported1023;
    reg signed [63:0] held_projection;
    reg [12:0] held_projection_row;
    reg [36:0] held_result_handle;
    reg [7:0] held_result_code;

    always #5 clk = ~clk;

    function automatic [271:0] constant_native_block;
        input [15:0] scale_f16;
        input signed [7:0] quant_value;
        integer quant_index;
        reg [271:0] value;
        begin
            value = 272'd0;
            value[15:0] = scale_f16;
            for (quant_index = 0; quant_index < 32;
                 quant_index = quant_index + 1)
                value[16 + quant_index * 8 +: 8] = quant_value;
            constant_native_block = value;
        end
    endfunction

    function automatic [271:0] model_projection_block;
        input [1:0] requested_kind;
        input integer requested_row;
        input integer requested_block;
        reg [271:0] value;
        begin
            value = 272'd0;
            if (requested_block == 0) begin
                value[15:0] = 16'h3c00;
                if (requested_kind != 2'd3)
                    value[23:16] = 8'h01;
                else begin
                    case (requested_row)
                        0: value[23:16] = 8'h01;
                        1: value[23:16] = 8'hff;
                        1023: begin
                            value[15:0] = 16'h3800;
                            value[23:16] = 8'h01;
                        end
                        default: value = 272'd0;
                    endcase
                end
            end
            model_projection_block = value;
        end
    endfunction

    truega_lfm25_resident_attention_join dut (
        .clk(clk), .reset_n(reset_n), .clear_i(clear), .abort_i(abort),
        .start_i(start), .start_ready_o(start_ready),
        .source_q8_handle_i(SOURCE_HANDLE),
        .destination_q30_handle_i(destination_handle),
        .layer_i(layer), .token_position_i(token_position),
        .norm_weight_valid_i(norm_valid), .norm_weight_ready_o(norm_ready),
        .norm_weight_key_o(norm_expected_key),
        .norm_weight_element_o(norm_expected_element),
        .norm_weight_key_i(norm_key), .norm_weight_element_i(norm_element),
        .norm_weight_bf16_i(norm_bf16),
        .projection_weight_valid_i(projection_weight_valid),
        .projection_weight_ready_o(projection_weight_ready),
        .projection_weight_kind_o(projection_expected_kind),
        .projection_weight_row_o(projection_expected_row),
        .projection_weight_block_o(projection_expected_block),
        .projection_weight_kind_i(projection_kind),
        .projection_weight_row_i(projection_row),
        .projection_weight_block_i(projection_block),
        .projection_weight_q8_block_i(projection_weight),
        .core_control_valid_i(core_valid),
        .core_control_ready_o(core_ready),
        .core_control_done_o(core_done), .import_pause_i(import_pause),
        .projection_output_valid_o(projection_output_valid),
        .projection_output_row_o(projection_output_row),
        .projection_output_q30_o(projection_output_q30),
        .result_valid_o(result_valid), .result_ready_i(result_ready),
        .result_error_o(result_error),
        .result_error_code_o(result_error_code),
        .result_handle_o(result_handle),
        .output_read_valid_i(output_read_valid),
        .output_read_ready_o(output_read_ready),
        .output_read_index_i(output_read_index),
        .output_read_rsp_valid_o(output_read_rsp_valid),
        .output_read_rsp_ready_i(output_read_rsp_ready),
        .output_read_error_o(output_read_error),
        .output_read_q30_o(output_read_q30),
        .resident_command_valid_o(join_command_valid),
        .resident_command_ready_i(join_command_ready),
        .resident_command_operation_o(join_command_operation),
        .resident_command_source0_handle_o(join_command_source0),
        .resident_command_source1_handle_o(join_command_source1),
        .resident_command_destination_handle_o(join_command_destination),
        .resident_result_valid_i(join_result_valid),
        .resident_result_ready_o(join_result_ready),
        .resident_result_error_i(join_result_error),
        .resident_result_handle_i(join_result_handle),
        .resident_abort_o(join_resident_abort),
        .resident_inspect_valid_o(join_inspect_valid),
        .resident_inspect_ready_i(join_inspect_ready),
        .resident_inspect_handle_o(join_inspect_handle),
        .resident_inspect_index_o(join_inspect_index),
        .resident_inspect_rsp_valid_i(join_inspect_rsp_valid),
        .resident_inspect_rsp_ready_o(join_inspect_rsp_ready),
        .resident_inspect_rsp_error_i(join_inspect_rsp_error),
        .resident_inspect_rsp_data_i(join_inspect_rsp_data),
        .resident_import_valid_o(join_import_valid),
        .resident_import_ready_i(join_import_ready),
        .resident_import_index_o(join_import_index),
        .resident_import_q30_o(join_import_q30),
        .query_rows_retired_o(q_rows), .key_rows_retired_o(k_rows),
        .value_rows_retired_o(v_rows), .output_rows_retired_o(out_rows),
        .import_elements_completed_o(imports), .poisoned_o(poisoned),
        .busy_o(busy)
    );

    truega_lfm25_resident_vector_engine resident (
        .clk(clk), .reset_n(reset_n && !clear),
        .abort_i(!setup_owner && join_resident_abort),
        .command_valid_i(resident_command_valid),
        .command_ready_o(resident_command_ready),
        .command_operation_i(resident_command_operation),
        .command_source0_handle_i(resident_command_source0),
        .command_source1_handle_i(setup_owner ? 37'd0 : join_command_source1),
        .command_destination_handle_i(resident_command_destination),
        .embedding_block_valid_i(setup_owner && embedding_valid),
        .embedding_block_ready_o(embedding_ready),
        .embedding_block_index_i(embedding_index),
        .embedding_q8_block_i(embedding_block),
        .weight_valid_i(setup_owner && setup_norm_valid),
        .weight_ready_o(setup_norm_ready),
        .weight_index_i(setup_norm_index),
        .weight_format_bf16_i(setup_norm_bf16),
        .weight_bits_i(setup_norm_bits),
        .import_valid_i(!setup_owner && join_import_valid),
        .import_ready_o(join_import_ready),
        .import_index_i(join_import_index), .import_q30_i(join_import_q30),
        .result_valid_o(resident_result_valid),
        .result_ready_i(resident_result_ready),
        .result_error_o(resident_result_error),
        .result_handle_o(resident_result_handle),
        .inspect_valid_i(!setup_owner && join_inspect_valid),
        .inspect_ready_o(join_inspect_ready),
        .inspect_handle_i(join_inspect_handle),
        .inspect_index_i(join_inspect_index),
        .inspect_rsp_valid_o(join_inspect_rsp_valid),
        .inspect_rsp_ready_i(join_inspect_rsp_ready),
        .inspect_rsp_error_o(join_inspect_rsp_error),
        .inspect_rsp_data_o(join_inspect_rsp_data),
        .session_epoch_o(), .busy_o()
    );

    task automatic setup_command;
        input [1:0] operation;
        input [36:0] source0;
        input [36:0] destination;
        begin
            setup_command_operation = operation;
            setup_command_source0 = source0;
            setup_command_destination = destination;
            setup_command_valid = 1'b1;
            while (!setup_command_ready) @(negedge clk);
            @(negedge clk);
            setup_command_valid = 1'b0;
        end
    endtask

    task automatic setup_expect_result;
        input [36:0] expected_handle;
        begin
            while (!setup_result_valid) @(negedge clk);
            if (resident_result_error || resident_result_handle !== expected_handle)
                failures = failures + 1;
            setup_result_ready = 1'b1;
            @(negedge clk);
            setup_result_ready = 1'b0;
        end
    endtask

    task automatic establish_resident_q8;
        begin
            setup_owner = 1'b1;
            setup_command(2'd0, STREAM_HANDLE, EMBEDDING_HANDLE);
            for (setup_number = 0; setup_number < 32;
                 setup_number = setup_number + 1) begin
                while (!embedding_ready) @(negedge clk);
                embedding_index = setup_number[4:0];
                embedding_block = constant_native_block(16'h3800, 8'sd2);
                embedding_valid = 1'b1;
                @(negedge clk);
                embedding_valid = 1'b0;
            end
            setup_expect_result(EMBEDDING_HANDLE);
            setup_command(2'd1, EMBEDDING_HANDLE, SOURCE_HANDLE);
            for (setup_number = 0; setup_number < 1024;
                 setup_number = setup_number + 1) begin
                while (!setup_norm_ready) @(negedge clk);
                setup_norm_index = setup_number[9:0];
                setup_norm_bf16 = setup_number[0];
                setup_norm_bits = setup_number[0]
                    ? 32'h00003f80 : 32'h3f800000;
                setup_norm_valid = 1'b1;
                @(negedge clk);
                setup_norm_valid = 1'b0;
            end
            setup_expect_result(SOURCE_HANDLE);
            setup_owner = 1'b0;
            @(negedge clk);
            $display("resident_attention_join source=resident-q8 ready");
        end
    endtask

    task automatic pulse_start;
        input [3:0] requested_layer;
        input [16:0] requested_position;
        input [36:0] requested_destination;
        begin
            layer = requested_layer;
            token_position = requested_position;
            destination_handle = requested_destination;
            while (!start_ready) @(negedge clk);
            start = 1'b1;
            @(negedge clk);
            start = 1'b0;
        end
    endtask

    task automatic feed_norm_weights;
        integer weight_number;
        begin
            for (weight_number = 0; weight_number < 128;
                 weight_number = weight_number + 1) begin
                while (!norm_ready) @(negedge clk);
                if (norm_expected_key !== (weight_number >= 64)
                        || norm_expected_element !== weight_number[5:0])
                    failures = failures + 1;
                norm_key = weight_number >= 64;
                norm_element = weight_number[5:0];
                norm_bf16 = 16'h3f80;
                norm_valid = 1'b1;
                @(negedge clk);
                norm_valid = 1'b0;
            end
        end
    endtask

    task automatic feed_projection;
        input [1:0] requested_kind;
        input integer requested_rows;
        begin
            // Sealed GQA is 1024 Q, 512 K, 512 V.  These assertions make a
            // silent 1024-row widening of K/V fail at the first extra row.
            if ((requested_kind == 2'd1 || requested_kind == 2'd2)
                    && requested_rows != 512)
                failures = failures + 1;
            for (row_number = 0; row_number < requested_rows;
                 row_number = row_number + 1) begin
                for (block_number = 0; block_number < 32;
                     block_number = block_number + 1) begin
                    while (!projection_weight_ready) @(negedge clk);
                    if (projection_expected_kind !== requested_kind
                            || projection_expected_row !== row_number[12:0]
                            || projection_expected_block !== block_number[4:0])
                        failures = failures + 1;
                    projection_kind = requested_kind;
                    projection_row = row_number[12:0];
                    projection_block = block_number[4:0];
                    projection_weight = model_projection_block(
                        requested_kind, row_number, block_number);
                    projection_weight_valid = 1'b1;
                    @(negedge clk);
                    projection_weight_valid = 1'b0;
                end
                if ((row_number & 255) == 255)
                    $display("resident_attention_join kind=%0d rows=%0d/%0d",
                        requested_kind, row_number + 1, requested_rows);
            end
        end
    endtask

    task automatic feed_through_core;
        begin
            feed_norm_weights();
            feed_projection(2'd0, 1024);
            feed_projection(2'd1, 512);
            feed_projection(2'd2, 512);
            $display("resident_attention_join gqa projections complete q=1024 k=512 v=512");
            while (!core_ready) @(negedge clk);
            core_valid = 1'b1;
            @(negedge clk);
            core_valid = 1'b0;
            while (!core_done) @(negedge clk);
            $display("resident_attention_join first-token core complete q8_blocks=32");
            if (q_rows != 11'd1024 || k_rows != 10'd512
                    || v_rows != 10'd512)
                failures = failures + 1;
        end
    endtask

    task automatic inspect_output;
        input [9:0] requested_index;
        input expected_error;
        input signed [63:0] expected_q30;
        reg signed [63:0] held_q30;
        begin
            output_read_index = requested_index;
            output_read_valid = 1'b1;
            while (!output_read_ready) @(negedge clk);
            @(negedge clk);
            output_read_valid = 1'b0;
            while (!output_read_rsp_valid) @(negedge clk);
            held_q30 = output_read_q30;
            repeat (3) begin
                @(negedge clk);
                if (!output_read_rsp_valid || output_read_q30 !== held_q30)
                    failures = failures + 1;
            end
            if (output_read_error !== expected_error)
                failures = failures + 1;
            if (!expected_error && held_q30 !== expected_q30)
                failures = failures + 1;
            output_read_rsp_ready = 1'b1;
            @(negedge clk);
            output_read_rsp_ready = 1'b0;
        end
    endtask

    task automatic consume_result;
        input expected_error;
        input [7:0] expected_code;
        input [36:0] expected_handle;
        begin
            while (!result_valid) @(negedge clk);
            held_result_handle = result_handle;
            held_result_code = result_error_code;
            repeat (3) begin
                @(negedge clk);
                if (!result_valid || result_handle !== held_result_handle
                        || result_error_code !== held_result_code)
                    failures = failures + 1;
            end
            if (result_error !== expected_error
                    || result_error_code !== expected_code
                    || result_handle !== expected_handle)
                failures = failures + 1;
            result_ready = 1'b1;
            @(negedge clk);
            result_ready = 1'b0;
        end
    endtask

    always @(posedge clk) begin
        if (join_import_valid && join_import_ready) begin
            case (join_import_index)
                10'd0: imported0 <= join_import_q30;
                10'd1: imported1 <= join_import_q30;
                10'd1023: imported1023 <= join_import_q30;
                default: begin end
            endcase
        end
    end

    always @(posedge clk) begin
        cycles <= cycles + 1;
        if (cycles > 100_000_000) begin
            $display("FAIL resident_attention_join timeout state=%0d q=%0d k=%0d v=%0d out=%0d import=%0d",
                dut.state, q_rows, k_rows, v_rows, out_rows, imports);
            $fatal(1);
        end
    end

    initial begin
        imported0 = 64'sd0;
        imported1 = 64'sd0;
        imported1023 = 64'sd0;
        repeat (5) @(negedge clk);
        reset_n = 1'b1;
        repeat (2) @(negedge clk);
        establish_resident_q8();

        // Complete first-token attention with exact 16:8 GQA dimensions.
        pulse_start(4'd2, 17'd0, DESTINATION_ONE);
        feed_through_core();
        fork : full_output
            begin
                feed_projection(2'd3, 1024);
            end
            begin
                while (!projection_output_valid) @(negedge clk);
                held_projection_row = projection_output_row;
                held_projection = projection_output_q30;
                import_pause = 1'b1;
                repeat (3) begin
                    @(negedge clk);
                    if (!projection_output_valid
                            || projection_output_row !== held_projection_row
                            || projection_output_q30 !== held_projection)
                        failures = failures + 1;
                end
                import_pause = 1'b0;
            end
        join
        while (!result_valid) @(negedge clk);
        if (out_rows != 13'd1024 || imports != 11'd1024
                || imported0 <= 64'sd0 || imported1 >= 64'sd0
                || imported1023 <= 64'sd0)
            failures = failures + 1;
        inspect_output(10'd0, 1'b0, imported0);
        inspect_output(10'd1, 1'b0, imported1);
        inspect_output(10'd1023, 1'b0, imported1023);
        consume_result(1'b0, 8'd0, DESTINATION_ONE);

        // A second valid attention layer advances only its own first-token
        // cache.  Abort after physical destination writes begin.  The join is
        // poisoned and the partially replaced destination remains unpublished.
        pulse_start(4'd5, 17'd0, DESTINATION_TWO);
        feed_through_core();
        fork : aborted_output
            begin
                feed_projection(2'd3, 1024);
            end
            begin
                while (imports < 11'd17) @(negedge clk);
                abort = 1'b1;
                @(negedge clk);
                abort = 1'b0;
            end
        join_any
        disable aborted_output;
        $display("resident_attention_join abort issued imports=%0d", imports);
        consume_result(1'b1, 8'd9, 37'd0);
        $display("resident_attention_join abort result consumed poison=%b", poisoned);
        if (!poisoned || start_ready)
            failures = failures + 1;
        inspect_output(10'd0, 1'b1, 64'sd0);
        $display("resident_attention_join aborted destination unpublished");

        // Clear is the only cache-safe poison recovery.  Re-establish the
        // source, then prove a wrong projection mode is rejected immediately.
        clear = 1'b1;
        @(negedge clk);
        clear = 1'b0;
        repeat (2) @(negedge clk);
        establish_resident_q8();
        $display("resident_attention_join poison clear and source reload complete");
        pulse_start(4'd8, 17'd0, DESTINATION_ONE);
        feed_norm_weights();
        while (!projection_weight_ready) @(negedge clk);
        if (projection_expected_kind != 2'd0
                || projection_expected_row != 13'd0
                || projection_expected_block != 5'd0)
            failures = failures + 1;
        projection_kind = 2'd1;
        projection_row = 13'd0;
        projection_block = 5'd0;
        projection_weight = model_projection_block(2'd1, 0, 0);
        projection_weight_valid = 1'b1;
        @(negedge clk);
        projection_weight_valid = 1'b0;
        consume_result(1'b1, 8'd5, 37'd0);

        // Position zero is a hard circuit contract, not a runtime cache mode.
        clear = 1'b1;
        @(negedge clk);
        clear = 1'b0;
        repeat (2) @(negedge clk);
        pulse_start(4'd2, 17'd1, DESTINATION_ONE);
        consume_result(1'b1, 8'd2, 37'd0);

        if (failures == 0)
            $display("PASS resident_attention_join resident_q8 exact_gqa_q1024_k512_v512 qk_norm=128 control_only_core=1 attention_q8_blocks=32 output_rows=1024 signed_witnesses backpressure=stable transactional_abort=unpublished strict_mode_position poison_clear_required no_host_math no_runtime_graph");
        else begin
            $display("FAIL resident_attention_join failures=%0d", failures);
            $fatal(1);
        end
        $finish;
    end
endmodule
