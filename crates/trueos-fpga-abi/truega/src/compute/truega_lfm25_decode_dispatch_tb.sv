`timescale 1ns/1ps
module truega_lfm25_decode_dispatch_tb;
    reg clk = 0;
    always #5 clk = ~clk;
    reg reset_n = 0;
    reg [31:0] command = 0;
    reg [31:0] position = 0;
    reg [31:0] epoch = 0;
    reg doorbell = 0;
    reg [31:0] doorbell_value = 0;
    wire [31:0] magic;
    wire [31:0] capability;
    wire [31:0] state;
    wire [31:0] result0;
    wire [31:0] result1;
    wire signed [63:0] score;
    wire execute_start;
    wire [3:0] execute_operation;
    wire [7:0] execute_layer;
    wire [31:0] execute_position;
    wire [7:0] execute_input_slot;
    wire [7:0] execute_residual_slot;
    wire [31:0] execute_epoch;
    wire session_begin;
    reg engine_done = 0;
    reg engine_error = 0;
    reg [31:0] engine_error_code = 0;
    reg [7:0] engine_result_slot = 0;
    reg [31:0] engine_result_position = 0;
    reg [31:0] engine_argmax_token = 0;
    reg [31:0] engine_argmax_rows = 0;
    reg signed [63:0] engine_argmax_score = 0;
    wire retire;
    wire [31:0] disabled_magic;
    wire [31:0] disabled_capability;
    integer failures = 0;

    truega_lfm25_decode_dispatch #(.ENABLE(1)) dut (
        .clk(clk), .reset_n(reset_n), .command_i(command),
        .position_i(position), .session_epoch_i(epoch),
        .doorbell_i(doorbell), .doorbell_value_i(doorbell_value),
        .capability_magic_o(magic), .capability_bits_o(capability),
        .state_o(state), .result0_o(result0), .result1_o(result1),
        .argmax_score_q30_o(score), .execute_start_o(execute_start),
        .execute_operation_o(execute_operation), .execute_layer_o(execute_layer),
        .execute_position_o(execute_position), .execute_input_slot_o(execute_input_slot),
        .execute_residual_slot_o(execute_residual_slot),
        .execute_session_epoch_o(execute_epoch),
        .execute_session_begin_o(session_begin), .engine_done_i(engine_done),
        .engine_error_i(engine_error), .engine_error_code_i(engine_error_code),
        .engine_result_slot_i(engine_result_slot),
        .engine_result_position_i(engine_result_position),
        .engine_argmax_token_i(engine_argmax_token),
        .engine_argmax_rows_i(engine_argmax_rows),
        .engine_argmax_score_q30_i(engine_argmax_score), .retire_o(retire)
    );

    truega_lfm25_decode_dispatch disabled (
        .clk(clk), .reset_n(reset_n), .command_i(32'd0),
        .position_i(32'd0), .session_epoch_i(32'd0),
        .doorbell_i(1'b0), .doorbell_value_i(32'd0),
        .capability_magic_o(disabled_magic),
        .capability_bits_o(disabled_capability), .state_o(), .result0_o(),
        .result1_o(), .argmax_score_q30_o(), .execute_start_o(),
        .execute_operation_o(), .execute_layer_o(), .execute_position_o(),
        .execute_input_slot_o(), .execute_residual_slot_o(),
        .execute_session_epoch_o(), .execute_session_begin_o(),
        .engine_done_i(1'b0), .engine_error_i(1'b0),
        .engine_error_code_i(32'd0), .engine_result_slot_i(8'd0),
        .engine_result_position_i(32'd0), .engine_argmax_token_i(32'd0),
        .engine_argmax_rows_i(32'd0), .engine_argmax_score_q30_i(64'sd0),
        .retire_o()
    );

    task ring;
        begin
            @(negedge clk);
            doorbell_value = 32'h4f434544;
            doorbell = 1;
            @(negedge clk);
            doorbell = 0;
        end
    endtask

    task complete_resident;
        input [7:0] slot;
        input [31:0] completed_position;
        begin
            @(negedge clk);
            engine_result_slot = slot;
            engine_result_position = completed_position;
            engine_done = 1;
            @(negedge clk);
            engine_done = 0;
        end
    endtask

    initial begin
        repeat (3) @(posedge clk);
        reset_n = 1;
        if (magic !== 32'h31444754 || capability !== 32'h000003ff)
            failures = failures + 1;
        if (disabled_magic !== 32'd0 || disabled_capability !== 32'd0)
            failures = failures + 1;

        // No operation may enter an uninstalled session except position-0 embedding.
        command = {8'hff, 8'd1, 8'd0, 8'd1};
        position = 0;
        epoch = 9;
        ring();
        if (state !== 32'd3 || result0 !== 32'hbad30004 || !retire)
            failures = failures + 1;

        command = 32'hffff_ff00;
        ring();
        if (state !== 32'd1 || !execute_start || !session_begin
            || execute_operation !== 0 || execute_epoch !== 9)
            failures = failures + 1;
        complete_resident(8'd1, 0);
        if (state !== 32'd2 || result0 !== 1 || result1 !== 0 || !retire)
            failures = failures + 1;

        // Operator RMSNorm: layer 0, Q30 input slot 1, no residual.
        command = {8'hff, 8'd1, 8'd0, 8'd1};
        ring();
        if (state !== 32'd1 || !execute_start || session_begin)
            failures = failures + 1;
        complete_resident(8'd2, 0);
        if (state !== 32'd2 || result0 !== 2)
            failures = failures + 1;

        // Full-width signed argmax score uses its dedicated output.
        command = {8'hff, 8'd2, 8'hff, 8'd9};
        ring();
        @(negedge clk);
        engine_argmax_token = 32'd65535;
        engine_argmax_rows = 32'd65536;
        engine_argmax_score = -64'sh1234_5678_9abc_def;
        engine_done = 1;
        @(negedge clk);
        engine_done = 0;
        if (state !== 32'd2 || result0 !== 65535 || result1 !== 65536
            || score !== -64'sh1234_5678_9abc_def || !retire)
            failures = failures + 1;

        // A new nonzero epoch at position zero begins a replacement session.
        command = 32'hffff_ff00;
        position = 0;
        epoch = 10;
        ring();
        if (!session_begin || state !== 32'd1)
            failures = failures + 1;
        complete_resident(8'd1, 0);

        if (failures == 0)
            $display("PASS lfm25_decode_dispatch ops=10 exact_magic epoch=install+replace retire=single argmax=i64");
        else
            $display("FAIL lfm25_decode_dispatch failures=%0d", failures);
        $finish;
    end
endmodule
