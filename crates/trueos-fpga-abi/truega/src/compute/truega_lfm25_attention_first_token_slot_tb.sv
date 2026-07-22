`timescale 1ns/1ps

module truega_lfm25_attention_first_token_slot_tb;
    localparam signed [63:0] Q30_ONE  = 64'sd1073741824;
    localparam signed [63:0] Q30_HALF = 64'sd536870912;

    reg clk = 1'b0;
    reg reset_n = 1'b0;

    reg norm_weight_valid_i = 1'b0;
    wire norm_weight_ready_o;
    reg norm_weight_key_i = 1'b0;
    reg [5:0] norm_weight_element_i = 6'd0;
    reg norm_weight_format_bf16_i = 1'b0;
    reg [31:0] norm_weight_bits_i = 32'd0;
    wire norm_weights_loaded_o;
    wire norm_weight_error_o;

    reg start_i = 1'b0;
    wire start_ready_o;
    reg [3:0] layer_i = 4'd2;
    reg [16:0] position_i = 17'd0;
    reg projected_valid_i = 1'b0;
    reg projected_last_i = 1'b0;
    reg signed [63:0] projected_q30_i = 64'sd0;
    wire projected_ready_o;

    wire attention_valid_o;
    reg attention_ready_i = 1'b1;
    wire [9:0] attention_index_o;
    wire signed [63:0] attention_q30_o;
    wire attention_last_o;
    wire busy_o;
    wire done_o;
    wire error_o;
    wire [16:0] valid_positions_o;

    integer failures = 0;
    integer weight_index;
    integer stream_index;
    integer output_count;
    integer cycles;
    integer vector_index;
    integer kv_index;
    integer layer_index;
    reg signed [63:0] input_value;
    reg signed [63:0] expected_value;
    reg [9:0] stalled_index;
    reg signed [63:0] stalled_value;
    reg stalled_last;
    reg output_stall_exercised = 1'b0;
    reg weight_backpressure_exercised = 1'b0;
    reg [3:0] attention_layers [0:5];

    always #5 clk = ~clk;

    truega_lfm25_attention_first_token_slot dut (
        .clk(clk),
        .reset_n(reset_n),
        .norm_weight_valid_i(norm_weight_valid_i),
        .norm_weight_ready_o(norm_weight_ready_o),
        .norm_weight_key_i(norm_weight_key_i),
        .norm_weight_element_i(norm_weight_element_i),
        .norm_weight_format_bf16_i(norm_weight_format_bf16_i),
        .norm_weight_bits_i(norm_weight_bits_i),
        .norm_weights_loaded_o(norm_weights_loaded_o),
        .norm_weight_error_o(norm_weight_error_o),
        .start_i(start_i),
        .start_ready_o(start_ready_o),
        .layer_i(layer_i),
        .position_i(position_i),
        .projected_valid_i(projected_valid_i),
        .projected_last_i(projected_last_i),
        .projected_q30_i(projected_q30_i),
        .projected_ready_o(projected_ready_o),
        .attention_valid_o(attention_valid_o),
        .attention_ready_i(attention_ready_i),
        .attention_index_o(attention_index_o),
        .attention_q30_o(attention_q30_o),
        .attention_last_o(attention_last_o),
        .busy_o(busy_o),
        .done_o(done_o),
        .error_o(error_o),
        .valid_positions_o(valid_positions_o)
    );

    task load_weight;
        input key;
        input [5:0] element;
        input format_bf16;
        input [31:0] bits;
        input expect_error;
        begin
            while (!norm_weight_ready_o) @(negedge clk);
            norm_weight_key_i = key;
            norm_weight_element_i = element;
            norm_weight_format_bf16_i = format_bf16;
            norm_weight_bits_i = bits;
            norm_weight_valid_i = 1'b1;
            @(negedge clk);
            norm_weight_valid_i = 1'b0;
            if (norm_weight_error_o !== expect_error) begin
                $display("FAIL weight key=%0d element=%0d error=%b expected=%b",
                    key, element, norm_weight_error_o, expect_error);
                failures = failures + 1;
            end
            @(negedge clk);
        end
    endtask

    task rejected_wrapper_start;
        input [3:0] layer;
        input [16:0] position;
        begin
            while (!start_ready_o) @(negedge clk);
            layer_i = layer;
            position_i = position;
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            if (!done_o || !error_o || busy_o) begin
                $display("FAIL rejected wrapper start layer=%0d position=%0d done=%b error=%b busy=%b",
                    layer, position, done_o, error_o, busy_o);
                failures = failures + 1;
            end
            @(negedge clk);
        end
    endtask

    task malformed_projection;
        input [3:0] layer;
        begin
            while (!start_ready_o) @(negedge clk);
            layer_i = layer;
            position_i = 17'd0;
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            while (!projected_ready_o) @(negedge clk);
            projected_q30_i = 64'sd0;
            projected_valid_i = 1'b1;
            projected_last_i = 1'b1;
            @(negedge clk);
            projected_valid_i = 1'b0;
            projected_last_i = 1'b0;
            if (!done_o || !error_o || busy_o || valid_positions_o != 0) begin
                $display("FAIL malformed projection done=%b error=%b busy=%b valid_positions=%0d",
                    done_o, error_o, busy_o, valid_positions_o);
                failures = failures + 1;
            end
            @(negedge clk);
        end
    endtask

    task run_first_token;
        input [3:0] layer;
        input exercise_backpressure;
        begin
            while (!start_ready_o) @(negedge clk);
            layer_i = layer;
            position_i = 17'd0;
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            if (!busy_o || error_o) begin
                $display("FAIL accepted start layer=%0d busy=%b error=%b",
                    layer, busy_o, error_o);
                failures = failures + 1;
            end

            if (exercise_backpressure) begin
                // Model writes must not perturb the fixed weights while the
                // composite owns the single transaction lane.
                norm_weight_key_i = 1'b0;
                norm_weight_element_i = 6'd0;
                norm_weight_format_bf16_i = 1'b0;
                norm_weight_bits_i = 32'h40000000;
                norm_weight_valid_i = 1'b1;
                repeat (3) begin
                    if (norm_weight_ready_o !== 1'b0) begin
                        $display("FAIL weight upload was not backpressured while busy");
                        failures = failures + 1;
                    end
                    @(negedge clk);
                end
                norm_weight_valid_i = 1'b0;
                weight_backpressure_exercised = 1'b1;
            end

            while (!projected_ready_o) @(negedge clk);
            for (stream_index = 0; stream_index < 2048;
                 stream_index = stream_index + 1) begin
                // Source-side gaps verify that projected data advances only
                // on valid/ready handshakes.
                if (exercise_backpressure && stream_index != 0
                    && stream_index % 257 == 0) begin
                    projected_valid_i = 1'b0;
                    projected_last_i = 1'b0;
                    @(negedge clk);
                end

                input_value = 64'sd0;
                if (stream_index < 1024) begin
                    if (stream_index % 64 == 0)
                        input_value = Q30_HALF;
                    else if (stream_index % 64 == 32)
                        input_value = Q30_HALF >>> 1;
                end else if (stream_index >= 1536) begin
                    vector_index = stream_index - 1536;
                    input_value = (vector_index + 1) * 64'sd1048576;
                end
                projected_q30_i = input_value;
                projected_last_i = stream_index == 2047;
                projected_valid_i = 1'b1;
                if (!projected_ready_o) begin
                    $display("FAIL projected input unexpectedly backpressured index=%0d",
                        stream_index);
                    failures = failures + 1;
                    while (!projected_ready_o) @(negedge clk);
                end
                @(negedge clk);
            end
            projected_valid_i = 1'b0;
            projected_last_i = 1'b0;

            output_count = 0;
            cycles = 0;
            attention_ready_i = 1'b1;
            while (!done_o && cycles < 4000000) begin
                @(negedge clk);
                cycles = cycles + 1;
                if (attention_valid_o) begin
                    if (exercise_backpressure && !output_stall_exercised
                        && attention_index_o == 10'd7) begin
                        stalled_index = attention_index_o;
                        stalled_value = attention_q30_o;
                        stalled_last = attention_last_o;
                        attention_ready_i = 1'b0;
                        repeat (4) begin
                            @(negedge clk);
                            cycles = cycles + 1;
                            if (!attention_valid_o
                                || attention_index_o !== stalled_index
                                || attention_q30_o !== stalled_value
                                || attention_last_o !== stalled_last) begin
                                $display("FAIL output changed under backpressure index=%0d",
                                    stalled_index);
                                failures = failures + 1;
                            end
                        end
                        attention_ready_i = 1'b1;
                        output_stall_exercised = 1'b1;
                    end

                    if (attention_ready_i) begin
                        kv_index = ((attention_index_o / 64) / 2) * 64
                            + (attention_index_o % 64);
                        expected_value = (kv_index + 1) * 64'sd1048576;
                        if (attention_q30_o !== expected_value
                            || attention_index_o !== output_count[9:0]
                            || attention_last_o !== (output_count == 1023)) begin
                            $display("FAIL layer=%0d output=%0d index=%0d got=%0d expected=%0d last=%b",
                                layer, output_count, attention_index_o,
                                attention_q30_o, expected_value,
                                attention_last_o);
                            failures = failures + 1;
                        end
                        output_count = output_count + 1;
                    end
                end
            end
            attention_ready_i = 1'b1;
            if (!done_o || error_o || output_count != 1024
                || valid_positions_o != 1) begin
                $display("FAIL first token layer=%0d done=%b error=%b outputs=%0d valid_positions=%0d cycles=%0d",
                    layer, done_o, error_o, output_count,
                    valid_positions_o, cycles);
                failures = failures + 1;
            end else begin
                $display("PASS lfm25_attention_first_token layer=%0d outputs=1024 cycles=%0d",
                    layer, cycles);
            end
            @(negedge clk);
        end
    endtask

    task rejected_consumed_layer;
        input [3:0] layer;
        begin
            while (!start_ready_o) @(negedge clk);
            layer_i = layer;
            position_i = 17'd0;
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            if (!done_o || !error_o || busy_o) begin
                $display("FAIL consumed layer did not retain first-token state layer=%0d done=%b error=%b busy=%b",
                    layer, done_o, error_o, busy_o);
                failures = failures + 1;
            end
            @(negedge clk);
        end
    endtask

    initial begin
        attention_layers[0] = 4'd2;
        attention_layers[1] = 4'd5;
        attention_layers[2] = 4'd8;
        attention_layers[3] = 4'd10;
        attention_layers[4] = 4'd12;
        attention_layers[5] = 4'd14;

        repeat (4) @(negedge clk);
        reset_n = 1'b1;
        @(negedge clk);

        // No transaction may begin from a partial model upload.
        rejected_wrapper_start(4'd2, 17'd0);

        // Invalid model bits are reported and leave that element unloaded.
        load_weight(1'b0, 6'd0, 1'b0, 32'h7f800000, 1'b1);
        if (norm_weights_loaded_o) begin
            $display("FAIL invalid weight marked model weights complete");
            failures = failures + 1;
        end

        for (weight_index = 0; weight_index < 64;
             weight_index = weight_index + 1) begin
            // Q[0] exercises F32 0.5; the remainder alternate F32/BF16 1.0.
            if (weight_index == 0)
                load_weight(1'b0, weight_index[5:0], 1'b0,
                    32'h3f000000, 1'b0);
            else if (weight_index[0])
                load_weight(1'b0, weight_index[5:0], 1'b1,
                    32'h00003f80, 1'b0);
            else
                load_weight(1'b0, weight_index[5:0], 1'b0,
                    32'h3f800000, 1'b0);
        end
        for (weight_index = 0; weight_index < 64;
             weight_index = weight_index + 1) begin
            // K[0] exercises signed BF16 -0.5.
            load_weight(1'b1, weight_index[5:0], 1'b1,
                weight_index == 0 ? 32'h0000bf00 : 32'h00003f80,
                1'b0);
        end

        if (!norm_weights_loaded_o
            || dut.q_norm_weight_q30[0] !== Q30_HALF
            || dut.q_norm_weight_q30[1] !== Q30_ONE
            || dut.k_norm_weight_q30[0] !== -Q30_HALF
            || dut.k_norm_weight_q30[1] !== Q30_ONE) begin
            $display("FAIL raw F32/BF16 norm-weight conversion or completeness");
            failures = failures + 1;
        end

        // This boundary is intentionally position zero only.
        rejected_wrapper_start(4'd2, 17'd1);

        // A malformed stream must not consume the layer's only cache slot.
        malformed_projection(4'd2);

        for (layer_index = 0; layer_index < 6;
             layer_index = layer_index + 1)
            run_first_token(attention_layers[layer_index], layer_index == 0);

        // Each of the six layer-local first-token states survived subsequent
        // transactions for the other layers.
        for (layer_index = 0; layer_index < 6;
             layer_index = layer_index + 1)
            rejected_consumed_layer(attention_layers[layer_index]);

        if (!weight_backpressure_exercised || !output_stall_exercised) begin
            $display("FAIL required backpressure checks were not exercised");
            failures = failures + 1;
        end

        if (failures == 0) begin
            $display("PASS lfm25_attention_first_token_baseline layers=6 cache_positions=1 external_cache=0 raw_norm=f32+bf16 rope=position0-only");
            $finish;
        end
        $display("FAIL lfm25_attention_first_token_baseline failures=%0d",
            failures);
        $finish_and_return(1);
    end
endmodule
