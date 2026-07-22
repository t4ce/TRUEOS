`timescale 1ns/1ps

module truega_lfm25_attention_token_slot_tb;
    localparam signed [63:0] Q30_ONE = 64'sd1073741824;
    reg clk = 1'b0;
    reg reset_n = 1'b0;
    reg start_i = 1'b0;
    reg [3:0] layer_i = 4'd2;
    reg [16:0] position_i = 17'd0;
    reg projected_valid_i = 1'b0;
    reg projected_last_i = 1'b0;
    reg signed [63:0] projected_q30_i = 64'sd0;
    wire projected_ready_o;
    wire norm_req_valid_o;
    wire norm_req_key_o;
    wire [5:0] norm_req_element_o;
    wire rope_req_valid_o;
    wire [16:0] rope_req_position_o;
    wire [4:0] rope_req_pair_o;
    wire cache_req_valid_o;
    wire cache_req_write_o;
    wire [29:0] cache_req_word_address_o;
    wire signed [63:0] cache_req_write_q30_o;
    reg cache_rsp_valid_i = 1'b0;
    reg signed [63:0] cache_rsp_read_q30_i = 64'sd0;
    reg signed [63:0] ddr_cache [0:12287];
    wire attention_valid_o;
    reg attention_ready_i = 1'b1;
    wire [9:0] attention_index_o;
    wire signed [63:0] attention_q30_o;
    wire attention_last_o;
    wire busy_o;
    wire done_o;
    wire error_o;
    wire [16:0] valid_positions_o;

    wire norm_rsp_valid_i = norm_req_valid_o;
    wire signed [63:0] norm_rsp_weight_q30_i = Q30_ONE;
    wire rope_rsp_valid_i = rope_req_valid_o;
    wire signed [63:0] rope_rsp_cos_q30_i = rope_req_position_o == 0
        ? Q30_ONE : (rope_req_pair_o == 0 ? 64'sd580145183 : Q30_ONE);
    wire signed [63:0] rope_rsp_sin_q30_i = rope_req_position_o == 0
        ? 64'sd0 : (rope_req_pair_o == 0 ? 64'sd903522590 : 64'sd0);

    integer failures = 0;
    integer stream_index;
    integer output_count;
    integer cycles;
    integer vector_index;
    integer q_element;
    integer kv_index;
    reg signed [63:0] input_value;
    reg signed [63:0] expected_value;

    always #5 clk = ~clk;
    always @(posedge clk) begin
        cache_rsp_valid_i <= 1'b0;
        if (cache_req_valid_o) begin
            if (cache_req_write_o)
                ddr_cache[cache_req_word_address_o] <= cache_req_write_q30_o;
            else begin
                cache_rsp_read_q30_i <= ddr_cache[cache_req_word_address_o];
                cache_rsp_valid_i <= 1'b1;
            end
        end
    end

    truega_lfm25_attention_token_slot #(
        .CACHE_POSITIONS(2), .EXTERNAL_CACHE(1)
    ) dut (
        .clk(clk), .reset_n(reset_n), .start_i(start_i),
        .layer_i(layer_i), .position_i(position_i),
        .projected_valid_i(projected_valid_i),
        .projected_last_i(projected_last_i),
        .projected_q30_i(projected_q30_i),
        .projected_ready_o(projected_ready_o),
        .norm_req_valid_o(norm_req_valid_o), .norm_req_key_o(norm_req_key_o),
        .norm_req_element_o(norm_req_element_o),
        .norm_rsp_valid_i(norm_rsp_valid_i),
        .norm_rsp_weight_q30_i(norm_rsp_weight_q30_i),
        .rope_req_valid_o(rope_req_valid_o),
        .rope_req_position_o(rope_req_position_o),
        .rope_req_pair_o(rope_req_pair_o),
        .rope_rsp_valid_i(rope_rsp_valid_i),
        .rope_rsp_cos_q30_i(rope_rsp_cos_q30_i),
        .rope_rsp_sin_q30_i(rope_rsp_sin_q30_i),
        .cache_req_valid_o(cache_req_valid_o),
        .cache_req_write_o(cache_req_write_o),
        .cache_req_word_address_o(cache_req_word_address_o),
        .cache_req_write_q30_o(cache_req_write_q30_o),
        .cache_req_ready_i(1'b1), .cache_rsp_valid_i(cache_rsp_valid_i),
        .cache_rsp_read_q30_i(cache_rsp_read_q30_i),
        .attention_valid_o(attention_valid_o),
        .attention_ready_i(attention_ready_i),
        .attention_index_o(attention_index_o),
        .attention_q30_o(attention_q30_o),
        .attention_last_o(attention_last_o),
        .busy_o(busy_o), .done_o(done_o), .error_o(error_o),
        .valid_positions_o(valid_positions_o)
    );

    task run_token;
        input [16:0] token_position;
        integer value_factor;
        begin
            value_factor = token_position == 0 ? 1 : 3;
            @(negedge clk);
            position_i = token_position;
            start_i = 1'b1;
            @(negedge clk);
            start_i = 1'b0;
            while (!projected_ready_o) @(negedge clk);

            for (stream_index = 0; stream_index < 2048;
                 stream_index = stream_index + 1) begin
                input_value = 64'sd0;
                if (stream_index < 1024) begin
                    // Exercise non-zero Q RMSNorm and pair-0 NEOX RoPE while
                    // K=0 keeps the reference attention scores exactly zero.
                    q_element = stream_index % 64;
                    if (q_element == 0) input_value = 64'sd536870912;
                    else if (q_element == 32) input_value = 64'sd268435456;
                end else if (stream_index >= 1536) begin
                    vector_index = stream_index - 1536;
                    input_value = (vector_index + 1) * 64'sd1048576
                        * value_factor;
                end
                projected_q30_i = input_value;
                projected_last_i = stream_index == 2047;
                projected_valid_i = 1'b1;
                @(negedge clk);
            end
            projected_valid_i = 1'b0;
            projected_last_i = 1'b0;

            output_count = 0;
            cycles = 0;
            while (!done_o && cycles < 4000000) begin
                @(negedge clk);
                cycles = cycles + 1;
                if (attention_valid_o) begin
                    kv_index = ((attention_index_o / 64) / 2) * 64
                        + (attention_index_o % 64);
                    expected_value = (kv_index + 1) * 64'sd1048576;
                    if (token_position != 0)
                        expected_value = expected_value * 2;
                    if (attention_q30_o !== expected_value
                        || attention_index_o !== output_count[9:0]
                        || attention_last_o !== (output_count == 1023)) begin
                        $display("FAIL token=%0d output=%0d index=%0d got=%0d expected=%0d last=%b",
                            token_position, output_count, attention_index_o,
                            attention_q30_o, expected_value, attention_last_o);
                        failures = failures + 1;
                    end
                    output_count = output_count + 1;
                end
            end
            if (!done_o || error_o || output_count != 1024
                || valid_positions_o != token_position + 1) begin
                $display("FAIL token=%0d done=%b error=%b outputs=%0d valid_positions=%0d cycles=%0d",
                    token_position, done_o, error_o, output_count,
                    valid_positions_o, cycles);
                failures = failures + 1;
            end else begin
                $display("PASS lfm25_attention_token position=%0d outputs=1024 valid_positions=%0d cycles=%0d",
                    token_position, valid_positions_o, cycles);
            end
        end
    endtask

    initial begin
        repeat (4) @(negedge clk);
        reset_n = 1'b1;
        run_token(17'd0);
        run_token(17'd1);
        if (failures == 0) begin
            $display("PASS lfm25_attention_composite token0+token1 qkv=2048 rms=24x64 rope=neox kv=stateful gqa=16:8 output=1024");
            $finish;
        end
        $display("FAIL lfm25_attention_composite failures=%0d", failures);
        $finish_and_return(1);
    end
endmodule
