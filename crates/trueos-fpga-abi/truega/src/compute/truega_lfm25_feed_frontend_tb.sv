`timescale 1ns/1ps

module truega_lfm25_feed_frontend_tb;
    reg clk = 1'b0;
    always #5 clk = ~clk;

    reg reset_n = 1'b0;
    reg state_reset = 1'b0;
    reg bar_valid = 1'b0;
    reg [18:0] bar_address = 19'd0;
    reg [31:0] bar_data = 32'd0;
    reg [3:0] bar_strobe = 4'd0;
    wire bar_ready;
    wire [31:0] capability_magic;
    wire [31:0] capability_version_bytes;
    wire [31:0] capability_bits;
    wire [31:0] capability_generation;
    wire [31:0] capability_shape_tag;
    wire item_valid;
    reg item_ready = 1'b0;
    wire [7:0] item_mode;
    wire [7:0] item_layer;
    wire [7:0] item_lane_mask;
    wire [7:0] item_format;
    wire [31:0] item_epoch;
    wire [31:0] item_sequence;
    wire [31:0] item_position;
    wire [31:0] item_token;
    wire [31:0] item_index;
    wire [15:0] item_stages;
    wire [15:0] item_last_slot;
    wire [15:0] item_payload_bytes;
    wire [31:0] item_generation;
    wire [31:0] item_shape_tag;
    reg payload_read_valid = 1'b0;
    reg [1:0] payload_read_bank = 2'd0;
    reg [7:0] payload_read_slot = 8'd0;
    reg [3:0] payload_read_word = 4'd0;
    wire payload_read_ready;
    wire payload_rsp_valid;
    wire [31:0] payload_read_data;
    wire payload_read_error;
    wire poisoned;

    integer assertions = 0;
    integer writes = 0;

    truega_lfm25_feed_frontend dut (
        .clk(clk),
        .reset_n(reset_n),
        .state_reset_i(state_reset),
        .bar2_write_valid_i(bar_valid),
        .bar2_write_address_i(bar_address),
        .bar2_write_data_i(bar_data),
        .bar2_write_strobe_i(bar_strobe),
        .bar2_write_ready_o(bar_ready),
        .capability_magic_o(capability_magic),
        .capability_version_record_bytes_o(capability_version_bytes),
        .capability_bits_o(capability_bits),
        .capability_model_generation_o(capability_generation),
        .capability_shape_set_tag_o(capability_shape_tag),
        .item_valid_o(item_valid),
        .item_ready_i(item_ready),
        .item_mode_o(item_mode),
        .item_layer_o(item_layer),
        .item_lane_mask_o(item_lane_mask),
        .item_payload_format_o(item_format),
        .item_session_epoch_o(item_epoch),
        .item_sequence_o(item_sequence),
        .item_position_o(item_position),
        .item_token_o(item_token),
        .item_index_o(item_index),
        .item_stages_per_lane_o(item_stages),
        .item_last_stage_slot_o(item_last_slot),
        .item_payload_bytes_per_stage_o(item_payload_bytes),
        .item_stage_generation_o(item_generation),
        .item_shape_tag_o(item_shape_tag),
        .payload_read_valid_i(payload_read_valid),
        .payload_read_bank_i(payload_read_bank),
        .payload_read_slot_i(payload_read_slot),
        .payload_read_word_i(payload_read_word),
        .payload_read_ready_o(payload_read_ready),
        .payload_read_rsp_valid_o(payload_rsp_valid),
        .payload_read_data_o(payload_read_data),
        .payload_read_error_o(payload_read_error),
        .poisoned_o(poisoned)
    );

    task automatic check;
        input condition;
        input [8*96-1:0] message;
        begin
            assertions = assertions + 1;
            if (!condition) begin
                $display("FAIL: %0s", message);
                $fatal(1);
            end
        end
    endtask

    task automatic reset_state;
        begin
            @(negedge clk);
            state_reset = 1'b1;
            @(negedge clk);
            state_reset = 1'b0;
            #1;
            check(!poisoned && !item_valid, "explicit reset returns frontend to idle");
        end
    endtask

    task automatic bar_write;
        input [18:0] address;
        input [31:0] data;
        input [3:0] strobe;
        begin
            @(negedge clk);
            bar_valid = 1'b1;
            bar_address = address;
            bar_data = data;
            bar_strobe = strobe;
            #1;
            check(bar_ready, "expected BAR2 write was accepted");
            @(negedge clk);
            bar_valid = 1'b0;
            bar_strobe = 4'd0;
            writes = writes + 1;
        end
    endtask

    task automatic blocked_bar_write;
        input [18:0] address;
        input [31:0] data;
        begin
            @(negedge clk);
            bar_valid = 1'b1;
            bar_address = address;
            bar_data = data;
            bar_strobe = 4'hf;
            #1;
            check(!bar_ready, "published payload rejects overwrite by backpressure");
            @(negedge clk);
            bar_valid = 1'b0;
            bar_strobe = 4'd0;
        end
    endtask

    function automatic [18:0] bank_base;
        input integer bank;
        begin
            case (bank)
                0: bank_base = 19'h00000;
                1: bank_base = 19'h04000;
                default: bank_base = 19'h08000;
            endcase
        end
    endfunction

    function automatic [31:0] payload_pattern;
        input integer bank;
        input integer slot;
        input integer word_index;
        input [7:0] salt;
        begin
            payload_pattern = {salt, bank[7:0], slot[7:0], word_index[7:0]};
        end
    endfunction

    task automatic stage_payload;
        input integer bank;
        input integer slot;
        input integer byte_count;
        input [7:0] salt;
        integer word_index;
        integer word_count;
        reg [3:0] final_strobe;
        begin
            word_count = (byte_count + 3) / 4;
            for (word_index = 0; word_index < word_count;
                 word_index = word_index + 1) begin
                if (word_index + 1 == word_count && byte_count % 4 != 0)
                    final_strobe = (1 << (byte_count % 4)) - 1;
                else
                    final_strobe = 4'hf;
                bar_write(bank_base(bank) + slot * 64 + word_index * 4,
                    payload_pattern(bank, slot, word_index, salt),
                    final_strobe);
            end
        end
    endtask

    task automatic stage_shape;
        input integer lanes;
        input integer stages;
        input integer byte_count;
        input [7:0] salt;
        integer stage_index;
        integer lane_index;
        begin
            // Rust FeedSequenceValidator is stage-major, lane-minor.  The RTL
            // accepts the physical writes in any order and proves the same exact set.
            for (stage_index = 0; stage_index < stages;
                 stage_index = stage_index + 1)
                for (lane_index = 0; lane_index < lanes;
                     lane_index = lane_index + 1)
                    stage_payload(lane_index, stage_index, byte_count, salt);
        end
    endtask

    task automatic write_commit_header;
        input [7:0] mode;
        input [7:0] layer;
        input [7:0] lane_mask;
        input [7:0] payload_format;
        input [31:0] epoch;
        input [31:0] sequence_value;
        input [31:0] position;
        input [31:0] token;
        input [31:0] item;
        input [15:0] stages;
        input [15:0] last_slot;
        input [15:0] payload_bytes;
        input [31:0] generation;
        input [31:0] shape_tag;
        input [31:0] capability_override;
        begin
            bar_write(19'h7f000, 32'h3244_4654, 4'hf);
            bar_write(19'h7f004, 32'h0040_0002, 4'hf);
            bar_write(19'h7f008, capability_override, 4'hf);
            bar_write(19'h7f00c,
                {payload_format, lane_mask, layer, mode}, 4'hf);
            bar_write(19'h7f010, epoch, 4'hf);
            bar_write(19'h7f014, sequence_value, 4'hf);
            bar_write(19'h7f018, position, 4'hf);
            bar_write(19'h7f01c, token, 4'hf);
            bar_write(19'h7f020, item, 4'hf);
            bar_write(19'h7f024, {last_slot, stages}, 4'hf);
            bar_write(19'h7f028, {16'd0, payload_bytes}, 4'hf);
            bar_write(19'h7f02c, generation, 4'hf);
            bar_write(19'h7f030, shape_tag, 4'hf);
            bar_write(19'h7f034, 32'd1, 4'hf);
            bar_write(19'h7f038, 32'd0, 4'hf);
        end
    endtask

    task automatic publish_magic;
        integer watchdog;
        begin
            bar_write(19'h7f03c, 32'h324d_4346, 4'hf);
            watchdog = 0;
            while (!item_valid && !poisoned && watchdog < 500) begin
                @(negedge clk);
                watchdog = watchdog + 1;
            end
            check(item_valid || poisoned,
                "sequential commit validation terminates");
        end
    endtask

    task automatic consume_item;
        begin
            @(negedge clk);
            item_ready = 1'b1;
            @(negedge clk);
            item_ready = 1'b0;
            #1;
            check(!item_valid, "ready retires published item");
        end
    endtask

    task automatic read_payload_word;
        input [1:0] bank;
        input [7:0] slot;
        input [3:0] word_index;
        input [31:0] expected;
        begin
            @(negedge clk);
            payload_read_valid = 1'b1;
            payload_read_bank = bank;
            payload_read_slot = slot;
            payload_read_word = word_index;
            #1;
            check(payload_read_ready, "payload read accepted while published");
            @(negedge clk);
            payload_read_valid = 1'b0;
            #1;
            check(payload_rsp_valid && !payload_read_error,
                "payload read returns valid synchronous response");
            check(payload_read_data == expected, "payload response retains staged bytes");
        end
    endtask

    initial begin
        repeat (3) @(negedge clk);
        reset_n = 1'b1;
        @(negedge clk);

        check(capability_magic == 32'h3246_4754, "TGF2 capability magic exact");
        check(capability_version_bytes == 32'h0040_0002,
            "TGF2 version and record bytes exact");
        check(capability_bits == 32'h0000_01ff, "TGF2 required capabilities exact");
        check(capability_generation == 32'd1, "sealed model generation exact");
        check(capability_shape_tag == 32'h03c6_2299, "fixed shape-set tag exact");

        // One complete 1024-element Q8_0 embedding row: 32 slots, 34 bytes each.
        stage_shape(1, 32, 34, 8'h11);
        write_commit_header(0, 8'hff, 1, 3, 7, 0, 0, 17, 0,
            32, 31, 34, 32, 32'h46ea2684, 32'h0000_01ff);
        publish_magic();
        check(item_valid && !poisoned, "one Q8 row publishes");
        check(item_mode == 0 && item_layer == 8'hff && item_lane_mask == 1,
            "Q8 row descriptor identity stable");
        check(item_stages == 32 && item_payload_bytes == 34
                && item_generation == 32,
            "Q8 row descriptor shape exact");
        read_payload_word(0, 0, 0, payload_pattern(0, 0, 0, 8'h11));
        read_payload_word(0, 0, 8,
            payload_pattern(0, 0, 8, 8'h11) & 32'h0000_ffff);
        consume_item();

        // The fixed FFN gate/up input publishes both 32-block lanes atomically.
        stage_shape(2, 32, 34, 8'h22);
        write_commit_header(13, 0, 3, 3, 8, 0, 0, 32'hffff_ffff, 0,
            32, 31, 34, 64, 32'hedd4a10d, 32'h0000_01ff);
        publish_magic();
        check(item_valid && item_mode == 13 && item_lane_mask == 3,
            "dual-lane FFN gate/up row publishes atomically");
        read_payload_word(1, 31, 0, payload_pattern(1, 31, 0, 8'h22));
        consume_item();
        reset_state(); // The 4,608-item request was intentionally not continued.

        // The largest lane count uses all three compact physical banks.
        stage_shape(3, 32, 34, 8'h2a);
        write_commit_header(5, 0, 7, 3, 8, 0, 0, 32'hffff_ffff, 0,
            32, 31, 34, 96, 32'h51ef7cfe, 32'h0000_01ff);
        publish_magic();
        check(item_valid && item_mode == 5 && item_lane_mask == 7,
            "three-lane short-convolution row publishes atomically");
        read_payload_word(2, 31, 8,
            payload_pattern(2, 31, 8, 8'h2a) & 32'h0000_ffff);
        consume_item();
        reset_state(); // The 1,024-item request was intentionally not continued.

        // Slots 32..143 remain accepted and tracked exactly as before, but no
        // sealed multi-lane shape can read them.  The extra-slot count makes
        // the exact commit poison without allocating unreachable payload RAM.
        stage_shape(1, 32, 34, 8'h2b);
        bar_write(bank_base(1) + 32 * 64, 32'hdeca_fbad, 4'hf);
        write_commit_header(0, 8'hff, 1, 3, 8, 0, 0, 23, 0,
            32, 31, 34, 32, 32'h46ea2684, 32'h0000_01ff);
        publish_magic();
        check(poisoned && !item_valid,
            "unused compact bank slot poisons at exact commit");
        reset_state();

        // Maximum staging shape: all 144 physical slots in one lane.
        stage_shape(1, 144, 34, 8'h33);
        write_commit_header(14, 0, 1, 3, 9, 0, 0, 32'hffff_ffff, 0,
            144, 143, 34, 144, 32'h9e59c637, 32'h0000_01ff);
        publish_magic();
        check(item_valid && item_stages == 144 && item_last_slot == 143,
            "144-block FFN down row publishes");
        read_payload_word(0, 143, 8,
            payload_pattern(0, 143, 8, 8'h33) & 32'h0000_ffff);
        consume_item();
        reset_state();

        // Control-only attention core is an exact zero-lane publication.
        write_commit_header(11, 2, 0, 0, 10, 0, 0, 32'hffff_ffff, 0,
            0, 16'hffff, 0, 0, 32'ha7e1ee5f, 32'h0000_01ff);
        publish_magic();
        check(item_valid && item_mode == 11 && item_lane_mask == 0
                && item_stages == 0 && item_last_slot == 16'hffff,
            "control-only first-token attention publishes");
        consume_item();

        // Commit before its exact payload set is complete poisons the frontend.
        write_commit_header(0, 8'hff, 1, 3, 11, 0, 0, 1, 0,
            32, 31, 34, 32, 32'h46ea2684, 32'h0000_01ff);
        publish_magic();
        check(poisoned && !item_valid, "early commit poisons without publication");
        reset_state();

        // Any widened/unknown capability bit fails closed.
        stage_shape(1, 32, 34, 8'h44);
        write_commit_header(0, 8'hff, 1, 3, 12, 0, 0, 1, 0,
            32, 31, 34, 32, 32'h46ea2684, 32'h8000_01ff);
        publish_magic();
        check(poisoned && !item_valid, "bad exact header poisons");
        reset_state();

        // A completed item pins request identity and requires sequence/item +1.
        stage_shape(1, 32, 34, 8'h55);
        write_commit_header(8, 2, 1, 3, 13, 0, 0, 32'hffff_ffff, 0,
            32, 31, 34, 32, 32'h15d68491, 32'h0000_01ff);
        publish_magic();
        check(item_valid && !poisoned, "multi-item request item zero publishes");
        consume_item();
        stage_shape(1, 32, 34, 8'h56);
        write_commit_header(8, 2, 1, 3, 13, 0, 0, 32'hffff_ffff, 0,
            32, 31, 34, 32, 32'h15d68491, 32'h0000_01ff);
        publish_magic();
        check(poisoned && !item_valid, "stale sequence/item/generation commit poisons");
        reset_state();

        // Hold valid under backpressure. BAR writes cannot overwrite RAM or record.
        stage_shape(1, 32, 34, 8'h66);
        write_commit_header(0, 8'hff, 1, 3, 14, 0, 0, 2, 0,
            32, 31, 34, 32, 32'h46ea2684, 32'h0000_01ff);
        publish_magic();
        check(item_valid && item_token == 2 && item_shape_tag == 32'h46ea2684,
            "descriptor visible before backpressure test");
        blocked_bar_write(19'h00000, 32'hdead_beef);
        repeat (3) @(negedge clk);
        check(item_valid && item_token == 2 && !poisoned,
            "descriptor remains stable under backpressure");
        read_payload_word(0, 0, 0, payload_pattern(0, 0, 0, 8'h66));
        consume_item();

        // Poison is sticky until explicit state_reset, then a clean item works.
        write_commit_header(11, 2, 0, 0, 0, 0, 0, 32'hffff_ffff, 0,
            0, 16'hffff, 0, 0, 32'ha7e1ee5f, 32'h0000_01ff);
        publish_magic();
        check(poisoned, "zero session epoch poisons");
        repeat (2) @(negedge clk);
        check(poisoned, "poison is sticky without explicit reset");
        reset_state();
        write_commit_header(11, 2, 0, 0, 15, 0, 0, 32'hffff_ffff, 0,
            0, 16'hffff, 0, 0, 32'ha7e1ee5f, 32'h0000_01ff);
        publish_magic();
        check(item_valid && !poisoned, "valid commit works after explicit recovery");

        $display("PASS truega_lfm25_feed_frontend assertions=%0d writes=%0d",
            assertions, writes);
        $finish;
    end
endmodule
