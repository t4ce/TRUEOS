// BAR2-staged, exact layer-0 LFM2.5 row sequencer.
//
// The host writes unchanged 34-byte Q8_0 blocks into 64-byte BAR2 slots:
//   0x0000: activation blocks
//   0x4000: gate/down weight blocks
//   0x8000: up weight blocks
//
// Mode 1 executes one 32-block gate row, one 32-block up row, and the fixed
// SiLU(gate)*up circuit under one doorbell/retirement. Mode 2 executes one
// 144-block down row. The generic three-function work-package path remains a
// separate compatibility interface.
module truega_lfm25_row_streamer (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 write_i,
    input  wire [16:0]          write_addr_dw_i,
    input  wire [31:0]          write_data_i,
    input  wire                 start_i,
    input  wire [1:0]           mode_i,
    output reg                  busy_o,
    output reg                  done_o,
    output reg                  error_o,
    output reg  [31:0]          error_code_o,
    output reg  signed [63:0]   gate_q30_o,
    output reg  signed [63:0]   up_q30_o,
    output reg  signed [63:0]   result_q30_o,
    output reg  [31:0]          accepted_write_count_o
);
    localparam [1:0] MODE_GATE_UP_SILU = 2'd1;
    localparam [1:0] MODE_DOWN         = 2'd2;

    localparam [3:0] ST_IDLE       = 4'd0;
    localparam [3:0] ST_READ       = 4'd1;
    localparam [3:0] ST_START      = 4'd2;
    localparam [3:0] ST_WAIT       = 4'd3;
    localparam [3:0] ST_SILU_START = 4'd4;
    localparam [3:0] ST_SILU_WAIT  = 4'd5;

    localparam [1:0] PHASE_GATE = 2'd0;
    localparam [1:0] PHASE_UP   = 2'd1;
    localparam [1:0] PHASE_DOWN = 2'd2;

    localparam [31:0] ERROR_BAD_MODE = 32'd1;
    localparam [31:0] ERROR_BUFFER   = 32'd2;
    localparam [31:0] ERROR_ROW      = 32'd3;
    localparam [31:0] ERROR_SILU     = 32'd4;

    // Each logical entry is 36 stored bytes (nine dwords); only the low
    // 272 bits are consumed. The surrounding 64-byte BAR slot makes the block
    // index a bit slice and lets Gowin infer wide synchronous block RAMs.
    reg [287:0] activation_memory [0:143];
    reg [287:0] weight0_memory [0:143];
    reg [287:0] weight1_memory [0:143];
    reg [143:0] activation_valid;
    reg [143:0] weight0_valid;
    reg [143:0] weight1_valid;

    wire write_activation = write_addr_dw_i[16:12] == 5'd0;
    wire write_weight0 = write_addr_dw_i[16:12] == 5'd1;
    wire write_weight1 = write_addr_dw_i[16:12] == 5'd2;
    wire [7:0] write_block_index = write_addr_dw_i[11:4];
    wire [3:0] write_word_index = write_addr_dw_i[3:0];
    wire write_slot_valid = write_block_index < 8'd144 && write_word_index < 4'd9;
    wire accept_write = write_i && !busy_o && write_slot_valid
                     && (write_activation || write_weight0 || write_weight1);

    reg [7:0] read_index;
    reg [287:0] activation_read_data;
    reg [287:0] weight0_read_data;
    reg [287:0] weight1_read_data;

    // Unreset data arrays with synchronous reads are intentional: validity is
    // reset and checked block-by-block before any entry can be consumed.
    always @(posedge clk) begin
        activation_read_data <= activation_memory[read_index];
        weight0_read_data <= weight0_memory[read_index];
        weight1_read_data <= weight1_memory[read_index];

        if (!reset_n) begin
            activation_valid <= 144'd0;
            weight0_valid <= 144'd0;
            weight1_valid <= 144'd0;
            accepted_write_count_o <= 32'd0;
        end else if (accept_write) begin
            accepted_write_count_o <= accepted_write_count_o + 32'd1;
            if (write_activation) begin
                if (write_word_index == 4'd0)
                    activation_valid[write_block_index] <= 1'b0;
                case (write_word_index)
                    4'd0: activation_memory[write_block_index][31:0] <= write_data_i;
                    4'd1: activation_memory[write_block_index][63:32] <= write_data_i;
                    4'd2: activation_memory[write_block_index][95:64] <= write_data_i;
                    4'd3: activation_memory[write_block_index][127:96] <= write_data_i;
                    4'd4: activation_memory[write_block_index][159:128] <= write_data_i;
                    4'd5: activation_memory[write_block_index][191:160] <= write_data_i;
                    4'd6: activation_memory[write_block_index][223:192] <= write_data_i;
                    4'd7: activation_memory[write_block_index][255:224] <= write_data_i;
                    4'd8: begin
                        activation_memory[write_block_index][287:256] <= write_data_i;
                        activation_valid[write_block_index] <= 1'b1;
                    end
                    default: begin end
                endcase
            end else if (write_weight0) begin
                if (write_word_index == 4'd0)
                    weight0_valid[write_block_index] <= 1'b0;
                case (write_word_index)
                    4'd0: weight0_memory[write_block_index][31:0] <= write_data_i;
                    4'd1: weight0_memory[write_block_index][63:32] <= write_data_i;
                    4'd2: weight0_memory[write_block_index][95:64] <= write_data_i;
                    4'd3: weight0_memory[write_block_index][127:96] <= write_data_i;
                    4'd4: weight0_memory[write_block_index][159:128] <= write_data_i;
                    4'd5: weight0_memory[write_block_index][191:160] <= write_data_i;
                    4'd6: weight0_memory[write_block_index][223:192] <= write_data_i;
                    4'd7: weight0_memory[write_block_index][255:224] <= write_data_i;
                    4'd8: begin
                        weight0_memory[write_block_index][287:256] <= write_data_i;
                        weight0_valid[write_block_index] <= 1'b1;
                    end
                    default: begin end
                endcase
            end else if (write_weight1) begin
                if (write_word_index == 4'd0)
                    weight1_valid[write_block_index] <= 1'b0;
                case (write_word_index)
                    4'd0: weight1_memory[write_block_index][31:0] <= write_data_i;
                    4'd1: weight1_memory[write_block_index][63:32] <= write_data_i;
                    4'd2: weight1_memory[write_block_index][95:64] <= write_data_i;
                    4'd3: weight1_memory[write_block_index][127:96] <= write_data_i;
                    4'd4: weight1_memory[write_block_index][159:128] <= write_data_i;
                    4'd5: weight1_memory[write_block_index][191:160] <= write_data_i;
                    4'd6: weight1_memory[write_block_index][223:192] <= write_data_i;
                    4'd7: weight1_memory[write_block_index][255:224] <= write_data_i;
                    4'd8: begin
                        weight1_memory[write_block_index][287:256] <= write_data_i;
                        weight1_valid[write_block_index] <= 1'b1;
                    end
                    default: begin end
                endcase
            end
        end
    end

    reg [3:0] state;
    reg [1:0] phase;
    reg [7:0] block_index;
    reg [7:0] final_index;
    reg [31:0] row_control;
    reg [271:0] row_activation;
    reg [271:0] row_weight;
    reg row_start;
    wire row_busy;
    wire row_done;
    wire row_error;
    wire signed [31:0] row_dot;
    wire signed [63:0] row_term_q30;
    wire signed [63:0] row_accumulator_q30;

    reg signed [63:0] silu_gate;
    reg signed [63:0] silu_up;
    reg silu_start;
    wire silu_busy;
    wire silu_done;
    wire silu_error;
    wire signed [63:0] silu_result;

    wire current_activation_valid = activation_valid[block_index];
    wire current_weight_valid = phase == PHASE_UP
        ? weight1_valid[block_index]
        : weight0_valid[block_index];

    truega_q8_0_row_block_slot #(
        .ROW_DIAGNOSTIC_ENABLE(1)
    ) row_slot (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(row_start),
        .control_i(row_control),
        .activation_block_i(row_activation),
        .weight_block_i(row_weight),
        .busy_o(row_busy),
        .done_o(row_done),
        .error_o(row_error),
        .dot_o(row_dot),
        .term_q30_o(row_term_q30),
        .row_q30_o(row_accumulator_q30)
    );

    truega_lfm25_silu_q30_slot #(
        .SILU_ENABLE(1)
    ) silu_slot (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(silu_start),
        .gate_q30_i(silu_gate),
        .up_q30_i(silu_up),
        .busy_o(silu_busy),
        .done_o(silu_done),
        .error_o(silu_error),
        .result_q30_o(silu_result)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= ST_IDLE;
            phase <= PHASE_GATE;
            block_index <= 8'd0;
            final_index <= 8'd31;
            read_index <= 8'd0;
            row_control <= 32'd0;
            row_activation <= 272'd0;
            row_weight <= 272'd0;
            row_start <= 1'b0;
            silu_gate <= 64'sd0;
            silu_up <= 64'sd0;
            silu_start <= 1'b0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            error_code_o <= 32'd0;
            gate_q30_o <= 64'sd0;
            up_q30_o <= 64'sd0;
            result_q30_o <= 64'sd0;
        end else begin
            row_start <= 1'b0;
            silu_start <= 1'b0;
            done_o <= 1'b0;

            case (state)
                ST_IDLE: begin
                    busy_o <= 1'b0;
                    if (start_i) begin
                        error_o <= 1'b0;
                        error_code_o <= 32'd0;
                        gate_q30_o <= 64'sd0;
                        up_q30_o <= 64'sd0;
                        result_q30_o <= 64'sd0;
                        block_index <= 8'd0;
                        read_index <= 8'd0;
                        if (mode_i == MODE_GATE_UP_SILU) begin
                            busy_o <= 1'b1;
                            phase <= PHASE_GATE;
                            final_index <= 8'd31;
                            state <= ST_READ;
                        end else if (mode_i == MODE_DOWN) begin
                            busy_o <= 1'b1;
                            phase <= PHASE_DOWN;
                            final_index <= 8'd143;
                            state <= ST_READ;
                        end else begin
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                            error_code_o <= ERROR_BAD_MODE;
                        end
                    end
                end

                // One explicit state is retained after changing read_index so
                // every wide-RAM output is registered before reaching a dot slot.
                ST_READ: begin
                    state <= ST_START;
                end

                ST_START: begin
                    if (!current_activation_valid || !current_weight_valid) begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= 1'b1;
                        error_code_o <= ERROR_BUFFER;
                        state <= ST_IDLE;
                    end else begin
                        row_control <= {16'd0, block_index, 5'd0,
                                        phase == PHASE_DOWN,
                                        block_index == final_index,
                                        block_index == 8'd0};
                        row_activation <= activation_read_data[271:0];
                        row_weight <= phase == PHASE_UP
                            ? weight1_read_data[271:0]
                            : weight0_read_data[271:0];
                        row_start <= 1'b1;
                        state <= ST_WAIT;
                    end
                end

                ST_WAIT: begin
                    if (row_done) begin
                        if (row_error) begin
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                            error_code_o <= ERROR_ROW;
                            state <= ST_IDLE;
                        end else if (block_index != final_index) begin
                            block_index <= block_index + 8'd1;
                            read_index <= block_index + 8'd1;
                            state <= ST_READ;
                        end else if (phase == PHASE_GATE) begin
                            gate_q30_o <= row_accumulator_q30;
                            block_index <= 8'd0;
                            read_index <= 8'd0;
                            phase <= PHASE_UP;
                            state <= ST_READ;
                        end else if (phase == PHASE_UP) begin
                            up_q30_o <= row_accumulator_q30;
                            silu_gate <= gate_q30_o;
                            silu_up <= row_accumulator_q30;
                            state <= ST_SILU_START;
                        end else begin
                            result_q30_o <= row_accumulator_q30;
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            state <= ST_IDLE;
                        end
                    end
                end

                ST_SILU_START: begin
                    silu_start <= 1'b1;
                    state <= ST_SILU_WAIT;
                end

                ST_SILU_WAIT: begin
                    if (silu_done) begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= silu_error;
                        error_code_o <= silu_error ? ERROR_SILU : 32'd0;
                        result_q30_o <= silu_result;
                        state <= ST_IDLE;
                    end
                end

                default: begin
                    state <= ST_IDLE;
                    busy_o <= 1'b0;
                    done_o <= 1'b1;
                    error_o <= 1'b1;
                    error_code_o <= ERROR_BAD_MODE;
                end
            endcase
        end
    end

    wire unused_row_busy = row_busy;
    wire unused_silu_busy = silu_busy;
    wire signed [31:0] unused_row_dot = row_dot;
    wire signed [63:0] unused_row_term = row_term_q30;
endmodule
