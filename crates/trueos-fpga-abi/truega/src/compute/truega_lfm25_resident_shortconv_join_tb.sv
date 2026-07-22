`timescale 1ns/1ps

module truega_lfm25_resident_shortconv_join_tb;
    localparam [31:0] EPOCH = 32'h5c00_0001;
    localparam [36:0] STREAM_HANDLE = {EPOCH, 1'b1, 4'd3};
    localparam [36:0] EMBEDDING_HANDLE = {EPOCH, 1'b0, 4'd0};
    localparam [36:0] SOURCE_HANDLE = {EPOCH, 1'b1, 4'd0};
    localparam [36:0] DESTINATION_ONE = {EPOCH, 1'b0, 4'd1};
    localparam [36:0] DESTINATION_TWO = {EPOCH, 1'b0, 4'd2};

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg clear = 1'b0;
    reg abort = 1'b0;
    reg layer_reset = 1'b0;
    reg [3:0] layer_reset_slot = 4'd3;
    wire layer_reset_ready;
    wire layer_reset_done;
    reg start = 1'b0;
    wire start_ready;
    reg [36:0] destination_handle = DESTINATION_ONE;
    reg [3:0] layer_slot = 4'd3;
    reg [31:0] token_position = 32'd0;

    reg triplet_valid = 1'b0;
    wire triplet_ready;
    wire [9:0] triplet_channel;
    wire [4:0] triplet_block;
    reg [271:0] triplet_b = 272'd0;
    reg [271:0] triplet_c = 272'd0;
    reg [271:0] triplet_x = 272'd0;
    reg [15:0] kernel_oldest = 16'h3f80;
    reg [15:0] kernel_newest = 16'h3f80;
    reg [15:0] kernel_current = 16'h3f80;

    reg projection_weight_valid = 1'b0;
    wire projection_weight_ready;
    wire [12:0] projection_weight_row_expected;
    wire [4:0] projection_weight_block_expected;
    reg [12:0] projection_weight_row = 13'd0;
    reg [4:0] projection_weight_block = 5'd0;
    reg [271:0] projection_weight = 272'd0;
    reg import_pause = 1'b0;
    wire projection_output_valid;
    wire [12:0] projection_output_row;
    wire signed [63:0] projection_output_q30;
    wire shortconv_output_accept;
    wire [4:0] shortconv_output_index;
    wire [271:0] shortconv_output_block;

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

    wire join_command_valid;
    wire join_command_ready;
    wire [1:0] join_command_operation;
    wire [36:0] join_command_source0;
    wire [36:0] join_command_source1;
    wire [36:0] join_command_destination;
    wire join_result_valid;
    wire join_result_ready;
    wire join_result_error;
    wire [36:0] join_result_handle;
    wire join_resident_abort;
    wire join_inspect_valid;
    wire join_inspect_ready;
    wire [36:0] join_inspect_handle;
    wire [9:0] join_inspect_index;
    wire join_inspect_rsp_valid;
    wire join_inspect_rsp_ready;
    wire join_inspect_rsp_error;
    wire [271:0] join_inspect_rsp_data;
    wire join_import_valid;
    wire join_import_ready;
    wire [9:0] join_import_index;
    wire signed [63:0] join_import_q30;
    wire [10:0] shortconv_channels;
    wire [12:0] projection_rows;
    wire [10:0] import_elements;
    wire busy;

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
    reg norm_weight_valid = 1'b0;
    wire norm_weight_ready;
    reg [9:0] norm_weight_index = 10'd0;
    reg norm_weight_bf16 = 1'b0;
    reg [31:0] norm_weight_bits = 32'd0;

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
    integer channel_number;
    integer block_number;
    integer row_number;
    integer cycles = 0;
    reg [15:0] position0_shortconv_scale;
    reg [15:0] position1_shortconv_scale;
    reg signed [63:0] imported0;
    reg signed [63:0] imported1;
    reg signed [63:0] imported1023;
    reg signed [63:0] position0_q30;
    reg signed [63:0] position1_q30;
    reg signed [63:0] held_projection_q30;
    reg [12:0] held_projection_row;
    reg [36:0] held_result_handle;
    reg [7:0] held_result_code;

    always #5 clk = ~clk;

    function automatic [271:0] sparse_block;
        input [4:0] requested_block;
        reg [271:0] value;
        begin
            value = 272'd0;
            value[15:0] = 16'h3c00;
            if (requested_block == 5'd0)
                value[23:16] = 8'h01;
            sparse_block = value;
        end
    endfunction

    function automatic [271:0] signed_row_weight;
        input integer requested_row;
        input integer requested_block;
        reg [271:0] value;
        begin
            value = 272'd0;
            if (requested_block == 0) begin
                value[15:0] = 16'h3c00;
                case (requested_row)
                    0: value[23:16] = 8'h01;
                    1: value[23:16] = 8'hff;
                    1023: begin
                        value[15:0] = 16'h3800;
                        value[23:16] = 8'h01;
                    end
                    default: begin end
                endcase
            end
            signed_row_weight = value;
        end
    endfunction

    truega_lfm25_resident_shortconv_join dut (
        .clk(clk), .reset_n(reset_n), .clear_i(clear), .abort_i(abort),
        .layer_reset_i(layer_reset),
        .layer_reset_slot_i(layer_reset_slot),
        .layer_reset_ready_o(layer_reset_ready),
        .layer_reset_done_o(layer_reset_done),
        .start_i(start), .start_ready_o(start_ready),
        .source_q8_handle_i(SOURCE_HANDLE),
        .destination_q30_handle_i(destination_handle),
        .layer_slot_i(layer_slot), .token_position_i(token_position),
        .triplet_valid_i(triplet_valid),
        .triplet_ready_o(triplet_ready),
        .triplet_channel_o(triplet_channel),
        .triplet_block_o(triplet_block),
        .triplet_b_q8_block_i(triplet_b),
        .triplet_c_q8_block_i(triplet_c),
        .triplet_x_q8_block_i(triplet_x),
        .kernel_oldest_bf16_i(kernel_oldest),
        .kernel_newest_bf16_i(kernel_newest),
        .kernel_current_bf16_i(kernel_current),
        .projection_weight_valid_i(projection_weight_valid),
        .projection_weight_ready_o(projection_weight_ready),
        .projection_weight_row_o(projection_weight_row_expected),
        .projection_weight_block_o(projection_weight_block_expected),
        .projection_weight_row_i(projection_weight_row),
        .projection_weight_block_i(projection_weight_block),
        .projection_weight_q8_block_i(projection_weight),
        .import_pause_i(import_pause),
        .projection_output_valid_o(projection_output_valid),
        .projection_output_row_o(projection_output_row),
        .projection_output_q30_o(projection_output_q30),
        .shortconv_output_accept_o(shortconv_output_accept),
        .shortconv_output_block_index_o(shortconv_output_index),
        .shortconv_output_q8_block_o(shortconv_output_block),
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
        .shortconv_channels_retired_o(shortconv_channels),
        .projection_rows_retired_o(projection_rows),
        .import_elements_completed_o(import_elements),
        .busy_o(busy)
    );

    truega_lfm25_resident_vector_engine resident (
        .clk(clk), .reset_n(reset_n && !clear),
        .abort_i(!setup_owner && join_resident_abort),
        .command_valid_i(resident_command_valid),
        .command_ready_o(resident_command_ready),
        .command_operation_i(resident_command_operation),
        .command_source0_handle_i(resident_command_source0),
        .command_source1_handle_i(setup_owner ? 37'd0
            : join_command_source1),
        .command_destination_handle_i(resident_command_destination),
        .embedding_block_valid_i(setup_owner && embedding_valid),
        .embedding_block_ready_o(embedding_ready),
        .embedding_block_index_i(embedding_index),
        .embedding_q8_block_i(embedding_block),
        .weight_valid_i(setup_owner && norm_weight_valid),
        .weight_ready_o(norm_weight_ready),
        .weight_index_i(norm_weight_index),
        .weight_format_bf16_i(norm_weight_bf16),
        .weight_bits_i(norm_weight_bits),
        .import_valid_i(!setup_owner && join_import_valid),
        .import_ready_o(join_import_ready),
        .import_index_i(join_import_index),
        .import_q30_i(join_import_q30),
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
            if (resident_result_error
                    || resident_result_handle !== expected_handle)
                failures = failures + 1;
            setup_result_ready = 1'b1;
            @(negedge clk);
            setup_result_ready = 1'b0;
        end
    endtask

    task automatic establish_resident_q8;
        begin
            setup_command(2'd0, STREAM_HANDLE, EMBEDDING_HANDLE);
            for (setup_number = 0; setup_number < 32;
                 setup_number = setup_number + 1) begin
                while (!embedding_ready) @(negedge clk);
                embedding_index = setup_number[4:0];
                embedding_block = sparse_block(setup_number[4:0]);
                embedding_valid = 1'b1;
                @(negedge clk);
                embedding_valid = 1'b0;
            end
            setup_expect_result(EMBEDDING_HANDLE);
            setup_command(2'd1, EMBEDDING_HANDLE, SOURCE_HANDLE);
            for (setup_number = 0; setup_number < 1024;
                 setup_number = setup_number + 1) begin
                while (!norm_weight_ready) @(negedge clk);
                norm_weight_index = setup_number[9:0];
                norm_weight_bf16 = setup_number[0];
                norm_weight_bits = setup_number[0]
                    ? 32'h00003f80 : 32'h3f800000;
                norm_weight_valid = 1'b1;
                @(negedge clk);
                norm_weight_valid = 1'b0;
            end
            setup_expect_result(SOURCE_HANDLE);
            setup_owner = 1'b0;
            @(negedge clk);
        end
    endtask

    task automatic reset_shortconv_layer;
        begin
            while (!layer_reset_ready) @(negedge clk);
            layer_reset = 1'b1;
            @(negedge clk);
            if (!layer_reset_done)
                failures = failures + 1;
            layer_reset = 1'b0;
            @(negedge clk);
        end
    endtask

    task automatic begin_operation;
        input [31:0] requested_position;
        input [36:0] requested_destination;
        begin
            while (!start_ready) @(negedge clk);
            token_position = requested_position;
            destination_handle = requested_destination;
            start = 1'b1;
            @(negedge clk);
            start = 1'b0;
        end
    endtask

    task automatic feed_shortconv_rows;
        begin
            for (channel_number = 0; channel_number < 1024;
                 channel_number = channel_number + 1) begin
                for (block_number = 0; block_number < 32;
                     block_number = block_number + 1) begin
                    while (!triplet_ready) @(negedge clk);
                    if (triplet_channel !== channel_number[9:0]
                            || triplet_block !== block_number[4:0])
                        failures = failures + 1;
                    triplet_b = sparse_block(block_number[4:0]);
                    triplet_c = sparse_block(block_number[4:0]);
                    triplet_x = sparse_block(block_number[4:0]);
                    triplet_valid = 1'b1;
                    @(negedge clk);
                    triplet_valid = 1'b0;
                end
                if ((channel_number & 255) == 255)
                    $display("resident_shortconv channels=%0d/1024 pos=%0d",
                        channel_number + 1, token_position);
            end
        end
    endtask

    task automatic feed_projection_row;
        input integer requested_row;
        begin
            for (block_number = 0; block_number < 32;
                 block_number = block_number + 1) begin
                while (!projection_weight_ready) @(negedge clk);
                if (projection_weight_row_expected !== requested_row[12:0]
                        || projection_weight_block_expected
                            !== block_number[4:0])
                    failures = failures + 1;
                projection_weight_row = requested_row[12:0];
                projection_weight_block = block_number[4:0];
                projection_weight = signed_row_weight(requested_row,
                    block_number);
                projection_weight_valid = 1'b1;
                @(negedge clk);
                projection_weight_valid = 1'b0;
            end
        end
    endtask

    task automatic feed_projection_rows;
        input test_backpressure;
        begin
            if (test_backpressure)
                import_pause = 1'b1;
            feed_projection_row(0);
            if (test_backpressure) begin
                while (!projection_output_valid) @(negedge clk);
                held_projection_row = projection_output_row;
                held_projection_q30 = projection_output_q30;
                repeat (3) begin
                    @(negedge clk);
                    if (!projection_output_valid
                            || projection_output_row !== held_projection_row
                            || projection_output_q30 !== held_projection_q30)
                        failures = failures + 1;
                end
                import_pause = 1'b0;
            end
            for (row_number = 1; row_number < 1024;
                 row_number = row_number + 1) begin
                feed_projection_row(row_number);
                if ((row_number & 255) == 255)
                    $display("resident_shortconv projection=%0d/1024 pos=%0d",
                        row_number + 1, token_position);
            end
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

    task automatic execute_operation;
        input [31:0] requested_position;
        input test_backpressure;
        begin
            begin_operation(requested_position, DESTINATION_ONE);
            feed_shortconv_rows();
            feed_projection_rows(test_backpressure);
            while (!result_valid) @(negedge clk);
            if (shortconv_channels != 11'd1024
                    || projection_rows != 13'd1024
                    || import_elements != 11'd1024)
                failures = failures + 1;
        end
    endtask

    always @(posedge clk) begin
        if (shortconv_output_accept && shortconv_output_index == 5'd0) begin
            if (token_position == 32'd0)
                position0_shortconv_scale <= shortconv_output_block[15:0];
            else if (token_position == 32'd1)
                position1_shortconv_scale <= shortconv_output_block[15:0];
        end
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
        if (cycles > 30_000_000) begin
            $display("FAIL resident_shortconv timeout state=%0d sc_state=%0d proj_state=%0d channel=%0d row=%0d",
                dut.state, dut.shortconv.state, dut.output_projection.state,
                triplet_channel, projection_weight_row_expected);
            $fatal(1);
        end
    end

    initial begin
        position0_shortconv_scale = 16'd0;
        position1_shortconv_scale = 16'd0;
        imported0 = 64'sd0;
        imported1 = 64'sd0;
        imported1023 = 64'sd0;
        repeat (5) @(negedge clk);
        reset_n = 1'b1;
        repeat (2) @(negedge clk);
        establish_resident_q8();
        reset_shortconv_layer();

        // Position zero starts from logical zero causal state.
        execute_operation(32'd0, 1'b1);
        position0_q30 = imported0;
        if (imported0 == 64'sd0 || !imported1[63]
                || imported1023 == 64'sd0)
            failures = failures + 1;
        inspect_output(10'd0, 1'b0, imported0);
        inspect_output(10'd1, 1'b0, imported1);
        consume_result(1'b0, 8'd0, DESTINATION_ONE);

        // Repeating position zero is rejected without changing the causal
        // state; position one must still be accepted afterward.
        begin_operation(32'd0, DESTINATION_ONE);
        consume_result(1'b1, 8'd3, 37'd0);

        execute_operation(32'd1, 1'b0);
        position1_q30 = imported0;
        if (position0_shortconv_scale == 16'd0
                || position1_shortconv_scale == 16'd0
                || position1_shortconv_scale == position0_shortconv_scale
                || position1_q30 == position0_q30)
            failures = failures + 1;
        inspect_output(10'd0, 1'b0, imported0);
        inspect_output(10'd1, 1'b0, imported1);
        consume_result(1'b0, 8'd0, DESTINATION_ONE);

        // Abort position two after one channel has physically advanced.  The
        // layer becomes poisoned and the untouched destination stays invalid.
        begin_operation(32'd2, DESTINATION_TWO);
        for (block_number = 0; block_number < 32;
             block_number = block_number + 1) begin
            while (!triplet_ready) @(negedge clk);
            triplet_b = sparse_block(block_number[4:0]);
            triplet_c = sparse_block(block_number[4:0]);
            triplet_x = sparse_block(block_number[4:0]);
            triplet_valid = 1'b1;
            @(negedge clk);
            triplet_valid = 1'b0;
        end
        while (!(triplet_ready && triplet_channel == 10'd1
                && triplet_block == 5'd0))
            @(negedge clk);
        abort = 1'b1;
        @(negedge clk);
        abort = 1'b0;
        consume_result(1'b1, 8'd6, 37'd0);
        inspect_output(10'd0, 1'b1, 64'sd0);
        begin_operation(32'd2, DESTINATION_TWO);
        consume_result(1'b1, 8'd3, 37'd0);

        if (failures == 0)
            $display("PASS resident_shortconv_join resident_q8_preload triplets=1024x32 projection=1024x32 positions=0+1 causal_cache_shift=strict signed_q30_import backpressure=stable abort=layer_poison destination=unpublished");
        else begin
            $display("FAIL resident_shortconv_join failures=%0d", failures);
            $fatal(1);
        end
        $finish;
    end
endmodule
