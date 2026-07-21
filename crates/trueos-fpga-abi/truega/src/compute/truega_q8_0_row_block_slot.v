// Stateful Q8_0 row sequencer for one block per 72-byte inline BAR call.
//
// The caller supplies a four-byte little-endian control header followed by the
// unchanged 34-byte activation and weight blocks:
//   byte 0: bit 0 = first, bit 1 = last; all other bits must be zero
//           bit 2 = wide row (144 blocks instead of 32)
//   byte 1: block index, 0..31
//   byte 2..3: reserved, must be zero
//
// Every accepted call returns the exact block dot, exact block Q30 term, and
// the signed Q30 row accumulator after that term.  A normal row is 0..31;
// first|last with index zero preserves the existing one-block diagnostic.  A
// new valid first block explicitly restarts an incomplete row.  Invalid order
// aborts row state and retires with error_o asserted.
//
// ROW_DIAGNOSTIC_ENABLE defaults to zero.  The active generated function
// wrapper must opt in together with its paired Rust ABI change.
module truega_q8_0_row_block_slot #(
    parameter ROW_DIAGNOSTIC_ENABLE = 0
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
    wire first_i = control_i[0];
    wire last_i = control_i[1];
    wire wide_i = control_i[2];
    wire [7:0] block_index_i = control_i[15:8];
    wire control_reserved = (control_i[31:16] != 16'd0)
                         || (control_i[7:3] != 5'd0);
    wire accept = ROW_DIAGNOSTIC_ENABLE && start_i && !busy_o;
    reg row_active;
    reg [7:0] expected_index;
    reg signed [63:0] accumulator;
    reg active_first;
    reg active_last;
    reg active_wide;
    reg [271:0] activation_block_reg;
    reg [271:0] weight_block_reg;
    reg block_start;
    wire block_busy;
    wire block_done;
    wire signed [31:0] block_dot;
    wire signed [63:0] block_term_q30;
    wire block_scale_error;

    wire [7:0] final_index_i = wide_i ? 8'd143 : 8'd31;
    wire sequence_valid = !control_reserved
                       && (block_index_i <= final_index_i)
                       && (first_i
                           ? (block_index_i == 8'd0)
                           : (row_active
                              && (wide_i == active_wide)
                              && (block_index_i == expected_index)))
                       && (last_i
                           ? ((first_i && (block_index_i == 8'd0))
                              || (block_index_i == final_index_i))
                           : (block_index_i != final_index_i));

    truega_q8_0_block_slot block_slot (
        .clk(clk),
        .reset_n(reset_n),
        .start_i(block_start),
        .activation_block_i(activation_block_reg),
        .weight_block_i(weight_block_reg),
        .busy_o(block_busy),
        .done_o(block_done),
        .dot_o(block_dot),
        .term_q30_o(block_term_q30),
        .scale_error_o(block_scale_error)
    );

    always @(posedge clk) begin
        if (!reset_n) begin
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            dot_o <= 32'sd0;
            term_q30_o <= 64'sd0;
            row_q30_o <= 64'sd0;
            row_active <= 1'b0;
            expected_index <= 8'd0;
            accumulator <= 64'sd0;
            active_first <= 1'b0;
            active_last <= 1'b0;
            active_wide <= 1'b0;
            activation_block_reg <= 272'd0;
            weight_block_reg <= 272'd0;
            block_start <= 1'b0;
        end else begin
            done_o <= 1'b0;
            block_start <= 1'b0;

            if (accept) begin
                dot_o <= 32'sd0;
                term_q30_o <= 64'sd0;
                row_q30_o <= 64'sd0;
                if (!sequence_valid) begin
                    busy_o <= 1'b0;
                    done_o <= 1'b1;
                    error_o <= 1'b1;
                    row_active <= 1'b0;
                    expected_index <= 8'd0;
                    accumulator <= 64'sd0;
                end else begin
                    busy_o <= 1'b1;
                    error_o <= 1'b0;
                    active_first <= first_i;
                    active_last <= last_i;
                    active_wide <= wide_i;
                    activation_block_reg <= activation_block_i;
                    weight_block_reg <= weight_block_i;
                    block_start <= 1'b1;
                    if (first_i) begin
                        row_active <= 1'b0;
                        expected_index <= 8'd0;
                        accumulator <= 64'sd0;
                    end
                end
            end else if (busy_o && block_done) begin
                busy_o <= 1'b0;
                done_o <= 1'b1;
                error_o <= block_scale_error;
                dot_o <= block_dot;
                term_q30_o <= block_term_q30;
                if (active_first) begin
                    accumulator <= block_term_q30;
                    row_q30_o <= block_term_q30;
                end else begin
                    accumulator <= accumulator + block_term_q30;
                    row_q30_o <= accumulator + block_term_q30;
                end

                if (block_scale_error || active_last) begin
                    row_active <= 1'b0;
                    expected_index <= 8'd0;
                end else begin
                    row_active <= 1'b1;
                    expected_index <= active_first ? 8'd1 : expected_index + 8'd1;
                end
            end
        end
    end

    wire unused_block_busy = block_busy;
endmodule
