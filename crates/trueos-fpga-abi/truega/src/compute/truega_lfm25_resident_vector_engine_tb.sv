`timescale 1ns/1ps

module truega_lfm25_resident_vector_engine_tb;
    localparam [1:0] OP_TOKEN_EMBEDDING = 2'd0;
    localparam [1:0] OP_RMSNORM = 2'd1;
    localparam [1:0] OP_RESIDUAL_ADD = 2'd2;
    localparam [1:0] OP_IMPORT_Q30 = 2'd3;

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg abort = 1'b0;
    always #5 clk = ~clk;

    reg command_valid = 1'b0;
    wire command_ready;
    reg [1:0] command_operation = 2'd0;
    reg [36:0] command_source0 = 37'd0;
    reg [36:0] command_source1 = 37'd0;
    reg [36:0] command_destination = 37'd0;

    reg embedding_valid = 1'b0;
    wire embedding_ready;
    reg [4:0] embedding_index = 5'd0;
    reg [271:0] embedding_block = 272'd0;
    reg weight_valid = 1'b0;
    wire weight_ready;
    reg [9:0] weight_index = 10'd0;
    reg weight_bf16 = 1'b0;
    reg [31:0] weight_bits = 32'd0;
    reg import_valid = 1'b0;
    wire import_ready;
    reg [9:0] import_index = 10'd0;
    reg signed [63:0] import_q30 = 64'sd0;

    wire result_valid;
    reg result_ready = 1'b0;
    wire result_error;
    wire [36:0] result_handle;

    reg inspect_valid = 1'b0;
    wire inspect_ready;
    reg [36:0] inspect_handle = 37'd0;
    reg [9:0] inspect_index = 10'd0;
    wire inspect_rsp_valid;
    reg inspect_rsp_ready = 1'b0;
    wire inspect_rsp_error;
    wire [271:0] inspect_rsp_data;
    wire [31:0] session_epoch;
    wire busy;

    integer failures = 0;
    integer index;
    reg [36:0] held_handle;
    reg held_error;
    reg [271:0] held_inspect_data;
    reg held_inspect_error;

    function automatic [36:0] make_handle;
        input [31:0] epoch;
        input type_q8;
        input [3:0] slot;
        begin
            make_handle = {epoch, type_q8, slot};
        end
    endfunction

    function automatic signed [63:0] imported_value;
        input integer requested_index;
        begin
            if (requested_index[0])
                imported_value = -64'sd4000000000 - requested_index;
            else
                imported_value = 64'sd5000000000 + requested_index;
        end
    endfunction

    function automatic [271:0] imported_inspect_value;
        input integer requested_index;
        reg signed [63:0] value;
        begin
            value = imported_value(requested_index);
            imported_inspect_value = {{208{value[63]}}, value};
        end
    endfunction

    truega_lfm25_resident_vector_engine #(
        .Q30_SLOTS(2), .Q8_SLOTS(2)
    ) dut (
        .clk(clk), .reset_n(reset_n), .abort_i(abort),
        .command_valid_i(command_valid), .command_ready_o(command_ready),
        .command_operation_i(command_operation),
        .command_source0_handle_i(command_source0),
        .command_source1_handle_i(command_source1),
        .command_destination_handle_i(command_destination),
        .embedding_block_valid_i(embedding_valid),
        .embedding_block_ready_o(embedding_ready),
        .embedding_block_index_i(embedding_index),
        .embedding_q8_block_i(embedding_block),
        .weight_valid_i(weight_valid), .weight_ready_o(weight_ready),
        .weight_index_i(weight_index),
        .weight_format_bf16_i(weight_bf16), .weight_bits_i(weight_bits),
        .import_valid_i(import_valid), .import_ready_o(import_ready),
        .import_index_i(import_index), .import_q30_i(import_q30),
        .result_valid_o(result_valid), .result_ready_i(result_ready),
        .result_error_o(result_error), .result_handle_o(result_handle),
        .inspect_valid_i(inspect_valid), .inspect_ready_o(inspect_ready),
        .inspect_handle_i(inspect_handle), .inspect_index_i(inspect_index),
        .inspect_rsp_valid_o(inspect_rsp_valid),
        .inspect_rsp_ready_i(inspect_rsp_ready),
        .inspect_rsp_error_o(inspect_rsp_error),
        .inspect_rsp_data_o(inspect_rsp_data),
        .session_epoch_o(session_epoch), .busy_o(busy)
    );

    task automatic start_command;
        input [1:0] operation;
        input [36:0] source0;
        input [36:0] source1;
        input [36:0] destination;
        begin
            @(negedge clk);
            command_operation = operation;
            command_source0 = source0;
            command_source1 = source1;
            command_destination = destination;
            command_valid = 1'b1;
            while (!command_ready)
                @(negedge clk);
            @(negedge clk);
            command_valid = 1'b0;
        end
    endtask

    task automatic feed_embedding;
        input [7:0] quant;
        input [15:0] scale;
        begin
            for (index = 0; index < 32; index = index + 1) begin
                @(negedge clk);
                embedding_index = index[4:0];
                embedding_block = {{32{quant}}, scale};
                embedding_valid = 1'b1;
                while (!embedding_ready)
                    @(negedge clk);
                @(negedge clk);
                embedding_valid = 1'b0;
            end
        end
    endtask

    task automatic feed_zero_weights;
        input use_bf16;
        begin
            for (index = 0; index < 1024; index = index + 1) begin
                @(negedge clk);
                weight_index = index[9:0];
                weight_bf16 = use_bf16;
                weight_bits = 32'd0;
                weight_valid = 1'b1;
                while (!weight_ready)
                    @(negedge clk);
                @(negedge clk);
                weight_valid = 1'b0;
            end
        end
    endtask

    task automatic feed_import_elements;
        input integer count;
        integer import_element;
        begin
            for (import_element = 0; import_element < count;
                 import_element = import_element + 1) begin
                while (!import_ready)
                    @(negedge clk);
                import_index = import_element[9:0];
                import_q30 = imported_value(import_element);
                import_valid = 1'b1;
                @(negedge clk);
                import_valid = 1'b0;
                // The internal handle is unpublished until the transaction's
                // full-vector commit has completed.
                if (result_handle !== 37'd0)
                    failures = failures + 1;
            end
        end
    endtask

    task automatic expect_result;
        input expected_error;
        input [36:0] expected_handle;
        begin
            while (!result_valid)
                @(negedge clk);
            held_error = result_error;
            held_handle = result_handle;
            repeat (3) begin
                @(negedge clk);
                if (!result_valid || result_error !== held_error
                        || result_handle !== held_handle)
                    failures = failures + 1;
            end
            if (held_error !== expected_error)
                failures = failures + 1;
            if (!expected_error && held_handle !== expected_handle)
                failures = failures + 1;
            if (expected_error && held_handle !== 37'd0)
                failures = failures + 1;
            result_ready = 1'b1;
            @(negedge clk);
            result_ready = 1'b0;
        end
    endtask

    task automatic inspect_value;
        input [36:0] handle;
        input [9:0] requested_index;
        input expected_error;
        input [271:0] expected_data;
        begin
            @(negedge clk);
            inspect_handle = handle;
            inspect_index = requested_index;
            inspect_valid = 1'b1;
            while (!inspect_ready)
                @(negedge clk);
            @(negedge clk);
            inspect_valid = 1'b0;
            while (!inspect_rsp_valid)
                @(negedge clk);
            held_inspect_error = inspect_rsp_error;
            held_inspect_data = inspect_rsp_data;
            repeat (3) begin
                @(negedge clk);
                if (!inspect_rsp_valid
                        || inspect_rsp_error !== held_inspect_error
                        || inspect_rsp_data !== held_inspect_data)
                    failures = failures + 1;
            end
            if (held_inspect_error !== expected_error)
                failures = failures + 1;
            if (!expected_error && held_inspect_data !== expected_data)
                failures = failures + 1;
            inspect_rsp_ready = 1'b1;
            @(negedge clk);
            inspect_rsp_ready = 1'b0;
        end
    endtask

    task automatic pulse_abort;
        begin
            @(negedge clk);
            abort = 1'b1;
            @(negedge clk);
            abort = 1'b0;
        end
    endtask

    task automatic run_session;
        input [31:0] epoch;
        input [7:0] quant;
        input [15:0] scale;
        input signed [63:0] embedded_q30;
        input signed [63:0] residual_q30;
        input rms_bf16;
        input [3:0] stream_slot;
        input [3:0] rms_slot;
        reg [36:0] stream_handle;
        reg [36:0] hidden_handle;
        reg [36:0] residual_handle;
        reg [36:0] rms_handle;
        begin
            stream_handle = make_handle(epoch, 1'b1, stream_slot);
            hidden_handle = make_handle(epoch, 1'b0, 4'd0);
            residual_handle = make_handle(epoch, 1'b0, 4'd1);
            rms_handle = make_handle(epoch, 1'b1, rms_slot);

            start_command(OP_TOKEN_EMBEDDING, stream_handle, 37'd0,
                          hidden_handle);
            feed_embedding(quant, scale);
            expect_result(1'b0, hidden_handle);
            if (session_epoch !== epoch)
                failures = failures + 1;
            inspect_value(hidden_handle, 10'd0, 1'b0,
                          {{208{embedded_q30[63]}}, embedded_q30});
            inspect_value(hidden_handle, 10'd1023, 1'b0,
                          {{208{embedded_q30[63]}}, embedded_q30});

            start_command(OP_RESIDUAL_ADD, hidden_handle, hidden_handle,
                          residual_handle);
            expect_result(1'b0, residual_handle);
            inspect_value(residual_handle, 10'd0, 1'b0,
                          {{208{residual_q30[63]}}, residual_q30});
            inspect_value(residual_handle, 10'd777, 1'b0,
                          {{208{residual_q30[63]}}, residual_q30});

            start_command(OP_RMSNORM, hidden_handle, 37'd0, rms_handle);
            feed_zero_weights(rms_bf16);
            expect_result(1'b0, rms_handle);
            inspect_value(rms_handle, 10'd0, 1'b0, 272'd0);
            inspect_value(rms_handle, 10'd31, 1'b0, 272'd0);
        end
    endtask

    initial begin
        repeat (5) @(negedge clk);
        reset_n = 1'b1;
        repeat (2) @(negedge clk);
        if (session_epoch !== 32'd0 || busy)
            failures = failures + 1;

        run_session(32'h51a7_0001, 8'h02, 16'h3800,
                    64'sd1073741824, 64'sd2147483648,
                    1'b0, 4'd0, 4'd0);

        run_session(32'h51a7_0002, 8'hfc, 16'h3400,
                    -64'sd1073741824, -64'sd2147483648,
                    1'b1, 4'd1, 4'd1);

        // Begin overwriting the committed Q8 slot, allow five new blocks to
        // land, then abort. The old value and partial replacement must both be
        // unreadable because no full-vector commit occurred.
        start_command(OP_RMSNORM,
                      make_handle(32'h51a7_0002, 1'b0, 4'd0), 37'd0,
                      make_handle(32'h51a7_0002, 1'b1, 4'd1));
        feed_zero_weights(1'b1);
        while (dut.store.q8_transaction_count[1] < 6'd5)
            @(negedge clk);
        pulse_abort();
        expect_result(1'b1, 37'd0);
        inspect_value(make_handle(32'h51a7_0002, 1'b1, 4'd1),
                      10'd0, 1'b1, 272'd0);

        // Import is a destination-only internal join. Stale destinations,
        // out-of-range slots, and nonzero (aliasing) sources are rejected
        // before the store transaction can invalidate anything.
        start_command(OP_IMPORT_Q30, 37'd0, 37'd0,
                      make_handle(32'h51a7_0001, 1'b0, 4'd1));
        expect_result(1'b1, 37'd0);
        start_command(OP_IMPORT_Q30, 37'd0, 37'd0,
                      make_handle(32'h51a7_0002, 1'b0, 4'd15));
        expect_result(1'b1, 37'd0);
        start_command(OP_IMPORT_Q30,
                      make_handle(32'h51a7_0002, 1'b0, 4'd1), 37'd0,
                      make_handle(32'h51a7_0002, 1'b0, 4'd1));
        expect_result(1'b1, 37'd0);

        // A complete ordered signed-i64 stream becomes readable atomically.
        start_command(OP_IMPORT_Q30, 37'd0, 37'd0,
                      make_handle(32'h51a7_0002, 1'b0, 4'd1));
        feed_import_elements(1024);
        expect_result(1'b0,
                      make_handle(32'h51a7_0002, 1'b0, 4'd1));
        inspect_value(make_handle(32'h51a7_0002, 1'b0, 4'd1),
                      10'd0, 1'b0,
                      imported_inspect_value(0));
        inspect_value(make_handle(32'h51a7_0002, 1'b0, 4'd1),
                      10'd511, 1'b0,
                      imported_inspect_value(511));
        inspect_value(make_handle(32'h51a7_0002, 1'b0, 4'd1),
                      10'd1023, 1'b0,
                      imported_inspect_value(1023));

        // An out-of-order element is accepted as a protocol error but never
        // written; begin already invalidated the old committed destination.
        start_command(OP_IMPORT_Q30, 37'd0, 37'd0,
                      make_handle(32'h51a7_0002, 1'b0, 4'd1));
        while (!import_ready)
            @(negedge clk);
        import_index = 10'd1;
        import_q30 = 64'sd99;
        import_valid = 1'b1;
        @(negedge clk);
        import_valid = 1'b0;
        expect_result(1'b1, 37'd0);
        inspect_value(make_handle(32'h51a7_0002, 1'b0, 4'd1),
                      10'd0, 1'b1, 272'd0);

        // A mid-vector abort leaves the partial destination unpublished and
        // unreadable even though payload writes have physically occurred.
        start_command(OP_IMPORT_Q30, 37'd0, 37'd0,
                      make_handle(32'h51a7_0002, 1'b0, 4'd0));
        feed_import_elements(17);
        pulse_abort();
        expect_result(1'b1, 37'd0);
        inspect_value(make_handle(32'h51a7_0002, 1'b0, 4'd0),
                      10'd0, 1'b1, 272'd0);

        // Session-one handles are stale after TokenEmbedding begins session two.
        inspect_value(make_handle(32'h51a7_0001, 1'b0, 4'd0),
                      10'd0, 1'b1, 272'd0);
        start_command(OP_RESIDUAL_ADD,
                      make_handle(32'h51a7_0001, 1'b0, 4'd0),
                      make_handle(32'h51a7_0001, 1'b0, 4'd0),
                      make_handle(32'h51a7_0001, 1'b0, 4'd1));
        expect_result(1'b1, 37'd0);

        // A repeated TokenEmbedding epoch cannot revive same-epoch handles.
        start_command(OP_TOKEN_EMBEDDING,
                      make_handle(32'h51a7_0002, 1'b1, 4'd0), 37'd0,
                      make_handle(32'h51a7_0002, 1'b0, 4'd0));
        expect_result(1'b1, 37'd0);
        if (session_epoch !== 32'h51a7_0002)
            failures = failures + 1;

        // Type and slot mismatches are rejected before any resident access.
        start_command(OP_RMSNORM,
                      make_handle(32'h51a7_0002, 1'b1, 4'd0), 37'd0,
                      make_handle(32'h51a7_0002, 1'b1, 4'd0));
        expect_result(1'b1, 37'd0);
        start_command(OP_RESIDUAL_ADD,
                      make_handle(32'h51a7_0002, 1'b0, 4'd15),
                      make_handle(32'h51a7_0002, 1'b0, 4'd0),
                      make_handle(32'h51a7_0002, 1'b0, 4'd1));
        expect_result(1'b1, 37'd0);

        if (failures == 0)
            $display("PASS lfm25_resident_vector_engine sessions=2 ops=embedding+rmsnorm+residual+import_q30 exact_q8_dequant typed_epoch_handles stable_ready_valid transactional_full_import=1024 order_error=unpublished partial_abort=unpublished no_payload_reset");
        else begin
            $display("FAIL lfm25_resident_vector_engine failures=%0d", failures);
            $fatal(1);
        end
        $finish;
    end

    initial begin
        #30000000;
        $display("FAIL lfm25_resident_vector_engine global timeout");
        $fatal(1);
    end
endmodule
