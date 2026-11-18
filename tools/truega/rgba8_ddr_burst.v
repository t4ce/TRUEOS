// Pack eight little-endian RGBA8 pixels into the DDR3 controller's
// 256-bit application word. Put this before the pixel-to-DDR async FIFO.
module rgba8_ddr_burst (
    input wire clk, reset,
    input wire pixel_valid,
    output wire pixel_ready,
    input wire [31:0] pixel_data,
    output reg burst_valid,
    input wire burst_ready,
    output reg [255:0] burst_data
);
    reg [2:0] pixel_index;
    reg [223:0] partial;
    assign pixel_ready = !reset && (pixel_index != 7 || !burst_valid || burst_ready);

    always @(posedge clk) begin
        if (reset) begin
            pixel_index <= 0;
            burst_valid <= 0;
        end else begin
            if (burst_ready)
                burst_valid <= 0;
            if (pixel_valid && pixel_ready) begin
                pixel_index <= pixel_index + 1'b1;
                if (pixel_index == 7) begin
                    burst_data <= {pixel_data, partial};
                    burst_valid <= 1;
                end else begin
                    partial[32 * pixel_index +: 32] <= pixel_data;
                end
            end
        end
    end
endmodule
