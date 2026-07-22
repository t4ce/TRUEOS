// One attention layer's synthesizable FPGA-local KV storage handshake.
//
// The production shape is the sealed initial context (16,384 positions), 8 KV
// heads, 64 elements, interleaved K/V 64-bit Q30 words.  The same interface
// can bind to the board DDR controller when on-chip RAM capacity is not used;
// the small CACHE_POSITIONS override is used by the deterministic simulation.
module truega_lfm25_kv_cache_slot #(
    parameter integer CACHE_POSITIONS = 16384,
    parameter integer CACHE_WORDS = CACHE_POSITIONS * 8 * 64 * 2,
    parameter integer ADDRESS_BITS = $clog2(CACHE_WORDS)
) (
    input  wire                 clk,
    input  wire                 reset_n,
    input  wire                 start_i,
    input  wire                 clear_i,
    input  wire                 write_i,
    input  wire [16:0]          position_i,
    input  wire [2:0]           kv_head_i,
    input  wire [5:0]           element_i,
    input  wire                 value_i,
    input  wire signed [63:0]   write_q30_i,
    output reg                  busy_o,
    output reg                  done_o,
    output reg                  error_o,
    output reg [16:0]           valid_positions_o,
    output reg [ADDRESS_BITS-1:0] word_address_o,
    output reg signed [63:0]    read_q30_o
);
    reg signed [63:0] memory [0:CACHE_WORDS-1];
    reg clear;
    reg write;
    reg [16:0] position;
    reg [2:0] kv_head;
    reg [5:0] element;
    reg value_kind;
    reg signed [63:0] write_data;
    wire [ADDRESS_BITS-1:0] request_address =
        ((((position * 8) + kv_head) * 64 + element) * 2 + value_kind);

    always @(posedge clk) begin
        if (!reset_n) begin
            clear <= 1'b0;
            write <= 1'b0;
            position <= 17'd0;
            kv_head <= 3'd0;
            element <= 6'd0;
            value_kind <= 1'b0;
            write_data <= 64'sd0;
            busy_o <= 1'b0;
            done_o <= 1'b0;
            error_o <= 1'b0;
            valid_positions_o <= 17'd0;
            word_address_o <= {ADDRESS_BITS{1'b0}};
            read_q30_o <= 64'sd0;
        end else begin
            done_o <= 1'b0;
            if (start_i && !busy_o) begin
                clear <= clear_i;
                write <= write_i;
                position <= position_i;
                kv_head <= kv_head_i;
                element <= element_i;
                value_kind <= value_i;
                write_data <= write_q30_i;
                busy_o <= 1'b1;
                error_o <= 1'b0;
            end else if (busy_o) begin
                busy_o <= 1'b0;
                done_o <= 1'b1;
                if (clear) begin
                    valid_positions_o <= 17'd0;
                    word_address_o <= {ADDRESS_BITS{1'b0}};
                    read_q30_o <= 64'sd0;
                end else if (position >= CACHE_POSITIONS) begin
                    error_o <= 1'b1;
                end else if (write && position != valid_positions_o) begin
                    error_o <= 1'b1;
                end else if (!write && position >= valid_positions_o) begin
                    error_o <= 1'b1;
                end else begin
                    word_address_o <= request_address;
                    if (write) begin
                        memory[request_address] <= write_data;
                        if (value_kind && kv_head == 3'd7 && element == 6'd63)
                            valid_positions_o <= valid_positions_o + 17'd1;
                    end else begin
                        read_q30_o <= memory[request_address];
                    end
                end
            end
        end
    end
endmodule
