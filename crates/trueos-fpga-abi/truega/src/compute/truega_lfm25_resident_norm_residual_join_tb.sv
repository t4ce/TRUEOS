`timescale 1ns/1ps

module truega_lfm25_resident_norm_residual_join_tb;
    localparam [1:0] OP_TOKEN_EMBEDDING = 2'd0;
    localparam [1:0] OP_IMPORT_Q30      = 2'd3;
    localparam       JOIN_RMSNORM       = 1'b0;
    localparam       JOIN_RESIDUAL      = 1'b1;
    localparam [7:0] ERROR_HANDLE       = 8'd1;
    localparam [7:0] ERROR_WEIGHT_ORDER = 8'd2;
    localparam [7:0] ERROR_ABORT        = 8'd6;

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg clear = 1'b0;
    reg abort = 1'b0;
    always #5 clk = ~clk;

    reg join_start = 1'b0;
    wire join_start_ready;
    reg join_operation = JOIN_RMSNORM;
    reg [36:0] join_source0 = 37'd0;
    reg [36:0] join_source1 = 37'd0;
    reg [36:0] join_destination = 37'd0;
    reg [31:0] join_position = 32'd0;

    reg join_weight_valid = 1'b0;
    wire join_weight_ready;
    wire [9:0] join_expected_weight;
    reg [9:0] join_weight_index = 10'd0;
    reg join_weight_bf16 = 1'b1;
    reg [31:0] join_weight_bits = 32'd0;

    wire join_result_valid;
    reg join_result_ready = 1'b0;
    wire join_result_error;
    wire [7:0] join_result_code;
    wire join_result_operation;
    wire [31:0] join_result_position;
    wire [36:0] join_result_handle;

    reg output_read_valid = 1'b0;
    wire output_read_ready;
    reg [9:0] output_read_index = 10'd0;
    wire output_read_rsp_valid;
    reg output_read_rsp_ready = 1'b0;
    wire output_read_error;
    wire [271:0] output_read_data;

    wire join_command_valid;
    wire join_command_ready;
    wire [1:0] join_command_operation;
    wire [36:0] join_command_source0;
    wire [36:0] join_command_source1;
    wire [36:0] join_command_destination;
    wire join_resident_result_valid;
    wire join_resident_result_ready;
    wire join_resident_result_error;
    wire [36:0] join_resident_result_handle;
    wire join_resident_abort;
    wire join_resident_weight_valid;
    wire join_resident_weight_ready;
    wire [9:0] join_resident_weight_index;
    wire join_resident_weight_bf16;
    wire [31:0] join_resident_weight_bits;
    wire join_inspect_valid;
    wire join_inspect_ready;
    wire [36:0] join_inspect_handle;
    wire [9:0] join_inspect_index;
    wire join_inspect_rsp_valid;
    wire join_inspect_rsp_ready;
    wire join_inspect_rsp_error;
    wire [271:0] join_inspect_rsp_data;
    wire [10:0] join_weights_accepted;
    wire join_poisoned;
    wire join_busy;

    // The setup owner uses the same resident engine to establish one session
    // and transactionally preload its two Q30 source vectors.  It is disabled
    // before every joined operation, so there is only one resident owner.
    reg setup_owner = 1'b1;
    reg setup_command_valid = 1'b0;
    wire setup_command_ready;
    reg [1:0] setup_command_operation = 2'd0;
    reg [36:0] setup_command_source0 = 37'd0;
    reg [36:0] setup_command_source1 = 37'd0;
    reg [36:0] setup_command_destination = 37'd0;
    wire setup_result_valid;
    reg setup_result_ready = 1'b0;
    reg embedding_valid = 1'b0;
    wire embedding_ready;
    reg [4:0] embedding_index = 5'd0;
    reg [271:0] embedding_block = 272'd0;
    reg setup_import_valid = 1'b0;
    wire setup_import_ready;
    reg [9:0] setup_import_index = 10'd0;
    reg signed [63:0] setup_import_q30 = 64'sd0;

    wire resident_command_valid = setup_owner
        ? setup_command_valid : join_command_valid;
    wire resident_command_ready;
    wire [1:0] resident_command_operation = setup_owner
        ? setup_command_operation : join_command_operation;
    wire [36:0] resident_command_source0 = setup_owner
        ? setup_command_source0 : join_command_source0;
    wire [36:0] resident_command_source1 = setup_owner
        ? setup_command_source1 : join_command_source1;
    wire [36:0] resident_command_destination = setup_owner
        ? setup_command_destination : join_command_destination;
    wire resident_result_valid;
    wire resident_result_ready = setup_owner
        ? setup_result_ready : join_resident_result_ready;
    wire resident_result_error;
    wire [36:0] resident_result_handle;
    wire resident_weight_ready;
    wire resident_inspect_ready;
    wire resident_inspect_rsp_valid;
    wire resident_inspect_rsp_error;
    wire [271:0] resident_inspect_rsp_data;

    assign setup_command_ready = setup_owner && resident_command_ready;
    assign setup_result_valid = setup_owner && resident_result_valid;
    assign join_command_ready = !setup_owner && resident_command_ready;
    assign join_resident_result_valid = !setup_owner
        && resident_result_valid;
    assign join_resident_result_error = resident_result_error;
    assign join_resident_result_handle = resident_result_handle;
    assign join_resident_weight_ready = !setup_owner
        && resident_weight_ready;
    assign setup_import_ready = setup_owner && resident_import_ready;
    assign join_inspect_ready = !setup_owner && resident_inspect_ready;
    assign join_inspect_rsp_valid = !setup_owner
        && resident_inspect_rsp_valid;
    assign join_inspect_rsp_error = resident_inspect_rsp_error;
    assign join_inspect_rsp_data = resident_inspect_rsp_data;

    wire resident_import_ready;
    wire [31:0] resident_epoch;
    wire resident_busy;

    integer failures = 0;
    integer element;
    integer weight_stalls = 0;
    integer joined_command_accepts = 0;
    integer accepts_before_invalid;
    integer cycles = 0;
    reg [31:0] active_epoch = 32'd0;
    reg [36:0] stream_handle = 37'd0;
    reg [36:0] source0_handle = 37'd0;
    reg [36:0] source1_handle = 37'd0;
    reg [36:0] q8_destination = 37'd0;
    reg [36:0] q30_destination = 37'd0;
    reg [36:0] held_result_handle;
    reg [7:0] held_result_code;
    reg held_result_error;
    reg held_result_operation;
    reg [31:0] held_result_position;
    reg [271:0] held_read_data;
    reg held_read_error;

    function automatic [36:0] make_handle;
        input [31:0] epoch;
        input type_q8;
        input [3:0] slot;
        begin
            make_handle = {epoch, type_q8, slot};
        end
    endfunction

    function automatic signed [63:0] source0_value;
        input integer requested_index;
        begin
            source0_value = requested_index[0]
                ? -64'sd1073741824 : 64'sd1073741824;
        end
    endfunction

    function automatic signed [63:0] source1_value;
        input integer requested_index;
        begin
            source1_value = requested_index[0]
                ? 64'sd268435456 + requested_index
                : -64'sd268435456 - requested_index;
        end
    endfunction

    function automatic [271:0] q30_inspect_value;
        input signed [63:0] value;
        begin
            q30_inspect_value = {{208{value[63]}}, value};
        end
    endfunction

    function automatic [271:0] embedding_payload;
        input integer requested_block;
        integer lane;
        integer global_index;
        reg [271:0] value;
        begin
            value = 272'd0;
            // binary16 0.5, multiplied by signed +/-2, yields exact +/-1 Q30.
            value[15:0] = 16'h3800;
            for (lane = 0; lane < 32; lane = lane + 1) begin
                global_index = requested_block * 32 + lane;
                value[16 + lane * 8 +: 8] = global_index[0]
                    ? 8'hfe : 8'h02;
            end
            embedding_payload = value;
        end
    endfunction

    always @(posedge clk) begin
        cycles <= cycles + 1;
        if (!setup_owner && join_command_valid && join_command_ready)
            joined_command_accepts <= joined_command_accepts + 1;
        if (cycles > 3000000) begin
            $display("FAIL resident_norm_residual_join timeout state=%0d resident_state=%0d weights=%0d",
                     dut.state, resident.state, join_weights_accepted);
            $finish;
        end
    end

    truega_lfm25_resident_norm_residual_join dut (
        .clk(clk), .reset_n(reset_n), .clear_i(clear), .abort_i(abort),
        .start_i(join_start), .start_ready_o(join_start_ready),
        .operation_i(join_operation),
        .source0_q30_handle_i(join_source0),
        .source1_q30_handle_i(join_source1),
        .destination_handle_i(join_destination),
        .token_position_i(join_position),
        .weight_valid_i(join_weight_valid),
        .weight_ready_o(join_weight_ready),
        .expected_weight_index_o(join_expected_weight),
        .weight_index_i(join_weight_index),
        .weight_format_bf16_i(join_weight_bf16),
        .weight_bits_i(join_weight_bits),
        .result_valid_o(join_result_valid),
        .result_ready_i(join_result_ready),
        .result_error_o(join_result_error),
        .result_error_code_o(join_result_code),
        .result_operation_o(join_result_operation),
        .result_token_position_o(join_result_position),
        .result_handle_o(join_result_handle),
        .output_read_valid_i(output_read_valid),
        .output_read_ready_o(output_read_ready),
        .output_read_index_i(output_read_index),
        .output_read_rsp_valid_o(output_read_rsp_valid),
        .output_read_rsp_ready_i(output_read_rsp_ready),
        .output_read_error_o(output_read_error),
        .output_read_data_o(output_read_data),
        .resident_command_valid_o(join_command_valid),
        .resident_command_ready_i(join_command_ready),
        .resident_command_operation_o(join_command_operation),
        .resident_command_source0_handle_o(join_command_source0),
        .resident_command_source1_handle_o(join_command_source1),
        .resident_command_destination_handle_o(join_command_destination),
        .resident_result_valid_i(join_resident_result_valid),
        .resident_result_ready_o(join_resident_result_ready),
        .resident_result_error_i(join_resident_result_error),
        .resident_result_handle_i(join_resident_result_handle),
        .resident_abort_o(join_resident_abort),
        .resident_weight_valid_o(join_resident_weight_valid),
        .resident_weight_ready_i(join_resident_weight_ready),
        .resident_weight_index_o(join_resident_weight_index),
        .resident_weight_format_bf16_o(join_resident_weight_bf16),
        .resident_weight_bits_o(join_resident_weight_bits),
        .resident_inspect_valid_o(join_inspect_valid),
        .resident_inspect_ready_i(join_inspect_ready),
        .resident_inspect_handle_o(join_inspect_handle),
        .resident_inspect_index_o(join_inspect_index),
        .resident_inspect_rsp_valid_i(join_inspect_rsp_valid),
        .resident_inspect_rsp_ready_o(join_inspect_rsp_ready),
        .resident_inspect_rsp_error_i(join_inspect_rsp_error),
        .resident_inspect_rsp_data_i(join_inspect_rsp_data),
        .weights_accepted_o(join_weights_accepted),
        .poisoned_o(join_poisoned), .busy_o(join_busy)
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
        .weight_valid_i(!setup_owner && join_resident_weight_valid),
        .weight_ready_o(resident_weight_ready),
        .weight_index_i(join_resident_weight_index),
        .weight_format_bf16_i(join_resident_weight_bf16),
        .weight_bits_i(join_resident_weight_bits),
        .import_valid_i(setup_owner && setup_import_valid),
        .import_ready_o(resident_import_ready),
        .import_index_i(setup_import_index),
        .import_q30_i(setup_import_q30),
        .result_valid_o(resident_result_valid),
        .result_ready_i(resident_result_ready),
        .result_error_o(resident_result_error),
        .result_handle_o(resident_result_handle),
        .inspect_valid_i(!setup_owner && join_inspect_valid),
        .inspect_ready_o(resident_inspect_ready),
        .inspect_handle_i(join_inspect_handle),
        .inspect_index_i(join_inspect_index),
        .inspect_rsp_valid_o(resident_inspect_rsp_valid),
        .inspect_rsp_ready_i(!setup_owner && join_inspect_rsp_ready),
        .inspect_rsp_error_o(resident_inspect_rsp_error),
        .inspect_rsp_data_o(resident_inspect_rsp_data),
        .session_epoch_o(resident_epoch), .busy_o(resident_busy)
    );

    task automatic setup_command;
        input [1:0] operation;
        input [36:0] source0;
        input [36:0] source1;
        input [36:0] destination;
        begin
            @(negedge clk);
            setup_command_operation = operation;
            setup_command_source0 = source0;
            setup_command_source1 = source1;
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
            if (resident_result_error
                    || resident_result_handle !== expected_handle)
                failures = failures + 1;
            setup_result_ready = 1'b1;
            @(negedge clk);
            setup_result_ready = 1'b0;
        end
    endtask

    task automatic establish_session;
        input [31:0] epoch;
        begin
            active_epoch = epoch;
            stream_handle = make_handle(epoch, 1'b1, 4'd3);
            source0_handle = make_handle(epoch, 1'b0, 4'd0);
            source1_handle = make_handle(epoch, 1'b0, 4'd1);
            q8_destination = make_handle(epoch, 1'b1, 4'd0);
            q30_destination = make_handle(epoch, 1'b0, 4'd2);

            setup_command(OP_TOKEN_EMBEDDING, stream_handle, 37'd0,
                          source0_handle);
            for (element = 0; element < 32; element = element + 1) begin
                @(negedge clk);
                embedding_index = element[4:0];
                embedding_block = embedding_payload(element);
                embedding_valid = 1'b1;
                while (!embedding_ready)
                    @(negedge clk);
                @(negedge clk);
                embedding_valid = 1'b0;
            end
            setup_expect_result(source0_handle);

            setup_command(OP_IMPORT_Q30, 37'd0, 37'd0, source1_handle);
            for (element = 0; element < 1024; element = element + 1) begin
                @(negedge clk);
                setup_import_index = element[9:0];
                setup_import_q30 = source1_value(element);
                setup_import_valid = 1'b1;
                while (!setup_import_ready)
                    @(negedge clk);
                @(negedge clk);
                setup_import_valid = 1'b0;
            end
            setup_expect_result(source1_handle);
            if (resident_epoch !== epoch)
                failures = failures + 1;
            setup_owner = 1'b0;
            @(negedge clk);
        end
    endtask

    task automatic start_join;
        input operation;
        input [36:0] source0;
        input [36:0] source1;
        input [36:0] destination;
        input [31:0] position;
        begin
            @(negedge clk);
            join_operation = operation;
            join_source0 = source0;
            join_source1 = source1;
            join_destination = destination;
            join_position = position;
            join_start = 1'b1;
            while (!join_start_ready)
                @(negedge clk);
            @(negedge clk);
            join_start = 1'b0;
        end
    endtask

    task automatic expect_join_result;
        input expected_error;
        input [7:0] expected_code;
        input expected_operation;
        input [31:0] expected_position;
        input [36:0] expected_handle;
        begin
            while (!join_result_valid)
                @(negedge clk);
            held_result_error = join_result_error;
            held_result_code = join_result_code;
            held_result_operation = join_result_operation;
            held_result_position = join_result_position;
            held_result_handle = join_result_handle;
            repeat (4) begin
                @(negedge clk);
                if (!join_result_valid
                        || join_result_error !== held_result_error
                        || join_result_code !== held_result_code
                        || join_result_operation !== held_result_operation
                        || join_result_position !== held_result_position
                        || join_result_handle !== held_result_handle)
                    failures = failures + 1;
            end
            if (held_result_error !== expected_error
                    || held_result_code !== expected_code
                    || held_result_operation !== expected_operation
                    || held_result_position !== expected_position
                    || held_result_handle !== expected_handle)
                failures = failures + 1;
            join_result_ready = 1'b1;
            @(negedge clk);
            join_result_ready = 1'b0;
        end
    endtask

    task automatic read_destination;
        input [9:0] requested_index;
        input expected_error;
        input [271:0] expected_data;
        begin
            @(negedge clk);
            output_read_index = requested_index;
            output_read_valid = 1'b1;
            while (!output_read_ready)
                @(negedge clk);
            @(negedge clk);
            output_read_valid = 1'b0;
            while (!output_read_rsp_valid)
                @(negedge clk);
            held_read_error = output_read_error;
            held_read_data = output_read_data;
            repeat (3) begin
                @(negedge clk);
                if (!output_read_rsp_valid
                        || output_read_error !== held_read_error
                        || output_read_data !== held_read_data)
                    failures = failures + 1;
            end
            if (held_read_error !== expected_error
                    || (!expected_error && held_read_data !== expected_data))
                failures = failures + 1;
            output_read_rsp_ready = 1'b1;
            @(negedge clk);
            output_read_rsp_ready = 1'b0;
        end
    endtask

    task automatic feed_bf16_zero_weights;
        begin
            for (element = 0; element < 1024; element = element + 1) begin
                @(negedge clk);
                join_weight_index = element[9:0];
                join_weight_bf16 = 1'b1;
                join_weight_bits = 32'd0;
                join_weight_valid = 1'b1;
                while (!join_weight_ready) begin
                    weight_stalls = weight_stalls + 1;
                    if (join_expected_weight !== element[9:0])
                        failures = failures + 1;
                    @(negedge clk);
                end
                @(negedge clk);
                join_weight_valid = 1'b0;
            end
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

    task automatic clear_and_reown;
        begin
            setup_owner = 1'b1;
            @(negedge clk);
            clear = 1'b1;
            @(negedge clk);
            clear = 1'b0;
            repeat (2) @(negedge clk);
            if (join_poisoned || join_busy || resident_epoch != 32'd0)
                failures = failures + 1;
        end
    endtask

    initial begin
        repeat (5) @(negedge clk);
        reset_n = 1'b1;
        repeat (2) @(negedge clk);

        establish_session(32'h6e00_0001);

        // Exact signed residual over all 1,024 elements.  Result metadata and
        // readback remain stable under explicit consumer backpressure.
        start_join(JOIN_RESIDUAL, source0_handle, source1_handle,
                   q30_destination, 32'd7);
        expect_join_result(1'b0, 8'd0, JOIN_RESIDUAL, 32'd7,
                           q30_destination);
        read_destination(10'd0, 1'b0,
            q30_inspect_value(source0_value(0) + source1_value(0)));
        read_destination(10'd1, 1'b0,
            q30_inspect_value(source0_value(1) + source1_value(1)));
        read_destination(10'd511, 1'b0,
            q30_inspect_value(source0_value(511) + source1_value(511)));
        read_destination(10'd1023, 1'b0,
            q30_inspect_value(source0_value(1023) + source1_value(1023)));

        // Full resident Q30 -> RMSNorm(BF16) -> resident Q8_0.  Zero BF16
        // weights make every byte and scale exactly zero while still driving
        // the complete reduction/normalization/quantization circuit.
        start_join(JOIN_RMSNORM, source0_handle, 37'd0,
                   q8_destination, 32'd8);
        feed_bf16_zero_weights();
        expect_join_result(1'b0, 8'd0, JOIN_RMSNORM, 32'd8,
                           q8_destination);
        if (join_weights_accepted !== 11'd1024 || weight_stalls == 0)
            failures = failures + 1;
        read_destination(10'd0, 1'b0, 272'd0);
        read_destination(10'd17, 1'b0, 272'd0);
        read_destination(10'd31, 1'b0, 272'd0);

        // Domain, epoch, source-count, and alias validation happens before a
        // resident command can mutate destination validity.
        accepts_before_invalid = joined_command_accepts;
        start_join(JOIN_RMSNORM,
                   make_handle(active_epoch, 1'b1, 4'd0), 37'd0,
                   make_handle(active_epoch, 1'b1, 4'd1), 32'd9);
        expect_join_result(1'b1, ERROR_HANDLE, JOIN_RMSNORM, 32'd9, 37'd0);
        start_join(JOIN_RESIDUAL, source0_handle, source1_handle,
                   source0_handle, 32'd10);
        expect_join_result(1'b1, ERROR_HANDLE, JOIN_RESIDUAL, 32'd10, 37'd0);
        start_join(JOIN_RESIDUAL, source0_handle,
                   make_handle(active_epoch + 32'd1, 1'b0, 4'd1),
                   q30_destination, 32'd11);
        expect_join_result(1'b1, ERROR_HANDLE, JOIN_RESIDUAL, 32'd11, 37'd0);
        if (joined_command_accepts != accepts_before_invalid || join_poisoned)
            failures = failures + 1;

        // A malformed first RMS weight starts no publishable replacement.
        // It poisons this fixed controller until clear and the destination
        // remains unreadable even though its payload RAM received no reset.
        start_join(JOIN_RMSNORM, source0_handle, 37'd0,
                   make_handle(active_epoch, 1'b1, 4'd1), 32'd12);
        @(negedge clk);
        join_weight_index = 10'd1;
        join_weight_bf16 = 1'b1;
        join_weight_bits = 32'h00003f80;
        join_weight_valid = 1'b1;
        while (!join_weight_ready)
            @(negedge clk);
        @(negedge clk);
        join_weight_valid = 1'b0;
        expect_join_result(1'b1, ERROR_WEIGHT_ORDER,
                           JOIN_RMSNORM, 32'd12, 37'd0);
        if (!join_poisoned || join_start_ready)
            failures = failures + 1;
        read_destination(10'd0, 1'b1, 272'd0);

        // Clear resets only control/valid metadata, then a fresh nonzero epoch
        // recovers the exact same fixed circuit.
        clear_and_reown();
        establish_session(32'h6e00_0002);

        // Abort after multiple residual elements have physically entered the
        // destination transaction.  The partially written slot is unpublished.
        start_join(JOIN_RESIDUAL, source0_handle, source1_handle,
                   q30_destination, 32'd21);
        while (resident.store.q30_transaction_count[2] < 11'd8)
            @(negedge clk);
        pulse_abort();
        expect_join_result(1'b1, ERROR_ABORT,
                           JOIN_RESIDUAL, 32'd21, 37'd0);
        if (!join_poisoned)
            failures = failures + 1;
        read_destination(10'd0, 1'b1, 272'd0);

        // A second reset/new epoch proves poison and transaction recovery,
        // then completes all 1,024 signed residual elements again.
        clear_and_reown();
        establish_session(32'h6e00_0003);
        start_join(JOIN_RESIDUAL, source0_handle, source1_handle,
                   q30_destination, 32'd22);
        expect_join_result(1'b0, 8'd0,
                           JOIN_RESIDUAL, 32'd22, q30_destination);
        read_destination(10'd1023, 1'b0,
            q30_inspect_value(source0_value(1023) + source1_value(1023)));

        if (failures == 0)
            $display("PASS resident_norm_residual_join typed_handles session_epoch slot_domain token_position rmsnorm=1024xbf16->32q8 residual=1024signed_q30 exact_order backpressure=stable abort_poison=sticky partial_destination=unpublished reset_recovery=new_epoch");
        else
            $display("FAIL resident_norm_residual_join failures=%0d", failures);
        $finish;
    end

    wire unused_resident_busy = resident_busy;
endmodule
