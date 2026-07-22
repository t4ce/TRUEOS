`timescale 1ns/1ps

module truega_lfm25_decode_engine_tb;
    localparam integer PROJECTION_ROWS = 1024;
    localparam [31:0] EPOCH_ONE = 32'hdec0_0001;
    localparam [31:0] EPOCH_TWO = 32'hdec0_0002;

    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg clear = 1'b0;
    reg start = 1'b0;
    wire start_ready;
    reg [31:0] session_epoch = 32'd0;

    reg embedding_valid = 1'b0;
    wire embedding_ready;
    wire [4:0] expected_embedding_block;
    reg [4:0] embedding_block_index = 5'd0;
    reg [271:0] embedding_block = 272'd0;

    reg norm_weight_valid = 1'b0;
    wire norm_weight_ready;
    wire [9:0] expected_norm_weight;
    reg [9:0] norm_weight_index = 10'd0;
    reg norm_weight_bf16 = 1'b0;
    reg [31:0] norm_weight_bits = 32'd0;

    reg projection_weight_valid = 1'b0;
    wire projection_weight_ready;
    wire [12:0] expected_projection_row;
    wire [4:0] expected_projection_block;
    reg [12:0] projection_weight_row = 13'd0;
    reg [4:0] projection_weight_block = 5'd0;
    reg [271:0] projection_weight = 272'd0;

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

    wire [31:0] active_epoch;
    wire [12:0] projection_rows_retired;
    wire busy;

    integer failures = 0;
    integer block_number;
    integer weight_number;
    integer row_number;
    integer cycles = 0;
    reg signed [63:0] captured_row0;
    reg signed [63:0] captured_row1;
    reg signed [63:0] captured_row1023;
    reg [36:0] held_result_handle;
    reg [7:0] held_error_code;

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

    function automatic [271:0] projection_row_block;
        input integer requested_row;
        input integer requested_block;
        begin
            if (requested_block != 0) begin
                projection_row_block = 272'd0;
            end else begin
                case (requested_row)
                    0: projection_row_block = native_block(16'h3800, 8'sd2);
                    1: projection_row_block = native_block(16'h3800, -8'sd2);
                    1023: projection_row_block =
                        native_block(16'h3400, 8'sd1);
                    default: projection_row_block = 272'd0;
                endcase
            end
        end
    endfunction

    truega_lfm25_decode_engine #(
        .PROJECTION_ROWS(PROJECTION_ROWS)
    ) dut (
        .clk(clk), .reset_n(reset_n), .clear_i(clear),
        .start_i(start), .start_ready_o(start_ready),
        .session_epoch_i(session_epoch),
        .embedding_valid_i(embedding_valid),
        .embedding_ready_o(embedding_ready),
        .embedding_block_index_o(expected_embedding_block),
        .embedding_block_index_i(embedding_block_index),
        .embedding_q8_block_i(embedding_block),
        .norm_weight_valid_i(norm_weight_valid),
        .norm_weight_ready_o(norm_weight_ready),
        .norm_weight_index_o(expected_norm_weight),
        .norm_weight_index_i(norm_weight_index),
        .norm_weight_format_bf16_i(norm_weight_bf16),
        .norm_weight_bits_i(norm_weight_bits),
        .projection_weight_valid_i(projection_weight_valid),
        .projection_weight_ready_o(projection_weight_ready),
        .projection_weight_row_o(expected_projection_row),
        .projection_weight_block_o(expected_projection_block),
        .projection_weight_row_i(projection_weight_row),
        .projection_weight_block_i(projection_weight_block),
        .projection_weight_q8_block_i(projection_weight),
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
        .active_session_epoch_o(active_epoch),
        .projection_rows_retired_o(projection_rows_retired),
        .busy_o(busy)
    );

    task automatic begin_session;
        input [31:0] epoch;
        begin
            while (!start_ready)
                @(negedge clk);
            session_epoch = epoch;
            start = 1'b1;
            @(negedge clk);
            start = 1'b0;
        end
    endtask

    task automatic feed_embedding;
        begin
            for (block_number = 0; block_number < 32;
                 block_number = block_number + 1) begin
                while (!embedding_ready)
                    @(negedge clk);
                if (expected_embedding_block !== block_number[4:0])
                    failures = failures + 1;
                embedding_block_index = block_number[4:0];
                // Every element dequantizes to exactly Q30 one.
                embedding_block = constant_native_block(16'h3800, 8'sd2);
                embedding_valid = 1'b1;
                @(negedge clk);
                embedding_valid = 1'b0;
            end
        end
    endtask

    task automatic feed_norm_weights;
        begin
            for (weight_number = 0; weight_number < 1024;
                 weight_number = weight_number + 1) begin
                while (!norm_weight_ready)
                    @(negedge clk);
                if (expected_norm_weight !== weight_number[9:0])
                    failures = failures + 1;
                norm_weight_index = weight_number[9:0];
                norm_weight_bf16 = weight_number[0];
                norm_weight_bits = weight_number[0]
                    ? 32'h00003f80 : 32'h3f800000;
                norm_weight_valid = 1'b1;
                @(negedge clk);
                norm_weight_valid = 1'b0;
            end
        end
    endtask

    task automatic feed_projection_block;
        input integer requested_row;
        input integer requested_block;
        input [271:0] requested_weight;
        begin
            while (!projection_weight_ready)
                @(negedge clk);
            if (expected_projection_row !== requested_row[12:0]
                    || expected_projection_block !== requested_block[4:0])
                failures = failures + 1;
            projection_weight_row = requested_row[12:0];
            projection_weight_block = requested_block[4:0];
            projection_weight = requested_weight;
            projection_weight_valid = 1'b1;
            @(negedge clk);
            projection_weight_valid = 1'b0;
        end
    endtask

    task automatic feed_projection_row;
        input integer requested_row;
        begin
            for (block_number = 0; block_number < 32;
                 block_number = block_number + 1)
                feed_projection_block(requested_row, block_number,
                    projection_row_block(requested_row, block_number));
        end
    endtask

    task automatic inspect_output;
        input [9:0] requested_index;
        input expected_error;
        input signed [63:0] expected_value;
        reg signed [63:0] held_value;
        begin
            output_read_index = requested_index;
            output_read_valid = 1'b1;
            while (!output_read_ready)
                @(negedge clk);
            @(negedge clk);
            output_read_valid = 1'b0;
            while (!output_read_rsp_valid)
                @(negedge clk);
            held_value = output_read_q30;
            repeat (3) begin
                @(negedge clk);
                if (!output_read_rsp_valid
                        || output_read_q30 !== held_value)
                    failures = failures + 1;
            end
            if (output_read_error !== expected_error)
                failures = failures + 1;
            if (!expected_error && held_value !== expected_value)
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
            held_error_code = result_error_code;
            repeat (3) begin
                @(negedge clk);
                if (!result_valid || result_handle !== held_result_handle
                        || result_error_code !== held_error_code)
                    failures = failures + 1;
            end
            if (result_error !== expected_error
                    || result_error_code !== expected_code)
                failures = failures + 1;
            if (result_handle !== expected_handle)
                failures = failures + 1;
            result_ready = 1'b1;
            @(negedge clk);
            result_ready = 1'b0;
        end
    endtask

    // Capture the signed values at the actual projection/import handshake.
    always @(posedge clk) begin
        if (dut.projection_result_valid && dut.projection_result_ready) begin
            case (dut.projection_result_row)
                13'd0: captured_row0 <= dut.projection_result_q30;
                13'd1: captured_row1 <= dut.projection_result_q30;
                13'd1023: captured_row1023 <= dut.projection_result_q30;
                default: begin end
            endcase
        end
    end

    always @(posedge clk) begin
        cycles <= cycles + 1;
        if (cycles > 5_000_000) begin
            $display("FAIL lfm25_decode_engine timeout state=%0d rv_state=%0d projection_state=%0d row=%0d block=%0d",
                dut.state, dut.resident_vectors.state, dut.projection.state,
                expected_projection_row, expected_projection_block);
            $fatal(1);
        end
    end

    initial begin
        captured_row0 = 64'sd0;
        captured_row1 = 64'sd0;
        captured_row1023 = 64'sd0;
        repeat (5) @(negedge clk);
        reset_n = 1'b1;
        repeat (2) @(negedge clk);

        // Zero is not a valid resident-session epoch.
        begin_session(32'd0);
        consume_result(1'b1, 8'd1, 37'd0);

        // Complete executable join.  The projection input is read back from
        // the resident Q8 slot, and all 1,024 outputs commit atomically into a
        // distinct resident Q30 slot.
        begin_session(EPOCH_ONE);
        feed_embedding();
        feed_norm_weights();
        for (row_number = 0; row_number < PROJECTION_ROWS;
             row_number = row_number + 1) begin
            feed_projection_row(row_number);
            if ((row_number & 255) == 255)
                $display("decode_join progress=%0d/%0d", row_number + 1,
                    PROJECTION_ROWS);
        end
        while (!result_valid)
            @(negedge clk);
        if (result_error || result_handle
                !== {EPOCH_ONE, 1'b0, 4'd1}
                || active_epoch !== EPOCH_ONE
                || projection_rows_retired != PROJECTION_ROWS)
            failures = failures + 1;
        if (captured_row0 == 64'sd0 || captured_row1 == 64'sd0
                || captured_row1023 == 64'sd0
                || captured_row0[63] || !captured_row1[63]
                || captured_row1023[63])
            failures = failures + 1;
        inspect_output(10'd0, 1'b0, captured_row0);
        inspect_output(10'd1, 1'b0, captured_row1);
        inspect_output(10'd1023, 1'b0, captured_row1023);
        consume_result(1'b0, 8'd0, {EPOCH_ONE, 1'b0, 4'd1});

        // Start a fresh epoch, let three projection rows enter the import
        // transaction, then violate the ordered row tag.  The joined engine
        // must abort and leave the partially written destination unreadable.
        begin_session(EPOCH_TWO);
        feed_embedding();
        feed_norm_weights();
        feed_projection_row(0);
        feed_projection_row(1);
        feed_projection_row(2);
        while (!projection_weight_ready)
            @(negedge clk);
        projection_weight_row = 13'd4;
        projection_weight_block = 5'd0;
        projection_weight = 272'd0;
        projection_weight_valid = 1'b1;
        @(negedge clk);
        projection_weight_valid = 1'b0;
        consume_result(1'b1, 8'd4, 37'd0);
        inspect_output(10'd0, 1'b1, 64'sd0);

        if (failures == 0)
            $display("PASS lfm25_decode_engine joined=embedding->rmsnorm->projection1024->resident_import typed_epoch_handles exact_signed_i64_flow stable_result transactional_partial_abort=unreadable no_host_math no_payload_reset");
        else begin
            $display("FAIL lfm25_decode_engine failures=%0d", failures);
            $fatal(1);
        end
        $finish;
    end
endmodule
