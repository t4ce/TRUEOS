`timescale 1ns/1ps

module truega_lfm25_resident_ffn_join_tb;
    localparam [31:0] EPOCH = 32'hff10_0001;
    localparam [36:0] STREAM_HANDLE = {EPOCH, 1'b1, 4'd3};
    localparam [36:0] EMBEDDING_HANDLE = {EPOCH, 1'b0, 4'd0};
    localparam [36:0] SOURCE_HANDLE = {EPOCH, 1'b1, 4'd0};
    localparam [36:0] DESTINATION_HANDLE = {EPOCH, 1'b0, 4'd1};

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg clear = 1'b0;
    reg abort = 1'b0;
    reg start = 1'b0;
    wire start_ready;

    reg row_start = 1'b0;
    reg row_down = 1'b0;
    reg [12:0] row_index = 13'd0;
    wire row_ready;
    wire expected_row_down;
    wire [12:0] expected_row_index;
    reg weight_valid = 1'b0;
    reg [7:0] weight_block_index = 8'd0;
    reg [271:0] weight0 = 272'd0;
    reg [271:0] weight1 = 272'd0;
    wire weight_ready;
    wire [7:0] expected_weight_block;
    wire row_done;
    wire row_error;
    wire row_done_down;
    wire [12:0] row_done_index;
    reg import_pause = 1'b0;
    wire adapter_valid;
    wire [9:0] adapter_index;
    wire signed [63:0] adapter_q30;
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
    wire [12:0] gate_up_rows;
    wire [10:0] down_rows;
    wire [10:0] import_elements;
    wire busy;

    // Direct owner is used only to establish the resident Q8 source handle.
    reg setup_owner = 1'b1;
    reg setup_command_valid = 1'b0;
    wire setup_command_ready;
    reg [1:0] setup_command_operation = 2'd0;
    reg [36:0] setup_command_source0 = 37'd0;
    reg [36:0] setup_command_source1 = 37'd0;
    reg [36:0] setup_command_destination = 37'd0;
    reg setup_result_ready = 1'b0;
    wire setup_result_valid;
    wire setup_result_error;
    wire [36:0] setup_result_handle;
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
    wire [36:0] resident_command_source1 = setup_owner
        ? setup_command_source1 : join_command_source1;
    wire [36:0] resident_command_destination = setup_owner
        ? setup_command_destination : join_command_destination;
    wire resident_command_ready;
    wire resident_result_valid;
    wire resident_result_ready = setup_owner
        ? setup_result_ready : join_result_ready;
    wire resident_result_error;
    wire [36:0] resident_result_handle;
    wire resident_busy;

    assign setup_command_ready = setup_owner && resident_command_ready;
    assign setup_result_valid = setup_owner && resident_result_valid;
    assign setup_result_error = resident_result_error;
    assign setup_result_handle = resident_result_handle;
    assign join_command_ready = !setup_owner && resident_command_ready;
    assign join_result_valid = !setup_owner && resident_result_valid;
    assign join_result_error = resident_result_error;
    assign join_result_handle = resident_result_handle;

    integer failures = 0;
    integer row_number;
    integer block_number;
    integer setup_number;
    integer cycles = 0;
    reg signed [63:0] imported0;
    reg signed [63:0] imported1;
    reg signed [63:0] imported2;
    reg signed [63:0] imported1023;
    reg signed [63:0] held_adapter_q30;
    reg [9:0] held_adapter_index;
    reg [36:0] held_result_handle;
    reg [7:0] held_result_code;

    always #5 clk = ~clk;

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

    truega_lfm25_resident_ffn_join dut (
        .clk(clk), .reset_n(reset_n), .clear_i(clear), .abort_i(abort),
        .start_i(start), .start_ready_o(start_ready),
        .source_q8_handle_i(SOURCE_HANDLE),
        .destination_q30_handle_i(DESTINATION_HANDLE),
        .row_start_i(row_start), .row_down_i(row_down),
        .row_index_i(row_index), .row_ready_o(row_ready),
        .expected_row_down_o(expected_row_down),
        .expected_row_index_o(expected_row_index),
        .weight_valid_i(weight_valid),
        .weight_block_index_i(weight_block_index),
        .weight0_q8_block_i(weight0), .weight1_q8_block_i(weight1),
        .weight_ready_o(weight_ready),
        .expected_weight_block_o(expected_weight_block),
        .row_done_o(row_done), .row_error_o(row_error),
        .row_done_down_o(row_done_down),
        .row_done_index_o(row_done_index),
        .import_pause_i(import_pause),
        .import_adapter_valid_o(adapter_valid),
        .import_adapter_index_o(adapter_index),
        .import_adapter_q30_o(adapter_q30),
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
        .gate_up_rows_completed_o(gate_up_rows),
        .down_rows_completed_o(down_rows),
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
        .command_source1_handle_i(resident_command_source1),
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
        .session_epoch_o(), .busy_o(resident_busy)
    );

    task automatic setup_command;
        input [1:0] operation;
        input [36:0] source0;
        input [36:0] destination;
        begin
            setup_command_operation = operation;
            setup_command_source0 = source0;
            setup_command_source1 = 37'd0;
            setup_command_destination = destination;
            setup_command_valid = 1'b1;
            while (!setup_command_ready)
                @(negedge clk);
            @(negedge clk);
            setup_command_valid = 1'b0;
        end
    endtask

    task automatic setup_expect_result;
        input [36:0] expected_handle;
        begin
            while (!setup_result_valid)
                @(negedge clk);
            if (setup_result_error || setup_result_handle !== expected_handle)
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
                while (!embedding_ready)
                    @(negedge clk);
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
                while (!norm_weight_ready)
                    @(negedge clk);
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

    task automatic pulse_start;
        begin
            while (!start_ready)
                @(negedge clk);
            start = 1'b1;
            @(negedge clk);
            start = 1'b0;
        end
    endtask

    task automatic begin_expected_row;
        input requested_down;
        input integer requested_row;
        begin
            while (!row_ready)
                @(negedge clk);
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

    task automatic feed_weight;
        input integer requested_block;
        input [271:0] requested_weight0;
        input [271:0] requested_weight1;
        begin
            while (!weight_ready)
                @(negedge clk);
            if (expected_weight_block !== requested_block[7:0])
                failures = failures + 1;
            weight_block_index = requested_block[7:0];
            weight0 = requested_weight0;
            weight1 = requested_weight1;
            weight_valid = 1'b1;
            @(negedge clk);
            weight_valid = 1'b0;
        end
    endtask

    task automatic finish_row;
        input requested_down;
        input integer requested_row;
        begin
            while (!row_done)
                @(negedge clk);
            if (row_error || row_done_down !== requested_down
                    || row_done_index !== requested_row[12:0])
                failures = failures + 1;
        end
    endtask

    task automatic execute_full_ffn;
        begin
            pulse_start();
            for (row_number = 0; row_number < 4608;
                 row_number = row_number + 1) begin
                begin_expected_row(1'b0, row_number);
                for (block_number = 0; block_number < 32;
                     block_number = block_number + 1) begin
                    if (row_number < 32 && block_number == 0)
                        feed_weight(block_number,
                            native_block(16'h3800, 8'sd1),
                            native_block(16'h3800, 8'sd1));
                    else
                        feed_weight(block_number, 272'd0, 272'd0);
                end
                finish_row(1'b0, row_number);
                if ((row_number & 1023) == 1023)
                    $display("resident_ffn_join gate_up=%0d/4608",
                        row_number + 1);
            end
            for (row_number = 0; row_number < 1024;
                 row_number = row_number + 1) begin
                begin_expected_row(1'b1, row_number);
                for (block_number = 0; block_number < 144;
                     block_number = block_number + 1) begin
                    if (row_number < 2 && block_number == 0)
                        feed_weight(block_number,
                            native_block(16'h3800, 8'sd1), 272'd0);
                    else
                        feed_weight(block_number, 272'd0, 272'd0);
                end
                finish_row(1'b1, row_number);
                if ((row_number & 255) == 255)
                    $display("resident_ffn_join down=%0d/1024",
                        row_number + 1);
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
            while (!output_read_ready)
                @(negedge clk);
            @(negedge clk);
            output_read_valid = 1'b0;
            while (!output_read_rsp_valid)
                @(negedge clk);
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
            while (!result_valid)
                @(negedge clk);
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
                10'd2: imported2 <= join_import_q30;
                10'd1023: imported1023 <= join_import_q30;
                default: begin end
            endcase
        end
    end

    always @(posedge clk) begin
        cycles <= cycles + 1;
        if (cycles > 50_000_000) begin
            $display("FAIL resident_ffn_join timeout state=%0d ffn_state=%0d gu=%0d down=%0d import=%0d",
                dut.state, dut.ffn.state, gate_up_rows, down_rows,
                import_elements);
            $fatal(1);
        end
    end

    initial begin
        imported0 = 64'sd0;
        imported1 = 64'sd0;
        imported2 = 64'sd0;
        imported1023 = 64'sd0;
        repeat (5) @(negedge clk);
        reset_n = 1'b1;
        repeat (2) @(negedge clk);
        establish_resident_q8();

        // Full fixed shape and complete atomic publication.  Pause the first
        // elastic item for three cycles and require its index/data to hold.
        import_pause = 1'b1;
        execute_full_ffn();
        while (!adapter_valid)
            @(negedge clk);
        held_adapter_index = adapter_index;
        held_adapter_q30 = adapter_q30;
        repeat (3) begin
            @(negedge clk);
            if (!adapter_valid || adapter_index !== held_adapter_index
                    || adapter_q30 !== held_adapter_q30)
                failures = failures + 1;
        end
        import_pause = 1'b0;
        while (!result_valid)
            @(negedge clk);
        if (gate_up_rows != 13'd4608 || down_rows != 11'd1024
                || import_elements != 11'd1024)
            failures = failures + 1;
        if (imported0 == 64'sd0 || imported0 !== imported1
                || imported2 !== 64'sd0 || imported1023 !== 64'sd0)
            failures = failures + 1;
        inspect_output(10'd0, 1'b0, imported0);
        inspect_output(10'd1, 1'b0, imported1);
        inspect_output(10'd2, 1'b0, imported2);
        inspect_output(10'd1023, 1'b0, imported1023);
        consume_result(1'b0, 8'd0, DESTINATION_HANDLE);

        // Re-execute the fixed FFN against the still-resident Q8 source.  Abort
        // after physical writes have begun; begin already invalidated the old
        // destination and the partial replacement must be unreadable.
        execute_full_ffn();
        while (import_elements < 11'd17)
            @(negedge clk);
        abort = 1'b1;
        @(negedge clk);
        abort = 1'b0;
        consume_result(1'b1, 8'd5, 37'd0);
        inspect_output(10'd0, 1'b1, 64'sd0);

        if (failures == 0)
            $display("PASS resident_ffn_join shared_resident_q8_handle full_gate_up=4608 full_down=1024 elastic_sync_read_adapter=stable ordered_import=1024 committed_readback partial_abort=unpublished no_host_math no_runtime_graph");
        else begin
            $display("FAIL resident_ffn_join failures=%0d", failures);
            $fatal(1);
        end
        $finish;
    end
endmodule
