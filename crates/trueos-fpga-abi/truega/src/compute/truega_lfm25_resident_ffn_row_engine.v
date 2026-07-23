// Standalone resident layer-0 LFM2.5 FFN row engine.
//
// The engine owns one complete fixed-shape transaction:
//   1. load 32 native Q8_0 activation blocks;
//   2. consume 4,608 ordered rows of paired gate/up weight blocks;
//   3. retain SiLU(gate)*up Q30 values in groups of 32 and quantize each group
//      with the strict native Q30->Q8_0 slot, producing 144 resident blocks;
//   4. consume 1,024 ordered down rows over those 144 blocks and retain the
//      resulting Q30[1024] vector.
//
// This module is a fixed circuit, not a command parser. The future BAR wrapper
// may retire row_done_o through MSI, but no BAR, MSI, DMA, or queue logic lives
// here. Payload memories are intentionally unreset and synchronously read.
// Sequencing metadata is reset. Any malformed request or arithmetic error
// poisons the transaction until clear_i explicitly starts over.
module truega_lfm25_resident_ffn_row_engine (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 clear_i,

    input  wire                 activation_valid_i,
    input  wire [4:0]           activation_block_index_i,
    input  wire [271:0]         activation_block_i,
    output wire                 activation_ready_o,
    output wire [4:0]           activation_block_index_o,

    input  wire                 row_start_i,
    input  wire                 row_down_i,
    input  wire [12:0]          row_index_i,
    output wire                 row_ready_o,
    output wire                 row_down_o,
    output wire [12:0]          row_index_o,

    input  wire                 weight_valid_i,
    input  wire [7:0]           weight_block_index_i,
    input  wire [271:0]         weight0_block_i,
    input  wire [271:0]         weight1_block_i,
    output wire                 weight_ready_o,
    output wire [7:0]           weight_block_index_o,

    output reg                  row_done_o,
    output reg                  row_error_o,
    output reg                  row_done_down_o,
    output reg  [12:0]          row_done_index_o,
    output reg                  poison_o,
    output reg  [7:0]           error_code_o,
    output wire                 busy_o,
    output reg                  complete_o,

    input  wire                 output_read_i,
    input  wire [9:0]           output_read_index_i,
    output reg                  output_read_valid_o,
    output reg                  output_read_error_o,
    output reg  signed [63:0]   output_read_q30_o,

    output reg  [5:0]           activation_blocks_loaded_o,
    output reg  [12:0]          gate_up_rows_completed_o,
    output reg  [7:0]           down_activation_blocks_o,
    output reg  [10:0]          down_rows_completed_o
);
    localparam [4:0] ST_LOAD_ACTIVATION = 5'd0;
    localparam [4:0] ST_ROW_READY       = 5'd1;
    localparam [4:0] ST_GU_READ         = 5'd2;
    localparam [4:0] ST_GU_FEED         = 5'd3;
    localparam [4:0] ST_GU_WAIT_BLOCK   = 5'd4;
    localparam [4:0] ST_GU_DRAIN        = 5'd5;
    localparam [4:0] ST_SILU_START      = 5'd6;
    localparam [4:0] ST_SILU_WAIT       = 5'd7;
    localparam [4:0] ST_QUANT_START     = 5'd8;
    localparam [4:0] ST_QUANT_READ      = 5'd9;
    localparam [4:0] ST_QUANT_FEED      = 5'd10;
    localparam [4:0] ST_QUANT_WAIT      = 5'd11;
    localparam [4:0] ST_DOWN_READ       = 5'd12;
    localparam [4:0] ST_DOWN_FEED       = 5'd13;
    localparam [4:0] ST_DOWN_WAIT_BLOCK = 5'd14;
    localparam [4:0] ST_DOWN_DRAIN      = 5'd15;
    localparam [4:0] ST_COMPLETE        = 5'd16;
    localparam [4:0] ST_POISON          = 5'd17;

    localparam [7:0] ERROR_ACTIVATION_ORDER = 8'd1;
    localparam [7:0] ERROR_ROW_REQUEST      = 8'd2;
    localparam [7:0] ERROR_WEIGHT_ORDER     = 8'd3;
    localparam [7:0] ERROR_GATE_UP_ROW      = 8'd4;
    localparam [7:0] ERROR_SILU             = 8'd5;
    localparam [7:0] ERROR_QUANTIZE         = 8'd6;
    localparam [7:0] ERROR_DOWN_ROW         = 8'd7;
    localparam [7:0] ERROR_INTERNAL         = 8'd8;

    localparam [12:0] GATE_UP_ROWS = 13'd4608;
    localparam [10:0] DOWN_ROWS = 11'd1024;

    reg [4:0] state;
    reg [12:0] active_row_index;
    reg active_row_down;
    reg [4:0] activation_read_index;
    reg [7:0] down_activation_read_index;
    reg [5:0] quant_sample_index;
    reg [4:0] silu_read_index;
    reg [5:0] gate_up_block_index;
    reg [7:0] down_block_index;
    reg down_block_in_flight;

    reg [271:0] activation_memory [0:31];
    // Keep this tiny 32-word scratch in registers so the much larger resident
    // tensor stores retain the scarce BSRAM blocks.  The synchronous
    // read/write behavior is unchanged.
    reg signed [63:0] silu_group_memory [0:31]
        /* synthesis syn_ramstyle="registers" */;
    reg [271:0] down_activation_memory [0:143];
    reg signed [63:0] output_memory [0:1023];

    reg [271:0] activation_read_data;
    reg signed [63:0] silu_read_data;
    reg [271:0] down_activation_read_data;

    wire gate_up_phase = gate_up_rows_completed_o < GATE_UP_ROWS;
    assign activation_ready_o = state == ST_LOAD_ACTIVATION && !poison_o;
    assign activation_block_index_o = activation_blocks_loaded_o[4:0];
    assign row_ready_o = state == ST_ROW_READY && !poison_o;
    assign row_down_o = !gate_up_phase;
    assign row_index_o = gate_up_phase
        ? gate_up_rows_completed_o
        : {2'd0, down_rows_completed_o};
    assign weight_block_index_o = gate_up_phase
        ? {2'd0, gate_up_block_index}
        : down_block_index;
    assign busy_o = state != ST_LOAD_ACTIVATION
                 && state != ST_ROW_READY
                 && state != ST_COMPLETE
                 && state != ST_POISON;

    // Unreset payload RAM reads. Validity is represented only by the small
    // counters above and is cleared logically by reset/clear.
    always @(posedge clk) begin
        activation_read_data <= activation_memory[activation_read_index];
        silu_read_data <= silu_group_memory[silu_read_index];
        down_activation_read_data <=
            down_activation_memory[down_activation_read_index];

        output_read_valid_o <= 1'b0;
        output_read_error_o <= 1'b0;
        if (reset_n && !clear_i && output_read_i) begin
            if (!poison_o
                    && output_read_index_i < down_rows_completed_o) begin
                output_read_q30_o <= output_memory[output_read_index_i];
                output_read_valid_o <= 1'b1;
            end else begin
                output_read_error_o <= 1'b1;
            end
        end
    end

    reg gate_up_start;
    wire gate_feeder_ready;
    wire [4:0] gate_feeder_index;
    wire gate_busy;
    wire gate_done;
    wire gate_error;
    wire [5:0] gate_blocks_accepted;
    wire signed [63:0] gate_row_q30;
    wire up_feeder_ready;
    wire [4:0] up_feeder_index;
    wire up_busy;
    wire up_done;
    wire up_error;
    wire [5:0] up_blocks_accepted;
    wire signed [63:0] up_row_q30;

    wire gate_up_index_match = weight_block_index_i
        == {2'd0, gate_up_block_index};
    wire gate_up_internal_index_match = gate_feeder_index
        == gate_up_block_index[4:0]
        && up_feeder_index == gate_up_block_index[4:0];
    wire gate_up_feed_valid = state == ST_GU_FEED
        && weight_valid_i && gate_up_index_match
        && gate_up_internal_index_match;

    truega_lfm25_gate_row_slot #(
        .DIAGNOSTIC_ENABLE(1)
    ) gate_row (
        .clk(clk),
        .reset_n(reset_n && !clear_i && !poison_o),
        .start_i(gate_up_start),
        .feeder_ready_o(gate_feeder_ready),
        .feeder_block_index_o(gate_feeder_index),
        .feeder_valid_i(gate_up_feed_valid),
        .feeder_activation_block_i(activation_read_data),
        .feeder_weight_block_i(weight0_block_i),
        .busy_o(gate_busy),
        .done_o(gate_done),
        .error_o(gate_error),
        .blocks_accepted_o(gate_blocks_accepted),
        .row_q30_o(gate_row_q30)
    );

    truega_lfm25_gate_row_slot #(
        .DIAGNOSTIC_ENABLE(1)
    ) up_row (
        .clk(clk),
        .reset_n(reset_n && !clear_i && !poison_o),
        .start_i(gate_up_start),
        .feeder_ready_o(up_feeder_ready),
        .feeder_block_index_o(up_feeder_index),
        .feeder_valid_i(gate_up_feed_valid),
        .feeder_activation_block_i(activation_read_data),
        .feeder_weight_block_i(weight1_block_i),
        .busy_o(up_busy),
        .done_o(up_done),
        .error_o(up_error),
        .blocks_accepted_o(up_blocks_accepted),
        .row_q30_o(up_row_q30)
    );

    assign weight_ready_o = !poison_o && (
        (state == ST_GU_FEED && gate_feeder_ready && up_feeder_ready)
        || (state == ST_DOWN_FEED && !down_block_in_flight));

    reg silu_start;
    wire silu_busy;
    wire silu_done;
    wire silu_error;
    wire signed [63:0] silu_result;

    truega_lfm25_silu_q30_slot #(
        .SILU_ENABLE(1)
    ) silu (
        .clk(clk),
        .reset_n(reset_n && !clear_i && !poison_o),
        .start_i(silu_start),
        .gate_q30_i(gate_row_q30),
        .up_q30_i(up_row_q30),
        .busy_o(silu_busy),
        .done_o(silu_done),
        .error_o(silu_error),
        .result_q30_o(silu_result)
    );

    reg quant_start;
    reg quant_sample_valid;
    wire quant_sample_ready;
    wire quant_busy;
    wire quant_done;
    wire quant_error;
    wire [5:0] quant_samples_accepted;
    wire [271:0] quant_block;

    truega_q30_to_q8_0_block_slot quantize_down_activation (
        .clk(clk),
        .reset_n(reset_n && !clear_i && !poison_o),
        .start_i(quant_start),
        .sample_valid_i(quant_sample_valid),
        .sample_q30_i(silu_read_data),
        .sample_ready_o(quant_sample_ready),
        .busy_o(quant_busy),
        .done_o(quant_done),
        .error_o(quant_error),
        .samples_accepted_o(quant_samples_accepted),
        .q8_block_o(quant_block)
    );

    wire down_feed_accept = state == ST_DOWN_FEED
        && weight_valid_i && !down_block_in_flight
        && weight_block_index_i == down_block_index;
    wire down_block_valid;
    wire signed [20:0] down_block_dot;
    wire signed [63:0] down_block_term;
    wire down_row_valid;
    wire signed [63:0] down_row_q30;
    wire down_scale_error;

    truega_q8_0_gemv down_row (
        .clk(clk),
        .reset_n(reset_n && !clear_i && !poison_o),
        .valid_i(down_feed_accept),
        .row_first_i(down_feed_accept && down_block_index == 8'd0),
        .row_last_i(down_feed_accept && down_block_index == 8'd143),
        .activation_scale_f16_i(down_activation_read_data[15:0]),
        .weight_scale_f16_i(weight0_block_i[15:0]),
        .activation_quants_i(down_activation_read_data[271:16]),
        .weight_quants_i(weight0_block_i[271:16]),
        .block_valid_o(down_block_valid),
        .block_dot_o(down_block_dot),
        .block_term_q30_o(down_block_term),
        .row_valid_o(down_row_valid),
        .row_q30_o(down_row_q30),
        .scale_error_o(down_scale_error)
    );

    task automatic poison_transaction;
        input [7:0] code;
        begin
            poison_o <= 1'b1;
            error_code_o <= code;
            row_done_o <= 1'b1;
            row_error_o <= 1'b1;
            row_done_down_o <= active_row_down;
            row_done_index_o <= active_row_index;
            state <= ST_POISON;
        end
    endtask

    always @(posedge clk) begin
        if (!reset_n || clear_i) begin
            state <= ST_LOAD_ACTIVATION;
            active_row_index <= 13'd0;
            active_row_down <= 1'b0;
            activation_read_index <= 5'd0;
            down_activation_read_index <= 8'd0;
            quant_sample_index <= 6'd0;
            silu_read_index <= 5'd0;
            gate_up_block_index <= 6'd0;
            down_block_index <= 8'd0;
            down_block_in_flight <= 1'b0;
            gate_up_start <= 1'b0;
            silu_start <= 1'b0;
            quant_start <= 1'b0;
            quant_sample_valid <= 1'b0;
            row_done_o <= 1'b0;
            row_error_o <= 1'b0;
            row_done_down_o <= 1'b0;
            row_done_index_o <= 13'd0;
            poison_o <= 1'b0;
            error_code_o <= 8'd0;
            complete_o <= 1'b0;
            activation_blocks_loaded_o <= 6'd0;
            gate_up_rows_completed_o <= 13'd0;
            down_activation_blocks_o <= 8'd0;
            down_rows_completed_o <= 11'd0;
        end else begin
            gate_up_start <= 1'b0;
            silu_start <= 1'b0;
            quant_start <= 1'b0;
            quant_sample_valid <= 1'b0;
            row_done_o <= 1'b0;
            row_error_o <= 1'b0;

            if (row_start_i && !row_ready_o) begin
                active_row_index <= row_index_i;
                active_row_down <= row_down_i;
                poison_transaction(ERROR_ROW_REQUEST);
            end else begin
                case (state)
                    ST_LOAD_ACTIVATION: begin
                        if (activation_valid_i && activation_ready_o) begin
                            if (activation_block_index_i
                                    != activation_blocks_loaded_o[4:0]) begin
                                active_row_index <= 13'd0;
                                active_row_down <= 1'b0;
                                poison_transaction(ERROR_ACTIVATION_ORDER);
                            end else begin
                                activation_memory[activation_blocks_loaded_o[4:0]]
                                    <= activation_block_i;
                                activation_blocks_loaded_o
                                    <= activation_blocks_loaded_o + 6'd1;
                                if (activation_blocks_loaded_o == 6'd31)
                                    state <= ST_ROW_READY;
                            end
                        end
                    end

                    ST_ROW_READY: begin
                        if (row_start_i) begin
                            active_row_index <= row_index_i;
                            active_row_down <= row_down_i;
                            if (row_down_i != row_down_o
                                    || row_index_i != row_index_o) begin
                                poison_transaction(ERROR_ROW_REQUEST);
                            end else if (!row_down_o) begin
                                gate_up_block_index <= 6'd0;
                                activation_read_index <= 5'd0;
                                gate_up_start <= 1'b1;
                                state <= ST_GU_READ;
                            end else begin
                                down_block_index <= 8'd0;
                                down_activation_read_index <= 8'd0;
                                down_block_in_flight <= 1'b0;
                                state <= ST_DOWN_READ;
                            end
                        end
                    end

                    ST_GU_READ: begin
                        state <= ST_GU_FEED;
                    end

                    ST_GU_FEED: begin
                        if (weight_valid_i && weight_ready_o) begin
                            if (!gate_up_index_match
                                    || !gate_up_internal_index_match) begin
                                poison_transaction(ERROR_WEIGHT_ORDER);
                            end else if (gate_up_block_index == 6'd31) begin
                                state <= ST_GU_DRAIN;
                            end else begin
                                gate_up_block_index
                                    <= gate_up_block_index + 6'd1;
                                activation_read_index
                                    <= gate_up_block_index[4:0] + 5'd1;
                                state <= ST_GU_WAIT_BLOCK;
                            end
                        end
                    end

                    ST_GU_WAIT_BLOCK: begin
                        if (gate_feeder_ready && up_feeder_ready)
                            state <= ST_GU_READ;
                    end

                    ST_GU_DRAIN: begin
                        if (gate_done && up_done) begin
                            if (gate_error || up_error
                                    || gate_blocks_accepted != 6'd32
                                    || up_blocks_accepted != 6'd32) begin
                                poison_transaction(ERROR_GATE_UP_ROW);
                            end else begin
                                state <= ST_SILU_START;
                            end
                        end
                    end

                    ST_SILU_START: begin
                        silu_start <= 1'b1;
                        state <= ST_SILU_WAIT;
                    end

                    ST_SILU_WAIT: begin
                        if (silu_done) begin
                            if (silu_error) begin
                                poison_transaction(ERROR_SILU);
                            end else begin
                                silu_group_memory[active_row_index[4:0]]
                                    <= silu_result;
                                if (active_row_index[4:0] == 5'd31) begin
                                    silu_read_index <= 5'd0;
                                    quant_sample_index <= 6'd0;
                                    state <= ST_QUANT_START;
                                end else begin
                                    gate_up_rows_completed_o
                                        <= gate_up_rows_completed_o + 13'd1;
                                    row_done_o <= 1'b1;
                                    row_done_down_o <= 1'b0;
                                    row_done_index_o <= active_row_index;
                                    state <= ST_ROW_READY;
                                end
                            end
                        end
                    end

                    ST_QUANT_START: begin
                        quant_start <= 1'b1;
                        silu_read_index <= 5'd0;
                        quant_sample_index <= 6'd0;
                        state <= ST_QUANT_READ;
                    end

                    ST_QUANT_READ: begin
                        state <= ST_QUANT_FEED;
                    end

                    ST_QUANT_FEED: begin
                        if (quant_sample_ready) begin
                            quant_sample_valid <= 1'b1;
                            if (quant_sample_index == 6'd31) begin
                                state <= ST_QUANT_WAIT;
                            end else begin
                                quant_sample_index <= quant_sample_index + 6'd1;
                                silu_read_index <= quant_sample_index[4:0] + 5'd1;
                                state <= ST_QUANT_READ;
                            end
                        end
                    end

                    ST_QUANT_WAIT: begin
                        if (quant_done) begin
                            if (quant_error
                                    || quant_samples_accepted != 6'd32) begin
                                poison_transaction(ERROR_QUANTIZE);
                            end else begin
                                down_activation_memory[
                                    down_activation_blocks_o] <= quant_block;
                                down_activation_blocks_o
                                    <= down_activation_blocks_o + 8'd1;
                                gate_up_rows_completed_o
                                    <= gate_up_rows_completed_o + 13'd1;
                                row_done_o <= 1'b1;
                                row_done_down_o <= 1'b0;
                                row_done_index_o <= active_row_index;
                                state <= ST_ROW_READY;
                            end
                        end
                    end

                    ST_DOWN_READ: begin
                        state <= ST_DOWN_FEED;
                    end

                    ST_DOWN_FEED: begin
                        if (weight_valid_i && weight_ready_o) begin
                            if (weight_block_index_i != down_block_index) begin
                                poison_transaction(ERROR_WEIGHT_ORDER);
                            end else begin
                                down_block_in_flight <= 1'b1;
                                state <= ST_DOWN_WAIT_BLOCK;
                            end
                        end
                    end

                    ST_DOWN_WAIT_BLOCK: begin
                        if (down_block_valid) begin
                            down_block_in_flight <= 1'b0;
                            if (down_scale_error) begin
                                poison_transaction(ERROR_DOWN_ROW);
                            end else if (down_block_index == 8'd143) begin
                                if (!down_row_valid) begin
                                    poison_transaction(ERROR_INTERNAL);
                                end else begin
                                    output_memory[active_row_index[9:0]]
                                        <= down_row_q30;
                                    down_rows_completed_o
                                        <= down_rows_completed_o + 11'd1;
                                    row_done_o <= 1'b1;
                                    row_done_down_o <= 1'b1;
                                    row_done_index_o <= active_row_index;
                                    if (active_row_index == 13'd1023) begin
                                        complete_o <= 1'b1;
                                        state <= ST_COMPLETE;
                                    end else begin
                                        state <= ST_ROW_READY;
                                    end
                                end
                            end else begin
                                down_block_index <= down_block_index + 8'd1;
                                down_activation_read_index
                                    <= down_block_index + 8'd1;
                                state <= ST_DOWN_READ;
                            end
                        end
                    end

                    ST_DOWN_DRAIN: begin
                        poison_transaction(ERROR_INTERNAL);
                    end

                    ST_COMPLETE: begin
                        complete_o <= 1'b1;
                    end

                    ST_POISON: begin
                        poison_o <= 1'b1;
                    end

                    default: begin
                        poison_transaction(ERROR_INTERNAL);
                    end
                endcase
            end
        end
    end

    wire unused_gate_busy = gate_busy;
    wire unused_up_busy = up_busy;
    wire unused_silu_busy = silu_busy;
    wire unused_quant_busy = quant_busy;
    wire signed [20:0] unused_down_dot = down_block_dot;
    wire signed [63:0] unused_down_term = down_block_term;
endmodule
