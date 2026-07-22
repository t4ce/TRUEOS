// Cached-activation Q8_0 row sequencer.
//
// The original inline operation remains unchanged when control bits 5:4 are zero.
// Two additional fixed operations reuse the same 72-byte input envelope:
//   bit 4: cache activation_block_i at block_index (weight_block_i is ignored)
//   bit 5: process two consecutive cached activations; activation_block_i carries
//          weight block N and weight_block_i carries weight block N+1
//
// Pair mode retires only after both exact block operations have accumulated.  Its
// diagnostic dot/term are those of N+1 and row_q30 is the accumulator after both
// terms.  This halves work-package/MSI traffic without changing native Q8_0 blocks,
// the fixed envelopes, or the proven single-block compatibility path.
module truega_q8_0_cached_pair_slot #(
    parameter CACHED_PAIR_ENABLE = 0
) (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 start_i,
    input  wire [31:0]          control_i,
    input  wire [271:0]         activation_block_i,
    input  wire [271:0]         weight_block_i,
    output reg                  busy_o,
    output reg                  done_o,
    output reg                  error_o,
    output reg  signed [31:0]   dot_o,
    output reg  signed [63:0]   term_q30_o,
    output reg  signed [63:0]   row_q30_o
);
    localparam [3:0] STATE_IDLE        = 4'd0;
    localparam [3:0] STATE_DECODE      = 4'd1;
    localparam [3:0] STATE_SINGLE      = 4'd2;
    localparam [3:0] STATE_SINGLE_WAIT = 4'd3;
    localparam [3:0] STATE_PAIR_READ0  = 4'd4;
    localparam [3:0] STATE_PAIR_START0 = 4'd5;
    localparam [3:0] STATE_PAIR_WAIT0  = 4'd6;
    localparam [3:0] STATE_PAIR_START1 = 4'd7;
    localparam [3:0] STATE_PAIR_WAIT1  = 4'd8;

    wire accept = CACHED_PAIR_ENABLE && start_i && !busy_o;

    // A synchronous read and unreset storage allow Gowin to infer block RAM.  Only
    // validity is reset; cached data is never consumed until both pair entries have
    // been explicitly loaded by the host.
    reg [271:0] activation_cache [0:143];
    reg [143:0] cache_valid;
    reg [7:0] cache_read_index;
    reg [271:0] cache_read_data;

    reg [3:0] state;
    reg [31:0] active_control;
    reg [271:0] payload0_reg;
    reg [271:0] payload1_reg;
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

    wire active_wide = active_control[2];
    wire active_cache_load = active_control[4];
    wire active_cached_pair = active_control[5];
    wire [7:0] active_block_index = active_control[15:8];
    wire [7:0] active_final_index = active_wide ? 8'd143 : 8'd31;
    wire active_control_valid = active_control[31:16] == 16'd0
                             && active_control[7:6] == 2'd0
                             && !(active_cache_load && active_cached_pair);
    wire cache_load_valid = active_control_valid && active_cache_load
                          && active_block_index <= active_final_index;
    wire cached_pair_valid = active_control_valid && active_cached_pair
                           && active_block_index < active_final_index
                           && active_block_index[0] == 1'b0;
    wire cache_write = state == STATE_DECODE && cache_load_valid;

    always @(posedge clk) begin
        if (cache_write)
            activation_cache[active_block_index] <= payload0_reg;
        cache_read_data <= activation_cache[cache_read_index];
    end

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

    always @(posedge clk) begin
        if (!reset_n) begin
            state <= STATE_IDLE;
            cache_valid <= 144'd0;
            cache_read_index <= 8'd0;
            active_control <= 32'd0;
            payload0_reg <= 272'd0;
            payload1_reg <= 272'd0;
            row_control <= 32'd0;
            row_activation <= 272'd0;
            row_weight <= 272'd0;
            row_start <= 1'b0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            dot_o <= 32'sd0;
            term_q30_o <= 64'sd0;
            row_q30_o <= 64'sd0;
        end else begin
            row_start <= 1'b0;
            done_o <= 1'b0;

            case (state)
                STATE_IDLE: begin
                    busy_o <= 1'b0;
                    if (accept) begin
                        dot_o <= 32'sd0;
                        term_q30_o <= 64'sd0;
                        row_q30_o <= 64'sd0;
                        error_o <= 1'b0;
                        active_control <= control_i;
                        payload0_reg <= activation_block_i;
                        payload1_reg <= weight_block_i;
                        busy_o <= 1'b1;
                        state <= STATE_DECODE;
                    end
                end

                // Decode only registered package data. Besides simplifying the
                // slot boundary, this prevents BAR input fanout from reaching the
                // block-RAM address/write-enable path in one 100 MHz cycle.
                STATE_DECODE: begin
                        // Select the registered index independently of validation;
                        // invalid operations never consume the RAM output. This
                        // keeps cache-valid reduction logic off the BRAM address CE.
                        cache_read_index <= active_block_index;
                        if (active_cache_load) begin
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            if (cache_load_valid) begin
                                cache_valid[active_block_index] <= 1'b1;
                            end else begin
                                error_o <= 1'b1;
                            end
                            state <= STATE_IDLE;
                        end else if (active_cached_pair) begin
                            if (!cached_pair_valid
                                    || !cache_valid[active_block_index]
                                    || !cache_valid[active_block_index + 8'd1]) begin
                                busy_o <= 1'b0;
                                done_o <= 1'b1;
                                error_o <= 1'b1;
                                state <= STATE_IDLE;
                            end else begin
                                state <= STATE_PAIR_READ0;
                            end
                        end else if (!active_control_valid) begin
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                            state <= STATE_IDLE;
                        end else begin
                            state <= STATE_SINGLE;
                        end
                end

                STATE_SINGLE: begin
                    row_control <= active_control;
                    row_activation <= payload0_reg;
                    row_weight <= payload1_reg;
                    row_start <= 1'b1;
                    state <= STATE_SINGLE_WAIT;
                end

                STATE_SINGLE_WAIT: begin
                    if (row_done) begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= row_error;
                        dot_o <= row_dot;
                        term_q30_o <= row_term_q30;
                        row_q30_o <= row_accumulator_q30;
                        state <= STATE_IDLE;
                    end
                end

                // Wait one full clock after selecting the synchronous cache port.
                STATE_PAIR_READ0: begin
                    state <= STATE_PAIR_START0;
                end

                STATE_PAIR_START0: begin
                    row_control <= {active_control[31:16], active_control[15:8],
                                    5'd0, active_control[2], 1'b0, active_control[0]};
                    row_activation <= cache_read_data;
                    row_weight <= payload0_reg;
                    row_start <= 1'b1;
                    cache_read_index <= active_control[15:8] + 8'd1;
                    state <= STATE_PAIR_WAIT0;
                end

                STATE_PAIR_WAIT0: begin
                    if (row_done) begin
                        if (row_error) begin
                            busy_o <= 1'b0;
                            done_o <= 1'b1;
                            error_o <= 1'b1;
                            state <= STATE_IDLE;
                        end else begin
                            state <= STATE_PAIR_START1;
                        end
                    end
                end

                STATE_PAIR_START1: begin
                    row_control <= {active_control[31:16],
                                    active_control[15:8] + 8'd1,
                                    5'd0, active_control[2], active_control[1], 1'b0};
                    row_activation <= cache_read_data;
                    row_weight <= payload1_reg;
                    row_start <= 1'b1;
                    state <= STATE_PAIR_WAIT1;
                end

                STATE_PAIR_WAIT1: begin
                    if (row_done) begin
                        busy_o <= 1'b0;
                        done_o <= 1'b1;
                        error_o <= row_error;
                        dot_o <= row_dot;
                        term_q30_o <= row_term_q30;
                        row_q30_o <= row_accumulator_q30;
                        state <= STATE_IDLE;
                    end
                end

                default: begin
                    state <= STATE_IDLE;
                    busy_o <= 1'b0;
                    done_o <= 1'b1;
                    error_o <= 1'b1;
                end
            endcase
        end
    end

    wire unused_row_busy = row_busy;
endmodule
