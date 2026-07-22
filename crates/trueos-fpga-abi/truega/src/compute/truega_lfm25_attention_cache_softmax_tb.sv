`timescale 1ns/1ps

module truega_lfm25_attention_cache_softmax_tb;
    localparam signed [63:0] Q30_ONE = 64'sd1073741824;
    reg clk = 1'b0;
    reg reset_n = 1'b0;
    integer failures = 0;
    integer cycles;
    integer i;
    reg signed [63:0] stable_a;
    reg signed [63:0] stable_b;

    always #5 clk = ~clk;

    reg addr_start = 1'b0;
    reg addr_append = 1'b0;
    reg addr_reset_layer = 1'b0;
    reg [3:0] addr_layer = 4'd0;
    reg [16:0] addr_position = 17'd0;
    reg [3:0] addr_q_head = 4'd0;
    reg [2:0] addr_kv_head = 3'd0;
    reg [5:0] addr_element = 6'd0;
    reg addr_value = 1'b0;
    wire addr_busy;
    wire addr_done;
    wire addr_error;
    wire [2:0] addr_mapped_head;
    wire [16:0] addr_valid_positions;
    wire [29:0] addr_word;
    wire [32:0] addr_byte;

    truega_lfm25_kv_address_slot #(.INITIAL_CONTEXT(4)) address (
        .clk(clk), .reset_n(reset_n), .start_i(addr_start),
        .append_i(addr_append), .reset_layer_i(addr_reset_layer),
        .layer_i(addr_layer), .position_i(addr_position),
        .query_head_i(addr_q_head), .kv_head_i(addr_kv_head),
        .element_i(addr_element), .value_i(addr_value),
        .busy_o(addr_busy), .done_o(addr_done), .error_o(addr_error),
        .mapped_kv_head_o(addr_mapped_head),
        .valid_positions_o(addr_valid_positions),
        .word_address_o(addr_word), .byte_address_o(addr_byte)
    );

    reg cache_start = 1'b0;
    reg cache_clear = 1'b0;
    reg cache_write = 1'b0;
    reg [16:0] cache_position = 17'd0;
    reg [2:0] cache_head = 3'd0;
    reg [5:0] cache_element = 6'd0;
    reg cache_value = 1'b0;
    reg signed [63:0] cache_write_data = 64'sd0;
    wire cache_busy;
    wire cache_done;
    wire cache_error;
    wire [16:0] cache_valid_positions;
    wire [11:0] cache_word;
    wire signed [63:0] cache_read_data;

    truega_lfm25_kv_cache_slot #(.CACHE_POSITIONS(4)) cache (
        .clk(clk), .reset_n(reset_n), .start_i(cache_start),
        .clear_i(cache_clear), .write_i(cache_write),
        .position_i(cache_position), .kv_head_i(cache_head),
        .element_i(cache_element), .value_i(cache_value),
        .write_q30_i(cache_write_data), .busy_o(cache_busy),
        .done_o(cache_done), .error_o(cache_error),
        .valid_positions_o(cache_valid_positions),
        .word_address_o(cache_word), .read_q30_o(cache_read_data)
    );

    reg dot_start = 1'b0;
    reg [3:0] dot_q_head = 4'd0;
    reg [2:0] dot_kv_head = 3'd0;
    reg dot_valid = 1'b0;
    reg dot_last = 1'b0;
    reg signed [63:0] dot_q = 64'sd0;
    reg signed [63:0] dot_k = 64'sd0;
    wire dot_ready;
    wire dot_busy;
    wire dot_done;
    wire dot_error;
    wire [2:0] dot_mapped_head;
    wire signed [63:0] dot_score;

    truega_lfm25_gqa_dot_slot dot (
        .clk(clk), .reset_n(reset_n), .start_i(dot_start),
        .query_head_i(dot_q_head), .kv_head_i(dot_kv_head),
        .sample_valid_i(dot_valid), .sample_last_i(dot_last),
        .query_q30_i(dot_q), .key_q30_i(dot_k),
        .sample_ready_o(dot_ready), .busy_o(dot_busy), .done_o(dot_done),
        .error_o(dot_error), .mapped_kv_head_o(dot_mapped_head),
        .score_q30_o(dot_score)
    );

    reg soft_start = 1'b0;
    reg soft_begin = 1'b0;
    reg soft_last = 1'b0;
    reg signed [63:0] soft_score = 64'sd0;
    reg signed [63:0] soft_value = 64'sd0;
    wire soft_busy;
    wire soft_done;
    wire soft_error;
    wire soft_result_valid;
    wire signed [63:0] soft_result;

    truega_lfm25_online_softmax_value_slot softmax (
        .clk(clk), .reset_n(reset_n), .start_i(soft_start),
        .begin_i(soft_begin), .last_i(soft_last),
        .score_q30_i(soft_score), .value_q30_i(soft_value),
        .busy_o(soft_busy), .done_o(soft_done), .error_o(soft_error),
        .result_valid_o(soft_result_valid), .result_q30_o(soft_result)
    );

    task address_call;
        input append;
        input [3:0] layer;
        input [16:0] position;
        input [3:0] q_head;
        input [2:0] kv_head;
        input [5:0] element;
        input value_kind;
        begin
            @(negedge clk);
            addr_append = append;
            addr_reset_layer = 1'b0;
            addr_layer = layer;
            addr_position = position;
            addr_q_head = q_head;
            addr_kv_head = kv_head;
            addr_element = element;
            addr_value = value_kind;
            addr_start = 1'b1;
            @(negedge clk);
            addr_start = 1'b0;
            while (!addr_done) @(negedge clk);
        end
    endtask

    task cache_call;
        input write;
        input [16:0] position;
        input [2:0] head;
        input [5:0] element;
        input value_kind;
        input signed [63:0] data;
        begin
            @(negedge clk);
            cache_clear = 1'b0;
            cache_write = write;
            cache_position = position;
            cache_head = head;
            cache_element = element;
            cache_value = value_kind;
            cache_write_data = data;
            cache_start = 1'b1;
            @(negedge clk);
            cache_start = 1'b0;
            while (!cache_done) @(negedge clk);
        end
    endtask

    task soft_call;
        input first;
        input final_sample;
        input signed [63:0] score;
        input signed [63:0] value;
        begin
            @(negedge clk);
            soft_begin = first;
            soft_last = final_sample;
            soft_score = score;
            soft_value = value;
            soft_start = 1'b1;
            @(negedge clk);
            soft_start = 1'b0;
            while (!soft_done) @(negedge clk);
            if (soft_error) begin
                $display("FAIL softmax unexpected error");
                failures = failures + 1;
            end
        end
    endtask

    initial begin
        repeat (4) @(negedge clk);
        reset_n = 1'b1;

        // Commit two positions and prove the 16:8 GQA mapping and exact layout.
        address_call(1'b1, 4'd2, 17'd0, 4'd7, 3'd0, 6'd0, 1'b0);
        if (addr_error || addr_word != 30'd0 || addr_byte != 33'd0
            || addr_mapped_head != 3'd3) begin
            $display("FAIL kv address first word=%0d byte=%0d mapped=%0d error=%b",
                addr_word, addr_byte, addr_mapped_head, addr_error);
            failures = failures + 1;
        end
        address_call(1'b1, 4'd2, 17'd0, 4'd15, 3'd7, 6'd63, 1'b1);
        if (addr_error || addr_word != 30'd1023
            || addr_valid_positions != 17'd1 || addr_mapped_head != 3'd7) begin
            $display("FAIL kv address commit0 word=%0d valid=%0d mapped=%0d error=%b",
                addr_word, addr_valid_positions, addr_mapped_head, addr_error);
            failures = failures + 1;
        end
        address_call(1'b1, 4'd2, 17'd1, 4'd6, 3'd7, 6'd63, 1'b1);
        if (addr_error || addr_word != 30'd2047
            || addr_valid_positions != 17'd2 || addr_mapped_head != 3'd3) begin
            $display("FAIL kv address commit1 word=%0d valid=%0d mapped=%0d error=%b",
                addr_word, addr_valid_positions, addr_mapped_head, addr_error);
            failures = failures + 1;
        end
        address_call(1'b0, 4'd2, 17'd0, 4'd7, 3'd0, 6'd0, 1'b0);
        if (addr_error || addr_word != 30'd0) begin
            $display("FAIL kv address read0 word=%0d error=%b", addr_word, addr_error);
            failures = failures + 1;
        end

        // Actual FPGA-local storage: write K[0,0,0], commit V tail, then read K.
        cache_call(1'b1, 17'd0, 3'd0, 6'd0, 1'b0, 64'sd123456789);
        cache_call(1'b1, 17'd0, 3'd7, 6'd63, 1'b1, -64'sd99);
        if (cache_error || cache_valid_positions != 17'd1) begin
            $display("FAIL kv cache commit valid=%0d error=%b",
                cache_valid_positions, cache_error);
            failures = failures + 1;
        end
        cache_call(1'b0, 17'd0, 3'd0, 6'd0, 1'b0, 64'sd0);
        if (cache_error || cache_word != 12'd0
            || cache_read_data !== 64'sd123456789) begin
            $display("FAIL kv cache read data=%0d word=%0d error=%b",
                cache_read_data, cache_word, cache_error);
            failures = failures + 1;
        end

        // 64 products of 0.5 * 0.25 sum to 8.0, then scale 1/8 = 1.0.
        @(negedge clk);
        dot_q_head = 4'd7;
        dot_kv_head = 3'd3;
        dot_start = 1'b1;
        @(negedge clk);
        dot_start = 1'b0;
        for (i = 0; i < 64; i = i + 1) begin
            while (!dot_ready) @(negedge clk);
            dot_q = 64'sd536870912;
            dot_k = 64'sd268435456;
            dot_last = i == 63;
            dot_valid = 1'b1;
            @(negedge clk);
            dot_valid = 1'b0;
        end
        cycles = 0;
        while (!dot_done && cycles < 6000) begin
            @(negedge clk);
            cycles = cycles + 1;
        end
        if (!dot_done || dot_error || dot_mapped_head != 3'd3
            || dot_score !== Q30_ONE) begin
            $display("FAIL gqa dot score=%0d mapped=%0d error=%b cycles=%0d",
                dot_score, dot_mapped_head, dot_error, cycles);
            failures = failures + 1;
        end

        // exp weights are 1, 1/2, 1/4.  Compare with the real-equation Q30
        // result 1.142857142857; polynomial bound permits 2e-4 absolute.
        soft_call(1'b1, 1'b0, 64'sd0, Q30_ONE);
        soft_call(1'b0, 1'b0, -64'sd744261118, 64'sd3221225472);
        soft_call(1'b0, 1'b1, -64'sd1488522236, -64'sd2147483648);
        if (!soft_result_valid
            || soft_result < 64'sd1226918764
            || soft_result > 64'sd1227348262) begin
            $display("FAIL online softmax result=%0d expected~=1227133513", soft_result);
            failures = failures + 1;
        end

        // Stable max subtraction: adding +10 to every score is bit-exact.
        soft_call(1'b1, 1'b0, 64'sd0, -64'sd536870912);
        soft_call(1'b0, 1'b0, -Q30_ONE, 64'sd2147483648);
        soft_call(1'b0, 1'b1, -64'sd2147483648, 64'sd4294967296);
        stable_a = soft_result;
        soft_call(1'b1, 1'b0, 64'sd10737418240, -64'sd536870912);
        soft_call(1'b0, 1'b0, 64'sd9663676416, 64'sd2147483648);
        soft_call(1'b0, 1'b1, 64'sd8589934592, 64'sd4294967296);
        stable_b = soft_result;
        if (stable_a !== stable_b) begin
            $display("FAIL online softmax shift invariance a=%0d b=%0d", stable_a, stable_b);
            failures = failures + 1;
        end

        if (failures == 0) begin
            $display("PASS lfm25_attention_cache_softmax positions=2 gqa=16:8 scale=1/8 cache=rw online=stable exp_abs<=1.6e-4");
            $finish;
        end
        $display("FAIL lfm25_attention_cache_softmax failures=%0d", failures);
        $finish_and_return(1);
    end
endmodule
