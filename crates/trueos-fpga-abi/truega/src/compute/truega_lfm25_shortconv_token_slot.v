// Complete fixed one-token LFM2.5 shortconv circuit for ten recurrent layers.
//
// A command selects a recurrent-layer state slot and its exact token position.
// The 1024-element activation is loaded once as 32 native Q8_0 blocks.  Then
// exactly 1024 b/c/x input-projection row triplets are consumed, 32 blocks per
// row.  The fixed triplet GEMVs, causal three-tap channel, and Q30->Q8_0 block
// quantizer produce exactly 32 y blocks for the existing output-projection GEMV.
//
// Two 10x1024 unreset synchronous state memories hold {oldest,newest}.  Invalid
// metadata means logical zero state, so reset does not require clearing RAM.
// Each channel commits {old1,bx} only after its channel circuit succeeds.  Any
// later failure poisons the selected layer; explicit state_reset_i is required
// before another token can use a partially advanced layer.
module truega_lfm25_shortconv_token_slot (
    input  wire                clk,
    input  wire                reset_n,

    // Abort during an active token cannot roll back already advanced channel
    // RAM, so the selected layer is poisoned until explicit state_reset_i.
    // poison_layer_i extends the same safety rule to a downstream projection
    // or resident-import failure after this slot has completed its token.
    input  wire                abort_i,
    input  wire                poison_layer_i,
    input  wire [3:0]          poison_layer_slot_i,

    input  wire                state_reset_i,
    input  wire [3:0]          state_reset_layer_i,
    output wire                state_reset_ready_o,
    output reg                 state_reset_done_o,

    input  wire                start_i,
    input  wire [3:0]          layer_slot_i,
    input  wire [31:0]         token_position_i,

    input  wire                activation_valid_i,
    output wire                activation_ready_o,
    output wire [4:0]          activation_block_index_o,
    input  wire [271:0]        activation_q8_block_i,

    input  wire                row_valid_i,
    output wire                row_ready_o,
    output wire [9:0]          row_channel_index_o,
    output wire [4:0]          row_block_index_o,
    input  wire [271:0]        row_b_weight_q8_block_i,
    input  wire [271:0]        row_c_weight_q8_block_i,
    input  wire [271:0]        row_x_weight_q8_block_i,
    input  wire [15:0]         kernel_oldest_bf16_i,
    input  wire [15:0]         kernel_newest_bf16_i,
    input  wire [15:0]         kernel_current_bf16_i,

    output wire                output_valid_o,
    input  wire                output_ready_i,
    output wire [4:0]          output_block_index_o,
    output wire                output_last_o,
    output wire [271:0]        output_y_q8_block_o,

    output reg                 busy_o,
    output reg                 done_o,
    output reg                 error_o,
    output reg [10:0]          channels_retired_o,
    output reg [5:0]           blocks_retired_o
);
    localparam [3:0] ST_IDLE          = 4'd0;
    localparam [3:0] ST_LOAD_ACT      = 4'd1;
    localparam [3:0] ST_QUANT_START   = 4'd2;
    localparam [3:0] ST_ROW_START     = 4'd3;
    localparam [3:0] ST_ROW_FEED      = 4'd4;
    localparam [3:0] ST_WAIT_TRIPLET  = 4'd5;
    localparam [3:0] ST_CHANNEL_START = 4'd6;
    localparam [3:0] ST_WAIT_CHANNEL  = 4'd7;
    localparam [3:0] ST_QUANT_FEED    = 4'd8;
    localparam [3:0] ST_WAIT_QUANT    = 4'd9;
    localparam [3:0] ST_OUTPUT        = 4'd10;

    reg [3:0] state;
    reg [3:0] active_layer;
    reg [31:0] active_position;
    reg active_command;
    reg [5:0] activation_count;
    reg [9:0] channel_index;
    reg [4:0] output_block_index;

    // Unreset synchronous activation store.  Keep the read in its own
    // registered process, matching the projection-engine RAM template, so
    // Gowin can implement the 32x272 payload as parallel BSRAM slices instead
    // of a wide bank of flip-flops and dynamic muxes.
    reg [271:0] activation_memory [0:31]
        /* synthesis syn_ramstyle="block_ram" */;
    reg [4:0] activation_read_index;
    reg [271:0] activation_block;
    // Keep each recurrent layer in its own 1K-deep physical bank.  Flattening
    // this as 10,240x64 makes Gowin select its 16K-deep BSRAM geometry and
    // wastes almost half of every primitive.  The explicit banks preserve the
    // exact ten-layer state while mapping each 1,024x64 bank densely.
    reg signed [63:0] state_oldest_memory_0 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_oldest_memory_1 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_oldest_memory_2 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_oldest_memory_3 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_oldest_memory_4 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_oldest_memory_5 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_oldest_memory_6 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_oldest_memory_7 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_oldest_memory_8 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_oldest_memory_9 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_newest_memory_0 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_newest_memory_1 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_newest_memory_2 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_newest_memory_3 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_newest_memory_4 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_newest_memory_5 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_newest_memory_6 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_newest_memory_7 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_newest_memory_8 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_newest_memory_9 [0:1023]
        /* synthesis syn_ramstyle="block_ram" */;
    reg signed [63:0] state_oldest;
    reg signed [63:0] state_newest;
    reg [9:0] state_read_channel;

    reg [9:0] layer_state_valid;
    reg [9:0] layer_state_poisoned;
    reg [31:0] layer_next_position [0:9];
    integer metadata_index;

    wire command_layer_valid = layer_slot_i < 4'd10;
    wire command_position_valid = command_layer_valid
        && !layer_state_poisoned[layer_slot_i]
        && (layer_state_valid[layer_slot_i]
            ? token_position_i == layer_next_position[layer_slot_i]
            : token_position_i == 32'd0);
    // Arora V 138K BSRAM does not implement read-before-write mode.  Keep the
    // recurrent state port in the vendor's documented normal-mode template:
    // the registered read and the write are explicitly mutually exclusive.
    wire state_memory_read = (state == ST_ROW_START)
        && layer_state_valid[active_layer];
    wire state_memory_write = (state == ST_WAIT_CHANNEL)
        && channel_done && !channel_error
        && !(abort_i && active_command);
    wire state_memory_enable = state_memory_read || state_memory_write;
    wire activation_accept = activation_valid_i && activation_ready_o;
    wire row_accept = row_valid_i && row_ready_o;
    wire output_accept = output_valid_o && output_ready_i;

    reg triplet_start;
    wire triplet_feeder_ready;
    wire [4:0] triplet_block_index;
    wire triplet_busy;
    wire triplet_done;
    wire triplet_error;
    wire [5:0] triplet_blocks;
    wire signed [63:0] triplet_b;
    wire signed [63:0] triplet_c;
    wire signed [63:0] triplet_x;

    reg [15:0] kernel_oldest;
    reg [15:0] kernel_newest;
    reg [15:0] kernel_current;
    reg channel_start;
    wire channel_busy;
    wire channel_done;
    wire channel_error;
    wire signed [63:0] channel_bx;
    wire signed [63:0] channel_conv;
    wire signed [63:0] channel_y;
    wire signed [63:0] channel_state_oldest;
    wire signed [63:0] channel_state_newest;

    reg quant_start;
    reg quant_sample_valid;
    reg signed [63:0] quant_sample;
    wire quant_sample_ready;
    wire quant_busy;
    wire quant_done;
    wire quant_error;
    wire [5:0] quant_samples;
    wire [271:0] quant_block;
    reg [271:0] output_block;

    assign state_reset_ready_o = (state == ST_IDLE) && !start_i;
    assign activation_ready_o = state == ST_LOAD_ACT;
    assign activation_block_index_o = activation_count[4:0];
    assign row_ready_o = (state == ST_ROW_FEED) && triplet_feeder_ready;
    assign row_channel_index_o = channel_index;
    assign row_block_index_o = triplet_block_index;
    assign output_valid_o = state == ST_OUTPUT;
    assign output_block_index_o = output_block_index;
    assign output_last_o = output_block_index == 5'd31;
    assign output_y_q8_block_o = output_block;

    truega_lfm25_shortconv_triplet_row_slot triplet (
        .clk(clk), .reset_n(reset_n), .start_i(triplet_start),
        .feeder_ready_o(triplet_feeder_ready),
        .feeder_block_index_o(triplet_block_index),
        .feeder_valid_i(row_accept),
        .feeder_activation_block_i(activation_block),
        .feeder_b_weight_block_i(row_b_weight_q8_block_i),
        .feeder_c_weight_block_i(row_c_weight_q8_block_i),
        .feeder_x_weight_block_i(row_x_weight_q8_block_i),
        .busy_o(triplet_busy), .done_o(triplet_done),
        .error_o(triplet_error), .blocks_accepted_o(triplet_blocks),
        .b_q30_o(triplet_b), .c_q30_o(triplet_c), .x_q30_o(triplet_x)
    );

    truega_lfm25_shortconv_channel_slot causal_channel (
        .clk(clk), .reset_n(reset_n), .start_i(channel_start),
        .b_q30_i(triplet_b), .c_q30_i(triplet_c), .x_q30_i(triplet_x),
        .state_oldest_q30_i(state_oldest),
        .state_newest_q30_i(state_newest),
        .kernel_oldest_bf16_i(kernel_oldest),
        .kernel_newest_bf16_i(kernel_newest),
        .kernel_current_bf16_i(kernel_current),
        .busy_o(channel_busy), .done_o(channel_done), .error_o(channel_error),
        .bx_q30_o(channel_bx), .conv_q30_o(channel_conv), .y_q30_o(channel_y),
        .state_oldest_q30_o(channel_state_oldest),
        .state_newest_q30_o(channel_state_newest)
    );

    truega_q30_to_q8_0_block_slot quantize_y (
        .clk(clk), .reset_n(reset_n), .start_i(quant_start),
        .sample_valid_i(quant_sample_valid), .sample_q30_i(quant_sample),
        .sample_ready_o(quant_sample_ready), .busy_o(quant_busy),
        .done_o(quant_done), .error_o(quant_error),
        .samples_accepted_o(quant_samples), .q8_block_o(quant_block)
    );

    // The address is prefetched while the serialized triplet GEMV retires the
    // preceding block.  No row can be accepted until triplet_feeder_ready, so
    // the registered BSRAM output is stable before the next acceptance edge.
    // Contents and the output register are intentionally unreset; the complete
    // 32-block load and controller state are their validity metadata.
    always @(posedge clk) begin
        activation_block <= activation_memory[activation_read_index];
    end

    // Registered normal-mode read port.  This is deliberately separate from
    // the write process below: combining an unconditional read with a
    // conditional write makes Gowin infer WRITE_MODE=read-before-write, which
    // is illegal for the GW5AST-138B BSRAM primitive.
    always @(posedge clk) begin
        if (!reset_n) begin
            state_oldest <= 64'sd0;
            state_newest <= 64'sd0;
        end else if ((state == ST_ROW_START)
                     && !layer_state_valid[active_layer]) begin
            state_oldest <= 64'sd0;
            state_newest <= 64'sd0;
        end else if (state_memory_enable && !state_memory_write) begin
            case (active_layer)
                4'd0: begin
                    state_oldest <= state_oldest_memory_0[channel_index];
                    state_newest <= state_newest_memory_0[channel_index];
                end
                4'd1: begin
                    state_oldest <= state_oldest_memory_1[channel_index];
                    state_newest <= state_newest_memory_1[channel_index];
                end
                4'd2: begin
                    state_oldest <= state_oldest_memory_2[channel_index];
                    state_newest <= state_newest_memory_2[channel_index];
                end
                4'd3: begin
                    state_oldest <= state_oldest_memory_3[channel_index];
                    state_newest <= state_newest_memory_3[channel_index];
                end
                4'd4: begin
                    state_oldest <= state_oldest_memory_4[channel_index];
                    state_newest <= state_newest_memory_4[channel_index];
                end
                4'd5: begin
                    state_oldest <= state_oldest_memory_5[channel_index];
                    state_newest <= state_newest_memory_5[channel_index];
                end
                4'd6: begin
                    state_oldest <= state_oldest_memory_6[channel_index];
                    state_newest <= state_newest_memory_6[channel_index];
                end
                4'd7: begin
                    state_oldest <= state_oldest_memory_7[channel_index];
                    state_newest <= state_newest_memory_7[channel_index];
                end
                4'd8: begin
                    state_oldest <= state_oldest_memory_8[channel_index];
                    state_newest <= state_newest_memory_8[channel_index];
                end
                default: begin
                    state_oldest <= state_oldest_memory_9[channel_index];
                    state_newest <= state_newest_memory_9[channel_index];
                end
            endcase
        end
    end

    // Normal-mode write port.  Memory contents remain intentionally
    // unreset; layer_state_valid supplies logical zero state after reset.
    always @(posedge clk) begin
        if (state_memory_enable && state_memory_write) begin
            case (active_layer)
                4'd0: begin
                    state_oldest_memory_0[state_read_channel] <= channel_state_oldest;
                    state_newest_memory_0[state_read_channel] <= channel_state_newest;
                end
                4'd1: begin
                    state_oldest_memory_1[state_read_channel] <= channel_state_oldest;
                    state_newest_memory_1[state_read_channel] <= channel_state_newest;
                end
                4'd2: begin
                    state_oldest_memory_2[state_read_channel] <= channel_state_oldest;
                    state_newest_memory_2[state_read_channel] <= channel_state_newest;
                end
                4'd3: begin
                    state_oldest_memory_3[state_read_channel] <= channel_state_oldest;
                    state_newest_memory_3[state_read_channel] <= channel_state_newest;
                end
                4'd4: begin
                    state_oldest_memory_4[state_read_channel] <= channel_state_oldest;
                    state_newest_memory_4[state_read_channel] <= channel_state_newest;
                end
                4'd5: begin
                    state_oldest_memory_5[state_read_channel] <= channel_state_oldest;
                    state_newest_memory_5[state_read_channel] <= channel_state_newest;
                end
                4'd6: begin
                    state_oldest_memory_6[state_read_channel] <= channel_state_oldest;
                    state_newest_memory_6[state_read_channel] <= channel_state_newest;
                end
                4'd7: begin
                    state_oldest_memory_7[state_read_channel] <= channel_state_oldest;
                    state_newest_memory_7[state_read_channel] <= channel_state_newest;
                end
                4'd8: begin
                    state_oldest_memory_8[state_read_channel] <= channel_state_oldest;
                    state_newest_memory_8[state_read_channel] <= channel_state_newest;
                end
                default: begin
                    state_oldest_memory_9[state_read_channel] <= channel_state_oldest;
                    state_newest_memory_9[state_read_channel] <= channel_state_newest;
                end
            endcase
        end
    end

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            active_layer <= 4'd0;
            active_position <= 32'd0;
            active_command <= 1'b0;
            activation_count <= 6'd0;
            channel_index <= 10'd0;
            output_block_index <= 5'd0;
            activation_read_index <= 5'd0;
            state_read_channel <= 10'd0;
            layer_state_valid <= 10'd0;
            layer_state_poisoned <= 10'd0;
            triplet_start <= 1'b0;
            kernel_oldest <= 16'd0;
            kernel_newest <= 16'd0;
            kernel_current <= 16'd0;
            channel_start <= 1'b0;
            quant_start <= 1'b0;
            quant_sample_valid <= 1'b0;
            quant_sample <= 64'sd0;
            output_block <= 272'd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            channels_retired_o <= 11'd0;
            blocks_retired_o <= 6'd0;
            state_reset_done_o <= 1'b0;
            for (metadata_index = 0; metadata_index < 10;
                 metadata_index = metadata_index + 1)
                layer_next_position[metadata_index] <= 32'd0;
        end else begin
            done_o <= 1'b0;
            state_reset_done_o <= 1'b0;
            triplet_start <= 1'b0;
            channel_start <= 1'b0;
            quant_start <= 1'b0;

            if (abort_i && active_command) begin
                layer_state_poisoned[active_layer] <= 1'b1;
                state <= ST_IDLE;
                busy_o <= 1'b0;
                done_o <= 1'b1;
                error_o <= 1'b1;
                quant_sample_valid <= 1'b0;
                active_command <= 1'b0;
            end else case (state)
                ST_IDLE: begin
                    busy_o <= 1'b0;
                    active_command <= 1'b0;
                    quant_sample_valid <= 1'b0;
                    if (poison_layer_i) begin
                        if (poison_layer_slot_i < 4'd10)
                            layer_state_poisoned[poison_layer_slot_i] <= 1'b1;
                        else begin
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                        end
                    end else if (state_reset_i) begin
                        if (state_reset_layer_i < 4'd10) begin
                            layer_state_valid[state_reset_layer_i] <= 1'b0;
                            layer_state_poisoned[state_reset_layer_i] <= 1'b0;
                            layer_next_position[state_reset_layer_i] <= 32'd0;
                            state_reset_done_o <= 1'b1;
                            error_o <= 1'b0;
                        end else begin
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                        end
                    end else if (start_i) begin
                        if (!command_position_valid) begin
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                        end else begin
                            active_layer <= layer_slot_i;
                            active_position <= token_position_i;
                            active_command <= 1'b1;
                            activation_count <= 6'd0;
                            channel_index <= 10'd0;
                            output_block_index <= 5'd0;
                            activation_read_index <= 5'd0;
                            output_block <= 272'd0;
                            channels_retired_o <= 11'd0;
                            blocks_retired_o <= 6'd0;
                            busy_o <= 1'b1;
                            error_o <= 1'b0;
                            state <= ST_LOAD_ACT;
                        end
                    end
                end

                ST_LOAD_ACT: begin
                    if (activation_accept) begin
                        activation_memory[activation_count[4:0]]
                            <= activation_q8_block_i;
                        activation_count <= activation_count + 6'd1;
                        if (activation_count == 6'd31) begin
                            channel_index <= 10'd0;
                            output_block_index <= 5'd0;
                            state <= ST_QUANT_START;
                        end
                    end
                end

                ST_QUANT_START: begin
                    quant_start <= 1'b1;
                    quant_sample_valid <= 1'b0;
                    activation_read_index <= 5'd0;
                    state <= ST_ROW_START;
                end

                ST_ROW_START: begin
                    triplet_start <= 1'b1;
                    state_read_channel <= channel_index;
                    state <= ST_ROW_FEED;
                end

                ST_ROW_FEED: begin
                    if (row_accept) begin
                        if (triplet_block_index == 5'd0) begin
                            kernel_oldest <= kernel_oldest_bf16_i;
                            kernel_newest <= kernel_newest_bf16_i;
                            kernel_current <= kernel_current_bf16_i;
                        end
                        if (triplet_block_index != 5'd31)
                            activation_read_index
                                <= triplet_block_index + 5'd1;
                        else
                            state <= ST_WAIT_TRIPLET;
                    end
                end

                ST_WAIT_TRIPLET: begin
                    if (triplet_done) begin
                        if (triplet_error || triplet_blocks != 6'd32) begin
                            layer_state_poisoned[active_layer] <= 1'b1;
                            state <= ST_IDLE;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                        end else begin
                            state <= ST_CHANNEL_START;
                        end
                    end
                end

                ST_CHANNEL_START: begin
                    channel_start <= 1'b1;
                    state <= ST_WAIT_CHANNEL;
                end

                ST_WAIT_CHANNEL: begin
                    if (channel_done) begin
                        if (channel_error) begin
                            layer_state_poisoned[active_layer] <= 1'b1;
                            state <= ST_IDLE;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                        end else begin
                            // The dedicated normal-mode RAM process commits
                            // the state on this same successful retire edge.
                            channels_retired_o <= channels_retired_o + 11'd1;
                            quant_sample <= channel_y;
                            quant_sample_valid <= 1'b1;
                            state <= ST_QUANT_FEED;
                        end
                    end
                end

                ST_QUANT_FEED: begin
                    if (quant_sample_valid && quant_sample_ready) begin
                        quant_sample_valid <= 1'b0;
                        if (channel_index[4:0] == 5'd31) begin
                            state <= ST_WAIT_QUANT;
                        end else begin
                            channel_index <= channel_index + 10'd1;
                            activation_read_index <= 5'd0;
                            state <= ST_ROW_START;
                        end
                    end
                end

                ST_WAIT_QUANT: begin
                    if (quant_done) begin
                        if (quant_error || quant_samples != 6'd32) begin
                            layer_state_poisoned[active_layer] <= 1'b1;
                            state <= ST_IDLE;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                        end else begin
                            output_block <= quant_block;
                            state <= ST_OUTPUT;
                        end
                    end
                end

                ST_OUTPUT: begin
                    if (output_accept) begin
                        blocks_retired_o <= blocks_retired_o + 6'd1;
                        if (output_block_index == 5'd31) begin
                            layer_state_valid[active_layer] <= 1'b1;
                            layer_state_poisoned[active_layer] <= 1'b0;
                            layer_next_position[active_layer]
                                <= active_position + 32'd1;
                            active_command <= 1'b0;
                            state <= ST_IDLE;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b0;
                        end else begin
                            output_block_index <= output_block_index + 5'd1;
                            channel_index <= channel_index + 10'd1;
                            state <= ST_QUANT_START;
                        end
                    end
                end

                default: begin
                    if (active_command)
                        layer_state_poisoned[active_layer] <= 1'b1;
                    state <= ST_IDLE;
                    busy_o <= 1'b0;
                    done_o <= 1'b1;
                    error_o <= 1'b1;
                    quant_sample_valid <= 1'b0;
                end
            endcase
        end
    end

    wire unused_observability = triplet_busy ^ channel_busy ^ quant_busy
        ^ channel_bx[0] ^ channel_conv[0] ^ state_read_channel[0];
endmodule
