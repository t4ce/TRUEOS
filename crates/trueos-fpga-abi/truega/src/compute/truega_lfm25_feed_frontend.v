// Fixed TGF2 BAR2 staging and publication frontend.
//
// This block implements the byte-for-byte contract in
// crates/trueos-fpga-abi/src/lfm25_decode_feed.rs.  The duplicated literals below
// are intentional synthesis inputs and are checked by the Icarus testbench.  This
// is a fixed-function model feed, not an instruction decoder or a runtime graph.
//
// Payload RAM is deliberately not reset.  A small per-slot validity vector makes
// old RAM contents unreachable after reset or retirement; the byte masks are
// replaced on the first write to a newly-valid slot.
module truega_lfm25_feed_frontend (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 state_reset_i,

    input  wire                 bar2_write_valid_i,
    input  wire [18:0]          bar2_write_address_i,
    input  wire [31:0]          bar2_write_data_i,
    input  wire [3:0]           bar2_write_strobe_i,
    output wire                 bar2_write_ready_o,

    output wire [31:0]          capability_magic_o,
    output wire [31:0]          capability_version_record_bytes_o,
    output wire [31:0]          capability_bits_o,
    output wire [31:0]          capability_model_generation_o,
    output wire [31:0]          capability_shape_set_tag_o,

    output reg                  item_valid_o,
    input  wire                 item_ready_i,
    output reg  [7:0]           item_mode_o,
    output reg  [7:0]           item_layer_o,
    output reg  [7:0]           item_lane_mask_o,
    output reg  [7:0]           item_payload_format_o,
    output reg  [31:0]          item_session_epoch_o,
    output reg  [31:0]          item_sequence_o,
    output reg  [31:0]          item_position_o,
    output reg  [31:0]          item_token_o,
    output reg  [31:0]          item_index_o,
    output reg  [15:0]          item_stages_per_lane_o,
    output reg  [15:0]          item_last_stage_slot_o,
    output reg  [15:0]          item_payload_bytes_per_stage_o,
    output reg  [31:0]          item_stage_generation_o,
    output reg  [31:0]          item_shape_tag_o,

    // Synchronous, non-backpressured response convention used by the other
    // resident engines: a request accepted with read_valid && read_ready
    // produces exactly one rsp_valid pulse on the following cycle.  The
    // consumer must sample that pulse; there is intentionally no rsp_ready.
    input  wire                 payload_read_valid_i,
    input  wire [1:0]           payload_read_bank_i,
    input  wire [7:0]           payload_read_slot_i,
    input  wire [3:0]           payload_read_word_i,
    output wire                 payload_read_ready_o,
    output reg                  payload_read_rsp_valid_o,
    output reg  [31:0]          payload_read_data_o,
    output reg                  payload_read_error_o,

    output reg                  poisoned_o
);
    localparam [31:0] FEED_CAPABILITY_MAGIC = 32'h3246_4754; // "TGF2"
    localparam [31:0] FEED_VERSION_RECORD_BYTES = 32'h0040_0002;
    localparam [31:0] FEED_CAPABILITY_BITS = 32'h0000_01ff;
    localparam [31:0] FEED_MODEL_GENERATION = 32'd1;
    localparam [31:0] FEED_SHAPE_SET_TAG = 32'h03c6_2299;
    localparam [31:0] FEED_RECORD_MAGIC = 32'h3244_4654; // "TFD2"
    localparam [31:0] FEED_COMMIT_MAGIC = 32'h324d_4346; // "FCM2"

    localparam integer STAGING_SLOTS = 144;
    // BAR offsets retain three 144-slot apertures, but the immutable shape
    // table only needs all 144 slots in bank 0.  Every multi-lane mode has at
    // most 32 stages, so banks 1 and 2 can share a compact physical layout
    // without changing a single host-visible address.
    localparam integer STAGING_BANK0_SLOTS = 144;
    localparam integer STAGING_BANK1_SLOTS = 32;
    localparam integer STAGING_BANK2_SLOTS = 32;
    localparam integer STAGING_BANKS = 3;
    localparam integer STAGING_BANK1_BASE = STAGING_BANK0_SLOTS;
    localparam integer STAGING_BANK2_BASE = STAGING_BANK1_BASE
        + STAGING_BANK1_SLOTS;
    localparam integer WORDS_PER_SLOT = 16;
    localparam integer TOTAL_SLOTS = STAGING_SLOTS * STAGING_BANKS;
    localparam integer TOTAL_PAYLOAD_SLOTS = STAGING_BANK0_SLOTS
        + STAGING_BANK1_SLOTS + STAGING_BANK2_SLOTS;
    localparam integer TOTAL_WORDS = TOTAL_PAYLOAD_SLOTS * WORDS_PER_SLOT;
    localparam [18:0] COMMIT_OFFSET = 19'h7f000;

    assign capability_magic_o = FEED_CAPABILITY_MAGIC;
    assign capability_version_record_bytes_o = FEED_VERSION_RECORD_BYTES;
    assign capability_bits_o = FEED_CAPABILITY_BITS;
    assign capability_model_generation_o = FEED_MODEL_GENERATION;
    assign capability_shape_set_tag_o = FEED_SHAPE_SET_TAG;

    // 3 * 144 * 64 bytes.  No reset branch writes this array.
    reg [31:0] payload_memory [0:TOTAL_WORDS-1];
    // Byte masks only exist for physically readable payload slots.  The full
    // 432-slot validity vector below still records writes to unused high
    // bank-1/2 slots, so an extra high-slot write still makes the exact count
    // fail (or leaves a required slot absent during validation).  Keeping
    // unreachable masks would synthesize 224 * 64 needless state bits and a
    // very large selection network.
    reg [63:0] stage_byte_mask [0:TOTAL_PAYLOAD_SLOTS-1];
    reg [TOTAL_SLOTS-1:0] stage_slot_valid;
    reg [8:0] stage_valid_count;

    // Commit bytes 0..59 may be written in any order.  Dword 15 is never
    // buffered: a full write of commit_magic is the sole publication event.
    reg [31:0] commit_word [0:14];
    reg [59:0] commit_byte_valid;

    reg request_active;
    reg [7:0] request_mode;
    reg [7:0] request_layer;
    reg [31:0] request_session_epoch;
    reg [31:0] request_position;
    reg [31:0] request_token;
    reg [31:0] expected_sequence;
    reg [31:0] expected_item;
    reg validation_active;

    wire staging_bank0_hit = bar2_write_address_i < 19'h02400;
    wire staging_bank1_hit = bar2_write_address_i >= 19'h04000
        && bar2_write_address_i < 19'h06400;
    wire staging_bank2_hit = bar2_write_address_i >= 19'h08000
        && bar2_write_address_i < 19'h0a400;
    wire staging_hit = staging_bank0_hit || staging_bank1_hit
        || staging_bank2_hit;
    wire commit_header_hit = bar2_write_address_i >= COMMIT_OFFSET
        && bar2_write_address_i < COMMIT_OFFSET + 19'd60;
    wire commit_magic_hit = bar2_write_address_i == COMMIT_OFFSET + 19'd60;
    wire recognized_write = staging_hit || commit_header_hit || commit_magic_hit;
    assign bar2_write_ready_o = recognized_write && !poisoned_o
        && !item_valid_o && !validation_active;

    wire [1:0] write_bank = staging_bank0_hit ? 2'd0
        : staging_bank1_hit ? 2'd1 : 2'd2;
    wire [18:0] write_bank_base = staging_bank0_hit ? 19'h00000
        : staging_bank1_hit ? 19'h04000 : 19'h08000;
    wire [13:0] write_bank_relative = bar2_write_address_i - write_bank_base;
    wire [7:0] write_slot = write_bank_relative[13:6];
    wire [3:0] write_word = write_bank_relative[5:2];
    wire write_payload_slot_in_range = (write_bank == 2'd0
            && write_slot < STAGING_BANK0_SLOTS)
        || (write_bank == 2'd1 && write_slot < STAGING_BANK1_SLOTS)
        || (write_bank == 2'd2 && write_slot < STAGING_BANK2_SLOTS);

    integer write_slot_linear;
    integer write_payload_slot_linear;
    integer write_word_linear;
    integer commit_index;
    integer byte_index;
    integer read_payload_slot_linear;
    integer read_word_linear;
    reg [63:0] write_byte_bits;
    reg [63:0] needed_byte_mask;
    reg [8:0] required_slot_count;
    reg mode_valid;
    reg layer_valid;
    reg context_valid;
    reg commit_fields_valid;
    reg [31:0] mode_items;
    reg [15:0] mode_stages;
    reg [7:0] mode_lanes;
    reg [7:0] mode_lane_mask;
    reg [7:0] mode_format;
    reg [15:0] mode_payload_bytes;
    reg [31:0] mode_shape_tag;
    reg [31:0] expected_stage_generation_comb;
    reg payload_read_ok;
    reg payload_read_physical_ok;
    reg [31:0] payload_read_word_mask;

    // Validate a published payload one slot per cycle.  A single-cycle
    // reduction over all 432 validity/mask entries makes synthesis flatten an
    // enormous compare/fanout graph; the fixed ABI needs at most 144 checks.
    reg [1:0] validation_bank;
    reg [7:0] validation_slot;
    reg [1:0] validation_lanes;
    reg [7:0] validation_stages;
    reg [63:0] validation_needed_byte_mask;
    integer validation_linear;
    integer validation_payload_linear;

    function automatic [63:0] byte_mask_for_count;
        input [15:0] count;
        begin
            if (count >= 16'd64)
                byte_mask_for_count = 64'hffff_ffff_ffff_ffff;
            else if (count == 16'd0)
                byte_mask_for_count = 64'd0;
            else
                byte_mask_for_count = (64'h1 << count) - 64'h1;
        end
    endfunction

    function automatic is_attention_layer;
        input [7:0] layer;
        begin
            case (layer)
                8'd2, 8'd5, 8'd8, 8'd10, 8'd12, 8'd14:
                    is_attention_layer = 1'b1;
                default: is_attention_layer = 1'b0;
            endcase
        end
    endfunction

    function automatic is_shortconv_layer;
        input [7:0] layer;
        begin
            case (layer)
                8'd0, 8'd1, 8'd3, 8'd4, 8'd6, 8'd7,
                8'd9, 8'd11, 8'd13, 8'd15:
                    is_shortconv_layer = 1'b1;
                default: is_shortconv_layer = 1'b0;
            endcase
        end
    endfunction

    // Exact immutable shapes from FeedMode::shape().
    always @* begin
        mode_valid = 1'b1;
        mode_items = 32'd0;
        mode_stages = 16'd0;
        mode_lanes = 8'd0;
        mode_format = 8'd0;
        mode_payload_bytes = 16'd0;
        mode_shape_tag = 32'd0;
        case (commit_word[3][7:0])
            8'd0: begin mode_items=1; mode_stages=32; mode_lanes=1; mode_format=3; mode_payload_bytes=34; mode_shape_tag=32'h46ea2684; end
            8'd1: begin mode_items=1; mode_stages=32; mode_lanes=1; mode_format=1; mode_payload_bytes=64; mode_shape_tag=32'hf27a4365; end
            8'd2: begin mode_items=1; mode_stages=32; mode_lanes=1; mode_format=1; mode_payload_bytes=64; mode_shape_tag=32'h807dd706; end
            8'd3: begin mode_items=1; mode_stages=32; mode_lanes=1; mode_format=1; mode_payload_bytes=64; mode_shape_tag=32'h4c001627; end
            8'd4: begin mode_items=1; mode_stages=96; mode_lanes=1; mode_format=2; mode_payload_bytes=64; mode_shape_tag=32'h752febe3; end
            8'd5: begin mode_items=1024; mode_stages=32; mode_lanes=3; mode_format=3; mode_payload_bytes=34; mode_shape_tag=32'h51ef7cfe; end
            8'd6: begin mode_items=1024; mode_stages=32; mode_lanes=1; mode_format=3; mode_payload_bytes=34; mode_shape_tag=32'he6b98b1f; end
            8'd7: begin mode_items=1; mode_stages=2; mode_lanes=2; mode_format=1; mode_payload_bytes=64; mode_shape_tag=32'h64afb652; end
            8'd8: begin mode_items=1024; mode_stages=32; mode_lanes=1; mode_format=3; mode_payload_bytes=34; mode_shape_tag=32'h15d68491; end
            8'd9: begin mode_items=512; mode_stages=32; mode_lanes=1; mode_format=3; mode_payload_bytes=34; mode_shape_tag=32'ha0781952; end
            8'd10: begin mode_items=512; mode_stages=32; mode_lanes=1; mode_format=3; mode_payload_bytes=34; mode_shape_tag=32'hfb0fff95; end
            8'd11: begin mode_items=1; mode_stages=0; mode_lanes=0; mode_format=0; mode_payload_bytes=0; mode_shape_tag=32'ha7e1ee5f; end
            8'd12: begin mode_items=1024; mode_stages=32; mode_lanes=1; mode_format=3; mode_payload_bytes=34; mode_shape_tag=32'h1d0fbf65; end
            8'd13: begin mode_items=4608; mode_stages=32; mode_lanes=2; mode_format=3; mode_payload_bytes=34; mode_shape_tag=32'hedd4a10d; end
            8'd14: begin mode_items=1024; mode_stages=144; mode_lanes=1; mode_format=3; mode_payload_bytes=34; mode_shape_tag=32'h9e59c637; end
            8'd15: begin mode_items=65536; mode_stages=32; mode_lanes=1; mode_format=3; mode_payload_bytes=34; mode_shape_tag=32'he1188ec3; end
            default: mode_valid = 1'b0;
        endcase
        mode_lane_mask = mode_lanes == 0 ? 8'd0 : ((8'h1 << mode_lanes) - 1'b1);
    end

    // Domain, token, and first-token rules from FeedRequest::validate().
    always @* begin
        layer_valid = 1'b0;
        case (commit_word[3][7:0])
            8'd0, 8'd3, 8'd15:
                layer_valid = commit_word[3][15:8] == 8'hff;
            8'd1, 8'd2, 8'd13, 8'd14:
                layer_valid = commit_word[3][15:8] < 8'd16;
            8'd4, 8'd5, 8'd6:
                layer_valid = is_shortconv_layer(commit_word[3][15:8]);
            8'd7, 8'd8, 8'd9, 8'd10, 8'd11, 8'd12:
                layer_valid = is_attention_layer(commit_word[3][15:8]);
            default: layer_valid = 1'b0;
        endcase
    end

    always @* begin
        needed_byte_mask = byte_mask_for_count(mode_payload_bytes);
        case (mode_lanes)
            8'd0: required_slot_count = 9'd0;
            8'd1: required_slot_count = mode_stages[8:0];
            8'd2: required_slot_count = {mode_stages[7:0], 1'b0};
            8'd3: required_slot_count = mode_stages[8:0]
                + {mode_stages[7:0], 1'b0};
            default: required_slot_count = 9'h1ff;
        endcase

        expected_stage_generation_comb = (commit_word[5] + 1'b1)
            * mode_stages * mode_lanes;
        context_valid = commit_word[4] != 32'd0
            && commit_word[6] < 32'd16384;
        if (commit_word[3][7:0] >= 8'd7
                && commit_word[3][7:0] <= 8'd12)
            context_valid = context_valid && commit_word[6] == 32'd0;
        if (commit_word[3][7:0] == 8'd0)
            context_valid = context_valid && commit_word[7] < 32'd65536;
        else
            context_valid = context_valid && commit_word[7] == 32'hffff_ffff;

        if (request_active)
            context_valid = context_valid
                && commit_word[3][7:0] == request_mode
                && commit_word[3][15:8] == request_layer
                && commit_word[4] == request_session_epoch
                && commit_word[6] == request_position
                && commit_word[7] == request_token
                && commit_word[5] == expected_sequence
                && commit_word[8] == expected_item;
        else
            context_valid = context_valid
                && commit_word[5] == 32'd0 && commit_word[8] == 32'd0;

        commit_fields_valid = &commit_byte_valid
            && commit_word[0] == FEED_RECORD_MAGIC
            && commit_word[1] == FEED_VERSION_RECORD_BYTES
            && commit_word[2] == FEED_CAPABILITY_BITS
            && mode_valid && layer_valid && context_valid
            && commit_word[3][23:16] == mode_lane_mask
            && commit_word[3][31:24] == mode_format
            && commit_word[8] < mode_items
            && commit_word[9][15:0] == mode_stages
            && commit_word[9][31:16]
                == (mode_stages == 0 ? 16'hffff : mode_stages - 1'b1)
            && commit_word[10][15:0] == mode_payload_bytes
            && commit_word[10][31:16] == 16'd0
            && commit_word[11]
                == (mode_stages == 0 ? 32'd0 : expected_stage_generation_comb)
            && commit_word[12] == mode_shape_tag
            && commit_word[13] == FEED_MODEL_GENERATION
            && commit_word[14] == 32'd0;
    end

    always @* begin
        validation_linear = validation_bank * STAGING_SLOTS
            + validation_slot;
        validation_payload_linear = validation_slot;
        case (validation_bank)
            2'd1: validation_payload_linear = STAGING_BANK1_BASE
                + validation_slot;
            2'd2: validation_payload_linear = STAGING_BANK2_BASE
                + validation_slot;
            default: begin end
        endcase
    end

    assign payload_read_ready_o = item_valid_o && !poisoned_o;
    always @* begin
        read_payload_slot_linear = 0;
        payload_read_physical_ok = 1'b0;
        case (payload_read_bank_i)
            2'd0: if (payload_read_slot_i < STAGING_BANK0_SLOTS) begin
                read_payload_slot_linear = payload_read_slot_i;
                payload_read_physical_ok = 1'b1;
            end
            2'd1: if (payload_read_slot_i < STAGING_BANK1_SLOTS) begin
                read_payload_slot_linear = STAGING_BANK1_BASE
                    + payload_read_slot_i;
                payload_read_physical_ok = 1'b1;
            end
            2'd2: if (payload_read_slot_i < STAGING_BANK2_SLOTS) begin
                read_payload_slot_linear = STAGING_BANK2_BASE
                    + payload_read_slot_i;
                payload_read_physical_ok = 1'b1;
            end
            default: begin end
        endcase
        read_word_linear = read_payload_slot_linear * WORDS_PER_SLOT
            + payload_read_word_i;
        payload_read_ok = payload_read_physical_ok
            && payload_read_bank_i < mode_lanes
            && payload_read_slot_i < mode_stages
            && payload_read_word_i < WORDS_PER_SLOT
            && (payload_read_word_i * 4) < mode_payload_bytes;
        payload_read_word_mask = payload_read_ok ? 32'hffff_ffff : 32'd0;
        if (payload_read_ok
                && (payload_read_word_i * 4 + 4) > mode_payload_bytes) begin
            if (mode_payload_bytes[1:0] == 2'd1)
                payload_read_word_mask = 32'h0000_00ff;
            else if (mode_payload_bytes[1:0] == 2'd2)
                payload_read_word_mask = 32'h0000_ffff;
            else if (mode_payload_bytes[1:0] == 2'd3)
                payload_read_word_mask = 32'h00ff_ffff;
        end
    end

    always @* begin
        // Metadata preserves the complete three-aperture v2 behavior: writes
        // to unused high slots are counted and make the later exact commit
        // fail.  Only payload storage is compact because no valid fixed shape
        // can ever read those high bank-1/2 addresses.
        write_slot_linear = write_bank * STAGING_SLOTS + write_slot;
        write_payload_slot_linear = 0;
        case (write_bank)
            2'd0: if (write_slot < STAGING_BANK0_SLOTS)
                write_payload_slot_linear = write_slot;
            2'd1: if (write_slot < STAGING_BANK1_SLOTS)
                write_payload_slot_linear = STAGING_BANK1_BASE + write_slot;
            2'd2: if (write_slot < STAGING_BANK2_SLOTS)
                write_payload_slot_linear = STAGING_BANK2_BASE + write_slot;
            default: begin end
        endcase
        write_word_linear = write_payload_slot_linear * WORDS_PER_SLOT
            + write_word;
        commit_index = (bar2_write_address_i - COMMIT_OFFSET) >> 2;
        write_byte_bits = 64'd0;
        for (byte_index = 0; byte_index < 4; byte_index = byte_index + 1)
            if (bar2_write_strobe_i[byte_index])
                write_byte_bits[write_word * 4 + byte_index] = 1'b1;
    end

    always @(posedge clk) begin
        if (!reset_n) begin
            item_valid_o <= 1'b0;
            poisoned_o <= 1'b0;
            stage_slot_valid <= {TOTAL_SLOTS{1'b0}};
            stage_valid_count <= 9'd0;
            commit_byte_valid <= 60'd0;
            request_active <= 1'b0;
            expected_sequence <= 32'd0;
            expected_item <= 32'd0;
            payload_read_rsp_valid_o <= 1'b0;
            payload_read_data_o <= 32'd0;
            payload_read_error_o <= 1'b0;
            validation_active <= 1'b0;
            validation_bank <= 2'd0;
            validation_slot <= 8'd0;
            validation_lanes <= 2'd0;
            validation_stages <= 8'd0;
            validation_needed_byte_mask <= 64'd0;
            item_mode_o <= 8'd0;
            item_layer_o <= 8'd0;
            item_lane_mask_o <= 8'd0;
            item_payload_format_o <= 8'd0;
            item_session_epoch_o <= 32'd0;
            item_sequence_o <= 32'd0;
            item_position_o <= 32'd0;
            item_token_o <= 32'd0;
            item_index_o <= 32'd0;
            item_stages_per_lane_o <= 16'd0;
            item_last_stage_slot_o <= 16'd0;
            item_payload_bytes_per_stage_o <= 16'd0;
            item_stage_generation_o <= 32'd0;
            item_shape_tag_o <= 32'd0;
        end else if (state_reset_i) begin
            item_valid_o <= 1'b0;
            poisoned_o <= 1'b0;
            stage_slot_valid <= {TOTAL_SLOTS{1'b0}};
            stage_valid_count <= 9'd0;
            commit_byte_valid <= 60'd0;
            request_active <= 1'b0;
            expected_sequence <= 32'd0;
            expected_item <= 32'd0;
            payload_read_rsp_valid_o <= 1'b0;
            payload_read_error_o <= 1'b0;
            validation_active <= 1'b0;
            validation_bank <= 2'd0;
            validation_slot <= 8'd0;
            validation_lanes <= 2'd0;
            validation_stages <= 8'd0;
            validation_needed_byte_mask <= 64'd0;
        end else begin
            payload_read_rsp_valid_o <= 1'b0;
            payload_read_error_o <= 1'b0;
            if (payload_read_valid_i && payload_read_ready_o) begin
                payload_read_rsp_valid_o <= 1'b1;
                payload_read_error_o <= !payload_read_ok;
                payload_read_data_o <= payload_read_ok
                    ? payload_memory[read_word_linear] & payload_read_word_mask
                    : 32'd0;
            end

            if (validation_active) begin
                if (!stage_slot_valid[validation_linear]
                        || (stage_byte_mask[validation_payload_linear]
                            & validation_needed_byte_mask)
                            != validation_needed_byte_mask) begin
                    validation_active <= 1'b0;
                    poisoned_o <= 1'b1;
                end else if (validation_slot + 1'b1
                        == validation_stages) begin
                    if (validation_bank + 1'b1 == validation_lanes) begin
                        validation_active <= 1'b0;
                        item_valid_o <= 1'b1;
                        item_mode_o <= commit_word[3][7:0];
                        item_layer_o <= commit_word[3][15:8];
                        item_lane_mask_o <= commit_word[3][23:16];
                        item_payload_format_o <= commit_word[3][31:24];
                        item_session_epoch_o <= commit_word[4];
                        item_sequence_o <= commit_word[5];
                        item_position_o <= commit_word[6];
                        item_token_o <= commit_word[7];
                        item_index_o <= commit_word[8];
                        item_stages_per_lane_o <= commit_word[9][15:0];
                        item_last_stage_slot_o <= commit_word[9][31:16];
                        item_payload_bytes_per_stage_o <= commit_word[10][15:0];
                        item_stage_generation_o <= commit_word[11];
                        item_shape_tag_o <= commit_word[12];
                        if (!request_active) begin
                            request_active <= 1'b1;
                            request_mode <= commit_word[3][7:0];
                            request_layer <= commit_word[3][15:8];
                            request_session_epoch <= commit_word[4];
                            request_position <= commit_word[6];
                            request_token <= commit_word[7];
                        end
                    end else begin
                        validation_bank <= validation_bank + 1'b1;
                        validation_slot <= 8'd0;
                    end
                end else begin
                    validation_slot <= validation_slot + 1'b1;
                end
            end

            if (item_valid_o && item_ready_i) begin
                item_valid_o <= 1'b0;
                stage_slot_valid <= {TOTAL_SLOTS{1'b0}};
                stage_valid_count <= 9'd0;
                commit_byte_valid <= 60'd0;
                if (item_index_o + 1'b1 == mode_items) begin
                    request_active <= 1'b0;
                    expected_sequence <= 32'd0;
                    expected_item <= 32'd0;
                end else begin
                    expected_sequence <= item_sequence_o + 1'b1;
                    expected_item <= item_index_o + 1'b1;
                end
            end

            if (bar2_write_valid_i && bar2_write_ready_o) begin
                if (bar2_write_address_i[1:0] != 2'd0
                        || bar2_write_strobe_i == 4'd0) begin
                    poisoned_o <= 1'b1;
                end else if (staging_hit) begin
                    if (write_payload_slot_in_range)
                        for (byte_index = 0; byte_index < 4;
                             byte_index = byte_index + 1)
                            if (bar2_write_strobe_i[byte_index])
                                payload_memory[write_word_linear]
                                    [byte_index * 8 +: 8]
                                    <= bar2_write_data_i[byte_index * 8 +: 8];
                    if (!stage_slot_valid[write_slot_linear]) begin
                        stage_slot_valid[write_slot_linear] <= 1'b1;
                        if (write_payload_slot_in_range)
                            stage_byte_mask[write_payload_slot_linear]
                                <= write_byte_bits;
                        stage_valid_count <= stage_valid_count + 1'b1;
                    end else if (write_payload_slot_in_range)
                        stage_byte_mask[write_payload_slot_linear]
                            <= stage_byte_mask[write_payload_slot_linear]
                                | write_byte_bits;
                end else if (commit_header_hit) begin
                    for (byte_index = 0; byte_index < 4;
                         byte_index = byte_index + 1) begin
                        if (bar2_write_strobe_i[byte_index]) begin
                            commit_word[commit_index][byte_index * 8 +: 8]
                                <= bar2_write_data_i[byte_index * 8 +: 8];
                            commit_byte_valid[commit_index * 4 + byte_index]
                                <= 1'b1;
                        end
                    end
                end else if (commit_magic_hit) begin
                    if (bar2_write_strobe_i != 4'hf
                            || bar2_write_data_i != FEED_COMMIT_MAGIC
                            || !commit_fields_valid
                            || stage_valid_count != required_slot_count) begin
                        poisoned_o <= 1'b1;
                    end else if (required_slot_count != 0) begin
                        validation_active <= 1'b1;
                        validation_bank <= 2'd0;
                        validation_slot <= 8'd0;
                        validation_lanes <= mode_lanes[1:0];
                        validation_stages <= mode_stages[7:0];
                        validation_needed_byte_mask <= needed_byte_mask;
                    end else begin
                        item_valid_o <= 1'b1;
                        item_mode_o <= commit_word[3][7:0];
                        item_layer_o <= commit_word[3][15:8];
                        item_lane_mask_o <= commit_word[3][23:16];
                        item_payload_format_o <= commit_word[3][31:24];
                        item_session_epoch_o <= commit_word[4];
                        item_sequence_o <= commit_word[5];
                        item_position_o <= commit_word[6];
                        item_token_o <= commit_word[7];
                        item_index_o <= commit_word[8];
                        item_stages_per_lane_o <= commit_word[9][15:0];
                        item_last_stage_slot_o <= commit_word[9][31:16];
                        item_payload_bytes_per_stage_o <= commit_word[10][15:0];
                        item_stage_generation_o <= commit_word[11];
                        item_shape_tag_o <= commit_word[12];
                        if (!request_active) begin
                            request_active <= 1'b1;
                            request_mode <= commit_word[3][7:0];
                            request_layer <= commit_word[3][15:8];
                            request_session_epoch <= commit_word[4];
                            request_position <= commit_word[6];
                            request_token <= commit_word[7];
                        end
                    end
                end
            end
        end
    end
endmodule
