// Dual-clock FIFO for tagged frame bursts. Reset both sides together before
// arming capture. The read-side output register holds its word under stall.
module async_frame_tokens #(
    parameter ADDR_BITS = 9,
    parameter DATA_BITS = 258
) (
    input wire write_clk, write_reset,
    input wire write_valid,
    output wire write_ready,
    input wire [DATA_BITS-1:0] write_data,
    input wire read_clk, read_reset,
    output reg read_valid,
    input wire read_ready,
    output reg [DATA_BITS-1:0] read_data
);
    reg [DATA_BITS-1:0] memory [0:(1 << ADDR_BITS)-1];
    reg [ADDR_BITS:0] write_pointer, read_pointer, write_gray, read_gray;
    (* async_reg = "true" *) reg [ADDR_BITS:0] read_meta, read_sync;
    (* async_reg = "true" *) reg [ADDR_BITS:0] write_meta, write_sync;
    wire [ADDR_BITS:0] next_write = write_pointer + 1'b1;
    wire [ADDR_BITS:0] next_read = read_pointer + 1'b1;
    wire [ADDR_BITS:0] full_at = read_sync ^ (3 << (ADDR_BITS - 1));
    wire prefetch = (!read_valid || read_ready) && read_gray != write_sync;
    assign write_ready = !write_reset && write_gray != full_at;

    always @(posedge write_clk) begin
        if (write_reset) begin
            write_pointer <= 0;
            write_gray <= 0;
            read_meta <= 0;
            read_sync <= 0;
        end else begin
            read_meta <= read_gray;
            read_sync <= read_meta;
            if (write_valid && write_ready) begin
                memory[write_pointer[ADDR_BITS-1:0]] <= write_data;
                write_pointer <= next_write;
                write_gray <= next_write ^ (next_write >> 1);
            end
        end
    end

    always @(posedge read_clk) begin
        if (read_reset) begin
            read_pointer <= 0;
            read_gray <= 0;
            write_meta <= 0;
            write_sync <= 0;
            read_valid <= 0;
        end else begin
            write_meta <= write_gray;
            write_sync <= write_meta;
            if (prefetch) begin
                read_data <= memory[read_pointer[ADDR_BITS-1:0]];
                read_pointer <= next_read;
                read_gray <= next_read ^ (next_read >> 1);
                read_valid <= 1;
            end else if (read_ready) begin
                read_valid <= 0;
            end
        end
    end
endmodule
